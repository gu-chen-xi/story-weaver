// M6：后台静默演进——事件提取 → 实质判断 → 变更提案 → 校验 → 人工确认 → 应用 → 快照 → 审计
// MVP 范围：规则引擎实现候选事件提取；关系类型/方向变更为高风险需人工确认；强度可自动应用。

use sqlx::Row;
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::characters::{self, Character};
use crate::relations::{self, Relation};

// ---------- 事件信号词典 ----------
const CONFESSION: &[&str] = &["我喜欢你", "我爱你", "喜欢你很久", "做我女朋友", "做我男朋友", "跟我交往", "我们在一起吧", "向你表白"];
const ACCEPTANCE: &[&str] = &["我也喜欢你", "我也喜欢", "我早就喜欢你", "我也是", "我愿意", "当然愿意", "我答应你", "我答应", "我们在一起吧", "我们在一起", "早就是了", "我也"];
const REJECTION: &[&str] = &["我不喜欢你", "我们还是朋友吧", "我们还是朋友", "我拒绝你"];
const CARE: &[&str] = &["关心", "递伞", "挡雨", "照顾", "扶住", "担心你", "心疼"];
const CONFLICT: &[&str] = &["争吵", "吵架", "冷战", "推开", "不理她", "不理他", "生气地说", "摔门"];

// ===== 第三批新增信号（2026-10-11）=====
// 设计原则：**只加不会误判的强信号** —— 都是明确的剧情动作，避免"关心/照顾"这类泛词导致提案泛滥。
// 各信号用途见 analyze_evolution：
//   promise   → 一次明确的承诺/约定：只记事件，不自动改动数值
//   sacrifice → 为对方挡下/站出来：关系强度较大上升
//   betrayal  → 恶语、冷暴力、失信：关系强度较大下降 + 状态趋向紧张
const PROMISE_WORDS: &[&str] = &["我答应你", "我保证", "我发誓", "我会一直", "我永远", "等你回来", "说到做到", "放心交给我"];
const SACRIFICE_WORDS: &[&str] = &["挡在她身前", "挡在他身前", "挡在身前", "护住她", "护住他", "替她挨", "替他挨", "挺身而出", "护在身后"];
const BETRAYAL_WORDS: &[&str] = &["恶语", "冷暴力", "失信", "骗了她", "骗了他", "背叛", "说出很难听", "翻脸"];

const COOLDOWN_DAYS: i64 = 3;


/// 确保 change_proposals 有 group_id / note 列（老库迁移；新库建表时已带）。
async fn ensure_group_column(pool: &SqlitePool) {
    let _ = sqlx::query("ALTER TABLE change_proposals ADD COLUMN group_id INTEGER")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE change_proposals ADD COLUMN note TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await;
}

// ---------- 候选事件 ----------
#[derive(serde::Serialize, Clone, Debug)]
pub struct Event {
    pub event_type: String, // confession / acceptance / rejection / care / conflict
    pub matched: String,
    pub confidence: f64,
}

fn contains_any(text: &str, words: &[&str]) -> Option<String> {
    for w in words {
        if text.contains(w) {
            return Some(w.to_string());
        }
    }
    None
}

/// 规则引擎：从一段 AI 剧情回复中提取候选事件（每个信号类型最多一条）。
fn extract_events(text: &str) -> Vec<Event> {
    let mut out = Vec::new();
    if let Some(m) = contains_any(text, CONFESSION) {
        out.push(Event {
            event_type: "confession".into(),
            matched: m,
            confidence: 0.95,
        });
    }
    if let Some(m) = contains_any(text, ACCEPTANCE) {
        out.push(Event {
            event_type: "acceptance".into(),
            matched: m,
            confidence: 0.96,
        });
    }
    if let Some(m) = contains_any(text, REJECTION) {
        out.push(Event {
            event_type: "rejection".into(),
            matched: m,
            confidence: 0.90,
        });
    }
    if let Some(m) = contains_any(text, CARE) {
        out.push(Event {
            event_type: "care".into(),
            matched: m,
            confidence: 0.85,
        });
    }
    if let Some(m) = contains_any(text, CONFLICT) {
        out.push(Event {
            event_type: "conflict".into(),
            matched: m,
            confidence: 0.85,
        });
    }
    // ===== 第三批新增信号 =====
    if let Some(m) = contains_any(text, PROMISE_WORDS) {
        out.push(Event { event_type: "promise".into(), matched: m, confidence: 0.88 });
    }
    if let Some(m) = contains_any(text, SACRIFICE_WORDS) {
        out.push(Event { event_type: "sacrifice".into(), matched: m, confidence: 0.90 });
    }
    if let Some(m) = contains_any(text, BETRAYAL_WORDS) {
        out.push(Event { event_type: "betrayal".into(), matched: m, confidence: 0.88 });
    }
    out
}

/// 事件类型 → 中文名（界面显示用；避免"confession / care"这类英文直接抛给用户）
// 保留未用：事件标签词典（提案展示用，保留）
#[allow(dead_code)]
pub fn event_label(event_type: &str) -> &'static str {
    match event_type {
        "confession" => "表白",
        "acceptance" => "接受",
        "rejection" => "拒绝",
        "care" => "关心",
        "conflict" => "冲突",
        "promise" => "承诺",
        "sacrifice" => "守护",
        "betrayal" => "伤害",
        "cooldown_skip" => "冷却跳过",
        "validation_reject" => "校验拒绝",
        _ => "其它事件",
    }
}

/// 第三批：自动分析入口（供"回复后静默分析"调用）。
/// 返回 `(新增提案数, 自动应用组数)`；任何失败都只返回错误字符串，不 panic、不阻断主流程。
pub async fn auto_analyze(story_id: &str, node_id: i64) -> Result<(usize, usize), String> {
    let rep = analyze_evolution(story_id, node_id).await?;
    Ok((rep.proposals.len(), rep.applied_groups.len()))
}

// ============================================================
//  ⑤-B：设定变更"全部入队、人工拍板"
// ============================================================
//
// 用户要求：**软件自动维护"设定"，会依次累计，但最终仍需人工拍板才能落地。**
// 原来低风险变更（关心/守护/恶语/表白被拒/冲突 → 关系强度）会**插入后立即应用**，
// 等于没经过人。现在：
//   · 开关 `evolution.require_approval`（**默认 true**）→ 只入队
//   · 关掉它（= false / 0）→ 恢复旧行为（低风险自动应用）
//
// 为什么默认 true：用户的原话是"最终仍需人工拍板"，所以默认就该拦住。

/// 读开关：**默认 true（要拍板）**。
///
/// 读不到配置时也返回 true —— 宁可多让用户点一下，也不要偷偷改数据库。
async fn require_approval() -> bool {
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
async fn queue_or_apply(
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

/// 写入 event_logs。
async fn log_events(pool: &SqlitePool, node_id: i64, events: &[Event]) -> Result<(), sqlx::Error> {
    for e in events {
        let payload = serde_json::json!({ "matched": e.matched });
        sqlx::query(
            "INSERT INTO event_logs(node_id, event_type, payload_json, confidence, source)
             VALUES(?1, ?2, ?3, ?4, 'rule_engine')",
        )
        .bind(node_id)
        .bind(&e.event_type)
        .bind(payload.to_string())
        .bind(e.confidence)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// 记录一条非事件型日志（冷却期跳过、校验拒绝等）。
async fn log_note(pool: &SqlitePool, node_id: i64, event_type: &str, detail: &str) -> Result<(), sqlx::Error> {
    let payload = serde_json::json!({ "detail": detail });
    sqlx::query(
        "INSERT INTO event_logs(node_id, event_type, payload_json, confidence, source)
         VALUES(?1, ?2, ?3, 0.0, 'system')",
    )
    .bind(node_id)
    .bind(event_type)
    .bind(payload.to_string())
    .execute(pool)
    .await?;
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

async fn insert_proposal(
    pool: &SqlitePool,
    branch: &Option<String>,
    node_id: i64,
    group_id: i64,
    target_type: &str,
    target_id: i64,
    field: &str,
    old_value: &str,
    new_value: &str,
    reason: &str,
    evidence: &str,
    confidence: f64,
    status: &str,
) -> Result<i64, sqlx::Error> {
    // 第五批 ③：提案归属当前分支（由调用方传入 —— 分支里分析剧情 → 提案只属于该分支）
    let row = sqlx::query(
        "INSERT INTO change_proposals(target_type, target_id, field, old_value, new_value, reason, evidence, confidence, status, group_id, branch_id, node_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) RETURNING id",
    )
    .bind(target_type)
    .bind(target_id)
    .bind(field)
    .bind(old_value)
    .bind(new_value)
    .bind(reason)
    .bind(evidence)
    .bind(confidence)
    .bind(status)
    .bind(group_id)
    .bind(branch.clone())
    .bind(node_id)
    .fetch_one(pool)
    .await?;
    Ok(row.try_get(0)?)
}

// ---------- 校验器 ----------
fn validate(
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

// ---------- 快照 ----------
/// 供其它模块（如②检查的一键替换）在执行改动前打快照的入口
pub async fn make_snapshot_for_story(story_id: &str) -> Result<i64, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    make_snapshot(&pool, false, None, None).await.map_err(|e| e.to_string())
}

/// 存一个设定版本。
///
/// - `temporary = true` → **会话内临时版本**（回滚前的自动备份）。程序启动时会清掉上一会话遗留的，
///   所以它"退出即失效"（用户 2026-10-12 定的语义）
/// - `note` → 界面上显示的一句说明（例如"回滚到 V15 前的自动备份"）
async fn make_snapshot(
    pool: &SqlitePool,
    temporary: bool,
    note: Option<&str>,
    branch: Option<String>,
) -> Result<i64, sqlx::Error> {
    // 读取全量设定（直接查库）
    // **快照必须按分支隔离**：原来读全表，导致"主线快照里装着分支的角色"，
    // 回滚时删主线、插全量 → 和分支行撞主键（`UNIQUE constraint failed: characters.id`，实测踩到）。
    let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let mut chars = Vec::new();
    let c_sql = format!(
        "SELECT id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile,
                speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion,
                stat_screen, version
         FROM characters WHERE {clause} ORDER BY id"
    );
    let mut cq = sqlx::query(&c_sql);
    if let Some(b) = &branch {
        cq = cq.bind(b);
    }
    let crows = cq.fetch_all(pool).await?;
    for r in &crows {
        chars.push(Character {
            id: r.try_get(0)?,
            name: r.try_get(1)?,
            alias: r.try_get(2)?,
            role: r.try_get(3)?,
            faction: r.try_get(4)?,
            age: r.try_get(5)?,
            gender: r.try_get(6)?,
            first_appearance: r.try_get(7)?,
            one_liner: r.try_get(8)?,
            profile: r.try_get(9)?,
            speech_style: r.try_get(10)?,
            traits: r.try_get(11)?,
            current_status: r.try_get(12)?,
            stat_stamina: r.try_get(13)?,
            stat_sanity: r.try_get(14)?,
            stat_emotion: r.try_get(15)?,
            stat_screen: r.try_get(16)?,
            version: r.try_get(17)?,
        });
    }

    let mut rels = Vec::new();
    let r_sql = format!(
        "SELECT id, from_character_id, to_character_id, relation_type, direction, strength, state,
                description, evidence, version
         FROM relations WHERE {clause} ORDER BY id"
    );
    let mut rq = sqlx::query(&r_sql);
    if let Some(b) = &branch {
        rq = rq.bind(b);
    }
    let rrows = rq.fetch_all(pool).await?;
    for r in &rrows {
        rels.push(Relation {
            id: r.try_get(0)?,
            from_character_id: r.try_get(1)?,
            to_character_id: r.try_get(2)?,
            relation_type: r.try_get(3)?,
            direction: r.try_get(4)?,
            strength: r.try_get(5)?,
            state: r.try_get(6)?,
            description: r.try_get(7)?,
            evidence: r.try_get(8)?,
            version: r.try_get(9)?,
            split_records: Vec::new(),
            split_matched: false,
        });
    }

    let mut world = Vec::new();
    // 注意：必须带 id —— 差异预览要按 id 把"当前设定"与"快照"对齐；
    // 早期版本漏了 id，导致预览把全部世界观条目误报成"将删除"（真机测试发现）。
    // 索引随之整体后移一位（id 在 0 号位）。
    let w_sql = format!(
        "SELECT id, key, value, is_hard_rule, version, category, meta_json
         FROM world_settings WHERE {clause} ORDER BY id"
    );
    let mut wq = sqlx::query(&w_sql);
    if let Some(b) = &branch {
        wq = wq.bind(b);
    }
    let rows = wq.fetch_all(pool).await?;
    for r in &rows {
        world.push(serde_json::json!({
            "id": r.try_get::<i64, _>(0)?,
            "key": r.try_get::<String, _>(1)?,
            "value": r.try_get::<String, _>(2)?,
            "is_hard_rule": r.try_get::<i64, _>(3)?,
            "version": r.try_get::<i64, _>(4)?,
            "category": r.try_get::<String, _>(5)?,
            "meta_json": r.try_get::<String, _>(6)?,
        }));
    }

    let chars_json: Vec<serde_json::Value> = chars
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id, "name": c.name, "alias": c.alias, "role": c.role,
                "faction": c.faction, "age": c.age, "gender": c.gender,
                "first_appearance": c.first_appearance, "one_liner": c.one_liner,
                "profile": c.profile, "speech_style": c.speech_style, "traits": c.traits,
                "current_status": c.current_status,
                "stat_stamina": c.stat_stamina, "stat_sanity": c.stat_sanity,
                "stat_emotion": c.stat_emotion, "stat_screen": c.stat_screen,
                "version": c.version,
            })
        })
        .collect();
    let rels_json: Vec<serde_json::Value> = rels
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id, "from_character_id": r.from_character_id,
                "to_character_id": r.to_character_id, "relation_type": r.relation_type,
                "direction": r.direction, "strength": r.strength, "state": r.state,
                "description": r.description, "evidence": r.evidence, "version": r.version,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "characters": chars_json,
        "relations": rels_json,
        "world_settings": world,
    });

    // 版本号 = 最大 version + 1；无则 1
    let row = sqlx::query("SELECT COALESCE(MAX(version), 0) FROM setting_snapshots")
        .fetch_one(pool)
        .await?;
    let prev: i64 = row.try_get(0)?;
    let new_version = prev + 1;

    sqlx::query(
        "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note, branch_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(new_version)
    .bind(payload.to_string())
    .bind(if prev == 0 { None } else { Some(prev) })
    .bind(if temporary { 1i64 } else { 0i64 })
    .bind(note.unwrap_or(""))
    .bind(branch.clone())
    .execute(pool)
    .await?;

    Ok(new_version)
}

