//! 第五批 ③：故事分支（实施方案 §5.2）
//!
//! ## 口径（与设计文档一致，别处不要另立说法）
//!
//! - **主线 = `branch_id IS NULL`**（不叫 main/trunk，就是 NULL）
//! - **分支 = `branches` 表的一行**；`fork_node_id` 指向分叉点（INTEGER，与 `timeline_nodes.id` 同型）
//! - **创建分支时复制"世界状态"**：`characters` / `relations` / `world_settings` / `setting_snapshots`
//!   —— 是**数据复制**，不是引用；对话节点**不复制**（分叉点之前靠 `parent_id` 链共享）
//! - **不复制临时设定版本**（`temporary = 1`）：那是源分支"回滚前 / 编辑前"的操作痕迹，不该被继承
//! - **复制出的行由数据库自增分配新 id**（这些表的 id 是 INTEGER，不是 UUID）→
//!   所以切换分支后角色 id 会变，界面需要清空右侧面板的选中（与设计文档 §5.4 一致）
//!
//! ## 嵌套分支
//!
//! `source_branch_id` 非 NULL 表示从某分支再分叉。**复制逻辑与一级分支完全相同**，
//! 只是过滤条件换成源分支 id —— 因为状态是"行级带 branch_id"的，不需要为嵌套另写代码。
//! **源分支以分叉点所在的分支为准**（而非由调用方指定），避免调用方传错导致状态复制错线。

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::Row;
use sqlx::SqlitePool;
use std::str::FromStr;



/// 分支预设配色池（按创建顺序轮替）
const COLORS: &[&str] = &["#3B82F6", "#F59E0B", "#10B981", "#EF4444", "#8B5CF6"];

/// 需要随分支复制的"世界状态"表 —— **写成常量数组**。
///
/// 为什么写成数组而不是散在代码里：将来给故事库加一张状态表时，只要在这里加一行，
/// 创建分支与测试都会自动覆盖到（测试就是按这个数组逐表断言的）。
/// 每项 = (表名, 除 branch_id 外要复制的列)。
// 保留未用：常量：世界状态表清单（备份/快照用）
#[allow(dead_code)]
pub const WORLD_STATE_TABLES: &[(&str, &str)] = &[
    ("characters", "（逐行复制并记录 id 映射 → 关系要用）"),
    ("relations", "（逐行复制并把两端 id 重写为新分支的角色）"),
    ("world_settings", "key, value, is_hard_rule, version, category, meta_json"),
    (
        "setting_snapshots",
        "version, payload_json, parent_version, created_at, temporary, note",
    ),
];

/// 可以直接 `INSERT ... SELECT` 整表复制的表（**无外键**，不需要重映射）。
///
/// 角色与关系**不在这里**：它们必须逐行复制（角色要记录 `旧 id → 新 id`，关系要用该映射
/// 重写两端）—— 否则分支的关系会指向源分支的角色行，分支就不自包含了。
const COPY_AS_IS_TABLES: &[(&str, &str)] = &[
    ("world_settings", "key, value, is_hard_rule, version, category, meta_json"),
    (
        "setting_snapshots",
        "version, payload_json, parent_version, created_at, temporary, note",
    ),
];

#[derive(Debug, Clone, serde::Serialize)]
pub struct Branch {
    pub id: String,
    pub story_id: String,
    pub name: String,
    /// 分叉点（`timeline_nodes.id`）
    pub fork_node_id: i64,
    /// 从哪个分支 fork（None = 从主线）
    pub source_branch_id: Option<String>,
    pub color: String,
    pub created_at: String,
    /// 该分支自己的节点数（主线不在此列表，故主线不用这个字段）
    #[serde(default)]
    pub node_count: i64,
    /// 第五批 ④：分支备注（一句话说明这条线是干什么的，便于管理平行故事）
    #[serde(default)]
    pub note: String,
}

fn row_to_branch(r: &sqlx::sqlite::SqliteRow) -> Result<Branch, sqlx::Error> {
    Ok(Branch {
        id: r.try_get(0)?,
        story_id: r.try_get(1)?,
        name: r.try_get(2)?,
        fork_node_id: r.try_get(3)?,
        source_branch_id: r.try_get(4)?,
        color: r.try_get(5)?,
        created_at: r.try_get(6)?,
        node_count: 0,
        note: r.try_get(7).unwrap_or_default(),
    })
}

