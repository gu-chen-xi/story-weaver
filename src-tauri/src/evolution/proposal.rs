// 从 `evolution.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `evolution.rs` 里的其余条目（子模块本来就能访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `use proposal::*;` 取回，
// 外部 `crate::evolution::xxx` 的调用路径**全部不变**。

use super::*;

/// 读开关：**默认 true（要拍板）**。
///
/// 读不到配置时也返回 true —— 宁可多让用户点一下，也不要偷偷改数据库。
pub(super) async fn require_approval() -> bool {
    let Ok(lib) = crate::db::library_pool().await else {
        return true;
    };
    match crate::db::get_config(&lib, "evolution.require_approval").await {
        Ok(Some(v)) => {
            let v = v.trim().to_ascii_lowercase();
            !(v == "false" || v == "0" || v == "off" || v == "no")
        }
        _ => true,
    }
}

/// 提案已插入后调用：**按开关决定"只入队"还是"立即应用"**。
///
/// 两种情况下都会把该组的提案读回来放进报告 —— 界面要能看到队列里多了什么。
pub(super) async fn queue_or_apply(
    pool: &SqlitePool,
    story_id: &str,
    group: i64,
    need_approval: bool,
    proposals: &mut Vec<Proposal>,
    applied_groups: &mut Vec<i64>,
    notes: &mut Vec<String>,
) -> Result<(), String> {
    // 回读整组（一组可能有多条字段变更）
    if let Ok(rows) = sqlx::query(
        "SELECT id, COALESCE(group_id, id), target_type, target_id, field, old_value, new_value,
                reason, evidence, confidence, status, note, created_at, node_id
         FROM change_proposals WHERE group_id = ?1 ORDER BY id",
    )
    .bind(group)
    .fetch_all(pool)
    .await
    {
        for r in &rows {
            proposals.push(proposal_from_row(r));
        }
    }

    if need_approval {
        notes.push(format!("已加入待确认队列（组 {group}）"));
    } else {
        match apply_group(pool, story_id, group).await {
            Ok(r) => {
                applied_groups.push(group);
                notes.push(r);
            }
            Err(e) => notes.push(format!("自动应用失败：{e}")),
        }
    }
    Ok(())
}

// ---------- 提案 ----------
#[derive(serde::Serialize, Clone)]
pub struct Proposal {
    pub id: i64,
    pub group_id: i64,
    pub target_type: String,
    pub target_id: i64,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub reason: String,
    pub evidence: String,
    pub confidence: f64,
    pub status: String,
    pub note: String,
    pub created_at: String,
    /// ⑤-B：这条提案来自哪个时间线节点（拍板时显示"来源节点"）
    pub node_id: Option<i64>,
}

// ---------- 校验器 ----------
pub(super) fn validate(
    field: &str,
    new_value: &str,
    confidence: f64,
    evidence: &str,
    hard_rules: &[(String, String)],
) -> Result<(), String> {
    // 字段合法
    if !["relation_type", "direction", "strength", "current_status"].contains(&field) {
        return Err(format!("字段不合法：{}", field));
    }
    // 置信度达标
    if confidence < 0.8 {
        return Err(format!("置信度不足：{:.2}", confidence));
    }
    // 证据充分
    if evidence.trim().is_empty() {
        return Err("证据为空".into());
    }
    // 数值越界（strength 0-100）
    if field == "strength" {
        let v: i64 = new_value.parse().map_err(|_| "强度不是数字".to_string())?;
        if !(0..=100).contains(&v) {
            return Err(format!("强度越界：{}", v));
        }
    }
    // 世界观硬规则冲突
    if field == "relation_type" {
        for (k, v) in hard_rules {
            if (k.contains("恋爱") || k.contains("交往") || k.contains("关系"))
                && (v.contains("禁止") || v.contains("不能") || v.contains("不允许"))
            {
                return Err(format!("世界观硬规则冲突：{} = {}", k, v));
            }
        }
    }
    Ok(())
}