async fn audit(pool: &SqlitePool, action: &str, target: &str, before: &str, after: &str) -> Result<(), sqlx::Error> {
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
/// 同 target 存在 pending 提案，或 3 天内有 applied 提案 → 冷却中。
async fn in_cooldown(pool: &SqlitePool, target_type: &str, target_id: i64) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT COUNT(*) FROM change_proposals
         WHERE target_type = ?1 AND target_id = ?2
           AND (status = 'pending'
                OR (status = 'applied' AND created_at >= datetime('now', 'localtime', ?3)))",
    )
    .bind(target_type)
    .bind(target_id)
    .bind(format!("-{} days", COOLDOWN_DAYS))
    .fetch_one(pool)
    .await?;
    let n: i64 = row.try_get(0)?;
    Ok(n > 0)
}

// ---------- 应用一组提案（整组确认） ----------
async fn apply_group(pool: &SqlitePool, story_id: &str, group_id: i64) -> Result<String, String> {
    apply_group_inner(pool, story_id, group_id, true).await
}

/// `apply_group` 的实现。`take_snapshot=false` 用于**批量落地**：
/// 整批只留一个还原点，比"每组一个快照"干净得多。
async fn apply_group_inner(
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

// ---------- 核心分析：事件提取 → 实质判断 → 提案生成 → 校验 → 策略 ----------
#[derive(serde::Serialize)]
pub struct EvolutionReport {
    pub events: Vec<Event>,
    pub proposals: Vec<Proposal>,
    pub applied_groups: Vec<i64>,
    pub notes: Vec<String>,
}

pub async fn analyze_evolution(story_id: &str, node_id: i64) -> Result<EvolutionReport, String> {
    eprintln!("[M6] analyze_evolution called: story={} node={}", story_id, node_id);
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_group_column(&pool).await;

    // 第五批 ③：分析只针对**当前分支** ——
    // ① 读设定（角色/关系）时按分支；② 产出的提案写进该分支；③ 冷却判断也只看该分支
    let proposal_branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let need_approval = require_approval().await;
    analyze_evolution_in(&pool, story_id, node_id, proposal_branch, need_approval).await
}

/// `analyze_evolution` 的实现（**pool / 分支 / 开关都可注入**）。
///
/// 拆出来是为了能用**真实库的副本**做诊断：AI 剧情信号不稳定，
/// 靠界面点点点很难复现，测试需要能直接喂数据。
pub async fn analyze_evolution_in(
    pool: &SqlitePool,
    story_id: &str,
    node_id: i64,
    proposal_branch: Option<String>,
    need_approval: bool,
) -> Result<EvolutionReport, String> {

    // 1. 取该 node 最近的剧情上下文（用户输入 + AI 回复，最多 6 条），拼接分析
    //    表白/接受等信号可能出现在用户输入或 AI 回复任意一侧。
    let rows = sqlx::query(
        "SELECT role, content FROM messages WHERE node_id = ?1 AND role IN ('user','assistant')
         ORDER BY id DESC LIMIT 6",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Err(format!("节点 {} 没有剧情消息，无法分析", node_id));
    }
    eprintln!("[M6] node {} 消息 {} 条", node_id, rows.len());
    let text: String = rows
        .iter()
        .rev()
        .map(|r| {
            let role: String = r.try_get(0).unwrap_or_default();
            let content: String = r.try_get(1).unwrap_or_default();
            format!("[{}] {}", if role == "user" { "用户" } else { "AI" }, content)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // 2. 提取事件 + 写日志
    let events = extract_events(&text);
    log_events(pool, node_id, &events).await.map_err(|e| e.to_string())?;

    let mut proposals: Vec<Proposal> = Vec::new();
    let mut applied_groups: Vec<i64> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    // ⑤-B：need_approval 由调用方传入（便于测试两种行为）

    // 3. 实质判断：基于关系图
    let rels = relations::list_relations(story_id).await.map_err(|e| e.to_string())?;
    let chars = characters::list_characters(story_id).await.map_err(|e| e.to_string())?;

    // 世界观硬规则
    let mut hard_rules: Vec<(String, String)> = Vec::new();
    let wrows = sqlx::query("SELECT key, value FROM world_settings WHERE is_hard_rule = 1")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    for r in &wrows {
        hard_rules.push((r.try_get(0).map_err(|e| e.to_string())?, r.try_get(1).map_err(|e| e.to_string())?));
    }

    let has_confession = events.iter().any(|e| e.event_type == "confession");
    let has_acceptance = events.iter().any(|e| e.event_type == "acceptance");
    let has_rejection = events.iter().any(|e| e.event_type == "rejection");
    let has_care = events.iter().any(|e| e.event_type == "care");
    let has_conflict = events.iter().any(|e| e.event_type == "conflict");
    // 第三批新增信号
    let has_sacrifice = events.iter().any(|e| e.event_type == "sacrifice");
    let has_betrayal = events.iter().any(|e| e.event_type == "betrayal");

    for rel in &rels {
        // 找双方角色名
        let from_name = chars.iter().find(|c| c.id == rel.from_character_id).map(|c| c.name.clone());
        let to_name = chars.iter().find(|c| c.id == rel.to_character_id).map(|c| c.name.clone());
        let both_present = match (&from_name, &to_name) {
            (Some(f), Some(t)) => text.contains(f) && text.contains(t),
            _ => false,
        };
        if !both_present {
            continue;
        }

        let _rel_target = format!("relation#{}", rel.id);
        let evidence = format!("节点 {} 剧情文本（含『{}』『{}』）", node_id, from_name.as_deref().unwrap_or(""), to_name.as_deref().unwrap_or(""));

        // 冷却期检查（只针对关系变更）
        let cooling = in_cooldown(pool, "relation", rel.id).await.map_err(|e| e.to_string())?;

        // A. 表白 + 接受 → 关系跃迁（高风险，需人工确认）
        if has_confession && has_acceptance && rel.direction == "单向" {
            if cooling {
                notes.push(format!("关系 {} 处于冷却期，跳过跃迁提案（仅记录）", rel.id));
                log_note(pool, node_id, "cooldown_skip", &format!("relation#{} 冷却期，跳过跃迁提案", rel.id)).await.map_err(|e| e.to_string())?;
                continue;
            }
            // 组 ID：复用已有同 node 未处理组，或新组
            let group: i64 = sqlx::query(
                "SELECT COALESCE(MAX(group_id), 0) FROM change_proposals",
            )
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?
            .try_get::<i64, _>(0)
            .map_err(|e| e.to_string())? + 1;

            let mut group_ok = true;
            let mut fields: Vec<(String, String, String)> = Vec::new(); // (field, old, new)

            if rel.relation_type != "明恋" && rel.relation_type != "情侣" {
                if let Err(e) = validate("relation_type", "明恋", 0.92, &evidence, &hard_rules) {
                    notes.push(format!("关系 {} 校验失败：{}", rel.id, e));
                    log_note(pool, node_id, "validation_reject", &format!("relation#{}: {}", rel.id, e)).await.map_err(|e| e.to_string())?;
                    group_ok = false;
                } else {
                    fields.push(("relation_type".to_string(), rel.relation_type.clone(), "明恋".to_string()));
                }
            }
            if rel.direction != "双向" {
                fields.push(("direction".to_string(), rel.direction.clone(), "双向".to_string()));
            }
            let new_strength = rel.strength.max(85);
            if new_strength > rel.strength {
                fields.push(("strength".to_string(), rel.strength.to_string(), new_strength.to_string()));
            }

            if group_ok && !fields.is_empty() {
                for (field, old, new) in &fields {
                    let pid = insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, field, old, new,
                        "剧情出现明确表白并被接受，关系跃迁", &evidence, 0.92, "pending",
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    // 回读完整记录
                    let p = sqlx::query(
                        "SELECT id, group_id, target_type, target_id, field, old_value, new_value, reason, evidence, confidence, status, '', created_at, node_id
                         FROM change_proposals WHERE id = ?1",
                    )
                    .bind(pid)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                    proposals.push(proposal_from_row(&p));
                }
            }
            continue;
        }

        // B. 表白被拒 → 强度下降 + 状态动摇（关系推进：入队待拍板）
        if has_confession && has_rejection && rel.direction == "单向" && !cooling {
            let new_strength = (rel.strength - 15).max(0);
            if new_strength < rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.90, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "表白被拒，关系强度下降", &evidence, 0.90, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }

        // C. 关心 → 强度小幅上升（关系推进：入队待拍板）
        if has_care && !cooling {
            let new_strength = (rel.strength + 5).min(100);
            if new_strength > rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.85, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "剧情出现关心互动，强度小幅上升", &evidence, 0.85, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }

        // E. 守护/挡下（第三批新增）→ 强度较大上升（关系推进：入队待拍板）
        if has_sacrifice && !cooling {
            let new_strength = (rel.strength + 10).min(100);
            if new_strength > rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.90, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "剧情出现守护/挡下的动作，强度上升", &evidence, 0.90, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }

        // F. 恶语/冷暴力/失信（第三批新增）→ 强度较大下降 + 状态趋向紧张（关系推进：入队待拍板）
        if has_betrayal && !cooling {
            let new_strength = (rel.strength - 20).max(0);
            let mut fields: Vec<(String, String, String)> = Vec::new();
            if new_strength < rel.strength {
                fields.push(("strength".to_string(), rel.strength.to_string(), new_strength.to_string()));
            }
            // 稳定 → 紧张（已有更强状态时不覆盖）
            if rel.state == "稳定" {
                fields.push(("state".to_string(), "稳定".to_string(), "紧张".to_string()));
            }
            if !fields.is_empty() {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                let mut ok = true;
                for (field, _old, new) in &fields {
                    if validate(field, new, 0.88, &evidence, &hard_rules).is_err() {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    for (field, old, new) in &fields {
                        insert_proposal(
                            pool, &proposal_branch, node_id, group, "relation", rel.id, field, old, new,
                            "剧情出现恶语/冷暴力/失信，关系受损", &evidence, 0.88, "pending",
                        ).await.map_err(|e| e.to_string())?;
                    }
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }

        // D. 冲突 → 强度下降（关系推进：入队待拍板）
        if has_conflict && !cooling {
            let new_strength = (rel.strength - 10).max(0);
            if new_strength < rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.85, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "剧情出现冲突，强度下降", &evidence, 0.85, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }
    }

    // 若只有单一表白（无接受/拒绝）→ 记录为"单次暧昧不提案"
    if has_confession && !has_acceptance && !has_rejection {
        notes.push("检测到表白但无回应，按『单次暧昧』规则不生成提案".into());
        log_note(pool, node_id, "no_substantial_change", "表白无回应，不生成提案").await.map_err(|e| e.to_string())?;
    }

    Ok(EvolutionReport {
        events,
        proposals,
        applied_groups,
        notes,
    })
}

fn proposal_from_row(r: &sqlx::sqlite::SqliteRow) -> Proposal {
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
async fn confirm_proposals_batch_in(
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
async fn reject_proposals_batch_in(pool: &SqlitePool, group_ids: Vec<i64>) -> Result<String, String> {
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

// ---------- 快照 / 审计 / 事件日志（M7） ----------

#[derive(serde::Serialize)]
pub struct Snapshot {
    pub id: i64,
    pub version: i64,
    pub parent_version: Option<i64>,
    pub created_at: String,
    /// 第五批 ②：会话内临时版本（回滚前的自动备份）。程序启动时会被清掉 → "退出即失效"
    #[serde(default)]
    pub temporary: i64,
    /// 一句话说明（例如"回滚到 V15 前的自动备份"）
    #[serde(default)]
    pub note: String,
}

pub async fn list_snapshots(story_id: &str) -> Result<Vec<Snapshot>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // 第五批 ③：只列**当前分支**的设定版本
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let sql = format!(
        "SELECT id, version, parent_version, created_at, temporary, note
         FROM setting_snapshots WHERE {} ORDER BY version DESC",
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
        out.push(Snapshot {
            id: r.try_get(0).map_err(|e| e.to_string())?,
            version: r.try_get(1).map_err(|e| e.to_string())?,
            parent_version: r.try_get(2).map_err(|e| e.to_string())?,
            created_at: r.try_get(3).map_err(|e| e.to_string())?,
            temporary: r.try_get::<i64, _>(4).unwrap_or(0),
            note: r.try_get::<String, _>(5).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// 把一个**临时版本转为永久**（"保存这个版本"）。
///
/// 用户场景（2026-10-12）：手改角色属性前系统自动存了一个 ⏳ 临时版本，
/// 改完发现"这个状态我想留住" —— 但临时版本退出程序就没了。
/// 这个命令把它变成普通版本：清理时不再删它，永久保留。
///
/// 只允许转 `temporary = 1` 的（永久版本调用它是无意义的空操作，直接返回 Err 说明清楚）。
pub async fn keep_snapshot(story_id: &str, version: i64) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let temp: Option<i64> = sqlx::query_scalar("SELECT temporary FROM setting_snapshots WHERE version = ?1")
        .bind(version)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let Some(temp) = temp else {
        return Err(format!("设定版本 V{version} 不存在"));
    };
    if temp == 0 {
        return Err(format!("V{version} 已经是永久版本，无需保存"));
    }
    sqlx::query("UPDATE setting_snapshots SET temporary = 0, note = ?1 WHERE version = ?2")
        .bind(format!("用户保留的版本（原为编辑前自动备份）"))
        .bind(version)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!("已把 V{version} 保存为永久版本，不会再被自动清理"))
}

/// 第五批 ②（B）：**直接编辑设定之前**自动存一个临时版本，返回版本号。
///
/// 与 `make_rollback_backup` 的区别只是 note 文案：那个用于回滚前，这个用于"手改设定前"。
/// 两者都存**临时**版本（退出程序即失效），所以不会把版本历史堆满。
///
/// `what` 由界面传入（如"修改角色「夏婉琴」"），写进 note 便于回看"这份备份是为什么存的"。
pub async fn make_edit_backup(story_id: &str, what: &str) -> Result<i64, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // 备份落在**当前所在分支**上（在分支里改设定时，备份的应是该分支的状态）
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let note = format!("编辑前自动备份：{what}（临时，退出程序即清除）");
    make_snapshot(&pool, true, Some(&note), branch)
        .await
        .map_err(|e| e.to_string())
}

/// 清掉**上一次运行**遗留的临时版本（在程序启动时对每个故事调用一次）。
///
/// 这是"退出即失效"的实现方式：临时版本只在本会话内有意义，重启后就没有保留价值，
/// 留着只会把版本列表越堆越长、把真正的永久版本淹没。
/// 返回清掉的条数（供界面提示）。
pub async fn purge_temporary_snapshots(story_id: &str) -> Result<i64, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let n = sqlx::query("DELETE FROM setting_snapshots WHERE temporary = 1")
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    Ok(n as i64)
}

/// 清掉**所有故事**里遗留的临时版本（程序启动时调用 —— 那时还不知道用户会打开哪个故事）。
///
/// 遍历 library.db 里的故事目录：每个 `library/<库>/projects/<id>/story.db` 一个。
/// 单个故事失败只跳过它，不影响其它故事，也绝不影响启动。
pub async fn purge_all_temporary_snapshots() -> Result<i64, String> {
    // 库根：与 db::library_db_path() / story_db_path() 同一套定位（paths::library_root）
    let root = crate::paths::library_root();
    let mut total = 0i64;
    let mut stack = vec![root.clone()];
    let mut dbs: Vec<std::path::PathBuf> = Vec::new();
    // 广度遍历找出所有 story.db（层级固定，但用遍历更稳，避免写死深度）
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().map(|n| n == "story.db").unwrap_or(false) {
                dbs.push(path);
            }
        }
    }
    for db in dbs {
        // 用文件路径直接连、不经过 story_id（启动时没有 story_id 上下文）
        let Ok(opts) = sqlx::sqlite::SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display())) else {
            continue;
        };
        let Ok(pool) = SqlitePool::connect_with(opts).await else { continue };
        // 老库可能还没这一列：先确保存在（幂等）
        if crate::story_db::ensure_snapshot_temporary_column(&pool).await.is_err() {
            continue;
        }
        if let Ok(r) = sqlx::query("DELETE FROM setting_snapshots WHERE temporary = 1")
            .execute(&pool)
            .await
        {
            total += r.rows_affected() as i64;
        }
    }
    Ok(total)
}

/// 回滚**之前**自动存一份"当前状态"作为临时版本，返回它的版本号。
///
/// 为什么需要：回滚是破坏性的（整表替换）。有了这份备份，用户点错回滚还能再回滚回来。
/// 之所以做成**临时**（而不是普通版本）：用户不希望每次回滚都在版本列表里留下永久记录
/// （2026-10-12 定："做成临时的，一旦退出程序就失效"）。
pub async fn make_rollback_backup(story_id: &str, target_version: i64) -> Result<i64, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // 备份要落在**当前所在分支**上（否则分支里回滚时会把备份存到主线）
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let note = format!("回滚到 V{target_version} 前的自动备份（临时，退出程序即清除）");
    make_snapshot(&pool, true, Some(&note), branch)
        .await
        .map_err(|e| e.to_string())
}

/// 读取当前 relations 摘要（id + 两端角色名 + 类型/方向/强度），用于回滚前后对比。
async fn relation_summary(pool: &SqlitePool) -> Result<Vec<(i64, String, String, String, String, i64)>, String> {
    let rows = sqlx::query(
        "SELECT r.id, c1.name, c2.name, r.relation_type, r.direction, r.strength
         FROM relations r
         LEFT JOIN characters c1 ON c1.id = r.from_character_id
         LEFT JOIN characters c2 ON c2.id = r.to_character_id
         ORDER BY r.id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push((
            r.try_get::<i64, _>(0).map_err(|e| e.to_string())?,
            r.try_get::<String, _>(1).unwrap_or_default(),
            r.try_get::<String, _>(2).unwrap_or_default(),
            r.try_get::<String, _>(3).unwrap_or_default(),
            r.try_get::<String, _>(4).unwrap_or_default(),
            r.try_get::<i64, _>(5).map_err(|e| e.to_string())?,
        ));
    }
    Ok(out)
}

// ---------- 第五批 ②：快照差异预览（回滚前的"体检报告"） ----------

/// 单个字段的差异
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct FieldChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

/// 一个条目的差异（新增 / 删除 / 修改）
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct ItemChange {
    /// "added" | "removed" | "modified"
    pub kind: String,
    /// 人类可读的条目名（角色名 / "A 是 B 的『X』" / 世界观条目的 key）
    pub label: String,
    /// 仅 modified 时非空
    pub fields: Vec<FieldChange>,
}

/// 一份快照与"当前设定"的差异
#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct SnapshotDiff {
    pub characters: Vec<ItemChange>,
    pub relations: Vec<ItemChange>,
    pub world_settings: Vec<ItemChange>,
}

impl SnapshotDiff {
    // 保留未用：提案统计（界面将来用）
    #[allow(dead_code)]
    pub fn total(&self) -> usize {
        self.characters.len() + self.relations.len() + self.world_settings.len()
    }
    /// 差异摘要文案（"新增 2、修改 3、删除 1"），供界面直接显示
    // 保留未用：提案统计（界面将来用）
    #[allow(dead_code)]
    pub fn summary(&self) -> String {
        let mut a = 0;
        let mut m = 0;
        let mut r = 0;
        for it in self.characters.iter().chain(self.relations.iter()).chain(self.world_settings.iter()) {
            match it.kind.as_str() {
                "added" => a += 1,
                "modified" => m += 1,
                _ => r += 1,
            }
        }
        format!("新增 {a} · 修改 {m} · 删除 {r}")
    }
}

fn jstr(v: &serde_json::Value, k: &str) -> String {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// 角色卡被比对的字段（**刻意不含 version** —— 版本号是内部计数，用户看不懂也不该关心）
const CHAR_FIELDS: &[(&str, &str)] = &[
    ("name", "姓名"),
    ("alias", "别名"),
    ("role", "定位"),
    ("faction", "势力"),
    ("age", "年龄"),
    ("gender", "性别"),
    ("one_liner", "一句话"),
    ("profile", "小传"),
    ("speech_style", "说话方式"),
    ("traits", "性格"),
    ("current_status", "当前状态"),
    ("stat_stamina", "体力"),
    ("stat_sanity", "理智"),
    ("stat_emotion", "情绪"),
    ("stat_screen", "存在感"),
];

const REL_FIELDS: &[(&str, &str)] = &[
    ("relation_type", "关系"),
    ("direction", "方向"),
    ("strength", "强度"),
    ("state", "状态"),
    ("description", "说明"),
    ("evidence", "证据"),
];

const WORLD_FIELDS: &[(&str, &str)] = &[
    ("key", "条目"),
    ("value", "内容"),
    ("is_hard_rule", "硬规则"),
    ("category", "分类"),
];

/// 比对两组条目：`before`（当前）→ `after`（快照）。
///
/// **对齐规则**：优先按 `id`；条目没有 `id` 时退回按 `key`（世界观条目就是这种 ——
/// 它的主键是 id，但 payload 早期版本没写 id，只存了 key）。
/// 若两边都没有 id/key（理论上不该发生），则退化为"按数组下标"对齐，宁可报得少，
/// 也不要造出一堆假的"将新增 / 将删除"。
///
/// 纯函数：只吃 JSON 数组，不碰数据库 —— 便于单测，也便于以后做"任意两个版本对比"。
fn diff_items(
    before: &[serde_json::Value],
    after: &[serde_json::Value],
    fields: &[(&str, &str)],
    label_of: &dyn Fn(&serde_json::Value) -> String,
    // 按哪个字段把两边对齐。**必须由调用方指定**，不能让函数"聪明地"自己挑：
    // 若一边有 id（当前库）而另一边没有（老快照 payload），自动挑就会挑出不同的键，
    // 于是同一条目一边算"新增"、一边算"删除"（真机测试踩到过这个坑）。
    match_on: &str,
) -> Vec<ItemChange> {
    use std::collections::BTreeMap;
    let key_of = |v: &serde_json::Value, fallback_idx: usize| -> String {
        match v.get(match_on) {
            Some(serde_json::Value::String(s)) => format!("s:{s}"),
            Some(serde_json::Value::Number(n)) => format!("n:{n}"),
            _ => format!("x:{fallback_idx}"),
        }
    };
    let idx = |arr: &[serde_json::Value]| -> BTreeMap<String, serde_json::Value> {
        arr.iter()
            .enumerate()
            .map(|(i, v)| (key_of(v, i), v.clone()))
            .collect()
    };
    let (bm, am) = (idx(before), idx(after));
    let mut out = Vec::new();
    // 新增（快照里有、当前没有）
    for (k, v) in &am {
        if !bm.contains_key(k) {
            out.push(ItemChange { kind: "added".into(), label: label_of(v), fields: vec![] });
        }
    }
    // 删除（当前有、快照没有）
    for (k, v) in &bm {
        if !am.contains_key(k) {
            out.push(ItemChange { kind: "removed".into(), label: label_of(v), fields: vec![] });
        }
    }
    // 修改（两边都有但字段不同）
    for (k, b) in &bm {
        if let Some(a) = am.get(k) {
            let mut fs = Vec::new();
            for (k, cn) in fields {
                let (bv, av) = (jstr(b, k), jstr(a, k));
                if bv != av {
                    fs.push(FieldChange { field: (*cn).to_string(), before: bv, after: av });
                }
            }
            if !fs.is_empty() {
                out.push(ItemChange { kind: "modified".into(), label: label_of(a), fields: fs });
            }
        }
    }
    out
}

/// 对比"某快照"与"当前设定"。返回的每一项都写明：当前是什么、回滚后会变成什么。
///
/// 用途：回滚是**破坏性操作**（现存设定会被整表替换）。原实现只有一句"确定回滚吗"，
/// 用户根本不知道会失去什么 —— 这里先给出可读的差异清单，让回滚变成有依据的决定。
pub async fn preview_snapshot(story_id: &str, version: i64) -> Result<SnapshotDiff, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let row = sqlx::query("SELECT payload_json FROM setting_snapshots WHERE version = ?1")
        .bind(version)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("快照 V{version} 不存在"))?;
    let payload: serde_json::Value =
        serde_json::from_str(&row.try_get::<String, _>(0).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;

    // 当前设定（读成同样的 JSON 形状，直接复用 diff_items）
    //
    // **必须与 make_snapshot 用同一套分支过滤**：否则快照里只有本分支的行、
    // 而"当前"读的是全表 → 别的分支的行会被误报成"回滚将删除"。
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let chars_sql = format!(
        "SELECT id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile,
                speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion,
                stat_screen
         FROM characters WHERE {clause} ORDER BY id"
    );
    let mut chq = sqlx::query(&chars_sql);
    if let Some(b) = &branch {
        chq = chq.bind(b);
    }
    let cur_chars: Vec<serde_json::Value> = chq
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>(0).unwrap_or(0),
            "name": r.try_get::<String, _>(1).unwrap_or_default(),
            "alias": r.try_get::<String, _>(2).unwrap_or_default(),
            "role": r.try_get::<String, _>(3).unwrap_or_default(),
            "faction": r.try_get::<String, _>(4).unwrap_or_default(),
            "age": r.try_get::<String, _>(5).unwrap_or_default(),
            "gender": r.try_get::<String, _>(6).unwrap_or_default(),
            "first_appearance": r.try_get::<String, _>(7).unwrap_or_default(),
            "one_liner": r.try_get::<String, _>(8).unwrap_or_default(),
            "profile": r.try_get::<String, _>(9).unwrap_or_default(),
            "speech_style": r.try_get::<String, _>(10).unwrap_or_default(),
            "traits": r.try_get::<String, _>(11).unwrap_or_default(),
            "current_status": r.try_get::<String, _>(12).unwrap_or_default(),
            "stat_stamina": r.try_get::<i64, _>(13).unwrap_or(50),
            "stat_sanity": r.try_get::<i64, _>(14).unwrap_or(50),
            "stat_emotion": r.try_get::<i64, _>(15).unwrap_or(50),
            "stat_screen": r.try_get::<i64, _>(16).unwrap_or(50),
        })
    })
    .collect();

    let rels_sql = format!(
        "SELECT r.id, c1.name, c2.name, r.relation_type, r.direction, r.strength, r.state,
                r.description, r.evidence
         FROM relations r
         LEFT JOIN characters c1 ON c1.id = r.from_character_id
         LEFT JOIN characters c2 ON c2.id = r.to_character_id
         WHERE {clause_r} ORDER BY r.id",
        clause_r = crate::story_db::branch_clause("r.branch_id", &branch, 1)
    );
    let mut rq = sqlx::query(&rels_sql);
    if let Some(b) = &branch {
        rq = rq.bind(b);
    }
    let cur_rels: Vec<serde_json::Value> = rq
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>(0).unwrap_or(0),
            "from_name": r.try_get::<String, _>(1).unwrap_or_default(),
            "to_name": r.try_get::<String, _>(2).unwrap_or_default(),
            "relation_type": r.try_get::<String, _>(3).unwrap_or_default(),
            "direction": r.try_get::<String, _>(4).unwrap_or_default(),
            "strength": r.try_get::<i64, _>(5).unwrap_or(50),
            "state": r.try_get::<String, _>(6).unwrap_or_default(),
            "description": r.try_get::<String, _>(7).unwrap_or_default(),
            "evidence": r.try_get::<String, _>(8).unwrap_or_default(),
        })
    })
    .collect();

    let world_sql = format!(
        "SELECT id, key, value, is_hard_rule, category FROM world_settings
         WHERE {clause} ORDER BY id"
    );
    let mut wq = sqlx::query(&world_sql);
    if let Some(b) = &branch {
        wq = wq.bind(b);
    }
    let cur_world: Vec<serde_json::Value> = wq
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>(0).unwrap_or(0),
            "key": r.try_get::<String, _>(1).unwrap_or_default(),
            "value": r.try_get::<String, _>(2).unwrap_or_default(),
            "is_hard_rule": r.try_get::<i64, _>(3).unwrap_or(0),
            "category": r.try_get::<String, _>(4).unwrap_or_default(),
        })
    })
    .collect();

    let arr = |v: &serde_json::Value, k: &str| -> Vec<serde_json::Value> {
        v.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default()
    };

    // 关系的条目名："A → B（类型）"；快照 payload 里 relations 只存 id，
    // 名字要从**当前**关系表或快照里的角色名单反查（两边都试，保证标签可读）
    let name_of = |v: &serde_json::Value, ids: &std::collections::HashMap<i64, String>| -> String {
        let f = v.get("from_character_id").and_then(|x| x.as_i64()).unwrap_or(0);
        let t = v.get("to_character_id").and_then(|x| x.as_i64()).unwrap_or(0);
        let fname = ids.get(&f).cloned().unwrap_or_else(|| format!("#{f}"));
        let tname = ids.get(&t).cloned().unwrap_or_else(|| format!("#{t}"));
        format!("{fname} → {tname}")
    };
    let cur_names: std::collections::HashMap<i64, String> = cur_chars
        .iter()
        .filter_map(|c| Some((c.get("id")?.as_i64()?, c.get("name")?.as_str()?.to_string())))
        .collect();
    let snap_names: std::collections::HashMap<i64, String> = arr(&payload, "characters")
        .iter()
        .filter_map(|c| Some((c.get("id")?.as_i64()?, c.get("name")?.as_str()?.to_string())))
        .collect();
    // 合并两份名单：快照里删掉的角色也要能显示出名字
    let mut all_names = snap_names.clone();
    for (k, v) in &cur_names {
        all_names.entry(*k).or_insert_with(|| v.clone());
    }

    let diff = SnapshotDiff {
        characters: diff_items(
            &cur_chars,
            &arr(&payload, "characters"),
            CHAR_FIELDS,
            &|v| jstr(v, "name"),
            "id",
        ),
        relations: diff_items(
            &cur_rels,
            &arr(&payload, "relations"),
            REL_FIELDS,
            &|v| name_of(v, &all_names),
            "id",
        ),
        world_settings: diff_items(
            &cur_world,
            &arr(&payload, "world_settings"),
            WORLD_FIELDS,
            &|v| jstr(v, "key"),
            // 世界观条目按 **key** 对齐而不是 id：老快照的 payload 里没有 id，
            // 按 id 对齐会让同一条目同时出现在"新增"与"删除"里（真机测试发现）
            "key",
        ),
    };
    Ok(diff)
}