/// 解析"要用哪条分支"：`None` 表示**沿用该故事记录的当前分支**（`stories.current_branch_id`）。
///
/// 为什么要这一层：前端大多数调用不必每次都传分支（传了就按传的来，没传就用"当前分支"）。
/// 界面切换分支时会同时更新 `stories.current_branch_id`，所以"不传"的语义是稳定的。
///
/// 注意返回值：`None` = 主线（`branch_id IS NULL`），`Some(id)` = 该分支。
pub async fn resolve_branch(story_id: &str, branch_id: &Option<String>) -> Result<Option<String>, String> {
    if let Some(b) = branch_id {
        return Ok(Some(b.clone()));
    }
    // 从 library.db 读该故事记录的当前分支；读不到就按主线处理（不让界面因此报错）
    let lib = crate::db::library_pool().await.map_err(|e| e.to_string())?;
    let cur: Option<Option<String>> = sqlx::query_scalar("SELECT current_branch_id FROM stories WHERE id = ?1")
        .bind(story_id)
        .fetch_optional(&lib)
        .await
        .map_err(|e| e.to_string())?;
    let Some(stored) = cur.flatten() else { return Ok(None) };

    // **校验这条分支是否真的存在**。
    // `current_branch_id` 是一个**跨库引用**（library.db 指向 story.db 里的 branches 行），
    // 删掉分支后这里就成了悬空引用 —— 界面会拿一个不存在的分支去过滤/选中，
    // 结果是白屏或"整个故事看不见任何节点"（实测踩到）。
    // 悬空时**归主线并顺手修正记录**，让状态自愈。
    // **两层校验**（任一不成立都归主线并自愈）：
    // ① library.db 里故事还在吗（故事被删 → 不该再有"当前分支"）
    // ② story.db 里那条分支还在吗（分支被删 → 悬空引用）
    //
    // ⚠ 为什么第 ② 层必须单独做：`current_branch_id` 是**跨库引用**，
    // 库层面没有外键。删掉分支后这里就成了悬空引用 —— 界面会拿一个不存在的分支
    // 去过滤/选中，结果是**白屏**或"整个故事看不到任何节点"（用户实测踩到）。
    //
    // ⚠ 为什么第 ① 层用 library.db 而不是打开故事库：故事库可能不存在/打不开，
    // 那种情况下不该让"解析当前分支"失败（它被大量读取路径调用）。
    let sid_num: i64 = story_id.parse().unwrap_or(-1);
    let story_ok: i64 = if sid_num > 0 {
        sqlx::query_scalar("SELECT COUNT(*) FROM stories WHERE id = ?1")
            .bind(sid_num)
            .fetch_one(&lib)
            .await
            .map_err(|e| e.to_string())?
    } else {
        0
    };
    // 故事库打不开时**不报错**，按"分支不存在"处理
    let branch_ok: i64 = match crate::story_db::open_story_pool(story_id).await {
        Ok(pool) => sqlx::query_scalar("SELECT COUNT(*) FROM branches WHERE id = ?1")
            .bind(&stored)
            .fetch_one(&pool)
            .await
            .unwrap_or(0),
        Err(_) => 0,
    };
    if story_ok == 0 || branch_ok == 0 {
        let _ = sqlx::query("UPDATE stories SET current_branch_id = NULL WHERE id = ?1")
            .bind(story_id)
            .execute(&lib)
            .await;
        return Ok(None);
    }
    Ok(Some(stored))
}