pub(super) async fn audit(pool: &SqlitePool, action: &str, target: &str, before: &str, after: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_logs(action, target, before, after, operator) VALUES(?1, ?2, ?3, ?4, 'system')")
        .bind(action)
        .bind(target)
        .bind(before)
        .bind(after)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------- 冷却期检查 ----------
/// 冷却判定（2026-10-29 定稿）：
///
/// 1. **同一个节点**里已经提过这个目标 → 跳过（一轮内不重复提）；
/// 2. **3 天内落地过**（applied）→ 跳过（防止反复推同一条关系）；
/// 3. pending **不阻塞新的一天**、rejected **完全不冷却** —— 拒绝说明这次判断不成立，
///    不该让下一轮跟着受罚（旧逻辑：有 pending 就一直跳过；用户实测挂一条待确认后从此再也不弹）。

#[cfg(test)]
mod cooldown_tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn cpool() -> SqlitePool {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE change_proposals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                target_type TEXT NOT NULL, target_id INTEGER,
                field TEXT NOT NULL DEFAULT '', old_value TEXT, new_value TEXT,
                reason TEXT, evidence TEXT, confidence REAL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending', note TEXT DEFAULT '',
                node_id INTEGER, group_id INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn insert(pool: &SqlitePool, node_id: i64, status: &str, days_ago: i64) {
        sqlx::query(
            "INSERT INTO change_proposals(target_type, target_id, field, status, node_id, created_at)
             VALUES('relation', 1, 'strength', ?1, ?2, datetime('now','localtime', ?3))",
        )
        .bind(status)
        .bind(node_id)
        .bind(format!("-{days_ago} days"))
        .execute(pool)
        .await
        .unwrap();
    }

    /// 同一节点已提过 → 跳过（一轮不重复）
    #[tokio::test]
    async fn same_node_blocks_but_next_day_does_not() {
        let pool = cpool().await;
        insert(&pool, 7, "pending", 0).await;
        assert!(in_cooldown(&pool, "relation", 1, 7).await.unwrap(), "同一节点要跳过");
        assert!(!in_cooldown(&pool, "relation", 1, 8).await.unwrap(), "跨天允许再提（pending 不阻塞新的一天）");
    }

    /// 拒绝不冷却：这次判断不成立，不该罚下一轮
    #[tokio::test]
    async fn rejected_never_cools() {
        let pool = cpool().await;
        insert(&pool, 7, "rejected", 1).await;
        assert!(!in_cooldown(&pool, "relation", 1, 9).await.unwrap());
    }

    /// 落地才进 3 天冷却
    #[tokio::test]
    async fn applied_cools_for_three_days() {
        let pool = cpool().await;
        insert(&pool, 7, "applied", 1).await;
        assert!(in_cooldown(&pool, "relation", 1, 9).await.unwrap(), "1 天前落地过 → 冷却");
        let fresh = cpool().await;
        insert(&fresh, 7, "applied", 5).await;
        assert!(!in_cooldown(&fresh, "relation", 1, 9).await.unwrap(), "5 天前落地 → 已过冷却");
    }
}
pub(super) async fn in_cooldown(
    pool: &SqlitePool,
    target_type: &str,
    target_id: i64,
    node_id: i64,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT COUNT(*) FROM change_proposals
         WHERE target_type = ?1 AND target_id = ?2
           AND (node_id = ?3
                OR (status = 'applied' AND created_at >= datetime('now', 'localtime', ?4)))",
    )
    .bind(target_type)
    .bind(target_id)
    .bind(node_id)
    .bind(format!("-{} days", COOLDOWN_DAYS))
    .fetch_one(pool)
    .await?;
    let n: i64 = row.try_get(0)?;
    Ok(n > 0)
}

// ---------- 应用一组提案（整组确认） ----------
pub(super) async fn apply_group(pool: &SqlitePool, story_id: &str, group_id: i64) -> Result<String, String> {
    apply_group_inner(pool, story_id, group_id, true).await
}

