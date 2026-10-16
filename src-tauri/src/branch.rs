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

use crate::story_db::story_db_path;

async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;
    // 分支列与 branches 表（幂等）
    crate::story_db::ensure_branch_columns(&pool).await?;
    Ok(pool)
}

/// 分支预设配色池（按创建顺序轮替）
const COLORS: &[&str] = &["#3B82F6", "#F59E0B", "#10B981", "#EF4444", "#8B5CF6"];

/// 需要随分支复制的"世界状态"表 —— **写成常量数组**。
///
/// 为什么写成数组而不是散在代码里：将来给故事库加一张状态表时，只要在这里加一行，
/// 创建分支与测试都会自动覆盖到（测试就是按这个数组逐表断言的）。
/// 每项 = (表名, 除 branch_id 外要复制的列)。
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
    let branch_ok: i64 = match open_story_pool(story_id).await {
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
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
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
                    stat_screen, version
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
                    stat_sanity, stat_emotion, stat_screen, version, branch_id)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18) RETURNING id",
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
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
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
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
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
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
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
mod tests {
    use super::*;

    /// 造一个临时故事库，返回 (pool, story_id, 目录)。
    ///
    /// 注意：`story_db_path` 是从 `<cwd>/../library/default/projects/<story_id>/story.db` 推出来的，
    /// 所以测试要真在 `src-tauri` 下跑（cargo test 的 cwd 就是 src-tauri）。
    /// 故事 id 以 `-branch-test` 结尾，避免与真机测试（读故事 "7"）互相干扰。
    async fn temp_story(tag: &str) -> (SqlitePool, String, std::path::PathBuf) {
        let sid = format!("{tag}-branch-test");
        // **测试故事建在 `target/test-library` 下**（不是真实数据目录！）：
        // 早期版本用 `paths::story_db(...)` 推导 → 会把临时故事写进用户的 library，
        // 既污染真实数据，也可能因权限被拒（实测：PermissionDenied）。
        // 与 paths::story_db(sid) 的拼法一致：<数据根>/library/default/projects/<sid>
        let dir = crate::paths::test_root().join("library").join("default").join("projects").join(&sid);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("story.db");
        let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
                is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
                day_summary TEXT NOT NULL DEFAULT '', parent_id INTEGER, branch_id TEXT)",
            "CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER NOT NULL,
                role TEXT NOT NULL, content TEXT NOT NULL, mode TEXT NOT NULL DEFAULT 'chat',
                meta TEXT NOT NULL DEFAULT '{}', branch_id TEXT)",
            "CREATE TABLE characters (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                alias TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '', faction TEXT NOT NULL DEFAULT '',
                age TEXT NOT NULL DEFAULT '', gender TEXT NOT NULL DEFAULT '',
                first_appearance TEXT NOT NULL DEFAULT '', one_liner TEXT NOT NULL DEFAULT '',
                profile TEXT NOT NULL DEFAULT '', speech_style TEXT NOT NULL DEFAULT '',
                traits TEXT NOT NULL DEFAULT '', current_status TEXT NOT NULL DEFAULT '',
                stat_stamina INTEGER DEFAULT 50, stat_sanity INTEGER DEFAULT 50,
                stat_emotion INTEGER DEFAULT 50, stat_screen INTEGER DEFAULT 50,
                version INTEGER DEFAULT 1, branch_id TEXT)",
            "CREATE TABLE relations (id INTEGER PRIMARY KEY AUTOINCREMENT, from_character_id INTEGER,
                to_character_id INTEGER, relation_type TEXT DEFAULT '', direction TEXT DEFAULT '单向',
                strength INTEGER DEFAULT 50, state TEXT DEFAULT '稳定', description TEXT DEFAULT '',
                evidence TEXT DEFAULT '', version INTEGER DEFAULT 1, branch_id TEXT)",
            "CREATE TABLE world_settings (id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT, value TEXT,
                is_hard_rule INTEGER DEFAULT 0, version INTEGER DEFAULT 1, category TEXT DEFAULT '设定',
                meta_json TEXT DEFAULT '{}', branch_id TEXT)",
            "CREATE TABLE setting_snapshots (id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
                payload_json TEXT NOT NULL, parent_version INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                temporary INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '', branch_id TEXT)",
            "CREATE TABLE branches (id TEXT PRIMARY KEY, story_id TEXT NOT NULL, name TEXT NOT NULL,
                fork_node_id INTEGER NOT NULL, source_branch_id TEXT,
                color TEXT NOT NULL DEFAULT '#6B7280',
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                note TEXT NOT NULL DEFAULT '')",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        // 主线数据：3 节点（1→2→3）+ 2 角色 + 1 关系 + 2 世界观 + 2 版本（1 永久 1 临时）
        for (label, sort) in [("第 1 天", 1), ("第 2 天", 2), ("第 3 天", 3)] {
            sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id) VALUES(?1, ?2, NULL)")
                .bind(label).bind(sort).execute(&pool).await.unwrap();
        }
        sqlx::query("UPDATE timeline_nodes SET parent_id = 1 WHERE id = 2").execute(&pool).await.unwrap();
        sqlx::query("UPDATE timeline_nodes SET parent_id = 2 WHERE id = 3").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO characters(name, role) VALUES('夏婉琴', '女主')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO characters(name, role) VALUES('宸熙', '男主')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO relations(from_character_id, to_character_id, relation_type, strength) VALUES(2, 1, '追求者', 83)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO world_settings(key, value) VALUES('背景介绍', '修道院')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO world_settings(key, value) VALUES('修女宿舍', '二楼')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO setting_snapshots(version, payload_json, temporary) VALUES(1, '{}', 0)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO setting_snapshots(version, payload_json, temporary, note) VALUES(2, '{}', 1, '编辑前自动备份')")
            .execute(&pool).await.unwrap();
        (pool, sid, dir)
    }

    /// 删掉临时故事目录。**测试不该在用户的 library/ 里留垃圾**。
    /// 必须在 pool 关闭之后删（Windows 会锁住打开着的 db 文件）。
    async fn cleanup(pool: SqlitePool, dir: std::path::PathBuf) {
        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn count(pool: &SqlitePool, sql: &str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(pool).await.unwrap_or(-1)
    }

    /// 在分支 A 里加一个节点并改一个角色名，返回该节点 id（供嵌套分支测试用）
    async fn seed_branch_a(pool: &SqlitePool, a_id: &str) -> i64 {
        sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id, branch_id) VALUES('第 3 天', 4, 2, ?1)")
            .bind(a_id).execute(pool).await.unwrap();
        sqlx::query("UPDATE characters SET name = '夏婉琴（已牺牲）' WHERE branch_id = ?1")
            .bind(a_id).execute(pool).await.unwrap();
        sqlx::query_scalar("SELECT id FROM timeline_nodes WHERE branch_id = ?1")
            .bind(a_id).fetch_one(pool).await.unwrap()
    }

    /// 创建 → 列表 → 详情
    #[tokio::test]
    async fn create_list_get_branch() {
        let (pool, sid, dir) = temp_story("crud").await;
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM branches").await, 0);

        let b = create_branch_in(&pool, &sid, "角色A牺牲线", 2).await.unwrap();
        assert_eq!(b.name, "角色A牺牲线");
        assert_eq!(b.fork_node_id, 2);
        assert_eq!(b.source_branch_id, None, "从主线分叉时源分支为 NULL");
        assert_eq!(b.color, COLORS[0], "第一个分支用配色池第一色");
        assert_eq!(b.node_count, 0, "新分支暂时没有自己的节点");

        let list = list_branches_in(&pool, &sid).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, b.id);

        let got = get_branch_in(&pool, &b.id).await.unwrap();
        assert_eq!(got.id, b.id);
        assert_eq!(got.name, "角色A牺牲线");
        cleanup(pool, dir).await;
    }

    /// **复制世界状态**：四张表的行数必须与原分支一致（临时版本除外）
    #[tokio::test]
    async fn create_branch_copies_world_state() {
        let (pool, sid, dir) = temp_story("copy").await;
        let b = create_branch_in(&pool, &sid, "分支甲", 2).await.unwrap();

        for (table, _cols) in WORLD_STATE_TABLES {
            let main = count(&pool, &format!("SELECT COUNT(*) FROM {table} WHERE branch_id IS NULL")).await;
            let copy = count(&pool, &format!("SELECT COUNT(*) FROM {table} WHERE branch_id = '{}'", b.id)).await;
            if *table == "setting_snapshots" {
                assert_eq!(main, 2, "主线有 2 个版本（1 永久 + 1 临时）");
                assert_eq!(copy, 1, "**临时版本不该被复制**，只复制 1 个永久版本");
            } else {
                assert_eq!(copy, main, "{table} 应完整复制（{main} 行）");
            }
        }
        let name: String = sqlx::query_scalar(&format!(
            "SELECT name FROM characters WHERE branch_id = '{}' ORDER BY id LIMIT 1", b.id))
            .fetch_one(&pool).await.unwrap();
        assert_eq!(name, "夏婉琴", "复制的是内容而非空壳");
        // 新行的 id 必须是新的（数据库自增），不能沿用原 id
        let ids: Vec<i64> = sqlx::query_scalar(&format!(
            "SELECT id FROM characters WHERE branch_id = '{}' ORDER BY id", b.id))
            .fetch_all(&pool).await.unwrap();
        let main_ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM characters WHERE branch_id IS NULL ORDER BY id")
            .fetch_all(&pool).await.unwrap();
        assert_ne!(ids, main_ids, "复制出的行必须是新 id（否则两分支会共用同一行）");
        cleanup(pool, dir).await;
    }

    /// 主线数据在创建分支后**一行不少、内容不变**
    #[tokio::test]
    async fn main_branch_is_untouched() {
        let (pool, sid, dir) = temp_story("untouched").await;
        let before = count(&pool, "SELECT COUNT(*) FROM characters WHERE branch_id IS NULL").await;
        create_branch_in(&pool, &sid, "分支乙", 3).await.unwrap();
        let after = count(&pool, "SELECT COUNT(*) FROM characters WHERE branch_id IS NULL").await;
        assert_eq!(before, after, "主线角色数不该变");
        assert_eq!(count(&pool, "SELECT COUNT(*) FROM timeline_nodes WHERE branch_id IS NULL").await, 3,
            "主线节点不该变");
        cleanup(pool, dir).await;
    }

    /// **嵌套分支**：从分支内的节点再分叉 → 源分支 = 那个分支，状态从它复制
    #[tokio::test]
    async fn nested_branch_copies_from_its_source() {
        let (pool, sid, dir) = temp_story("nested").await;
        let a = create_branch_in(&pool, &sid, "分支A", 2).await.unwrap();
        let node_a = seed_branch_a(&pool, &a.id).await;

        let b = create_branch_in(&pool, &sid, "分支A2", node_a).await.unwrap();
        assert_eq!(b.source_branch_id.as_deref(), Some(a.id.as_str()),
            "嵌套分支的源分支应是被分叉的那个分支");

        let name_b: String = sqlx::query_scalar(&format!(
            "SELECT name FROM characters WHERE branch_id = '{}' LIMIT 1", b.id))
            .fetch_one(&pool).await.unwrap();
        assert_eq!(name_b, "夏婉琴（已牺牲）", "嵌套分支应从源分支复制状态");
        let name_main: String = sqlx::query_scalar(
            "SELECT name FROM characters WHERE branch_id IS NULL LIMIT 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(name_main, "夏婉琴", "主线不受任何分支影响");
        cleanup(pool, dir).await;
    }

    /// **嵌套分支也要逐表断言行数**（不只是"角色名字对"）。
    ///
    /// 为什么单独加这一条：主线场景的 WHERE 是 `branch_id IS NULL`（**没有占位符**），
    /// 所以"匿名 `?` 与 `?1` 冲突"那个 bug 在主线场景**测不出来**、测试会照常通过。
    /// 只有源分支非 NULL（嵌套）时 WHERE 才用到占位符 —— 那种情况下
    /// `SELECT ..., ?1 ... WHERE branch_id = ?` 会让 WHERE 绑上新分支 id（此时新分支还没有行）
    /// → **静默复制出 0 行**（数据丢失但不报错）。这条测试就是为那个坑守门的。
    #[tokio::test]
    async fn nested_copy_copies_full_world_state() {
        let (pool, sid, dir) = temp_story("nested-copy").await;
        let a = create_branch_in(&pool, &sid, "分支A", 2).await.unwrap();
        let node_a = seed_branch_a(&pool, &a.id).await;
        let b = create_branch_in(&pool, &sid, "分支A2", node_a).await.unwrap();

        for (table, _cols) in WORLD_STATE_TABLES {
            let src = count(&pool, &format!("SELECT COUNT(*) FROM {table} WHERE branch_id = '{}'", a.id)).await;
            let dst = count(&pool, &format!("SELECT COUNT(*) FROM {table} WHERE branch_id = '{}'", b.id)).await;
            assert!(src > 0, "{table}：源分支 A 应当有数据（否则这条断言没意义）");
            assert_eq!(dst, src,
                "{table}：嵌套分支应完整复制源分支的 {src} 行 —— 若为 0 说明 WHERE 的占位符绑错了");
        }
        let a_after = count(&pool, &format!("SELECT COUNT(*) FROM characters WHERE branch_id = '{}'", a.id)).await;
        assert_eq!(a_after, 2, "源分支 A 的角色数不该变（复制只往目标分支写）");
        cleanup(pool, dir).await;
    }

    /// 校验与错误处理
    #[tokio::test]
    async fn rejects_bad_input() {
        let (pool, sid, dir) = temp_story("bad").await;
        assert!(create_branch_in(&pool, &sid, "   ", 2).await.is_err(), "空名称应被拒绝");
        let e = create_branch_in(&pool, &sid, "分支", 999).await.unwrap_err();
        assert!(e.contains("不存在"), "分叉点不存在应报错，实际：{e}");
        cleanup(pool, dir).await;
    }

    /// 颜色按创建顺序轮替
    #[tokio::test]
    async fn colors_rotate() {
        let (pool, sid, dir) = temp_story("colors").await;
        let mut got = Vec::new();
        for i in 0..(COLORS.len() + 1) {
            got.push(create_branch_in(&pool, &sid, &format!("分支{i}"), 2).await.unwrap().color);
        }
        assert_eq!(got[0], COLORS[0]);
        assert_eq!(got[1], COLORS[1]);
        assert_eq!(got[COLORS.len()], COLORS[0], "用满一轮后回到第一色");
        cleanup(pool, dir).await;
    }
    /// **分支隔离的端到端测试（第 3 步的核心验收）**：
    /// 建分支 → 在分支里改角色 → 主线不受影响、另一分支也看不到改动。
    #[tokio::test]
    async fn branch_character_isolation() {
        let (pool, sid, dir) = temp_story("isolate").await;
        let input = crate::characters::CharacterInput {
            name: "夏婉琴".into(),
            ..Default::default()
        };

        // 基线：主线看到 2 个角色
        let main_before = crate::characters::list_characters_in(&pool, &None).await.unwrap();
        assert_eq!(main_before.len(), 2, "主线应有 2 个角色");

        // 建两个分支（都从主线分叉）
        let a = create_branch_in(&pool, &sid, "分支A", 2).await.unwrap();
        let b = create_branch_in(&pool, &sid, "分支B", 2).await.unwrap();
        let ba = Some(a.id.clone());
        let bb = Some(b.id.clone());

        // 三个"世界"各自都能看到 2 个角色（复制完整）
        for (tag, br) in [("主线", None), ("分支A", ba.clone()), ("分支B", bb.clone())] {
            let n = crate::characters::list_characters_in(&pool, &br).await.unwrap().len();
            assert_eq!(n, 2, "{tag} 应有 2 个角色");
        }

        // 在分支 A 里把第一个角色改名 + 改属性
        let target = crate::characters::list_characters_in(&pool, &ba).await.unwrap()[0].clone();
        let mut edited = input.clone();
        edited.name = "夏婉琴（分支A版）".into();
        edited.role = "已牺牲".into();
        crate::characters::update_character_in(&pool, &ba, target.id, &edited).await.unwrap();

        // 断言隔离
        let name_a = &crate::characters::list_characters_in(&pool, &ba).await.unwrap()[0].name;
        assert_eq!(name_a, "夏婉琴（分支A版）", "分支 A 应看到自己的改动");

        let names_main: Vec<String> = crate::characters::list_characters_in(&pool, &None)
            .await.unwrap().into_iter().map(|c| c.name).collect();
        assert!(names_main.iter().all(|n| n == "夏婉琴" || n == "宸熙"),
            "**主线不该被分支的改动影响**，实际：{names_main:?}");

        let names_b: Vec<String> = crate::characters::list_characters_in(&pool, &bb)
            .await.unwrap().into_iter().map(|c| c.name).collect();
        assert!(names_b.iter().all(|n| n == "夏婉琴" || n == "宸熙"),
            "**分支 B 不该看到分支 A 的改动**，实际：{names_b:?}");

        // 在分支 A 里新增一个角色：只该出现在分支 A
        let mut newc = input.clone();
        newc.name = "分支A专属角色".into();
        crate::characters::create_character_in(&pool, &ba, &newc).await.unwrap();
        assert_eq!(crate::characters::list_characters_in(&pool, &ba).await.unwrap().len(), 3,
            "分支 A 应有 3 个角色");
        assert_eq!(crate::characters::list_characters_in(&pool, &None).await.unwrap().len(), 2,
            "主线仍应是 2 个角色");
        assert_eq!(crate::characters::list_characters_in(&pool, &bb).await.unwrap().len(), 2,
            "分支 B 仍应是 2 个角色");

        // 删除也只在本分支生效
        crate::characters::delete_character_in(&pool, &ba, target.id).await.unwrap();
        assert_eq!(crate::characters::list_characters_in(&pool, &ba).await.unwrap().len(), 2);
        assert_eq!(crate::characters::list_characters_in(&pool, &None).await.unwrap().len(), 2,
            "主线不受分支删除影响");

        cleanup(pool, dir).await;
    }

    /// **关系与世界观的隔离测试**（第 3 步 3-2 批验收）。
    #[tokio::test]
    async fn branch_relation_and_world_isolation() {
        let (pool, sid, dir) = temp_story("isolate2").await;
        let a = create_branch_in(&pool, &sid, "分支A", 2).await.unwrap();
        let ba = Some(a.id.clone());

        // ---- 关系 ----
        let main_rels = crate::relations::list_relations_in(&pool, &None).await.unwrap();
        let a_rels = crate::relations::list_relations_in(&pool, &ba).await.unwrap();
        assert_eq!(main_rels.len(), 1, "主线 1 条关系");
        assert_eq!(a_rels.len(), 1, "分支 A 应复制到 1 条关系");
        assert_ne!(main_rels[0].id, a_rels[0].id, "分支里的关系必须是新行（新 id）");
        // 分支里的关系应指向**分支自己的角色行**（而不是主线的角色行）
        let branch_char_ids: Vec<i64> = crate::characters::list_characters_in(&pool, &ba)
            .await.unwrap().into_iter().map(|c| c.id).collect();
        let main_char_ids: Vec<i64> = crate::characters::list_characters_in(&pool, &None)
            .await.unwrap().into_iter().map(|c| c.id).collect();
        assert!(branch_char_ids.contains(&a_rels[0].from_character_id),
            "分支关系的 from 应指向分支自己的角色（id 集合不相交）");
        assert!(!main_char_ids.contains(&a_rels[0].from_character_id),
            "**不该指到主线的角色行**");

        // 在分支 A 改强度 → 主线不变
        crate::relations::update_relation_in(&pool, &ba, a_rels[0].id, "追求者", "单向", 30, "紧张", "改了", "")
            .await.unwrap();
        let a2 = crate::relations::list_relations_in(&pool, &ba).await.unwrap();
        assert_eq!(a2[0].strength, 30, "分支 A 应看到改动");
        let m2 = crate::relations::list_relations_in(&pool, &None).await.unwrap();
        assert_eq!(m2[0].strength, 83, "**主线强度不该变**");

        // 在分支 A 新增一条关系 → 只影响分支 A
        crate::relations::create_relation_in(&pool, &ba, 1, 2, "朋友", "双向", 60, "稳定", "", "")
            .await.unwrap();
        assert_eq!(crate::relations::list_relations_in(&pool, &ba).await.unwrap().len(), 2);
        assert_eq!(crate::relations::list_relations_in(&pool, &None).await.unwrap().len(), 1,
            "主线关系数不变");

        // ---- 世界观 ----
        let main_w = crate::world::list_world_settings_in(&pool, &None).await.unwrap();
        let a_w = crate::world::list_world_settings_in(&pool, &ba).await.unwrap();
        assert_eq!(main_w.len(), 2, "主线 2 条世界观");
        assert_eq!(a_w.len(), 2, "分支 A 应复制到 2 条世界观");

        // 改分支 A 的一条
        crate::world::update_world_setting_in(
            &pool, &ba, a_w[0].id, "背景介绍", "改过的修道院", false, "设定", "{}").await.unwrap();
        let main_after = crate::world::list_world_settings_in(&pool, &None).await.unwrap();
        assert!(main_after.iter().any(|w| w.value == "修道院"),
            "**主线世界观不该被分支改动影响**");
        let a_after = crate::world::list_world_settings_in(&pool, &ba).await.unwrap();
        assert!(a_after.iter().any(|w| w.value == "改过的修道院"), "分支 A 应看到自己的改动");

        // 在分支 A 新增世界观 → 只影响分支 A
        crate::world::create_world_setting_in(&pool, &ba, "分支A专属地点", "地窖", false, "地理", "{}")
            .await.unwrap();
        assert_eq!(crate::world::list_world_settings_in(&pool, &ba).await.unwrap().len(), 3);
        assert_eq!(crate::world::list_world_settings_in(&pool, &None).await.unwrap().len(), 2,
            "主线世界观数不变");

        // 跨分支误改防护：拿主线的 id 去分支 A 里更新 → 不该改动任何东西
        let main_id = main_after[0].id;
        crate::world::update_world_setting_in(
            &pool, &ba, main_id, "不该生效", "不该生效", false, "设定", "{}").await.unwrap();
        let main_check = crate::world::list_world_settings_in(&pool, &None).await.unwrap();
        assert!(main_check.iter().all(|w| w.key != "不该生效"),
            "**带分支条件的更新不该改到主线的行**");

        cleanup(pool, dir).await;
    }

    /// **复现用户实测的那个 bug**（2026-10-16）：
    /// 在分支里加了一天、在主线里加一天 —— 两边的"下一天"必须各按各的线算，
    /// 且新节点的父必须指向**自己那条线**的最后一个节点（不能跨线挂）。
    ///
    /// 用户原话：分支里新建"第 12 天"、说话，切回主线后时间维里**还有**第 12 天，
    /// 而且还能在它上面发消息。根因两条：
    ///   ① 前端默认节点取的是"全局最后一个节点"（别的分支的）→ 已在前端修
    ///   ② 建节点时"最后一天 / 父节点"没有限分支 → 本测试守这条
    #[tokio::test]
    async fn new_day_is_scoped_to_its_own_branch() {
        let (pool, sid, dir) = temp_story("nextday").await;
        // 在分类"测试A"里加一天（模拟：分支 A 从第 3 天分叉）
        let a = create_branch_in(&pool, &sid, "测试A", 3).await.unwrap();
        let ba = Some(a.id.clone());

        // 分支里加一天
        // 分支里加一天：分叉点是第 3 天（共享历史）→ 新节点应是**第 4 天**
        let n_a = crate::timeline::create_next_day_node_in(&pool, &ba).await.unwrap();
        assert_eq!(n_a.date_label, "第 4 天",
            "分支里应接第 4 天（分叉点第 3 天算共享历史，基准含它）");
        assert_eq!(n_a.parent_id, Some(3), "分支第一个自己的节点，父应是分叉点（第 3 天）");

        // 主线里加一天：同样接第 4 天（两条线各自数天数）
        let n_main = crate::timeline::create_next_day_node_in(&pool, &None).await.unwrap();
        assert_eq!(n_main.date_label, "第 4 天", "主线自己也应是第 4 天");
        assert_eq!(n_main.parent_id, Some(3), "主线新节点的父应是主线的第 3 天");

        // 分支里再加一天 → 第 5 天，父是本分支上一天
        let n_a2 = crate::timeline::create_next_day_node_in(&pool, &ba).await.unwrap();
        assert_eq!(n_a2.date_label, "第 5 天", "分支的第二天应接第 5 天");
        assert_eq!(n_a2.parent_id, Some(n_a.id), "父应是分支自己的上一天");
        // 主线不受影响，仍是第 4 天为最后
        let n_main2 = crate::timeline::create_next_day_node_in(&pool, &None).await.unwrap();
        assert_eq!(n_main2.date_label, "第 5 天", "主线的第二天也应接第 5 天");

        // 两条线各有自己的"第 4 天"，互不干扰
        let a_days: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM timeline_nodes WHERE branch_id = '{}'", a.id)).fetch_one(&pool).await.unwrap();
        assert_eq!(a_days, 2, "分支里应有 2 个自己的节点");
        let main_days: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM timeline_nodes WHERE branch_id IS NULL").fetch_one(&pool).await.unwrap();
        assert_eq!(main_days, 5, "主线应是原有 3 个 + 新 2 个");

        // **父链不得跨线**：分支节点的父必须是分支自己的节点或分叉点，不能是主线的第 4 天
        let main_day4: i64 = sqlx::query_scalar(
            "SELECT id FROM timeline_nodes WHERE branch_id IS NULL AND date_label = '第 4 天'")
            .fetch_one(&pool).await.unwrap();
        let a_parent: Option<i64> = sqlx::query_scalar(&format!(
            "SELECT parent_id FROM timeline_nodes WHERE branch_id = '{}' ORDER BY sort_order LIMIT 1", a.id))
            .fetch_one(&pool).await.unwrap();
        assert_ne!(a_parent, Some(main_day4), "**分支节点的父不能是主线的同一天节点**");

        cleanup(pool, dir).await;
    }

    /// **真机集成测试（ignored）**：走完整代码路径 —— set_current_branch → 建节点 → 看落在哪个分支。
    ///
    /// 为什么单独写这条：单测里 `_in` 直接收 branch 参数，**跳过了 `resolve_branch`**；
    /// 而用户实测踩到的正是"解析出来是 NULL"这一环。这条必须走真实函数。
    /// 跑法：cargo test --lib live_switch_then_add_day -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn live_switch_then_add_day() {
        // 1) 造临时故事库 + 主线 3 天（父链 1→2→3）。
        //    **故事目录名必须用数字 id**：`stories.id` 是 INTEGER，且 story_db_path 用
        //    故事目录名当 id（两者必须一致）—— 用带字母的目录名会 datatype mismatch。
        let story_num: i64 = 9_900_001;
        let sid = story_num.to_string();
        // 目录名就是数字 id（与 story_db_path 的推导一致）
        // 故事目录就在"数据根"下 —— 与 story_db_path 用同一套解析（paths::story_db）
        let dir = crate::paths::story_db(&sid).parent().unwrap().to_path_buf();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        {
            let opts = SqliteConnectOptions::new().filename(dir.join("story.db")).create_if_missing(true);
            let tmp = SqlitePool::connect_with(opts).await.unwrap();
            for ddl in [
                "CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
                    sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
                    is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
                    day_summary TEXT NOT NULL DEFAULT '', parent_id INTEGER, branch_id TEXT)",
                "CREATE TABLE branches (id TEXT PRIMARY KEY, story_id TEXT NOT NULL, name TEXT NOT NULL,
                    fork_node_id INTEGER NOT NULL, source_branch_id TEXT,
                    color TEXT NOT NULL DEFAULT '#6B7280',
                    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
                // create_branch_in 要复制这四张表 → 测试库必须有
                "CREATE TABLE characters (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
                    alias TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '', faction TEXT NOT NULL DEFAULT '',
                    age TEXT NOT NULL DEFAULT '', gender TEXT NOT NULL DEFAULT '',
                    first_appearance TEXT NOT NULL DEFAULT '', one_liner TEXT NOT NULL DEFAULT '',
                    profile TEXT NOT NULL DEFAULT '', speech_style TEXT NOT NULL DEFAULT '',
                    traits TEXT NOT NULL DEFAULT '', current_status TEXT NOT NULL DEFAULT '',
                    stat_stamina INTEGER DEFAULT 50, stat_sanity INTEGER DEFAULT 50,
                    stat_emotion INTEGER DEFAULT 50, stat_screen INTEGER DEFAULT 50,
                    version INTEGER DEFAULT 1, branch_id TEXT)",
                "CREATE TABLE relations (id INTEGER PRIMARY KEY AUTOINCREMENT, from_character_id INTEGER,
                    to_character_id INTEGER, relation_type TEXT DEFAULT '', direction TEXT DEFAULT '单向',
                    strength INTEGER DEFAULT 50, state TEXT DEFAULT '稳定', description TEXT DEFAULT '',
                    evidence TEXT DEFAULT '', version INTEGER DEFAULT 1, branch_id TEXT)",
                "CREATE TABLE world_settings (id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT, value TEXT,
                    is_hard_rule INTEGER DEFAULT 0, version INTEGER DEFAULT 1, category TEXT DEFAULT '设定',
                    meta_json TEXT DEFAULT '{}', branch_id TEXT)",
                "CREATE TABLE setting_snapshots (id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
                    payload_json TEXT NOT NULL, parent_version INTEGER,
                    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                    temporary INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '', branch_id TEXT)",
            ] {
                sqlx::query(ddl).execute(&tmp).await.unwrap();
            }
            sqlx::query("INSERT INTO characters(name, role) VALUES('主线甲','女主')").execute(&tmp).await.unwrap();
            for (label, sort) in [("第 1 天", 1), ("第 2 天", 2), ("第 3 天", 3)] {
                sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id) VALUES(?1, ?2, NULL)")
                    .bind(label).bind(sort).execute(&tmp).await.unwrap();
            }
            sqlx::query("UPDATE timeline_nodes SET parent_id = 1 WHERE id = 2").execute(&tmp).await.unwrap();
            sqlx::query("UPDATE timeline_nodes SET parent_id = 2 WHERE id = 3").execute(&tmp).await.unwrap();
            tmp.close().await;
        }
        let opts2 = SqliteConnectOptions::new().filename(dir.join("story.db"));
        let pool = SqlitePool::connect_with(opts2).await.unwrap();
        // 2) 在真实 library.db 里为它建一行故事记录（resolve_branch 要从那里读 current_branch_id）
        let lib = crate::db::library_pool().await.unwrap();
        sqlx::query("DELETE FROM stories WHERE id = ?1").bind(story_num).execute(&lib).await.unwrap();
        sqlx::query("INSERT INTO stories(id, title, time_origin) VALUES(?1, ?2, '现代')")
            .bind(story_num).bind("live-switch 测试").execute(&lib).await.unwrap();

        // 3) 建分支（fork 到第 3 天）
        let b = create_branch_in(&pool, &sid, "测试分支", 3).await.unwrap();
        println!("[live] 分支已建 id={}", b.id);

        // 4) 设定当前分支（真实函数）
        crate::branch::set_current_branch(&sid, &Some(b.id.clone())).await.unwrap();

        // 5) **从 library.db 重新读一遍**（确认真的写进去了）
        let read_back = crate::branch::resolve_branch(&sid, &None).await.unwrap();
        println!("[live] set 之后 resolve_branch 读到 = {:?}", read_back);
        assert_eq!(read_back.as_deref(), Some(b.id.as_str()),
            "**set_current_branch 之后必须能读回来**（读不到 = 写入没生效）");

        // 6) 真实命令路径：create_next_day_node（内部自己 resolve 分支）
        //    注意：它开的是自己的 pool（story_db_path(sid)），而临时库就在那个路径上 → 可以直连
        let n = crate::timeline::create_next_day_node(&sid).await.unwrap();
        println!("[live] 新节点 id={} label={} parent={:?}", n.id, n.date_label, n.parent_id);

        // 7) 断言：新节点属于分支，且父 = 分叉点（第 3 天）
        let nb: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
            .bind(n.id).fetch_one(&pool).await.unwrap();
        println!("[live] 新节点的 branch_id = {:?}", nb);
        assert_eq!(nb.as_deref(), Some(b.id.as_str()),
            "**在分支里点「下一天」，新节点必须属于该分支**（落成主线 = 用户看到的带锁节点）");
        assert_eq!(n.parent_id, Some(3), "父应是分叉点");

        // 8) 切回主线，再建一天 → 必须落在主线
        crate::branch::set_current_branch(&sid, &None).await.unwrap();
        let n2 = crate::timeline::create_next_day_node(&sid).await.unwrap();
        let nb2: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
            .bind(n2.id).fetch_one(&pool).await.unwrap();
        println!("[live] 切回主线后新节点 branch_id = {:?}", nb2);
        assert_eq!(nb2, None, "切回主线后新建节点必须落在主线（branch_id = NULL）");

        // 清理
        sqlx::query("DELETE FROM stories WHERE id = ?1").bind(story_num).execute(&lib).await.unwrap();
        cleanup(pool, dir).await;
    }

    /// **复现用户第三轮实测的 bug**：分支里「+ 下一天」建出来的节点标签是"第 1 天"。
    ///
    /// 用户的真实场景：主线有第 1~11 天，在第 11 天建分支 → 分支里点「+ 下一天」
    /// 期望"第 12 天"，实际得到"第 1 天"（且带 🔒 = 落在分支上，标签却错）。
    #[tokio::test]
    async fn branch_next_day_counts_shared_history() {
        let (pool, sid, dir) = temp_story("shared-count").await;
        // 主线补到第 11 天（temp_story 只有 3 天）
        for d in 4..=11 {
            let last: i64 = sqlx::query_scalar(
                "SELECT id FROM timeline_nodes WHERE branch_id IS NULL ORDER BY sort_order DESC LIMIT 1")
                .fetch_one(&pool).await.unwrap();
            let maxo: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sort_order),0) FROM timeline_nodes")
                .fetch_one(&pool).await.unwrap();
            sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id) VALUES(?1, ?2, ?3)")
                .bind(format!("第 {d} 天")).bind(maxo + 1).bind(last).execute(&pool).await.unwrap();
        }
        let day11: i64 = sqlx::query_scalar(
            "SELECT id FROM timeline_nodes WHERE branch_id IS NULL AND date_label = '第 11 天'")
            .fetch_one(&pool).await.unwrap();

        // 在第 11 天建分支
        let b = create_branch_in(&pool, &sid, "第三轮测试A", day11).await.unwrap();
        let bb = Some(b.id.clone());
        assert_eq!(b.node_count, 0, "新分支还没有自己的节点");

        // **分支里点「+ 下一天」** → 必须是"第 12 天"（分叉点第 11 天 + 1）
        let n = crate::timeline::create_next_day_node_in(&pool, &bb).await.unwrap();
        println!("[test] 分支里建出的节点：label={} parent={:?}", n.date_label, n.parent_id);
        assert_eq!(n.date_label, "第 12 天",
            "**分支里第一个自己的节点应是第 12 天**（分叉点第 11 天算共享历史）");
        assert_eq!(n.parent_id, Some(day11), "父应是分叉点（第 11 天）");

        // 再加一天 → 第 13 天
        let n2 = crate::timeline::create_next_day_node_in(&pool, &bb).await.unwrap();
        assert_eq!(n2.date_label, "第 13 天", "分支的第二天应接第 13 天");
        assert_eq!(n2.parent_id, Some(n.id));

        // 主线不受影响：它的下一天也应是第 12 天
        let m = crate::timeline::create_next_day_node_in(&pool, &None).await.unwrap();
        assert_eq!(m.date_label, "第 12 天", "主线的下一天也应是第 12 天（各自数）");

        cleanup(pool, dir).await;
    }

    /// **复现用户第三轮实测的 bug**：删除分叉点后，分支的"下一天"变成"第 1 天"。
    ///
    /// 用户的真实数据：主线第 1~11 天 → 在第 11 天（节点 55）建"第三轮测试分支A"
    /// → 之后**第 11 天被删掉了** → 分支里点「+ 下一天」建出"第 1 天"（父指向不存在的 55）。
    ///
    /// 本测试钉两件事：
    /// ① **不允许删除被分支当作分叉点的节点**（从源头堵住"半坏"数据）
    /// ② 万一历史数据已经是"半坏"状态，天数也要**接着主线数**（兜底），不能重头数成第 1 天
    #[tokio::test]
    async fn deleting_fork_point_is_refused_and_degrades_gracefully() {
        let (pool, sid, dir) = temp_story("fork-guard").await;
        // 在第 3 天建分支
        let b = create_branch_in(&pool, &sid, "分支X", 3).await.unwrap();
        let bb = Some(b.id.clone());

        // ① 删分叉点必须被拒绝
        let e = crate::timeline::delete_timeline_node_in(&pool, 3).await.unwrap_err();
        let msg = e.to_string();
        println!("[test] 删分叉点的返回：{msg}");
        assert!(msg.contains("分叉点"), "错误信息应说明原因，实际：{msg}");
        assert!(msg.contains("分支X"), "错误信息应指出是哪条分支，实际：{msg}");
        // 节点必须还在
        let still: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes WHERE id = 3")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(still, 1, "分叉点不能被删掉");

        // 删**非**分叉点仍然允许（不能因为加了守卫就什么都删不了）
        sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id) VALUES('第 4 天', 9, 3)")
            .execute(&pool).await.unwrap();
        let four: i64 = sqlx::query_scalar("SELECT id FROM timeline_nodes WHERE date_label = '第 4 天'")
            .fetch_one(&pool).await.unwrap();
        crate::timeline::delete_timeline_node_in(&pool, four).await.unwrap();
        let gone: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes WHERE id = ?1")
            .bind(four).fetch_one(&pool).await.unwrap();
        assert_eq!(gone, 0, "非分叉点应能正常删除");

        // ② 手工把分叉点删掉（模拟历史"半坏"数据），兜底必须让天数接着主线数
        sqlx::query("DELETE FROM timeline_nodes WHERE id = 3").execute(&pool).await.unwrap();
        let n = crate::timeline::create_next_day_node_in(&pool, &bb).await.unwrap();
        println!("[test] 分叉点丢失后建出的节点：label={}", n.date_label);
        // 此时主线只剩「第 1 天」「第 2 天」两个天数节点（第 3 天被手工删了）
        // → 兜底把"主线最后一个节点"当起点 → 接着数就是第 3 天。
        // **关键是它不再是"第 1 天"** —— 说明兜底生效（没有从 0 重新数）。
        assert_eq!(n.date_label, "第 3 天",
            "分叉点丢失时应退化成'接着主线数'（旧行为会重头数成'第 1 天'）");

        cleanup(pool, dir).await;
    }

    /// **复现用户第四轮实测的 bug**：分支里连点「+ 下一天」出现**两个"第 12 天"**。
    ///
    /// 根因：`branch_path_ids` 返回的是"祖先"（不含起点自己）→ 分支已有"第 12 天"时，
    /// 基准里最大只剩"第 11 天" → max+1 = 12 → **又建了一个"第 12 天"**。
    ///
    /// 本测试连续加 3 天，断言**标签严格递增且无重复** —— 这类"差一"问题靠单次断言抓不住。
    #[tokio::test]
    async fn branch_days_never_repeat() {
        let (pool, sid, dir) = temp_story("no-dup").await;
        // 主线补到第 11 天
        for d in 4..=11 {
            let last: i64 = sqlx::query_scalar(
                "SELECT id FROM timeline_nodes WHERE branch_id IS NULL ORDER BY sort_order DESC LIMIT 1")
                .fetch_one(&pool).await.unwrap();
            let maxo: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sort_order),0) FROM timeline_nodes")
                .fetch_one(&pool).await.unwrap();
            sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id) VALUES(?1, ?2, ?3)")
                .bind(format!("第 {d} 天")).bind(maxo + 1).bind(last).execute(&pool).await.unwrap();
        }
        let day11: i64 = sqlx::query_scalar(
            "SELECT id FROM timeline_nodes WHERE branch_id IS NULL AND date_label = '第 11 天'")
            .fetch_one(&pool).await.unwrap();
        let b = create_branch_in(&pool, &sid, "测试A", day11).await.unwrap();
        let bb = Some(b.id.clone());

        // 分支里连加 3 天
        let mut labels = Vec::new();
        for _ in 0..3 {
            let n = crate::timeline::create_next_day_node_in(&pool, &bb).await.unwrap();
            labels.push(n.date_label);
        }
        println!("[test] 分支连加 3 天得到：{labels:?}");
        assert_eq!(labels, vec!["第 12 天", "第 13 天", "第 14 天"],
            "**分支连续加天必须严格递增**（重复 = 基准漏了起点自己）");

        // 库里该分支的天数标签也不能有重复
        let dup: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM (SELECT date_label FROM timeline_nodes WHERE branch_id = '{}'
             GROUP BY date_label HAVING COUNT(*) > 1)", b.id))
            .fetch_one(&pool).await.unwrap();
        assert_eq!(dup, 0, "分支里不该有重复的天数标签");

        // 主线也连加 2 天，同样不能重复
        let mut mlabels = Vec::new();
        for _ in 0..2 {
            let n = crate::timeline::create_next_day_node_in(&pool, &None).await.unwrap();
            mlabels.push(n.date_label);
        }
        assert_eq!(mlabels, vec!["第 12 天", "第 13 天"], "主线连加 2 天也必须递增");

        // **两条线的天数各自独立**：分支有 12/13/14，主线有 12/13 —— 互不干扰
        let b_days: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT date_label FROM timeline_nodes WHERE branch_id = '{}' ORDER BY sort_order", b.id))
            .fetch_all(&pool).await.unwrap();
        assert_eq!(b_days.len(), 3, "分支应有 3 个自己的天数节点");
        // 主线：原有 11 天 + 新加 2 天 = 13（全程不受分支影响）
        let m_total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM timeline_nodes WHERE branch_id IS NULL")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(m_total, 13, "主线应是 11 + 2 = 13 个节点，不受分支影响");
        // 而且主线的天数标签也不能有重复
        let m_dup: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM (SELECT date_label FROM timeline_nodes WHERE branch_id IS NULL
             GROUP BY date_label HAVING COUNT(*) > 1)")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(m_dup, 0, "主线里也不该有重复的天数标签");

        cleanup(pool, dir).await;
    }

    /// **防止"字段声明了但没从 SQL 查出来"**（用户实测踩到的 bug）。
    ///
    /// 症状：树状图把每条分支的节点都显示成"主线"。根因是 `list_timeline_nodes` 的
    /// SQL **没查 `branch_id`**，于是 Rust 侧 `try_get` 拿不到、前端收到 `undefined`。
    /// 这类错误**编译器完全不管**（字段有默认值），只能靠"真的读一次"来发现。
    #[tokio::test]
    async fn timeline_nodes_carry_branch_id() {
        let (pool, sid, dir) = temp_story("carry-bid").await;
        let b = create_branch_in(&pool, &sid, "带分支信息的线", 3).await.unwrap();
        // 在分支里建一个自己的节点
        let n = crate::timeline::create_next_day_node_in(&pool, &Some(b.id.clone())).await.unwrap();
        println!("[test] 分支节点：id={} label={} branch_id={:?}", n.id, n.date_label, n.branch_id);

        // ① 返回给前端的结构里必须带上 branch_id
        assert_eq!(n.branch_id.as_deref(), Some(b.id.as_str()),
            "**新建节点返回的结构必须带 branch_id**（否则界面只能显示成主线）");

        // ② 查询接口也必须带上（这才是前端列表用的那个）
        let all: Vec<crate::timeline::TimelineNode> =
            crate::timeline::list_timeline_nodes_in(&pool).await.unwrap();
        let mine = all.iter().find(|x| x.id == n.id).expect("应能查到新节点");
        println!("[test] list 返回的 branch_id = {:?}", mine.branch_id);
        assert_eq!(mine.branch_id.as_deref(), Some(b.id.as_str()),
            "**list_timeline_nodes 必须真的把 branch_id 查出来**（SQL 漏列 = 界面全显示成主线）");
        // 主线节点应为 None
        let main_node = all.iter().find(|x| x.id == 1).expect("应有第 1 天");
        assert_eq!(main_node.branch_id, None, "主线节点的 branch_id 应为空");

        cleanup(pool, dir).await;
    }

    /// 第五批 ③ 第 6 步：**删除分支**（用户要清理测试数据，所以补上）。
    ///
    /// 三条硬要求：
    /// ① 只删这条分支自己的节点 —— **共享历史（分叉点及之前）与主线必须一行不动**
    /// ② 别的分支以它的节点为分叉点时 → **拒绝**
    /// ③ 删的是"当前分支"时 → 记录归主线（否则界面拿到悬空引用会白屏，实测踩过）
    #[tokio::test]
    async fn delete_branch_is_scoped_and_guarded() {
        let (pool, sid, dir) = temp_story("del-branch").await;
        // 分支 A（从第 3 天分叉）里加一天
        let a = create_branch_in(&pool, &sid, "分支A", 3).await.unwrap();
        let node_a = seed_branch_a(&pool, &a.id).await;
        let main_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes WHERE branch_id IS NULL")
            .fetch_one(&pool).await.unwrap();
        let chars_main_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE branch_id IS NULL")
            .fetch_one(&pool).await.unwrap();

        // 分支 B 从**分支 A 的节点**再分叉 → 此时 A 不能被删
        let b = create_branch_in(&pool, &sid, "分支B", node_a).await.unwrap();
        let e = delete_branch_in(&pool, &sid, &a.id).await.unwrap_err();
        println!("[test] 删被依赖的分支：{e}");
        assert!(e.contains("分叉点"), "应说明原因，实际：{e}");
        assert!(e.contains("分支B"), "应指出是哪条分支依赖它，实际：{e}");
        // A 必须还在
        assert!(get_branch_in(&pool, &a.id).await.is_ok(), "被拒绝时不该真删掉");

        // 先删 B，再删 A
        delete_branch_in(&pool, &sid, &b.id).await.unwrap();
        assert!(get_branch_in(&pool, &b.id).await.is_err(), "B 应已删除");

        let msg = delete_branch_in(&pool, &sid, &a.id).await.unwrap();
        println!("[test] 删除结果：{msg}");
        assert!(msg.contains("分支A"), "返回信息应带分支名，实际：{msg}");
        assert!(get_branch_in(&pool, &a.id).await.is_err(), "A 应已删除");

        // ① 共享历史与主线**一行不动**
        let main_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes WHERE branch_id IS NULL")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(main_after, main_before, "**主线节点数必须不变**");
        let chars_main_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE branch_id IS NULL")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(chars_main_after, chars_main_before, "**主线角色必须不变**");
        // 分叉点（第 3 天）必须还在
        let fork_alive: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes WHERE id = 3")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(fork_alive, 1, "**分叉点不能被删**（它属于共享历史）");
        // 分支 A 自己的节点与状态副本应清干净
        let a_nodes: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM timeline_nodes WHERE branch_id = '{}'", a.id)).fetch_one(&pool).await.unwrap();
        assert_eq!(a_nodes, 0, "A 自己的节点应清空");
        let a_chars: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM characters WHERE branch_id = '{}'", a.id)).fetch_one(&pool).await.unwrap();
        assert_eq!(a_chars, 0, "A 的角色副本应清空");

        // 删不存在的分支 → 报错而不是静默成功
        assert!(delete_branch_in(&pool, &sid, "no-such-branch").await.is_err(), "删不存在的分支应报错");

        cleanup(pool, dir).await;
    }

    /// 第五批 ④：**改名与备注**（管理平行故事用）。
    ///
    /// 关键点：改名只动 `name` / `note`，**不碰 fork_node_id、不碰节点归属、不碰颜色** ——
    /// 它纯粹是"给人看的标签"，不该有任何副作用。
    #[tokio::test]
    async fn rename_branch_only_touches_label() {
        let (pool, sid, dir) = temp_story("rename").await;
        let b = create_branch_in(&pool, &sid, "旧名字", 3).await.unwrap();
        let node_a = seed_branch_a(&pool, &b.id).await;
        let before = get_branch_in(&pool, &b.id).await.unwrap();

        rename_branch_in(&pool, &b.id, "  新名字  ", "  这条线的备注  ").await.unwrap();
        let after = get_branch_in(&pool, &b.id).await.unwrap();

        assert_eq!(after.name, "新名字", "名称应被 trim 后保存");
        assert_eq!(after.note, "这条线的备注", "备注应被 trim 后保存");
        // 不该动的东西
        assert_eq!(after.fork_node_id, before.fork_node_id, "分叉点不能变");
        assert_eq!(after.color, before.color, "颜色不能变");
        assert_eq!(after.source_branch_id, before.source_branch_id, "源分支不能变");
        assert_eq!(after.id, before.id, "id 不能变");
        // 节点归属不受影响
        let nb: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
            .bind(node_a).fetch_one(&pool).await.unwrap();
        assert_eq!(nb.as_deref(), Some(b.id.as_str()), "节点仍属于这条分支");

        // 空名字要拒绝
        let e = rename_branch_in(&pool, &b.id, "   ", "x").await.unwrap_err();
        assert!(e.contains("不能为空"), "空名字应被拒绝，实际：{e}");
        // 改完名字后原名字仍在（空名字被拒 → 没被覆盖）
        assert_eq!(get_branch_in(&pool, &b.id).await.unwrap().name, "新名字", "被拒的请求不该改动数据");
        // 不存在的分支 → 报错
        assert!(rename_branch_in(&pool, "no-such", "x", "").await.is_err());

        cleanup(pool, dir).await;
    }

}