/// 记住某故事当前所在分支（切换分支时调用）。
pub async fn set_current_branch(story_id: &str, branch_id: &Option<String>) -> Result<(), String> {
    let lib = crate::db::library_pool().await.map_err(|e| e.to_string())?;
    sqlx::query("UPDATE stories SET current_branch_id = ?1 WHERE id = ?2")
        .bind(branch_id.clone())
        .bind(story_id)
        .execute(&lib)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 某节点属于哪条分支（`None` = 主线）。节点不存在则报错。
pub async fn branch_of_node(pool: &SqlitePool, node_id: i64) -> Result<Option<String>, String> {
    let b: Option<Option<String>> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    match b {
        Some(v) => Ok(v),
        None => Err(format!("分叉点（节点 {node_id}）不存在")),
    }
}

/// 创建分支（对外命令用；内部开库）。
pub async fn create_branch(story_id: &str, name: &str, fork_node_id: i64) -> Result<Branch, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    create_branch_in(&pool, story_id, name, fork_node_id).await
}

/// `create_branch` 的核心（接收 pool）。
///
/// **为什么要拆出 `_in` 版本**：测试若走 `create_branch`，它会另开一个连接池，
/// 测试自己那个池读不到刚写入的行（跨池可见性）→ 断言直接 RowNotFound。
/// 接收 pool 之后，测试用同一个池就能立刻读到，测的仍是同一段逻辑。
pub async fn create_branch_in(
    pool: &SqlitePool,
    story_id: &str,
    name: &str,
    fork_node_id: i64,
) -> Result<Branch, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("分支名称不能为空".into());
    }

    // 分叉点必须存在；源分支 = 它所在的分支（不接受调用方指定，避免传错导致复制错线）
    let src_branch = branch_of_node(pool, fork_node_id).await?;

    // 颜色按已有分支数轮替
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM branches WHERE story_id = ?1")
        .bind(story_id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    let color = COLORS[(n as usize) % COLORS.len()].to_string();
    let id = uuid::Uuid::new_v4().to_string();

    // 同一事务：插入 + 复制。任何一步失败整体回滚。
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    sqlx::query(
        "INSERT INTO branches(id, story_id, name, fork_node_id, source_branch_id, color)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&id)
    .bind(story_id)
    .bind(name)
    .bind(fork_node_id)
    .bind(src_branch.clone())
    .bind(&color)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;

    // 复制世界状态。分两类：
    //   ① **角色**：逐行复制并记录 `旧 id → 新 id` 的映射（关系要用它重写两端）
    //   ② **关系**：逐行复制，并把 from/to 重写成新分支自己的角色 id
    //      （否则分支的关系会指向源分支的角色行 = 分支不自包含，实测踩到过）
    //   ③ 世界观 / 设定版本：无外键，直接整表复制（SQL 层 `INSERT ... SELECT`）
    //
    // 参数编号：?1 = 新分支 id（写入），?2 = 源分支 id（WHERE）。
    // **必须用编号参数**：SQLite 会把匿名 `?` 当成 `?1`，于是
    // `SELECT ..., ?1 ... WHERE branch_id = ?` 会让 WHERE 也绑上新分支 id
    // （此时新分支还没有行）→ 静默复制出 0 行。这个坑在嵌套分支测试里踩到过。
    // 角色/关系段是 SELECT 后逐行 INSERT，语句里**没有** ?1（新分支 id 是单独 bind 的），
    // 所以条件里的占位符从 ?1 开始编号最省心。
    let src_clause = crate::story_db::branch_clause("branch_id", &src_branch, 1);
    let src_bind = src_branch.clone().unwrap_or_default();

    // ① 角色（记录 id 映射）
    let mut char_id_map: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    {
        let select_sql = format!(
            "SELECT id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile,
                    speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion,
                    stat_screen, version, background_past, motivation, goal, dialogue_examples, behavior_rules
             FROM characters WHERE {src_clause} ORDER BY id"
        );
        let mut q = sqlx::query(&select_sql);
        if src_branch.is_some() {
            q = q.bind(&src_bind);
        }
        let rows = q.fetch_all(&mut *tx).await.map_err(|e| e.to_string())?;
        for r in &rows {
            let old_id: i64 = r.try_get(0).map_err(|e| e.to_string())?;
            let new_row = sqlx::query(
                "INSERT INTO characters(name, alias, role, faction, age, gender, first_appearance,
                    one_liner, profile, speech_style, traits, current_status, stat_stamina,
                    stat_sanity, stat_emotion, stat_screen, version,
                    background_past, motivation, goal, dialogue_examples, behavior_rules, branch_id)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23) RETURNING id",
            )
            .bind(r.try_get::<String, _>(1).unwrap_or_default())
            .bind(r.try_get::<String, _>(2).unwrap_or_default())
            .bind(r.try_get::<String, _>(3).unwrap_or_default())
            .bind(r.try_get::<String, _>(4).unwrap_or_default())
            .bind(r.try_get::<String, _>(5).unwrap_or_default())
            .bind(r.try_get::<String, _>(6).unwrap_or_default())
            .bind(r.try_get::<String, _>(7).unwrap_or_default())
            .bind(r.try_get::<String, _>(8).unwrap_or_default())
            .bind(r.try_get::<String, _>(9).unwrap_or_default())
            .bind(r.try_get::<String, _>(10).unwrap_or_default())
            .bind(r.try_get::<String, _>(11).unwrap_or_default())
            .bind(r.try_get::<String, _>(12).unwrap_or_default())
            .bind(r.try_get::<i64, _>(13).unwrap_or(50))
            .bind(r.try_get::<i64, _>(14).unwrap_or(50))
            .bind(r.try_get::<i64, _>(15).unwrap_or(50))
            .bind(r.try_get::<i64, _>(16).unwrap_or(50))
            .bind(r.try_get::<i64, _>(17).unwrap_or(1))
            .bind(r.try_get::<String, _>(18).unwrap_or_default())
            .bind(r.try_get::<String, _>(19).unwrap_or_default())
            .bind(r.try_get::<String, _>(20).unwrap_or_default())
            .bind(r.try_get::<String, _>(21).unwrap_or_default())
            .bind(r.try_get::<String, _>(22).unwrap_or_default())
            .bind(&id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
            char_id_map.insert(old_id, new_row.try_get(0).map_err(|e| e.to_string())?);
        }
    }

    // ② 关系（两端 id 重写为新分支的角色）
    {
        let select_sql = format!(
            "SELECT from_character_id, to_character_id, relation_type, direction, strength, state,
                    description, evidence, version
             FROM relations WHERE {src_clause} ORDER BY id"
        );
        let mut q = sqlx::query(&select_sql);
        if src_branch.is_some() {
            q = q.bind(&src_bind);
        }
        let rows = q.fetch_all(&mut *tx).await.map_err(|e| e.to_string())?;
        for r in &rows {
            let old_from: i64 = r.try_get(0).unwrap_or(0);
            let old_to: i64 = r.try_get(1).unwrap_or(0);
            // 映射不到（源分支里角色行缺失，罕见）就保留原值，不让整次创建失败
            let new_from = char_id_map.get(&old_from).copied().unwrap_or(old_from);
            let new_to = char_id_map.get(&old_to).copied().unwrap_or(old_to);
            sqlx::query(
                "INSERT INTO relations(from_character_id, to_character_id, relation_type, direction,
                    strength, state, description, evidence, version, branch_id)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            )
            .bind(new_from)
            .bind(new_to)
            .bind(r.try_get::<String, _>(2).unwrap_or_default())
            .bind(r.try_get::<String, _>(3).unwrap_or_default())
            .bind(r.try_get::<i64, _>(4).unwrap_or(50))
            .bind(r.try_get::<String, _>(5).unwrap_or_default())
            .bind(r.try_get::<String, _>(6).unwrap_or_default())
            .bind(r.try_get::<String, _>(7).unwrap_or_default())
            .bind(r.try_get::<i64, _>(8).unwrap_or(1))
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    // ③ 世界观 / 设定版本（无外键，直接整表复制）
    for (table, cols) in COPY_AS_IS_TABLES {
        // 临时设定版本不继承（见模块头注释）
        let extra = if *table == "setting_snapshots" {
            " AND COALESCE(temporary, 0) = 0"
        } else {
            ""
        };
        // 这一段有 ?1（写入的新分支 id），所以 WHERE 的条件从 ?2 起编号
        let sql = format!(
            "INSERT INTO {table} ({cols}, branch_id)
             SELECT {cols}, ?1 FROM {table}
             WHERE {} {extra}",
            crate::story_db::branch_clause("branch_id", &src_branch, 2)
        );
        sqlx::query(&sql)
            .bind(&id)
            .bind(&src_bind)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }

    tx.commit().await.map_err(|e| e.to_string())?;

    // 用同一个 pool 读回（不调 get_branch：那个会另开池，测试里读不到）
    get_branch_in(pool, &id).await
}

/// 第五批 ④：**改名与备注**（管理平行故事用）。
///
/// 为什么不校验重名：分支名只是给人看的标签，用户可能刻意用同名区分"两次尝试"；
/// 真正的身份是 `id`。所以只挡住"空名字"。
pub async fn rename_branch(story_id: &str, branch_id: &str, name: &str, note: &str) -> Result<(), String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    rename_branch_in(&pool, branch_id, name, note).await
}

