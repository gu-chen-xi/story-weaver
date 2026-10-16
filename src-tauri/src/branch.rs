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
    Ok(cur.flatten())
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
        "SELECT id, story_id, name, fork_node_id, source_branch_id, color, created_at
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
        "SELECT id, story_id, name, fork_node_id, source_branch_id, color, created_at
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
    let root = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("..")
        .join("library");
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
        let dir = std::env::current_dir()
            .unwrap()
            .join("..")
            .join("library")
            .join("default")
            .join("projects")
            .join(&sid);
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
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
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

}
