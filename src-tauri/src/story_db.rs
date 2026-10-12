use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;
use std::str::FromStr;
use std::path::PathBuf;

/// 定位某故事的 story.db：<项目根>/library/default/projects/<story_id>/story.db
pub fn story_db_path(story_id: &str) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("..")
        .join("library")
        .join("default")
        .join("projects")
        .join(story_id)
        .join("story.db")
}

/// 为一个故事初始化 story.db（不存在则创建），建齐 M0 需要的全部表。
/// 注意：story.db 内不再冗余 story_id 列（每库一故事）。
pub async fn init_story_db(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await?;

    // 按 MVP §3 数据模型建表（stories 表在 library.db，这里不建）
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS timeline_nodes (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            date_label   TEXT NOT NULL,
            sort_order   INTEGER NOT NULL DEFAULT 0,
            node_type    TEXT NOT NULL DEFAULT 'day',   -- day / interlude / flashback
            is_flashback INTEGER NOT NULL DEFAULT 0,
            tags         TEXT NOT NULL DEFAULT ''        -- 局部化标签（逗号分隔）
        )",
    ).execute(&pool).await?;

    // 迁移：为已存在的 story.db 补 tags 列（幂等）
    ensure_timeline_tags_column(&pool).await?;
    // D 子项：日摘要列 + 阶段摘要表（幂等，老库自动获得）
    ensure_timeline_day_summary_column(&pool).await?;
    ensure_story_summaries_table(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS characters (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            name          TEXT NOT NULL,
            profile       TEXT NOT NULL DEFAULT '',
            alias         TEXT NOT NULL DEFAULT '',
            role          TEXT NOT NULL DEFAULT '',
            faction       TEXT NOT NULL DEFAULT '',
            age           TEXT NOT NULL DEFAULT '',
            gender        TEXT NOT NULL DEFAULT '',
            first_appearance TEXT NOT NULL DEFAULT '',
            one_liner     TEXT NOT NULL DEFAULT '',
            speech_style  TEXT NOT NULL DEFAULT '',
            traits        TEXT NOT NULL DEFAULT '',
            stat_stamina  INTEGER NOT NULL DEFAULT 50,
            stat_sanity   INTEGER NOT NULL DEFAULT 50,
            stat_emotion  INTEGER NOT NULL DEFAULT 50,
            stat_screen   INTEGER NOT NULL DEFAULT 50,
            current_status TEXT NOT NULL DEFAULT '',
            version       INTEGER NOT NULL DEFAULT 1
        )",
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS relations (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            from_character_id INTEGER NOT NULL,
            to_character_id   INTEGER NOT NULL,
            relation_type    TEXT NOT NULL,
            direction        TEXT NOT NULL DEFAULT 'unilateral', -- unilateral / bilateral
            strength         INTEGER NOT NULL DEFAULT 50 CHECK(strength BETWEEN 0 AND 100),
            state            TEXT NOT NULL DEFAULT 'stable',
            description      TEXT NOT NULL DEFAULT '',
            evidence         TEXT NOT NULL DEFAULT '',
            active_from      TEXT,
            active_to        TEXT,
            version          INTEGER NOT NULL DEFAULT 1
        )",
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS world_settings (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            key          TEXT NOT NULL,
            value        TEXT NOT NULL,
            is_hard_rule INTEGER NOT NULL DEFAULT 0,
            category     TEXT NOT NULL DEFAULT '设定',
            meta_json    TEXT NOT NULL DEFAULT '{}',
            version      INTEGER NOT NULL DEFAULT 1
        )",
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS messages (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id    INTEGER NOT NULL,
            role       TEXT NOT NULL,      -- user / assistant / system
            content    TEXT NOT NULL,
            mode       TEXT NOT NULL DEFAULT 'immersion', -- immersion / director
            meta       TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        )",
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS setting_snapshots (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            version       INTEGER NOT NULL,
            payload_json  TEXT NOT NULL,
            parent_version INTEGER,
            created_at    TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        )",
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS event_logs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id     INTEGER,
            event_type  TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            confidence  REAL NOT NULL DEFAULT 0.0,
            source      TEXT NOT NULL DEFAULT 'rule'
        )",
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS change_proposals (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            target_type TEXT NOT NULL,  -- character / relation / world
            target_id   INTEGER,
            field       TEXT NOT NULL,
            old_value   TEXT,
            new_value   TEXT,
            reason      TEXT,
            evidence    TEXT,
            confidence  REAL NOT NULL DEFAULT 0.0,
            status      TEXT NOT NULL DEFAULT 'pending', -- pending / applied / rejected / rolled_back
            note        TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        )",
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS audit_logs (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            action     TEXT NOT NULL,
            target     TEXT,
            before     TEXT,
            after      TEXT,
            operator   TEXT NOT NULL DEFAULT 'user',
            created_at TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        )",
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS configs (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cache_entries (
            path         TEXT PRIMARY KEY,
            size         INTEGER NOT NULL DEFAULT 0,
            type         TEXT NOT NULL,
            last_access  TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        )",
    ).execute(&pool).await?;

    // 调试用：每次发给 AI 的请求快照
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS debug_requests (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id         INTEGER,
            model           TEXT,
            system_prompt   TEXT NOT NULL,
            history_json    TEXT NOT NULL,
            user_text       TEXT NOT NULL,
            ai_reply        TEXT,
            estimated_tokens INTEGER NOT NULL DEFAULT 0,
            n_used          INTEGER NOT NULL DEFAULT 0,
            status          TEXT NOT NULL DEFAULT 'ok',
            error           TEXT,
            created_at      TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        )",
    ).execute(&pool).await?;

    ensure_story_columns(&pool).await?;

    Ok(pool)
}