/// `rename_branch` 的核心（接 pool，供测试直连）。
pub async fn rename_branch_in(pool: &SqlitePool, branch_id: &str, name: &str, note: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("分支名称不能为空".into());
    }
    let n = sqlx::query("UPDATE branches SET name = ?1, note = ?2 WHERE id = ?3")
        .bind(name)
        .bind(note.trim())
        .bind(branch_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if n == 0 {
        return Err("该分支不存在（可能已被删除）".into());
    }
    Ok(())
}

/// 删除分支（**只删这条分支自己的东西**，共享历史与主线一律不动）。
///
/// ## 安全检查（都是实测踩过或差点踩到的）
///
/// 1. **不能用"删分支的节点"来删** —— 分叉点属于**共享历史**（可能在主线上），
///    删它会连带把别的分支的起点也弄没（实测踩到：分支的分叉点被删 → 分支天数错乱）。
///    所以这里只删 `branch_id = 该分支` 的行，**分叉点不动**。
/// 2. **不能删还被别的分支依赖的分支** —— 若别的分支以"本分支的节点"为分叉点，
///    删掉后那些分支会失去起点。这种情况**直接拒绝并说明是哪条分支**。
/// 3. `stories.current_branch_id` 若指向被删分支 → **归主线**（否则界面拿到悬空引用会白屏）。
pub async fn delete_branch(story_id: &str, branch_id: &str) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    delete_branch_in(&pool, story_id, branch_id).await
}