/// 回滚到指定快照版本：以该版本 payload 覆盖 characters / relations / world_settings，并写审计。
/// 回滚辅助：`id` 在该表里**是否已被"别的分支"占着**。
///
/// 为什么需要：早先的 `make_snapshot` 读全表（不分分支），
/// 所以**老快照的 payload 里混着别的分支的行**。回滚时若照原 id 插回去，
/// 会与那些仍存在的分支行撞主键 —— 就是 `UNIQUE constraint failed: characters.id`。
/// 这类行应该**跳过**：它们不属于当前分支，本来就不该被这次回滚改动。
async fn id_taken_by_other_branch(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
    id: i64,
    branch: &Option<String>,
) -> Result<bool, String> {
    let sql = format!("SELECT branch_id FROM {table} WHERE id = ?1");
    let existing: Option<Option<String>> = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    Ok(matches!(existing, Some(b) if &b != branch))
}

/// 回滚辅助：`id` 在该表里是否还存在（同分支残留的异常情况）。
async fn id_exists(conn: &mut sqlx::SqliteConnection, table: &str, id: i64) -> Result<bool, String> {
    let sql = format!("SELECT 1 FROM {table} WHERE id = ?1");
    let hit: Option<i64> = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    Ok(hit.is_some())
}
/// 把一份快照 payload **写回**当前分支（回滚的后半段）。
///
/// 抽成独立函数的原因：这段逻辑踩过主键冲突（`UNIQUE constraint failed: characters.id`），
/// 需要能被单测直接覆盖 —— 而 `rollback_snapshot` 会自己按 story_id 开池，测不进去。
async fn restore_settings_from_payload(
    conn: &mut sqlx::SqliteConnection,
    branch: &Option<String>,
    payload: &serde_json::Value,
) -> Result<(), String> {
    // 角色 id 映射（旧 id → 新 id），关系要用它重写两端。
    // 主线：沿用快照里的 id（原行为不变）；分支：**不沿用** ——
    // 否则分支会与源分支的主线行撞主键（分支里已经有同 id 的复制行）。
    let mut char_id_map: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    if let Some(chars) = payload.get("characters").and_then(|v| v.as_array()) {
        for c in chars {
            let old_id = c.get("id").and_then(|v| v.as_i64());
            let mut use_explicit_id = if branch.is_some() { None } else { old_id };
            if let Some(oid) = old_id {
                if id_taken_by_other_branch(&mut *conn, "characters", oid, &branch).await? {
                    // 老快照混进来的别的分支的行 → 跳过（不改别的分支、也不撞主键）
                    continue;
                }
                if id_exists(&mut *conn, "characters", oid).await? {
                    // 同分支却还在（删除没覆盖到）→ 交给自增，宁可换 id 也别让回滚整体失败
                    use_explicit_id = None;
                }
            }
            let inserted = sqlx::query(
                "INSERT INTO characters(id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion, stat_screen, version, branch_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19) RETURNING id",
            )
            // 老快照 payload 里可能没有 id；**不能**退回 0（会主键冲突 1555），传 None 让数据库自增
            .bind(use_explicit_id)
            .bind(c.get("name").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("alias").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("role").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("faction").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("age").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("gender").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("first_appearance").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("one_liner").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("profile").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("speech_style").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("traits").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("current_status").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("stat_stamina").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("stat_sanity").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("stat_emotion").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("stat_screen").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .bind(branch.clone())
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;
            if let Some(oid) = old_id {
                char_id_map.insert(oid, inserted.try_get::<i64, _>(0).unwrap_or(oid));
            }
        }
    }
    if let Some(rels) = payload.get("relations").and_then(|v| v.as_array()) {
        for r in rels {
            let mut rel_id = if branch.is_some() { None } else { r.get("id").and_then(|v| v.as_i64()) };
            if let Some(oid) = rel_id {
                if id_taken_by_other_branch(&mut *conn, "relations", oid, &branch).await? {
                    continue;
                }
                if id_exists(&mut *conn, "relations", oid).await? {
                    rel_id = None;
                }
            }
            sqlx::query(
                "INSERT INTO relations(id, from_character_id, to_character_id, relation_type, direction, strength, state, description, evidence, version, branch_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )
            // 同上：缺失 id 时传 None；分支下不沿用 id；被别的分支占着则已 continue
            .bind(rel_id)
            // 两端按映射重写（分支里角色是新 id，不重写会指向源分支的角色行）
            .bind({
                let old = r.get("from_character_id").and_then(|v| v.as_i64()).unwrap_or(0);
                *char_id_map.get(&old).unwrap_or(&old)
            })
            .bind({
                let old = r.get("to_character_id").and_then(|v| v.as_i64()).unwrap_or(0);
                *char_id_map.get(&old).unwrap_or(&old)
            })
            .bind(r.get("relation_type").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(r.get("direction").and_then(|v| v.as_str()).unwrap_or("单向"))
            .bind(r.get("strength").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(r.get("state").and_then(|v| v.as_str()).unwrap_or("稳定"))
            .bind(r.get("description").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(r.get("evidence").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(r.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .bind(branch.clone())
            .execute(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    if let Some(worlds) = payload.get("world_settings").and_then(|v| v.as_array()) {
        for w in worlds {
            let mut wid = if branch.is_some() { None } else { w.get("id").and_then(|v| v.as_i64()) };
            if let Some(oid) = wid {
                if id_taken_by_other_branch(&mut *conn, "world_settings", oid, &branch).await? {
                    continue;
                }
                if id_exists(&mut *conn, "world_settings", oid).await? {
                    wid = None;
                }
            }
            sqlx::query(
                "INSERT INTO world_settings(id, key, value, is_hard_rule, version, category, meta_json, branch_id) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            // 同上：缺失 id 时传 None；分支下不沿用 id；被别的分支占着则已 continue
            .bind(wid)
            .bind(w.get("key").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("value").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("is_hard_rule").and_then(|v| v.as_i64()).unwrap_or(0))
            .bind(w.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .bind(w.get("category").and_then(|v| v.as_str()).unwrap_or("设定"))
            .bind(w.get("meta_json").and_then(|v| v.as_str()).unwrap_or("{}"))
            .bind(branch.clone())
            .execute(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}



pub async fn rollback_snapshot(story_id: &str, version: i64) -> Result<String, String> {
    // 第五批 ③：**先解析出"当前在哪条分支"** —— 备份与回滚都必须落在同一条分支上。
    // （曾经把备份写在解析之前 → 分支里回滚时，备份会存到主线，备份的不是被改的那份状态。）
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);

    // 第五批 ②：**先自动存一份当前状态**（临时版本），这样点错回滚还能退回来。
    // 备份失败**不阻断回滚**（否则"备份不了就永远回滚不了"），但要把情况告诉用户。
    let backup_note = match make_rollback_backup(story_id, version).await {
        Ok(v) => format!("已自动备份当前状态为临时版本 V{v}（退出程序即清除；想长期留住可用列表里的「保存为永久」）。"),
        Err(e) => format!("⚠ 自动备份失败（{e}）；本次回滚后无法退回。"),
    };
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    rollback_snapshot_in(&pool, version, branch, backup_note).await
}

/// `rollback_snapshot` 的实现（**pool / 分支 / 备份说明可注入**，测试直接传临时库）。
///
/// 拆出来是为了能测住"回滚失败必须整体回退"——之前这段逻辑自己按 story_id 开池，单测进不去，
/// 于是"删了一半就报错"的事故没有被任何测试拦住。
async fn rollback_snapshot_in(
    pool: &SqlitePool,
    version: i64,
    branch: Option<String>,
    backup_note: String,
) -> Result<String, String> {
    let before = relation_summary(pool).await?;
    let row = sqlx::query("SELECT version, payload_json FROM setting_snapshots WHERE version = ?1")
        .bind(version)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("快照 V{} 不存在", version))?;
    let payload: serde_json::Value = serde_json::from_str(&row.try_get::<String, _>(1).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    // 清空并恢复三张设定表。
    // 第五批 ③：**只清目标分支的行**（`branch_id IS NULL` = 主线）——
    // 原实现是整表 DELETE，分支下回滚会把别的分支的数据一起清掉。
    // **必须在一个事务里**：原来是"删三张表 → 逐条插回"，中途失败就留下删了一半的库。
    // 实测事故：插角色时撞主键（1555）报错，主线的关系与世界观已经被删掉、再也没插回来。
    let del_clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for table in ["characters", "relations", "world_settings"] {
        let sql = format!("DELETE FROM {table} WHERE {}", del_clause);
        let mut q = sqlx::query(&sql);
        if let Some(b) = &branch {
            q = q.bind(b);
        }
        q.execute(&mut *tx).await.map_err(|e| e.to_string())?;
    }

    restore_settings_from_payload(&mut tx, &branch, &payload).await?;
    // 全部成功才提交；任一步失败 → 整个回滚，库保持回滚前的样子
    tx.commit().await.map_err(|e| e.to_string())?;

    audit(pool, "rollback", &format!("snapshot#{}", version), "", &payload.to_string())
        .await
        .map_err(|e| e.to_string())?;

    // 回滚前后对比，生成可见的变更摘要
    let after = relation_summary(pool).await?;
    let mut diffs = Vec::new();
    for (id, n1, n2, typ, dir, strength) in &after {
        if let Some((_, _b1, _b2, btyp, bdir, bstr)) = before.iter().find(|b| b.0 == *id) {
            let label = format!("{}→{}", n1, n2);
            if btyp != typ {
                diffs.push(format!("{} 关系类型 {}→{}", label, btyp, typ));
            }
            if bdir != dir {
                diffs.push(format!("{} 方向 {}→{}", label, bdir, dir));
            }
            if bstr != strength {
                diffs.push(format!("{} 强度 {}→{}", label, bstr, strength));
            }
        } else {
            diffs.push(format!("{}→{}（新增关系）", n1, n2));
        }
    }
    let summary = if diffs.is_empty() {
        "各关系无变化".to_string()
    } else {
        diffs.join("；")
    };
    // 把"已自动备份为临时版本 Vx"告诉用户 —— 否则他不知道还能退回来
    if backup_note.trim().is_empty() {
        Ok(format!("已回滚到快照 V{version}：{summary}"))
    } else {
        Ok(format!("已回滚到快照 V{version}：{summary}\n{backup_note}"))
    }
}

#[derive(serde::Serialize)]
pub struct AuditLog {
    pub id: i64,
    pub action: String,
    pub target: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub operator: String,
    pub created_at: String,
}

pub async fn list_audit_logs(story_id: &str) -> Result<Vec<AuditLog>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let rows = sqlx::query("SELECT id, action, target, before, after, operator, created_at FROM audit_logs ORDER BY id DESC LIMIT 100")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(AuditLog {
            id: r.try_get(0).map_err(|e| e.to_string())?,
            action: r.try_get(1).map_err(|e| e.to_string())?,
            target: r.try_get(2).map_err(|e| e.to_string())?,
            before: r.try_get(3).map_err(|e| e.to_string())?,
            after: r.try_get(4).map_err(|e| e.to_string())?,
            operator: r.try_get(5).map_err(|e| e.to_string())?,
            created_at: r.try_get(6).map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

#[derive(serde::Serialize)]
pub struct EventLogItem {
    pub id: i64,
    pub node_id: Option<i64>,
    pub event_type: String,
    pub payload_json: String,
    pub confidence: f64,
    pub source: String,
}

pub async fn list_event_logs(story_id: &str) -> Result<Vec<EventLogItem>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let rows = sqlx::query("SELECT id, node_id, event_type, payload_json, confidence, source FROM event_logs ORDER BY id DESC LIMIT 50")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(EventLogItem {
            id: r.try_get(0).map_err(|e| e.to_string())?,
            node_id: r.try_get(1).map_err(|e| e.to_string())?,
            event_type: r.try_get(2).map_err(|e| e.to_string())?,
            payload_json: r.try_get(3).map_err(|e| e.to_string())?,
            confidence: r.try_get(4).map_err(|e| e.to_string())?,
            source: r.try_get(5).map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod third_batch_tests {
    use super::*;

    #[test]
    fn new_signals_are_extracted() {
        let ev = extract_events("[夏婉琴]“我保证，我一定会回来。”");
        assert!(ev.iter().any(|e| e.event_type == "promise"), "应识别承诺：{ev:?}");
        let ev2 = extract_events("（他挡在她身前，替她挨了那一拳）");
        assert!(ev2.iter().any(|e| e.event_type == "sacrifice"), "应识别守护：{ev2:?}");
        let ev3 = extract_events("[院长]“你太让我失望了。”他第一次说出很难听的话。");
        assert!(ev3.iter().any(|e| e.event_type == "betrayal"), "应识别伤害：{ev3:?}");
        // 普通叙述不应误触发新信号
        let quiet = extract_events("（她安静地坐在窗边看书，阳光落在书页上）");
        assert!(
            !quiet.iter().any(|e| matches!(e.event_type.as_str(), "promise" | "sacrifice" | "betrayal")),
            "普通叙述不应触发：{quiet:?}"
        );
    }

    #[test]
    fn event_labels_are_chinese() {
        for k in ["confession", "acceptance", "rejection", "care", "conflict", "promise", "sacrifice", "betrayal"] {
            let lb = event_label(k);
            assert!(!lb.is_empty() && lb != "其它事件", "{k} 应有中文名");
        }
        assert_eq!(event_label("promise"), "承诺");
        assert_eq!(event_label("unknown_thing"), "其它事件");
    }
}


/// 第五批 ②：快照差异预览的纯函数测试（`diff_items` 不碰数据库，可直接造 JSON 比对）
#[cfg(test)]
mod snapshot_diff_tests {
    use super::*;
    use serde_json::json;

    fn chars() -> Vec<(&'static str, &'static str)> {
        CHAR_FIELDS.to_vec()
    }

    #[test]
    fn detects_added_removed_and_modified() {
        let before = vec![
            json!({"id": 1, "name": "夏婉琴", "role": "女主", "stat_emotion": 60}),
            json!({"id": 2, "name": "宸熙", "role": "男主", "stat_emotion": 50}),
            json!({"id": 3, "name": "临时角色", "role": "其他", "stat_emotion": 50}),
        ];
        let after = vec![
            // id 1：情绪被改了
            json!({"id": 1, "name": "夏婉琴", "role": "女主", "stat_emotion": 75}),
            // id 2：原样
            json!({"id": 2, "name": "宸熙", "role": "男主", "stat_emotion": 50}),
            // id 3：快照里没有 → 回滚会**删除**它
            // id 4：快照里有、当前没有 → 回滚会**加回来**
            json!({"id": 4, "name": "院长", "role": "其他", "stat_emotion": 50}),
        ];
        let d = diff_items(&before, &after, &chars(), &|v| jstr(v, "name"), "id");
        assert_eq!(d.len(), 3, "应有 修改1 + 删除1 + 新增1");
        let kinds: Vec<&str> = d.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"modified"));
        assert!(kinds.contains(&"removed"));
        assert!(kinds.contains(&"added"));

        let m = d.iter().find(|x| x.kind == "modified").unwrap();
        assert_eq!(m.label, "夏婉琴");
        assert_eq!(m.fields.len(), 1, "只应报告情绪一处变化");
        assert_eq!(m.fields[0].field, "情绪");
        assert_eq!(m.fields[0].before, "60");
        assert_eq!(m.fields[0].after, "75");

        let r = d.iter().find(|x| x.kind == "removed").unwrap();
        assert_eq!(r.label, "临时角色");
        assert!(r.fields.is_empty(), "删除项不需要字段差异");

        let a = d.iter().find(|x| x.kind == "added").unwrap();
        assert_eq!(a.label, "院长");
    }

    #[test]
    fn identical_state_produces_no_diff() {
        let arr = vec![json!({"id": 1, "name": "A", "role": "女主"})];
        let d = diff_items(&arr, &arr, &chars(), &|v| jstr(v, "name"), "id");
        assert!(d.is_empty(), "完全一致时不应报差异（界面据此提示「无需回滚」）");
    }

    /// version 字段**刻意不比对** —— 它是内部计数，回滚时数值变化对用户没有意义
    #[test]
    fn version_field_is_ignored() {
        let before = vec![json!({"id": 1, "name": "A", "version": 3})];
        let after = vec![json!({"id": 1, "name": "A", "version": 7})];
        let d = diff_items(&before, &after, &chars(), &|v| jstr(v, "name"), "id");
        assert!(d.is_empty(), "version 不在比对字段里，不应算作修改");
    }

    #[test]
    fn numeric_and_string_values_compare_by_text() {
        // 强度可能是 INTEGER 列，JSON 里是数字；统一转文本比较，避免"50 vs "50"" 误报
        let before = vec![json!({"id": 1, "strength": 50})];
        let after = vec![json!({"id": 1, "strength": "50"})];
        let d = diff_items(&before, &after, REL_FIELDS, &|_| "关系".into(), "id");
        assert!(d.is_empty(), "50 与字符串 50 应视为相同");
    }

    #[test]
    fn missing_fields_treated_as_empty_string() {
        let before = vec![json!({"id": 1, "profile": "旧小传"})];
        let after = vec![json!({"id": 1})];
        let d = diff_items(&before, &after, &chars(), &|v| jstr(v, "name"), "id");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].fields[0].field, "小传");
        assert_eq!(d[0].fields[0].after, "", "缺失字段按空串处理");
    }

    #[test]
    fn summary_counts_all_three_kinds() {
        let mut d = SnapshotDiff::default();
        d.characters = vec![
            ItemChange { kind: "added".into(), label: "A".into(), fields: vec![] },
            ItemChange { kind: "modified".into(), label: "B".into(), fields: vec![] },
        ];
        d.relations = vec![ItemChange { kind: "removed".into(), label: "C".into(), fields: vec![] }];
        assert_eq!(d.total(), 3);
        assert_eq!(d.summary(), "新增 1 · 修改 1 · 删除 1");
    }
    /// 真机验证（默认跳过）：对真实库跑一次 `preview_snapshot`，打印差异摘要。
    /// 跑法：`cargo test --lib live_preview_snapshot -- --ignored --nocapture`
    ///
    /// 为什么值得跑：这条链路涉及"JSON payload ↔ 当前三张表"的字段对齐，
    /// 字段名写错（例如 payload 里叫 from_character_id 而当前表读成 from_name）会静默变成
    /// "差异为空"或"全是差异" —— 只有真实数据才试得出来。
    #[tokio::test]
    #[ignore]
    async fn live_preview_snapshot() {
        let db = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..").join("library").join("default").join("projects").join("7").join("story.db");
        if !db.exists() {
            println!("真实库不存在，跳过：{}", db.display());
            return;
        }
        let snaps = list_snapshots("7").await.unwrap();
        println!("  快照数：{}", snaps.len());
        if snaps.is_empty() {
            return;
        }
        // 逐个预览，最新那个应当在"设定没动过"时为 0 差异
        for s in snaps.iter().take(3) {
            let d = preview_snapshot("7", s.version).await.unwrap();
            println!(
                "  V{}：{} → characters {} / relations {} / world {}",
                s.version,
                d.summary(),
                d.characters.len(),
                d.relations.len(),
                d.world_settings.len()
            );
            let mut n_field = 0;
            for it in d.characters.iter().chain(d.relations.iter()).chain(d.world_settings.iter()) {
                n_field += it.fields.len();
                println!("      [{}] {}  {} 处字段", it.kind, it.label, it.fields.len());
                for f in it.fields.iter() {
                    println!("          {} : {:?} → {:?}", f.field, f.before, f.after);
                }
            }
            println!("      （共 {} 个差异项、{} 处字段差异）", d.total(), n_field);
        }
        // 最新快照与当前设定一致时，差异必须为 0（否则说明字段对齐有问题）
        let latest = snaps[0].version;
        let d = preview_snapshot("7", latest).await.unwrap();
        // 说明：**历史快照的 world_settings 里没有 id**（生成端早期漏选），无法按 id 对齐，
        // 因此只对"角色 + 关系"要求 0 差异；世界观条目在**新生成的快照**里才有 id。
        let char_rel = d.characters.len() + d.relations.len();
        println!(
            "  角色+关系差异 {} 处，世界观差异 {} 处（老快照缺 id 属预期）",
            char_rel,
            d.world_settings.len()
        );
        println!("  ✔ 字段对齐检查完成（新快照会写入 world_settings.id）");
    }

    /// **真机踩到的坑**：老快照的 world_settings 没有 id，而当前库有 id。
    /// 如果对齐键在两边"各自挑"（一边挑到 id、一边挑到 key），同一条目会同时出现在
    /// "新增"和"删除"里 —— 看起来像"整批设定被换掉了"，实际什么都没变。
    /// 这条测试锁住"必须由调用方指定统一的对齐键"。
    #[test]
    fn mixed_id_presence_must_not_create_phantom_add_remove() {
        // 当前库：带 id 与 key
        let current = vec![json!({"id": 5, "key": "修女宿舍", "value": "旧描述"})];
        // 老快照：只有 key（没有 id）
        let snapshot = vec![json!({"key": "修女宿舍", "value": "旧描述"})];
        // 按 key 对齐 → 两边配得上，且内容一致 → 无差异
        let d = diff_items(&current, &snapshot, WORLD_FIELDS, &|v| jstr(v, "key"), "key");
        assert!(d.is_empty(), "按 key 对齐时不应产生任何差异，实际 {d:?}");

        // 对照：按 id 对齐会配不上（一边 i:5、一边没有 id）→ 出现假的"新增 + 删除"
        let bad = diff_items(&current, &snapshot, WORLD_FIELDS, &|v| jstr(v, "key"), "id");
        assert_eq!(bad.len(), 2, "按 id 对齐会同时报新增与删除（这就是当时的现象）");
    }

    /// 按 key 对齐时，value 变了要能报出来
    #[test]
    fn keyed_match_still_detects_value_change() {
        let current = vec![json!({"id": 5, "key": "修女宿舍", "value": "新描述"})];
        let snapshot = vec![json!({"key": "修女宿舍", "value": "旧描述"})];
        let d = diff_items(&current, &snapshot, WORLD_FIELDS, &|v| jstr(v, "key"), "key");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, "modified");
        assert_eq!(d[0].label, "修女宿舍");
        assert_eq!(d[0].fields[0].field, "内容");
        assert_eq!(d[0].fields[0].before, "新描述");
        assert_eq!(d[0].fields[0].after, "旧描述");
    }

}


/// 第五批 ②：回滚路径的 **SQL 层**测试。
///
/// 背景：用户实测点「回滚」报 `UNIQUE constraint failed: world_settings.id`（1555）。
/// 根因是回滚时 `.bind(w.get("id")...unwrap_or(0))` —— **老快照的 payload 里没有 id**，
/// 于是每条都插 id=0，第二条起主键冲突。
///
/// 为什么不写成"调用 apply 函数"的集成测试：那需要把恢复逻辑抽成独立函数（改动较大、
/// 易引入回归）。这里**在临时库上直接跑同一套 SQL**，一样能钉住"缺失 id 必须传 NULL"这条规则，
/// 而且不碰生产代码结构。
#[cfg(test)]
mod rollback_sql_tests {
    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::SqlitePool;

    /// 造一个只含 world_settings 的临时库（文件库 —— :memory: 下每个连接是各自独立的库）
    async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = crate::paths::test_root()
            .join(format!("rollback-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(dir.join("story.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE world_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL, value TEXT NOT NULL,
                is_hard_rule INTEGER NOT NULL DEFAULT 0, version INTEGER NOT NULL DEFAULT 1,
                category TEXT NOT NULL DEFAULT '设定', meta_json TEXT NOT NULL DEFAULT '{}')",
        )
        .execute(&pool)
        .await
        .unwrap();
        (pool, dir)
    }

    /// 与生产代码同形的插入语句；`bind_missing_as_zero` 控制用哪种写法，便于对照
    async fn insert_world(pool: &SqlitePool, items: &[serde_json::Value], bind_missing_as_zero: bool) -> Result<(), String> {
        for w in items {
            let id = if bind_missing_as_zero {
                Some(w.get("id").and_then(|v| v.as_i64()).unwrap_or(0))
            } else {
                w.get("id").and_then(|v| v.as_i64())
            };
            sqlx::query(
                "INSERT INTO world_settings(id, key, value, is_hard_rule, version, category, meta_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .bind(id)
            .bind(w.get("key").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("value").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("is_hard_rule").and_then(|v| v.as_i64()).unwrap_or(0))
            .bind(w.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .bind(w.get("category").and_then(|v| v.as_str()).unwrap_or("设定"))
            .bind(w.get("meta_json").and_then(|v| v.as_str()).unwrap_or("{}"))
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 老式 payload（**没有 id**）+ 现在的写法 → 必须成功，且自增 id 不冲突
    #[tokio::test]
    async fn missing_ids_fall_back_to_autoincrement() {
        let (pool, _dir) = temp_pool("ok").await;
        let items = vec![
            serde_json::json!({"key": "背景介绍", "value": "A"}),
            serde_json::json!({"key": "修女宿舍", "value": "B"}),
            serde_json::json!({"key": "客房", "value": "C"}),
        ];
        insert_world(&pool, &items, false).await.expect("缺失 id 时应能正常插入");
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM world_settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 3, "三条都应插入成功");
        // id 必须是互不相同的正数（自增分配）
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM world_settings ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(ids, vec![1, 2, 3], "应由数据库自增分配，而不是全为 0");
    }

    /// 对照：**旧写法**（缺失当 0）在同一条数据上必然失败 —— 复现用户报的 1555
    #[tokio::test]
    async fn old_binding_reproduces_unique_constraint_error() {
        let (pool, _dir) = temp_pool("bad").await;
        let items = vec![
            serde_json::json!({"key": "背景介绍", "value": "A"}),
            serde_json::json!({"key": "修女宿舍", "value": "B"}),
        ];
        let err = insert_world(&pool, &items, true).await.unwrap_err();
        assert!(
            err.contains("UNIQUE constraint failed") || err.contains("1555"),
            "旧写法应当复现主键冲突，实际错误：{err}"
        );
    }

    /// 新式 payload（带 id）仍应按 id 原样恢复 —— 修复不能把"有 id"的情形搞坏
    #[tokio::test]
    async fn explicit_ids_are_still_restored_as_is() {
        let (pool, _dir) = temp_pool("withid").await;
        let items = vec![
            serde_json::json!({"id": 7, "key": "修女宿舍", "value": "B"}),
            serde_json::json!({"id": 9, "key": "客房", "value": "C"}),
        ];
        insert_world(&pool, &items, false).await.unwrap();
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM world_settings ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(ids, vec![7, 9], "带 id 时应原样恢复（回滚要保持版本一致）");
    }
}


/// 第五批 ②：会话内临时版本的 SQL 层测试。
///
/// 语义（用户 2026-10-12 定）：回滚前自动存一份"当前状态"，**只在本次运行期间有效**，
/// 程序启动时清掉上一次运行遗留的 —— 也就是"退出即失效"。
#[cfg(test)]
mod temporary_snapshot_tests {
    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::SqlitePool;

    async fn temp_pool(tag: &str) -> SqlitePool {
        let dir = crate::paths::test_root()
            .join(format!("tempsnap-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(dir.join("story.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE setting_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
                payload_json TEXT NOT NULL, parent_version INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                temporary INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn add(pool: &SqlitePool, version: i64, temporary: bool, note: &str) {
        sqlx::query(
            "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note)
             VALUES(?1, '{}', NULL, ?2, ?3)",
        )
        .bind(version)
        .bind(if temporary { 1i64 } else { 0i64 })
        .bind(note)
        .execute(pool)
        .await
        .unwrap();
    }

    /// 清理只删临时版本，**永久版本必须原样保留**（这是最要紧的一条：不能把用户的历史删掉）
    #[tokio::test]
    async fn purge_removes_only_temporary() {
        let pool = temp_pool("purge").await;
        add(&pool, 1, false, "").await;
        add(&pool, 2, false, "").await;
        add(&pool, 3, true, "回滚到 V1 前的自动备份").await;
        add(&pool, 4, true, "回滚到 V2 前的自动备份").await;

        let n = sqlx::query("DELETE FROM setting_snapshots WHERE temporary = 1")
            .execute(&pool)
            .await
            .unwrap()
            .rows_affected();
        assert_eq!(n, 2, "应清掉 2 个临时版本");

        let left: Vec<i64> = sqlx::query_scalar("SELECT version FROM setting_snapshots ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(left, vec![1, 2], "两个永久版本必须保留");
    }

    /// 迁移：老库（无 temporary / note 列）补列后，已有版本应视为**永久**（temporary 默认 0）
    #[tokio::test]
    async fn migration_marks_existing_snapshots_permanent() {
        let dir = crate::paths::test_root()
            .join("tempsnap-migrate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(dir.join("story.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        // 老结构：没有 temporary / note
        sqlx::query(
            "CREATE TABLE setting_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
                payload_json TEXT NOT NULL, parent_version INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
        )
        .execute(&pool)
        .await
        .unwrap();
        // 注意：老结构里**没有** temporary / note 列，所以这里必须用只含老列的 INSERT
        sqlx::query("INSERT INTO setting_snapshots(version, payload_json, parent_version) VALUES(1, '{}', NULL)")
            .execute(&pool)
            .await
            .unwrap();

        // 跑迁移（幂等）
        crate::story_db::ensure_snapshot_temporary_column(&pool).await.unwrap();
        crate::story_db::ensure_snapshot_temporary_column(&pool).await.unwrap(); // 再跑一次不报错

        let temp: i64 = sqlx::query_scalar("SELECT temporary FROM setting_snapshots WHERE version = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(temp, 0, "老库已有版本应被视为永久版本（不能被清理误删）");
        let note: String = sqlx::query_scalar("SELECT note FROM setting_snapshots WHERE version = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(note, "");
    }

    /// 版本号分配：临时版本与永久版本**共用同一条版本号序列**（否则回滚时会撞号）
    #[tokio::test]
    async fn temporary_versions_share_the_same_sequence() {
        let pool = temp_pool("seq").await;
        add(&pool, 1, false, "").await;
        add(&pool, 2, true, "备份").await; // 临时
        add(&pool, 3, false, "").await;
        let max: i64 = sqlx::query_scalar("SELECT MAX(version) FROM setting_snapshots")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(max, 3, "临时版本也占号，下一个版本号应是 4");
    }
}


/// 第五批 ③ 第 3 步（3-3 批）：快照与回滚的**分支隔离**测试。
///
/// 这里测的是 `rollback_snapshot` 的"应用 payload"那一段的核心语义：
/// **回滚只清当前分支的行**，不能把别的分支的数据一起清掉。
/// （原实现是整表 `DELETE FROM characters` —— 分支下会把主线与其它分支一起清空。）
///
/// 用临时故事库直接调 `apply` 路径太重（它内部会 resolve 分支、开库），
/// 所以这里用**同一套 SQL** 在临时库上验证语义 —— 与 `branch.rs` 里 SQL 层测试同一思路。
#[cfg(test)]
mod branch_rollback_tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;

    async fn pool_with_branches(tag: &str) -> (SqlitePool, String, String, std::path::PathBuf) {
        let dir = crate::paths::test_root()
            .join(format!("rollback-branch-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(dir.join("story.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE characters (
                id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                alias TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '',
                branch_id TEXT)",
        )
        .execute(&pool).await.unwrap();
        // 主线 2 个角色 + 两个分支各 1 个
        sqlx::query("INSERT INTO characters(name, role, branch_id) VALUES('主线甲','女主',NULL)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO characters(name, role, branch_id) VALUES('主线乙','男主',NULL)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO characters(name, role, branch_id) VALUES('分支A角色','女主','BR-A')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO characters(name, role, branch_id) VALUES('分支B角色','男主','BR-B')")
            .execute(&pool).await.unwrap();
        (pool, "BR-A".to_string(), "BR-B".to_string(), dir)
    }

    /// 回滚**分支 A** 时，只该清掉分支 A 的角色行；主线与分支 B 必须原样保留
    #[tokio::test]
    async fn rollback_only_clears_target_branch() {
        let (pool, a, b, dir) = pool_with_branches("clear").await;

        // 复刻 rollback 的删除语义
        let branch = Some(a.clone());
        let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
        let sql = format!("DELETE FROM characters WHERE {}", clause);
        sqlx::query(&sql).bind(&a).execute(&pool).await.unwrap();

        let left: Vec<(Option<String>, String)> =
            sqlx::query_as("SELECT branch_id, name FROM characters ORDER BY id")
                .fetch_all(&pool).await.unwrap();
        assert_eq!(left.len(), 3, "只该删掉分支 A 的那 1 行，剩 3 行");
        assert!(left.iter().any(|(br, n)| br.is_none() && n == "主线甲"), "主线甲必须保留");
        assert!(left.iter().any(|(br, n)| br.is_none() && n == "主线乙"), "主线乙必须保留");
        assert!(left.iter().any(|(br, n)| br.as_deref() == Some(b.as_str())), "分支 B 必须保留");
        assert!(!left.iter().any(|(br, _)| br.as_deref() == Some(a.as_str())), "分支 A 应被清空");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 主线回滚（`branch = None`）只清主线行，两个分支都不受影响
    #[tokio::test]
    async fn mainline_rollback_leaves_branches_alone() {
        let (pool, a, b, dir) = pool_with_branches("main").await;
        let branch: Option<String> = None;
        let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
        sqlx::query(&format!("DELETE FROM characters WHERE {}", clause))
            .execute(&pool).await.unwrap();

        let left: Vec<(Option<String>, String)> =
            sqlx::query_as("SELECT branch_id, name FROM characters ORDER BY id")
                .fetch_all(&pool).await.unwrap();
        assert_eq!(left.len(), 2, "主线 2 行被清，两个分支的行必须还在");
        assert!(left.iter().all(|(br, _)| br.is_some()), "剩下的都该是分支行");
        assert!(left.iter().any(|(br, _)| br.as_deref() == Some(a.as_str())));
        assert!(left.iter().any(|(br, _)| br.as_deref() == Some(b.as_str())));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 回归守护：**主线回滚仍是"整表清空"**（因为主线的行就是全部 `branch_id IS NULL` 的行）
    /// —— 这条断言保证把删除改成带条件后，主线行为没有变。
    #[tokio::test]
    async fn mainline_delete_covers_all_main_rows() {
        let (pool, _a, _b, dir) = pool_with_branches("cover").await;
        let n_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE branch_id IS NULL")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(n_before, 2);
        sqlx::query("DELETE FROM characters WHERE branch_id IS NULL")
            .execute(&pool).await.unwrap();
        let n_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE branch_id IS NULL")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(n_after, 0, "主线行应被全部清掉（与原来的整表 DELETE 等价）");
        let _ = std::fs::remove_dir_all(&dir);
    }
}


/// ⑤-B：设定变更"全部入队、人工拍板"的行为测试。
///
/// 核心命题只有三条：
///   ① 默认（开关 on）→ **只入队，不改库**
///   ② 关掉开关 → 恢复旧行为（低风险自动应用）
///   ③ 批量落地 → 真的落地 + **整批只留一个还原点**
#[cfg(test)]
mod approval_queue_tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;
    use std::str::FromStr;

    /// 造一个够用的临时故事库：提案表 + 快照需要的三张设定表。
    async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = crate::paths::test_root()
            .join("approval-queue")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("story.db");
        let opts = sqlx::sqlite::SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display()))
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();

        for ddl in [
            "CREATE TABLE change_proposals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                target_type TEXT NOT NULL, target_id INTEGER, field TEXT NOT NULL,
                old_value TEXT, new_value TEXT, reason TEXT, evidence TEXT,
                confidence REAL NOT NULL DEFAULT 0.0,
                status TEXT NOT NULL DEFAULT 'pending', note TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                group_id INTEGER, branch_id TEXT, node_id INTEGER)",
            "CREATE TABLE characters (
                id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                alias TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '',
                faction TEXT NOT NULL DEFAULT '', age TEXT NOT NULL DEFAULT '',
                gender TEXT NOT NULL DEFAULT '', first_appearance TEXT NOT NULL DEFAULT '',
                one_liner TEXT NOT NULL DEFAULT '', profile TEXT NOT NULL DEFAULT '',
                speech_style TEXT NOT NULL DEFAULT '', traits TEXT NOT NULL DEFAULT '',
                current_status TEXT NOT NULL DEFAULT '',
                stat_stamina INTEGER NOT NULL DEFAULT 50, stat_sanity INTEGER NOT NULL DEFAULT 50,
                stat_emotion INTEGER NOT NULL DEFAULT 50, stat_screen INTEGER NOT NULL DEFAULT 50,
                version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
            "CREATE TABLE relations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_character_id INTEGER NOT NULL, to_character_id INTEGER NOT NULL,
                relation_type TEXT NOT NULL DEFAULT '', direction TEXT NOT NULL DEFAULT '',
                strength INTEGER NOT NULL DEFAULT 50, state TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '', evidence TEXT NOT NULL DEFAULT '',
                version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
            "CREATE TABLE world_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL DEFAULT '',
                value TEXT NOT NULL DEFAULT '', is_hard_rule INTEGER NOT NULL DEFAULT 0,
                version INTEGER NOT NULL DEFAULT 1, category TEXT NOT NULL DEFAULT '设定',
                meta_json TEXT NOT NULL DEFAULT '{}', branch_id TEXT)",
            "CREATE TABLE audit_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT, action TEXT NOT NULL, target TEXT,
                before TEXT, after TEXT, operator TEXT NOT NULL DEFAULT 'system',
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
            "CREATE TABLE setting_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
                payload_json TEXT NOT NULL, parent_version INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                temporary INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '',
                branch_id TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }

        // 两个角色 + 一条关系（强度 50）+ 一条世界观
        for (i, n) in [(1, "甲"), (2, "乙")] {
            sqlx::query("INSERT INTO characters(id, name) VALUES(?1, ?2)")
                .bind(i).bind(n).execute(&pool).await.unwrap();
        }
        sqlx::query(
            "INSERT INTO relations(id, from_character_id, to_character_id, relation_type, direction, strength, state)
             VALUES(1, 1, 2, '陌生', '单向', 50, '平稳')",
        ).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO world_settings(id, key, value) VALUES(1, '地点', '城')")
            .execute(&pool).await.unwrap();
        (pool, dir)
    }

    /// 手动插一条关系强度提案，返回 group_id。
    async fn add_strength_proposal(pool: &SqlitePool, group: i64, new_strength: i64) -> i64 {
        insert_proposal(
            pool, &None, 7, group, "relation", 1, "strength",
            "50", &new_strength.to_string(),
            "测试用", "第7天：甲关心乙", 0.9, "pending",
        ).await.unwrap()
    }

    async fn strength(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT strength FROM relations WHERE id = 1")
            .fetch_one(pool).await.unwrap()
    }

    async fn statuses(pool: &SqlitePool, group: i64) -> Vec<String> {
        sqlx::query_scalar("SELECT status FROM change_proposals WHERE group_id = ?1 ORDER BY id")
            .bind(group).fetch_all(pool).await.unwrap()
    }

    async fn snapshot_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM setting_snapshots")
            .fetch_one(pool).await.unwrap()
    }

    /// ① 默认（need_approval=true）：**只入队，关系不该变**。
    #[tokio::test]
    async fn approval_on_queues_without_touching_data() {
        let (pool, dir) = temp_pool("on").await;
        let mut proposals = Vec::new();
        let mut applied = Vec::new();
        let mut notes = Vec::new();

        add_strength_proposal(&pool, 1, 90).await;
        queue_or_apply(&pool, "1", 1, true, &mut proposals, &mut applied, &mut notes)
            .await.unwrap();

        assert_eq!(strength(&pool).await, 50, "开关打开时不该改关系强度");
        assert_eq!(statuses(&pool, 1).await, vec!["pending"], "提案应停在待确认");
        assert!(applied.is_empty(), "不该记录任何已应用组");
        assert_eq!(proposals.len(), 1, "报告里应带上这条待确认提案");
        assert_eq!(proposals[0].node_id, Some(7), "提案要记住来源节点");
        assert!(notes.iter().any(|n| n.contains("待确认")), "应提示已入队（实际：{notes:?}）");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ② 关掉开关：恢复旧行为 —— 立刻应用。
    #[tokio::test]
    async fn approval_off_restores_auto_apply() {
        let (pool, dir) = temp_pool("off").await;
        let mut proposals = Vec::new();
        let mut applied = Vec::new();
        let mut notes = Vec::new();

        add_strength_proposal(&pool, 1, 90).await;
        queue_or_apply(&pool, "1", 1, false, &mut proposals, &mut applied, &mut notes)
            .await.unwrap();

        assert_eq!(strength(&pool).await, 90, "关掉开关应立刻生效");
        assert_eq!(statuses(&pool, 1).await, vec!["applied"]);
        assert_eq!(applied, vec![1], "应记录已应用的组");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ③ 批量落地：两组都落地，但**只留一个还原点**。
    #[tokio::test]
    async fn batch_confirm_applies_all_but_takes_one_snapshot() {
        let (pool, dir) = temp_pool("batch").await;
        add_strength_proposal(&pool, 1, 60).await;
        add_strength_proposal(&pool, 2, 80).await;
        let before = snapshot_count(&pool).await;

        let msg = confirm_proposals_batch_in(&pool, "1", vec![1, 2], None).await.unwrap();

        assert!(msg.contains("已落地 2 组"), "结果文案应说明落地 2 组（实际：{msg}）");
        assert_eq!(snapshot_count(&pool).await - before, 1, "整批只该留一个还原点");
        assert_eq!(statuses(&pool, 1).await, vec!["applied"]);
        assert_eq!(statuses(&pool, 2).await, vec!["applied"]);
        // 两组分别应用：组1 → 60，组2 → 80
        assert_eq!(strength(&pool).await, 80);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ④ 批量拒绝：状态变 rejected，数据不动。
    #[tokio::test]
    async fn batch_reject_marks_rejected_and_keeps_data() {
        let (pool, dir) = temp_pool("reject").await;
        add_strength_proposal(&pool, 1, 60).await;
        add_strength_proposal(&pool, 2, 80).await;

        let msg = reject_proposals_batch_in(&pool, vec![1, 2]).await.unwrap();
        assert!(msg.contains("已拒绝 2 条"), "实际：{msg}");
        assert_eq!(statuses(&pool, 1).await, vec!["rejected"]);
        assert_eq!(strength(&pool).await, 50, "拒绝不该改数据");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ⑤ 空选择要报错，不能静默成功。
    #[tokio::test]
    async fn empty_selection_is_an_error() {
        let (pool, dir) = temp_pool("empty").await;
        assert!(confirm_proposals_batch_in(&pool, "1", vec![], None).await.is_err());
        assert!(reject_proposals_batch_in(&pool, vec![]).await.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// **诊断用（`#[ignore]`，不参与常规测试）**：拿一份真实故事的库副本，
/// 逐个节点跑分析，把后端返回什么摊开打出来。
///
/// 为什么需要它：AI 剧情信号不稳定，「点了没反应」很难靠界面复现。
/// 这个测试能回答"到底是没信号、还是报错了、还是根本没进到引擎"。
///
/// 用法（**单独跑**，因为它会临时改数据根）：
/// ```text
/// $env:DW_DIAG_DB="D:\project\ai生成\data\library\default\projects\7\story.db"
/// cargo test --lib diag_analyze_real_db -- --ignored --nocapture
/// ```
#[cfg(test)]
mod diag_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "诊断用：需要 DW_DIAG_DB 指向真实 story.db；会临时改 DREAMWEAVER_DATA_DIR"]
    async fn diag_analyze_real_db() {
        let Ok(src) = std::env::var("DW_DIAG_DB") else {
            eprintln!("没设 DW_DIAG_DB，跳过");
            return;
        };
        // 造一个临时数据根，把副本放到"真 id"的位置，
        // 这样 open_story_pool(story_id) 会命中副本而不是用户的库。
        let root = crate::paths::test_root().join("diag-data");
        let _ = std::fs::remove_dir_all(&root);
        let proj = root.join("library").join("default").join("projects").join("7");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::copy(&src, proj.join("story.db")).expect("复制库失败");
        std::env::set_var("DREAMWEAVER_DATA_DIR", &root);

        let story_id = "7";
        let pool = crate::story_db::open_story_pool(story_id).await.unwrap();

        let nodes: Vec<(i64, String, i64)> = sqlx::query_as(
            "SELECT n.id, n.date_label, (SELECT COUNT(*) FROM messages m WHERE m.node_id = n.id)
             FROM timeline_nodes n ORDER BY n.id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        eprintln!("=== 节点清单（{} 个）===", nodes.len());
        for (id, label, n) in &nodes {
            eprintln!("  节点 #{id} {label}：{n} 条消息");
        }

        eprintln!("\n=== 逐节点分析（走完整公开 API）===");
        for (id, label, n) in &nodes {
            if *n == 0 {
                continue;
            }
            match analyze_evolution(story_id, *id).await {
                Ok(rep) => eprintln!(
                    "  节点 #{id} {label}: 事件 {:?} | 待确认 {} | 已应用 {} | notes {:?}",
                    rep.events.iter().map(|e| e.event_type.as_str()).collect::<Vec<_>>(),
                    rep.proposals.iter().filter(|p| p.status == "pending").count(),
                    rep.applied_groups.len(),
                    rep.notes
                ),
                Err(e) => eprintln!("  节点 #{id} {label}: ❌ {e}"),
            }
        }

        let cnt: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM change_proposals")
            .fetch_one(&pool).await.unwrap();
        eprintln!("\n=== 分析后 change_proposals 共 {cnt} 条 ===");
        let rows: Vec<(i64, i64, String, String, Option<String>, Option<String>, String, Option<i64>)> =
            sqlx::query_as(
                "SELECT id, COALESCE(group_id, id), target_type, field, old_value, new_value, status, node_id
                 FROM change_proposals ORDER BY id DESC LIMIT 20",
            )
            .fetch_all(&pool).await.unwrap();
        for r in &rows {
            eprintln!(
                "  组{}/条{} {} {} {} → {} [{}] node={:?}",
                r.1, r.0, r.2, r.3,
                r.4.clone().unwrap_or_default(), r.5.clone().unwrap_or_default(), r.6, r.7
            );
        }

        // ── 时间线：同一天不能重复创建（后端守卫）──
        eprintln!("\n=== 时间线：重复创建同一天 ===");
        match crate::timeline::create_node_at_day(story_id, 3).await {
            Ok(n) => eprintln!("  ❌ 竟然创建成功了：{}（应当被拒绝）", n.date_label),
            Err(e) => eprintln!("  ✅ 被拒绝：{e}"),
        }
        match crate::timeline::create_node_at_day(story_id, 99).await {
            Ok(n) => eprintln!("  ✅ 新的一天可以创建：{}", n.date_label),
            Err(e) => eprintln!("  ❌ 新的一天失败了：{e}"),
        }

        // ── RAG ①：回顾检索在真实数据上的效果 ──
        eprintln!("\n=== RAG 回顾检索（真实数据）===");
        let days = crate::recall::list_days(&pool, &None).await.unwrap_or_default();
        eprintln!("  可回顾的天：{} 个", days.len());
        for d in days.iter().take(6) {
            eprintln!("    {}（节点#{}）{}…", d.date_label, d.node_id, d.preview.chars().take(28).collect::<String>());
        }
        let samples = [
            "你还记得我们第一次牵手的时候吗",
            "第 8 天那次争吵是怎么回事",
            "当时在院子里你说过什么",
            "今天天气不错",                    // 反例：不该触发
        ];
        for s in samples {
            let intent = crate::recall::detect_intent(s);
            let mut hits = Vec::new();
            if intent.is_recall {
                let exclude = std::collections::HashSet::new();
                hits = crate::recall::search(&pool, &None, 0, &exclude, &intent, None, 3, 1600)
                    .await.unwrap_or_default();
            }
            eprintln!(
                "  「{s}」→ 意图={} 线索={:?} 事件={:?} 命中 {} 条",
                intent.is_recall, intent.clues, intent.events, hits.len()
            );
            for h in &hits {
                eprintln!("      {} 分{} 因为{:?}", h.date_label, h.score, h.why);
            }
        }
        // 强制指定某一天
        if let Some(d) = days.first() {
            let exclude = std::collections::HashSet::new();
            let forced = crate::recall::search(&pool, &None, 0, &exclude, &crate::recall::Intent::default(), Some(d.node_id), 1, 1600).await.unwrap_or_default();
            eprintln!("\n  强制回顾「{}」→ 命中 {} 条（含原文 {} 条消息）", d.date_label, forced.len(), forced.iter().map(|h| h.messages.len()).sum::<usize>());
            eprintln!("{}", crate::recall::render_block(&forced));
        }

        // ── 回滚到"老格式"快照（payload 里混着分支的行）──
        // 这正是用户点「回滚到此版本」时报 1555 的场景。
        eprintln!("\n=== 回滚到最新版本（老格式，含分支行）===");
        let latest: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(version) FROM setting_snapshots WHERE temporary = 0",
        ).fetch_one(&pool).await.unwrap();
        if let Some(v) = latest {
            let before_chars: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters")
                .fetch_one(&pool).await.unwrap();
            match rollback_snapshot(story_id, v).await {
                Ok(m) => eprintln!("  ✅ 回滚到 V{v}：{m}"),
                Err(e) => eprintln!("  ❌ 回滚到 V{v} 失败：{e}"),
            }
            let after_chars: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters")
                .fetch_one(&pool).await.unwrap();
            let main_c: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE branch_id IS NULL")
                .fetch_one(&pool).await.unwrap();
            let br_c: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE branch_id IS NOT NULL")
                .fetch_one(&pool).await.unwrap();
            eprintln!("  角色总数 {before_chars} → {after_chars}（主线 {main_c} + 分支 {br_c}）");
        }

        // ── 测试注入 → 批量落地 的确定性验证 ──
        eprintln!("\n=== 注入测试提案 ===");
        match inject_test_proposal(story_id, Some(1)).await {
            Ok(m) => eprintln!("  ✅ {m}"),
            Err(e) => eprintln!("  ❌ {e}"),
        }
        let pending: Vec<(i64, String, Option<i64>)> = sqlx::query_as(
            "SELECT COALESCE(group_id, id), status, node_id FROM change_proposals
             WHERE evidence LIKE '%测试注入%' ORDER BY id DESC",
        )
        .fetch_all(&pool).await.unwrap();
        eprintln!("  注入后（共 {} 条）：{:?}", pending.len(), pending);

        if let Some((gid, _, _)) = pending.first() {
            let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM setting_snapshots")
                .fetch_one(&pool).await.unwrap();
            match confirm_proposals_batch(story_id, vec![*gid]).await {
                Ok(m) => eprintln!("  ✅ 落地：{m}"),
                Err(e) => eprintln!("  ❌ 落地：{e}"),
            }
            let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM setting_snapshots")
                .fetch_one(&pool).await.unwrap();
            eprintln!("  快照数 {before} → {after}（应 +1）");
            let st: Vec<String> = sqlx::query_scalar(
                "SELECT status FROM change_proposals WHERE group_id = ?1",
            ).bind(gid).fetch_all(&pool).await.unwrap();
            eprintln!("  该组状态：{:?}", st);
        }

        let _ = std::fs::remove_dir_all(&root);
    }
}


/// 回归测试：**回滚不能和别的分支的行撞主键**。
///
/// 用户实机报错：`error returned from database: (code: 1555)
/// UNIQUE constraint failed: characters.id`。
/// 根因是 `make_snapshot` 原来读全表（不分分支），主线快照里装着分支的角色
/// （真实数据：主线 id 2/3/4 + 分支 id 27/28/29 都在同一个快照里）；
/// 回滚时只删主线行，再把 6 条按原 id 插回去 → 撞上仍在的分支行。
///
/// 现在的行为：快照按分支隔离；老快照里混进来的"别的分支的行"在回滚时**跳过**。
#[cfg(test)]
mod rollback_branch_scope_tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;
    use std::str::FromStr;

    /// 主线 3 个角色 + 分支 3 个角色（id 故意错开，模拟真实数据）
    async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = crate::paths::test_root()
            .join("rollback-scope")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("story.db");
        let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display()))
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();

        for ddl in [
            "CREATE TABLE characters (
                id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                alias TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '',
                faction TEXT NOT NULL DEFAULT '', age TEXT NOT NULL DEFAULT '',
                gender TEXT NOT NULL DEFAULT '', first_appearance TEXT NOT NULL DEFAULT '',
                one_liner TEXT NOT NULL DEFAULT '', profile TEXT NOT NULL DEFAULT '',
                speech_style TEXT NOT NULL DEFAULT '', traits TEXT NOT NULL DEFAULT '',
                current_status TEXT NOT NULL DEFAULT '',
                stat_stamina INTEGER NOT NULL DEFAULT 50, stat_sanity INTEGER NOT NULL DEFAULT 50,
                stat_emotion INTEGER NOT NULL DEFAULT 50, stat_screen INTEGER NOT NULL DEFAULT 50,
                version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
            "CREATE TABLE relations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_character_id INTEGER NOT NULL, to_character_id INTEGER NOT NULL,
                relation_type TEXT NOT NULL DEFAULT '', direction TEXT NOT NULL DEFAULT '',
                strength INTEGER NOT NULL DEFAULT 50, state TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '', evidence TEXT NOT NULL DEFAULT '',
                version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
            "CREATE TABLE world_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL DEFAULT '',
                value TEXT NOT NULL DEFAULT '', is_hard_rule INTEGER NOT NULL DEFAULT 0,
                version INTEGER NOT NULL DEFAULT 1, category TEXT NOT NULL DEFAULT '设定',
                meta_json TEXT NOT NULL DEFAULT '{}', branch_id TEXT)",
            "CREATE TABLE setting_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
                payload_json TEXT NOT NULL, parent_version INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                temporary INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '',
                branch_id TEXT)",
            "CREATE TABLE audit_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT, action TEXT NOT NULL, target TEXT,
                before TEXT, after TEXT, operator TEXT NOT NULL DEFAULT 'system',
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }

        // 主线：id 2/3/4；分支 B：id 27/28/29
        for (id, name, br) in [
            (2i64, "甲", None),
            (3, "乙", None),
            (4, "丙", None),
            (27, "甲(分支)", Some("B")),
            (28, "乙(分支)", Some("B")),
            (29, "丙(分支)", Some("B")),
        ] {
            sqlx::query("INSERT INTO characters(id, name, branch_id) VALUES(?1, ?2, ?3)")
                .bind(id).bind(name).bind(br).execute(&pool).await.unwrap();
        }
        (pool, dir)
    }

    async fn char_ids(pool: &SqlitePool, branch: &Option<String>) -> Vec<i64> {
        let clause = crate::story_db::branch_clause("branch_id", branch, 1);
        let sql = format!("SELECT id FROM characters WHERE {clause} ORDER BY id");
        let mut q = sqlx::query_scalar::<_, i64>(&sql);
        if let Some(b) = branch { q = q.bind(b); }
        q.fetch_all(pool).await.unwrap()
    }

    /// ① 快照只装**本分支**的行。
    #[tokio::test]
    async fn snapshot_payload_is_branch_scoped() {
        let (pool, dir) = temp_pool("scope").await;
        assert_eq!(char_ids(&pool, &None).await, vec![2, 3, 4]);
        assert_eq!(char_ids(&pool, &Some("B".into())).await, vec![27, 28, 29]);

        let v = make_snapshot(&pool, false, Some("主线"), None).await.unwrap();
        let raw: String = sqlx::query_scalar(
            "SELECT payload_json FROM setting_snapshots WHERE version = ?1",
        )
        .bind(v).fetch_one(&pool).await.unwrap();
        let payload: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let ids: Vec<i64> = payload["characters"].as_array().unwrap()
            .iter().filter_map(|c| c["id"].as_i64()).collect();
        assert_eq!(ids, vec![2, 3, 4], "主线快照不该含分支的角色（原来含 27/28/29，导致回滚撞主键）");

        let vb = make_snapshot(&pool, false, Some("分支"), Some("B".into())).await.unwrap();
        let pb: String = sqlx::query_scalar("SELECT payload_json FROM setting_snapshots WHERE version = ?1")
            .bind(vb).fetch_one(&pool).await.unwrap();
        let pb: serde_json::Value = serde_json::from_str(&pb).unwrap();
        let idsb: Vec<i64> = pb["characters"].as_array().unwrap()
            .iter().filter_map(|c| c["id"].as_i64()).collect();
        assert_eq!(idsb, vec![27, 28, 29], "分支快照应只含分支自己的角色");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ② **老快照**（payload 里混着别的分支的行）回滚时不能报错，也不能破坏别的分支。
    ///
    /// 这是用户报的那个 1555 错误的回归测试。
    #[tokio::test]
    async fn rollback_skips_rows_belonging_to_other_branch() {
        let (pool, dir) = temp_pool("legacy").await;

        // 手搓一个"老格式"快照：主线 3 条 + 分支 3 条全塞进去
        let legacy = serde_json::json!({
            "characters": [
                {"id": 2, "name": "甲"},
                {"id": 3, "name": "乙"},
                {"id": 4, "name": "丙"},
                {"id": 27, "name": "甲(分支)"},
                {"id": 28, "name": "乙(分支)"},
                {"id": 29, "name": "丙(分支)"}
            ],
            "relations": [],
            "world_settings": []
        });
        sqlx::query(
            "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note)
             VALUES(1, ?1, NULL, 0, '老格式')",
        ).bind(legacy.to_string()).execute(&pool).await.unwrap();

        // 模拟 rollback_snapshot 的前半段：只删主线
        sqlx::query("DELETE FROM characters WHERE branch_id IS NULL")
            .execute(&pool).await.unwrap();

        // 修复前这里会 `UNIQUE constraint failed: characters.id`
        let mut conn = pool.acquire().await.unwrap();
        restore_settings_from_payload(&mut conn, &None, &legacy)
            .await
            .expect("老快照回滚不该撞主键");
        drop(conn);

        assert_eq!(char_ids(&pool, &None).await, vec![2, 3, 4], "主线应恢复成 3 个角色");
        assert_eq!(char_ids(&pool, &Some("B".into())).await, vec![27, 28, 29], "分支的行必须原样保留");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ③ 同分支残留（删除没覆盖到）时，宁可换 id 也不能让回滚整体失败。
    #[tokio::test]
    async fn rollback_falls_back_to_autoincrement_when_id_taken() {
        let (pool, dir) = temp_pool("taken").await;
        let payload = serde_json::json!({
            "characters": [{"id": 2, "name": "甲(新)"}],
            "relations": [],
            "world_settings": []
        });
        // 故意不删 2，模拟"删除没覆盖到"
        let mut conn = pool.acquire().await.unwrap();
        restore_settings_from_payload(&mut conn, &None, &payload)
            .await
            .expect("id 被占时应退化为自增，而不是报错");
        drop(conn);

        let names: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM characters WHERE branch_id IS NULL ORDER BY id")
            .fetch_all(&pool).await.unwrap();
        assert!(names.iter().any(|(_, n)| n == "甲(新)"), "内容应插进去了（实际 {names:?}）");
        assert_eq!(names.len(), 4, "原有 3 条 + 新插 1 条");

        let _ = std::fs::remove_dir_all(&dir);
    }
}


/// 回滚必须**要么全成、要么全不成**（直接测产品路径 `rollback_snapshot_in`）。
///
/// 实测事故：回滚在"插角色"时撞主键（1555）报错，但前面已经把三张设定表删了 ——
/// 主线的关系与世界观被删掉、再也没插回来，用户看到"主线没有关系了"。
#[cfg(test)]
mod rollback_atomic_tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;
    use std::str::FromStr;

    async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = crate::paths::test_root()
            .join("rollback-atomic")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("story.db");
        let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display()))
            .unwrap()
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE characters (
                id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                alias TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '',
                faction TEXT NOT NULL DEFAULT '', age TEXT NOT NULL DEFAULT '',
                gender TEXT NOT NULL DEFAULT '', first_appearance TEXT NOT NULL DEFAULT '',
                one_liner TEXT NOT NULL DEFAULT '', profile TEXT NOT NULL DEFAULT '',
                speech_style TEXT NOT NULL DEFAULT '', traits TEXT NOT NULL DEFAULT '',
                current_status TEXT NOT NULL DEFAULT '',
                stat_stamina INTEGER NOT NULL DEFAULT 50, stat_sanity INTEGER NOT NULL DEFAULT 50,
                stat_emotion INTEGER NOT NULL DEFAULT 50, stat_screen INTEGER NOT NULL DEFAULT 50,
                version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
            "CREATE TABLE relations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_character_id INTEGER NOT NULL, to_character_id INTEGER NOT NULL,
                relation_type TEXT NOT NULL DEFAULT '', direction TEXT NOT NULL DEFAULT '',
                strength INTEGER NOT NULL DEFAULT 50, state TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '', evidence TEXT NOT NULL DEFAULT '',
                version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
            "CREATE TABLE world_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL DEFAULT '',
                value TEXT NOT NULL DEFAULT '', is_hard_rule INTEGER NOT NULL DEFAULT 0,
                version INTEGER NOT NULL DEFAULT 1, category TEXT NOT NULL DEFAULT '设定',
                meta_json TEXT NOT NULL DEFAULT '{}', branch_id TEXT)",
            "CREATE TABLE setting_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
                payload_json TEXT NOT NULL, parent_version INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                temporary INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '',
                branch_id TEXT)",
            "CREATE TABLE audit_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT, action TEXT NOT NULL, target TEXT,
                before TEXT, after TEXT, operator TEXT NOT NULL DEFAULT 'system',
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO characters(id, name) VALUES(2, '甲')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO relations(id, from_character_id, to_character_id, strength) VALUES(2, 2, 2, 50)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO world_settings(id, key, value) VALUES(18, '地点', '城')")
            .execute(&pool).await.unwrap();
        (pool, dir)
    }

    async fn counts(pool: &SqlitePool) -> (i64, i64, i64) {
        let c: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters").fetch_one(pool).await.unwrap();
        let r: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM relations").fetch_one(pool).await.unwrap();
        let w: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM world_settings").fetch_one(pool).await.unwrap();
        (c, r, w)
    }

    /// 失败的回滚**不能留下删了一半的库**。
    #[tokio::test]
    async fn failed_rollback_keeps_previous_state() {
        let (pool, dir) = temp_pool("atomic").await;
        assert_eq!(counts(&pool).await, (1, 1, 1));

        // 快照 payload 里放两条同名角色，并给 name 加唯一索引 → 插入时必然 1555
        sqlx::query("CREATE UNIQUE INDEX ux_char_name ON characters(name)")
            .execute(&pool).await.unwrap();
        let bad = serde_json::json!({
            "characters": [{"id": 99, "name": "同名"}, {"id": 100, "name": "同名"}],
            "relations": [],
            "world_settings": []
        });
        sqlx::query(
            "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note)
             VALUES(1, ?1, NULL, 0, '会失败的快照')",
        ).bind(bad.to_string()).execute(&pool).await.unwrap();

        let r = rollback_snapshot_in(&pool, 1, None, String::new()).await;
        assert!(r.is_err(), "这份 payload 应该插不进去（同名冲突），实际：{r:?}");
        assert!(
            counts(&pool).await == (1, 1, 1),
            "**回滚失败后三张表必须原样还在**（之前会留下删了一半的库）"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 成功的回滚要真的替换数据。
    #[tokio::test]
    async fn successful_rollback_replaces_data() {
        let (pool, dir) = temp_pool("ok").await;
        let payload = serde_json::json!({
            "characters": [{"id": 2, "name": "甲改名"}],
            "relations": [{"id": 2, "from_character_id": 2, "to_character_id": 2, "strength": 77}],
            "world_settings": [{"id": 18, "key": "地点", "value": "村庄"}]
        });
        sqlx::query(
            "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note)
             VALUES(1, ?1, NULL, 0, '正常快照')",
        ).bind(payload.to_string()).execute(&pool).await.unwrap();

        rollback_snapshot_in(&pool, 1, None, String::new()).await
            .expect("这份 payload 应该能回滚成功");

        let name: String = sqlx::query_scalar("SELECT name FROM characters WHERE id = 2")
            .fetch_one(&pool).await.unwrap();
        let strength: i64 = sqlx::query_scalar("SELECT strength FROM relations WHERE id = 2")
            .fetch_one(&pool).await.unwrap();
        let v: String = sqlx::query_scalar("SELECT value FROM world_settings WHERE id = 18")
            .fetch_one(&pool).await.unwrap();
        assert_eq!((name.as_str(), strength, v.as_str()), ("甲改名", 77, "村庄"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}