/// 迁移：确保 timeline_nodes 有 tags 列（旧库补列，幂等；每次打开库时调用）。
pub async fn ensure_timeline_tags_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols = sqlx::query("PRAGMA table_info(timeline_nodes)").fetch_all(pool).await?;
    let has_tags = cols.iter().any(|r| r.try_get::<String, _>(1).unwrap_or_default() == "tags");
    if !has_tags {
        sqlx::query("ALTER TABLE timeline_nodes ADD COLUMN tags TEXT NOT NULL DEFAULT ''")
            .execute(pool).await?;
    }
    Ok(())
}

/// 迁移：确保 timeline_nodes 有 day_summary 列（D 子项：日摘要，摘要属于"这一天"，不属于某条消息）。
/// 生成一次即固化、不回改 —— 见 ROADMAP §6.4 的三条硬规则。
pub async fn ensure_timeline_day_summary_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols = sqlx::query("PRAGMA table_info(timeline_nodes)").fetch_all(pool).await?;
    let has = cols.iter().any(|r| r.try_get::<String, _>(1).unwrap_or_default() == "day_summary");
    if !has {
        sqlx::query("ALTER TABLE timeline_nodes ADD COLUMN day_summary TEXT NOT NULL DEFAULT ''")
            .execute(pool).await?;
    }
    Ok(())
}

/// 阶段摘要表（D 子项：C 层分层摘要的**接口预留**）。
/// 本期只建表与读写函数，不自动生成 —— 等日摘要积累到一定天数再做"若干天滚成一段阶段摘要"。
pub async fn ensure_story_summaries_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS story_summaries (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            kind         TEXT NOT NULL DEFAULT 'phase',   -- phase（阶段摘要）
            day_from     INTEGER NOT NULL DEFAULT 0,      -- 覆盖的第几天起
            day_to       INTEGER NOT NULL DEFAULT 0,      -- 覆盖的第几天止
            content      TEXT NOT NULL,
            source_days  INTEGER NOT NULL DEFAULT 0,      -- 参与汇总的天数
            created_at   TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 迁移：确保 characters / relations 有 A/D/G 版面需要的扩展列（旧库补列，幂等）。
pub async fn ensure_story_columns(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    ensure_columns(pool, "characters", &[
        ("alias", "TEXT NOT NULL DEFAULT ''"),
        ("role", "TEXT NOT NULL DEFAULT ''"),
        ("faction", "TEXT NOT NULL DEFAULT ''"),
        ("age", "TEXT NOT NULL DEFAULT ''"),
        ("gender", "TEXT NOT NULL DEFAULT ''"),
        ("first_appearance", "TEXT NOT NULL DEFAULT ''"),
        ("one_liner", "TEXT NOT NULL DEFAULT ''"),
        ("speech_style", "TEXT NOT NULL DEFAULT ''"),
        ("traits", "TEXT NOT NULL DEFAULT ''"),
        ("stat_stamina", "INTEGER NOT NULL DEFAULT 50"),
        ("stat_sanity", "INTEGER NOT NULL DEFAULT 50"),
        ("stat_emotion", "INTEGER NOT NULL DEFAULT 50"),
        ("stat_screen", "INTEGER NOT NULL DEFAULT 50"),
    ]).await?;
    ensure_columns(pool, "relations", &[
        ("description", "TEXT NOT NULL DEFAULT ''"),
        ("evidence", "TEXT NOT NULL DEFAULT ''"),
    ]).await?;
    ensure_columns(pool, "world_settings", &[
        ("category", "TEXT NOT NULL DEFAULT '设定'"),
        ("meta_json", "TEXT NOT NULL DEFAULT '{}'"),
    ]).await?;
    Ok(())
}

async fn ensure_columns(pool: &SqlitePool, table: &str, cols: &[(&str, &str)]) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(&format!("PRAGMA table_info({})", table)).fetch_all(pool).await?;
    let existing: Vec<String> = rows.iter().map(|r| r.try_get::<String, _>(1).unwrap_or_default()).collect();
    for (name, ddl) in cols {
        if !existing.iter().any(|c| c == name) {
            sqlx::query(&format!("ALTER TABLE {} ADD COLUMN {} {}", table, name, ddl)).execute(pool).await?;
        }
    }
    Ok(())
}