/// `delete_branch` 的核心（接 pool，供测试直连）。
pub async fn delete_branch_in(pool: &SqlitePool, story_id: &str, branch_id: &str) -> Result<String, String> {
    let name: String = sqlx::query_scalar("SELECT name FROM branches WHERE id = ?1")
        .bind(branch_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "该分支不存在（可能已被删除）".to_string())?;

    // 检查 2：本分支的节点是否被别的分支当作分叉点
    let dependents: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM branches
         WHERE id <> ?1 AND fork_node_id IN (SELECT id FROM timeline_nodes WHERE branch_id = ?1)",
    )
    .bind(branch_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    // **真的拒绝**：别的分支以本分支的节点为起点时，删掉它那些分支就断了
    if !dependents.is_empty() {
        return Err(format!(
            "无法删除「{name}」：分支「{}」以它的节点为分叉点，删掉后那条线会失去起点。请先处理那些分支。",
            dependents.join("」「")
        ));
    }

    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;

    // 本分支自己的节点 + 它们的消息
    let node_ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM timeline_nodes WHERE branch_id = ?1")
        .bind(branch_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    for nid in &node_ids {
        sqlx::query("DELETE FROM messages WHERE node_id = ?1").bind(nid)
            .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    }
    sqlx::query("DELETE FROM timeline_nodes WHERE branch_id = ?1").bind(branch_id)
        .execute(&mut *tx).await.map_err(|e| e.to_string())?;

    // 本分支的世界状态副本（角色/关系/世界观/设定版本/提案/阶段摘要/审计）
    for table in [
        "characters", "relations", "world_settings", "setting_snapshots",
        "change_proposals", "story_summaries", "audit_logs", "event_logs",
    ] {
        let sql = format!("DELETE FROM {table} WHERE branch_id = ?1");
        let _ = sqlx::query(&sql).bind(branch_id).execute(&mut *tx).await;
    }

    sqlx::query("DELETE FROM branches WHERE id = ?1").bind(branch_id)
        .execute(&mut *tx).await.map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;

    // 检查 3：当前分支若指向它 → 归主线（否则界面拿到悬空引用会白屏，§10.28 的教训）
    if let Ok(lib) = crate::db::library_pool().await {
        let _ = sqlx::query(
            "UPDATE stories SET current_branch_id = NULL WHERE id = ?1 AND current_branch_id = ?2",
        )
        .bind(story_id)
        .bind(branch_id)
        .execute(&lib)
        .await;
    }

    Ok(format!("已删除分支「{name}」（{} 个节点）", node_ids.len()))
}

/// 列出某故事的全部分支（按创建时间升序）。
///
/// **主线不在此列表**：主线不是 `branches` 的一行，界面要把它自己补成第一项。
pub async fn list_branches(story_id: &str) -> Result<Vec<Branch>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    list_branches_in(&pool, story_id).await
}