/// `apply_group` 的实现。`take_snapshot=false` 用于**批量落地**：
/// 整批只留一个还原点，比"每组一个快照"干净得多。
pub(super) async fn apply_group_inner(
    pool: &SqlitePool,
    story_id: &str,
    group_id: i64,
    take_snapshot: bool,
) -> Result<String, String> {
    ensure_group_column(pool).await;
    let rows = sqlx::query(
        "SELECT id, target_type, target_id, field, old_value, new_value, status FROM change_proposals
         WHERE group_id = ?1 ORDER BY id",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Err(format!("提案组 {} 不存在", group_id));
    }
    let pending_idx: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.try_get::<String, _>(6).unwrap_or_default() == "pending")
        .map(|(i, _)| i)
        .collect();
    if pending_idx.is_empty() {
        return Err("该组没有待确认的提案".into());
    }

    let target_type: String = rows[pending_idx[0]].try_get(1).map_err(|e| e.to_string())?;
    let target_id: i64 = rows[pending_idx[0]].try_get(2).map_err(|e| e.to_string())?;

    if target_type != "relation" {
        return Err(format!("暂不支持应用 {} 类型提案", target_type));
    }

    // 读当前 relation
    let r = sqlx::query(
        "SELECT relation_type, direction, strength, version FROM relations WHERE id = ?1",
    )
    .bind(target_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut rel_type: String = r.try_get(0).map_err(|e| e.to_string())?;
    let mut direction: String = r.try_get(1).map_err(|e| e.to_string())?;
    let mut strength: i64 = r.try_get(2).map_err(|e| e.to_string())?;
    let version: i64 = r.try_get(3).map_err(|e| e.to_string())?;

    let before = serde_json::json!({ "relation_type": rel_type, "direction": direction, "strength": strength })
        .to_string();

    // 逐条应用
    for &i in &pending_idx {
        let field: String = rows[i].try_get(3).map_err(|e| e.to_string())?;
        let new_value: Option<String> = rows[i].try_get(5).map_err(|e| e.to_string())?;
        let Some(nv) = new_value else { continue };
        match field.as_str() {
            "relation_type" => rel_type = nv,
            "direction" => direction = nv,
            "strength" => {
                strength = nv.parse::<i64>().map_err(|e| e.to_string())?;
            }
            _ => {}
        }
    }

    sqlx::query(
        "UPDATE relations SET relation_type = ?1, direction = ?2, strength = ?3, version = ?4 WHERE id = ?5",
    )
    .bind(&rel_type)
    .bind(&direction)
    .bind(strength)
    .bind(version + 1)
    .bind(target_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    // 整组标记 applied
    sqlx::query("UPDATE change_proposals SET status = 'applied', note = '' WHERE group_id = ?1")
        .bind(group_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    // 新快照 + 审计
    // 应用提案后存一个**永久**版本（不是临时的 —— 这是正常的版本历史）
    // 批量落地时由调用方统一打一个快照，这里跳过。
    let v = if take_snapshot {
        make_snapshot(pool, false, None, None).await.map_err(|e| e.to_string())?
    } else {
        -1
    };
    let after = serde_json::json!({ "relation_type": rel_type, "direction": direction, "strength": strength })
        .to_string();
    // 审计是**尽力而为**：走到这里关系表已经改了、快照也打了，
    // 若 audit 失败还返回 Err，界面会显示"应用失败"而数据其实已经变了 —— 比丢一条审计更误导。
    if let Err(e) = audit(
        pool,
        "apply_proposal",
        &format!("relation#{}", target_id),
        &before,
        &after,
    )
    .await
    {
        eprintln!("[evolution] 审计写入失败（提案已应用）：{e}");
    }

    if take_snapshot {
        Ok(format!(
            "已应用提案组 {}，关系已更新为 {} / {} / {}，新快照 V{}（story {}）",
            group_id, rel_type, direction, strength, v, story_id
        ))
    } else {
        Ok(format!(
            "已应用提案组 {}，关系已更新为 {} / {} / {}",
            group_id, rel_type, direction, strength
        ))
    }
}

pub(super) fn proposal_from_row(r: &sqlx::sqlite::SqliteRow) -> Proposal {
    Proposal {
        id: r.get::<i64, _>(0),
        group_id: r.get::<i64, _>(1),
        target_type: r.get::<String, _>(2),
        target_id: r.get::<i64, _>(3),
        field: r.get::<String, _>(4),
        old_value: r.get::<Option<String>, _>(5),
        new_value: r.get::<Option<String>, _>(6),
        reason: r.get::<String, _>(7),
        evidence: r.get::<String, _>(8),
        confidence: r.get::<f64, _>(9),
        status: r.get::<String, _>(10),
        note: r.get::<String, _>(11),
        created_at: r.get::<String, _>(12),
        node_id: r.try_get::<Option<i64>, _>(13).unwrap_or(None),
    }
}

// ---------- 查询与操作命令 ----------
pub async fn list_proposals(story_id: &str) -> Result<Vec<Proposal>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_group_column(&pool).await;
    // 第五批 ③：只列**当前分支**的提案（主线 = branch_id IS NULL）
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let sql = format!(
        "SELECT id, COALESCE(group_id, id), target_type, target_id, field, old_value, new_value,
                reason, evidence, confidence, status, '', created_at, node_id
         FROM change_proposals WHERE {} ORDER BY id DESC LIMIT 100",
        clause
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = &branch {
        q = q.bind(b);
    }
    let rows = q
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(proposal_from_row(r));
    }
    Ok(out)
}

/// **测试注入**：不依赖 AI 剧情信号，直接造一条"待确认"提案。
///
/// 为什么需要：抽取靠规则匹配剧情文本（关心/表白/冲突…），
/// **AI 回复的措辞不稳定**，想稳定复现"拍板流程"很难。
/// 这个入口让"入队 → 勾选 → 落地 → 回滚"能确定性验证。
///
/// 注入的提案会明确写 `evidence = "【测试注入】..."`，一眼能认出来。
pub async fn inject_test_proposal(story_id: &str, node_id: Option<i64>) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_group_column(&pool).await;
    crate::story_db::ensure_proposal_node_column(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);

    // 挑一条现有关系；没有就报错（连关系都没有时，拍板无从谈起）
    let rel = sqlx::query("SELECT id, strength FROM relations ORDER BY id LIMIT 1")
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let Some(rel) = rel else {
        return Err("这个作品还没有任何关系，先去角色管理建两条关系再注入".into());
    };
    let rel_id: i64 = rel.try_get(0).map_err(|e| e.to_string())?;
    let strength: i64 = rel.try_get(1).map_err(|e| e.to_string())?;
    // 变 +5，但不超过 100；已经在 100 就 -5，保证一定有变化
    let new_strength = if strength < 100 { strength + 5 } else { strength - 5 };

    let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?
        .try_get::<i64, _>(0)
        .map_err(|e| e.to_string())?
        + 1;

    insert_proposal(
        &pool,
        &branch,
        node_id.unwrap_or(0),
        group,
        "relation",
        rel_id,
        "strength",
        &strength.to_string(),
        &new_strength.to_string(),
        "【测试注入】用于验证拍板流程，非剧情推导",
        "【测试注入】无需真实剧情信号",
        0.99,
        "pending",
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(format!("已注入 1 组待确认提案（组 {group}，关系#{rel_id} 强度 {strength} → {new_strength}）"))
}

/// ⑤-B：**批量落地**选中的提案（界面勾选多条后一次确认）。
///
/// 设计取舍：**整批只打一个快照**。否则勾 10 条就在「设定版本」里塞 10 个版本，
/// 用户回滚时也搞不清哪个对应哪次操作。
pub async fn confirm_proposals_batch(story_id: &str, group_ids: Vec<i64>) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    confirm_proposals_batch_in(&pool, story_id, group_ids, branch).await
}