/// 返回某 story.db 中表的数量与名称（自测用）。
pub async fn story_db_tables(story_id: &str) -> Result<(String, i64, Vec<String>), sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;

    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
    ).fetch_all(&pool).await?;

    let mut names = Vec::new();
    for r in &rows {
        names.push(r.try_get::<String, _>(0)?);
    }
    let n = names.len() as i64;
    Ok((path.display().to_string(), n, names))
}

/// 迁移：确保 timeline_nodes 有 `parent_id` 列（第五批 ①：时间线由「线性」升级为「树」）。
///
/// 为什么用 parent_id 而不是新增一张表：**「一个节点可以有多条后继」正是树的数据结构**。
/// 现在所有故事都是线性的（每个节点至多一个后继），`parent_id` 把它们表示成"退化的树"；
/// 等做分支系统时只需允许"多个节点共享同一个 parent_id"，**不必再改数据模型**。
pub async fn ensure_timeline_parent_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols = sqlx::query("PRAGMA table_info(timeline_nodes)").fetch_all(pool).await?;
    let has = cols.iter().any(|r| r.try_get::<String, _>(1).unwrap_or_default() == "parent_id");
    if !has {
        sqlx::query("ALTER TABLE timeline_nodes ADD COLUMN parent_id INTEGER")
            .execute(pool)
            .await?;
    }
    // 补链：每次打开库都跑一次（幂等）。
    // 关键点：**新建节点时 parent_id 是 NULL**，所以不能只在"首次补列"时回填 ——
    // 那样新建的节点永远没有父节点，树会碎成一片孤点。
    ensure_parent_links(pool).await
}

/// 把 `parent_id` 为 NULL 的节点接到"线性前一个节点"上（幂等，可反复调用）。
///
/// - 按 `(sort_order, id)` 排出全表顺序（历史数据里 sort_order 可能重复，必须带 id 次级键）
/// - 第 1 个节点的 parent 保持 NULL（树根）
/// - 之后的每个节点：若 parent_id 为 NULL，则指向前一个节点
///
/// 注意：这是在"还没有分支"的前提下的线性补链。**将来做分支系统时**，
/// 分叉出来的节点会被显式指定 parent_id（不为 NULL），因此不会被这里错误地拉直成一条线。
pub async fn ensure_parent_links(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let rows = sqlx::query("SELECT id FROM timeline_nodes ORDER BY sort_order ASC, id ASC")
        .fetch_all(pool)
        .await?;
    let mut prev: Option<i64> = None;
    for r in rows {
        let id: i64 = r.try_get(0)?;
        if let Some(pv) = prev {
            sqlx::query("UPDATE timeline_nodes SET parent_id = ?1 WHERE id = ?2 AND parent_id IS NULL")
                .bind(pv)
                .bind(id)
                .execute(pool)
                .await?;
        }
        prev = Some(id);
    }
    Ok(())
}

/// 迁移：给 setting_snapshots 加 `temporary` 列（第五批 ②：会话内临时版本）。
///
/// 用途：回滚是破坏性的，所以回滚前先自动存一份"当前状态"。这份备份**只在本次运行期间有效**
/// —— 程序启动时会把上一会话遗留的临时版本清掉（用户 2026-10-12 定：做成临时的，退出即失效）。
pub async fn ensure_snapshot_temporary_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols = sqlx::query("PRAGMA table_info(setting_snapshots)").fetch_all(pool).await?;
    let names: Vec<String> = cols
        .iter()
        .map(|r| r.try_get::<String, _>(1).unwrap_or_default())
        .collect();
    if !names.iter().any(|n| n == "temporary") {
        sqlx::query("ALTER TABLE setting_snapshots ADD COLUMN temporary INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }
    if !names.iter().any(|n| n == "note") {
        sqlx::query("ALTER TABLE setting_snapshots ADD COLUMN note TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }
    Ok(())
}