/// `list_branches` 的核心（接收 pool），理由同 `create_branch_in`。
pub async fn list_branches_in(pool: &SqlitePool, story_id: &str) -> Result<Vec<Branch>, String> {
    let rows = sqlx::query(
        "SELECT id, story_id, name, fork_node_id, source_branch_id, color, created_at, note
         FROM branches WHERE story_id = ?1 ORDER BY created_at ASC, id ASC",
    )
    .bind(story_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for r in &rows {
        let mut b = row_to_branch(r).map_err(|e| e.to_string())?;
        b.node_count = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes WHERE branch_id = ?1")
            .bind(&b.id)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        out.push(b);
    }
    Ok(out)
}

/// 单个分支详情（对外命令用；先找到它属于哪个故事库）。
// 保留未用：旧入口（已被 resolve_branch 取代）
#[allow(dead_code)]
pub async fn get_branch(branch_id: &str) -> Result<Branch, String> {
    let (pool, _sid) = find_pool_for_branch(branch_id).await?;
    get_branch_in(&pool, branch_id).await
}

/// `get_branch` 的核心（接收 pool），理由同 `create_branch_in`。
pub async fn get_branch_in(pool: &SqlitePool, branch_id: &str) -> Result<Branch, String> {
    let r = sqlx::query(
        "SELECT id, story_id, name, fork_node_id, source_branch_id, color, created_at, note
         FROM branches WHERE id = ?1",
    )
    .bind(branch_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("分支 {branch_id} 不存在"))?;
    let mut b = row_to_branch(&r).map_err(|e| e.to_string())?;
    b.node_count = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes WHERE branch_id = ?1")
        .bind(branch_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    Ok(b)
}

/// 按分支 id 找到它所属的故事库（遍历 library 下的 story.db）。
///
/// 分支 id 是全局 UUID，但库里没有"分支 → 故事"的索引；MVP 阶段故事数量很少，
/// 遍历一次足够（每个库一次 COUNT 查询）。将来故事多了可在 library.db 加索引。
// 保留未用：旧入口（已被 resolve_branch 取代）
#[allow(dead_code)]
async fn find_pool_for_branch(branch_id: &str) -> Result<(SqlitePool, String), String> {
    // 第五批「路径模型」修复：库根统一走 paths（不再依赖进程当前目录）
    let root = crate::paths::library_root();
    let mut stack = vec![root];
    let mut dbs: Vec<std::path::PathBuf> = Vec::new();
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().map(|n| n == "story.db").unwrap_or(false) {
                dbs.push(p);
            }
        }
    }
    for db in dbs {
        let Ok(opts) = SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display())) else {
            continue;
        };
        let Ok(pool) = SqlitePool::connect_with(opts).await else { continue };
        let _ = crate::story_db::ensure_branch_columns(&pool).await;
        let hit: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM branches WHERE id = ?1")
            .bind(branch_id)
            .fetch_one(&pool)
            .await
            .unwrap_or(0);
        if hit > 0 {
            let sid = db
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            return Ok((pool, sid));
        }
    }
    Err(format!("找不到分支 {branch_id} 所属的故事库"))
}

#[cfg(test)]
mod tests;