/// `confirm_proposals_batch` 的实现（**pool 可注入**，测试直接传临时库）。
pub(super) async fn confirm_proposals_batch_in(
    pool: &SqlitePool,
    story_id: &str,
    group_ids: Vec<i64>,
    branch: Option<String>,
) -> Result<String, String> {
    if group_ids.is_empty() {
        return Err("没有选中任何提案".into());
    }
    ensure_group_column(pool).await;

    // 先打一个快照（整批的还原点）—— 失败就不动数据
    let v = make_snapshot(pool, false, Some("批量落地设定变更"), branch)
        .await
        .map_err(|e| format!("打快照失败，已放弃本次批量落地：{e}"))?;

    let mut ok = 0usize;
    let mut fails: Vec<String> = Vec::new();
    for g in &group_ids {
        match apply_group_inner(pool, story_id, *g, false).await {
            Ok(_) => ok += 1,
            Err(e) => fails.push(format!("组 {g}：{e}")),
        }
    }

    let mut msg = format!("已落地 {ok} 组（还原点 V{v}）");
    if !fails.is_empty() {
        msg.push_str(&format!("；{} 组失败：{}", fails.len(), fails.join("；")));
    }
    Ok(msg)
}

/// ⑤-B：**批量拒绝**选中的提案（有争议的一次清掉）。
pub async fn reject_proposals_batch(story_id: &str, group_ids: Vec<i64>) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    reject_proposals_batch_in(&pool, group_ids).await
}

/// `reject_proposals_batch` 的实现（**pool 可注入**）。
pub(super) async fn reject_proposals_batch_in(pool: &SqlitePool, group_ids: Vec<i64>) -> Result<String, String> {
    if group_ids.is_empty() {
        return Err("没有选中任何提案".into());
    }
    ensure_group_column(pool).await;
    let mut n = 0usize;
    for g in &group_ids {
        let r = sqlx::query(
            "UPDATE change_proposals SET status = 'rejected', note = '用户拒绝' WHERE group_id = ?1 AND status = 'pending'",
        )
        .bind(g)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        n += r.rows_affected() as usize;
    }
    Ok(format!("已拒绝 {n} 条提案"))
}

pub async fn confirm_proposal_group(story_id: &str, group_id: i64) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    apply_group(&pool, story_id, group_id).await
}

pub async fn reject_proposal_group(story_id: &str, group_id: i64) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let r = sqlx::query(
        "UPDATE change_proposals SET status = 'rejected', note = '用户拒绝' WHERE group_id = ?1 AND status = 'pending'",
    )
    .bind(group_id)
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;
    if r.rows_affected() == 0 {
        return Err("该组没有待确认的提案".into());
    }
    audit(&pool, "reject_proposal", &format!("group#{}", group_id), "", "").await.map_err(|e| e.to_string())?;
    Ok(format!("已拒绝提案组 {}", group_id))
}
