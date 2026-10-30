use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::OnceLock;

/// 定位某故事的 story.db：<项目根>/library/default/projects/<story_id>/story.db
pub fn story_db_path(story_id: &str) -> PathBuf {
    // 第五批「路径模型」修复：见 paths 模块
    crate::paths::story_db(story_id)
}

// ============================================================
//  故事库连接池：**唯一入口** + 缓存
// ============================================================
//
// 背景（2026-10-17 排查）：`open_story_pool` 曾被复制粘贴到 **13 个模块**、
// 演化出 **6 种不同实现**，而且每种跑的迁移还不一样 ——
// 同一份数据库，从不同模块进来，表结构演化覆盖范围不同。
// 更糟的是**没有任何缓存**：每次调用都新建一个连接池。

/// 故事库连接池缓存。键是**解析后的绝对路径**（不是 story_id）——
/// 这样同一 id 在不同数据根下不会串（测试用临时目录时尤其重要）。
fn pool_cache() -> &'static tokio::sync::Mutex<HashMap<PathBuf, SqlitePool>> {
    static CACHE: OnceLock<tokio::sync::Mutex<HashMap<PathBuf, SqlitePool>>> = OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

/// 打开（或复用）某故事的连接池。
///
/// ## 行为
///
/// - **同一路径复用同一个池**：反复调用不会重复建池、重复跑迁移。
/// - **文件被删了就重建**：缓存里存着路径，命中时先确认文件还在；
///   不在就丢掉旧池重新建（测试清场 / 用户手工删库都会走到这里）。
/// - **不 `create_if_missing`**：与原 13 个副本一致 ——
///   库不存在时由 `init_story_db` 负责创建，这里不该悄悄建出一个空库。
/// - **迁移全部跑、且宽容处理错误**：
///   原来 13 个副本跑的迁移集合不同（见下表），统一取**并集**；
///   而 `ensure_*` 内部是 `PRAGMA` + `ALTER TABLE`，表不存在时必然失败，
///   若在这里向上抛错，会让"原本能跑"的调用路径开始失败（例如只建了部分表的测试库）。
///   所以逐个 `let _ =`：迁移失败不影响后续查询，真有问题会在查询时报更清楚的错。
///
/// | 迁移 | 原副本 |
/// |---|---|
/// | `ensure_story_columns` | characters / relations / world / evolution / generate×2 |
/// | `ensure_branch_columns` | branch / characters / relations / world / evolution / timeline |
/// | `ensure_snapshot_temporary_column` | evolution |
/// | `ensure_timeline_tags_column` | timeline |
/// | `ensure_timeline_day_summary_column` | timeline |
/// | `ensure_story_summaries_table` | timeline |
/// | `ensure_timeline_parent_column` | timeline |
pub async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    open_story_pool_at(story_db_path(story_id)).await
}

/// 按**路径**打开（`open_story_pool` 的实现；测试直接传临时路径）。
pub async fn open_story_pool_at(path: PathBuf) -> Result<SqlitePool, sqlx::Error> {

    // ① 缓存命中且文件仍在 → 直接复用
    {
        let mut cache = pool_cache().lock().await;
        if let Some(p) = cache.get(&path) {
            if path.exists() {
                return Ok(p.clone());
            }
            cache.remove(&path); // 文件没了 → 旧池作废
        }
    }

    // ② 建池 + 跑全部迁移
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;
    #[cfg(test)]
    note_pool_created(&path);
    apply_all_story_migrations(&pool).await;

    // ③ 放进缓存（并发下若已有人抢先放入，用他的，关掉自己这个）
    {
        let mut cache = pool_cache().lock().await;
        if let Some(existing) = cache.get(&path) {
            if path.exists() {
                pool.close().await;
                return Ok(existing.clone());
            }
            cache.remove(&path);
        }
        cache.insert(path, pool.clone());
    }
    Ok(pool)
}

/// 对故事库跑**全部**幂等迁移（并集，见 `open_story_pool` 注释）。
///
/// 刻意忽略错误：这些迁移是"给老库补列/补表"，
/// 失败通常意味着表还不存在（新建库的早期阶段），不该让调用方直接失败。
pub async fn apply_all_story_migrations(pool: &SqlitePool) {
    // 导演代价表要**先于** ensure_branch_columns 建好，否则那一步的分支列循环
    // 会因为"表还不存在"跳过它（老库升级上来时正好会踩）。
    let _ = ensure_director_costs_table(pool).await;
    // 赦免请求表（2026-10-27）：挂在 AI 回复下面的盒子，不进剧情历史
    let _ = crate::pardon::ensure_table(pool).await;
    // 关系强度账本（2026-10-29）：只增不改，撤销写反向行；必须早于 ensure_branch_columns
    let _ = crate::relation_ledger::ensure_table(pool).await;
    // 三维数值账本（2026-10-29）：插件 stat-growth 打开时才有行
    let _ = crate::stat_change::ensure_table(pool).await;
    // 章节（2026-10-30）：章 ↔ 天 多对多；只读聚合用
    let _ = crate::chapters::ensure_table(pool).await;
    // 伏笔清单（2026-10-30 阶段③）：逾期为现算，不落库
    let _ = crate::foreshadow::ensure_table(pool).await;
    let _ = ensure_timeline_tags_column(pool).await;
    let _ = ensure_timeline_day_summary_column(pool).await;
    // **这一条漏过**（2026-10-27 用户实测报错「no such column: day_summary_at」）：
    // 建库时 init_story_db_at 补过，但"老库走 open_story_pool"这条路径的迁移并集里没有它，
    // 于是更早建的故事一自动日摘要就炸。
    let _ = ensure_timeline_day_summary_at_column(pool).await;
    let _ = ensure_story_summaries_table(pool).await;
    let _ = ensure_timeline_parent_column(pool).await;
    let _ = ensure_story_columns(pool).await;
    let _ = ensure_branch_columns(pool).await;
    let _ = ensure_snapshot_temporary_column(pool).await;
    let _ = ensure_proposal_node_column(pool).await;
    // 2026-10-27：评估模型整套删除（用户定：多余，后面重规划）。
    // 表一起 DROP —— 和当初删 portrait_versions 同口径，不留孤儿。
    let _ = sqlx::query("DROP TABLE IF EXISTS reply_evaluations").execute(pool).await;
    // 数据自愈：把历史上"删节点没重接"留下的悬空 parent_id 接回去（幂等，通常 0 行）
    let _ = crate::timeline::repair_dangling_parents(pool).await;
}

/// 把已打开的池登记进缓存（供 `init_story_db` 复用，避免同一文件开两个池）。
async fn cache_put(path: PathBuf, pool: &SqlitePool) {
    let mut cache = pool_cache().lock().await;
    if let Some(existing) = cache.get(&path) {
        if path.exists() {
            // 已有别人放进去的池，保留它（都指向同一文件，谁先都一样）
            let _ = existing;
            return;
        }
        cache.remove(&path);
    }
    cache.insert(path, pool.clone());
}

/// **测试专用**：清空连接池缓存。
///
/// 测试会删掉临时目录；虽然 `open_story_pool` 命中时会检查文件是否还在，
/// 但显式清空更彻底（也能释放文件句柄，避免 Windows 上删目录失败）。
#[cfg(test)]
pub async fn clear_pool_cache() {
    // **只从缓存移除，不主动 close**。
    //
    // 为什么不能 close：`cargo test` 并行跑，别的测试可能正拿着同一个池在用；
    // 这里 close 会把它们弄挂（实测过：随机失败）。
    // 移除后池会在最后一个克隆 drop 时自然关闭。
    let mut cache = pool_cache().lock().await;
    cache.clear();
}

/// **测试专用**：按路径记录"新建了几个池"。
///
/// 为什么不用全局计数器：`cargo test` 是**并行**跑的，别的测试也在建池，
/// 全局计数会被它们干扰（实测第一次断言得到 4 而不是 1）。按路径各记各的才稳。
#[cfg(test)]
fn creations() -> &'static std::sync::Mutex<HashMap<PathBuf, usize>> {
    static C: OnceLock<std::sync::Mutex<HashMap<PathBuf, usize>>> = OnceLock::new();
    C.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

#[cfg(test)]
fn note_pool_created(path: &PathBuf) {
    *creations().lock().unwrap().entry(path.clone()).or_insert(0) += 1;
}

/// **测试专用**：该路径一共被新建过几次池。
#[cfg(test)]
pub fn pool_creations(path: &PathBuf) -> usize {
    *creations().lock().unwrap().get(path).unwrap_or(&0)
}

/// **测试专用**：当前缓存了几个池（用来断言"复用生效"）。
#[cfg(test)]
pub async fn pool_cache_len() -> usize {
    pool_cache().lock().await.len()
}

/// 为一个故事初始化 story.db（不存在则创建），建齐 M0 需要的全部表。
/// 注意：story.db 内不再冗余 story_id 列（每库一故事）。
pub async fn init_story_db(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    init_story_db_at(story_db_path(story_id)).await
}

/// `init_story_db` 的实现（按**路径**）——测试直接传临时路径，确保测的就是生产这条路径。
pub async fn init_story_db_at(path: PathBuf) -> Result<SqlitePool, sqlx::Error> {
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
    ensure_timeline_day_summary_at_column(&pool).await?;
    ensure_story_summaries_table(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS characters (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            name             TEXT NOT NULL,
            profile          TEXT NOT NULL DEFAULT '',
            alias            TEXT NOT NULL DEFAULT '',
            role             TEXT NOT NULL DEFAULT '',
            faction          TEXT NOT NULL DEFAULT '',
            age              TEXT NOT NULL DEFAULT '',
            gender           TEXT NOT NULL DEFAULT '',
            first_appearance TEXT NOT NULL DEFAULT '',
            one_liner        TEXT NOT NULL DEFAULT '',
            speech_style     TEXT NOT NULL DEFAULT '',
            traits           TEXT NOT NULL DEFAULT '',
            stat_stamina     INTEGER NOT NULL DEFAULT 50,
            stat_sanity      INTEGER NOT NULL DEFAULT 50,
            stat_emotion     INTEGER NOT NULL DEFAULT 50,
            stat_screen      INTEGER NOT NULL DEFAULT 50,
            current_status   TEXT NOT NULL DEFAULT '',
            version          INTEGER NOT NULL DEFAULT 1,
            -- v3.4「角色卡 v2」：过去 / 动机 / 目的 / 对话示例 / 行为准则
            background_past  TEXT NOT NULL DEFAULT '',
            appearance       TEXT NOT NULL DEFAULT '',   -- v3.4：外貌（作者设定，AI 不可改）
            motivation       TEXT NOT NULL DEFAULT '',
            goal             TEXT NOT NULL DEFAULT '',
            dialogue_examples TEXT NOT NULL DEFAULT '',
            behavior_rules   TEXT NOT NULL DEFAULT ''
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

    // 导演代价账本（2026-10-27）：新库直接建，老库由 ensure_director_costs_table 补
    ensure_director_costs_table(&pool).await?;

    // **必须跑全部幂等迁移**（2026-10-18 修）：建表语句不带"后加的列"
    // （timeline_nodes.parent_id、各表 branch_id、story_summaries …），全靠 ensure_* 补。
    // 之前这里只跑 ensure_story_columns 就把池子登记进缓存 ——
    // 之后 open_story_pool 命中缓存**直接返回、不再补迁移**，
    // 于是全新安装新建的故事库永远缺 parent_id / branch_id：
    //   ❌ 加载角色失败: no such column: branch_id
    //   ❌ 加载时间线失败: no such column: parent_id
    // 老库没暴露，是因为它们早被别的路径迁移过。
    apply_all_story_migrations(&pool).await;

    // 登记进缓存：后续 open_story_pool 直接复用，不必再开一个池
    cache_put(path.clone(), &pool).await;
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

/// 「上次自动生成日摘要时的 AI 回复条数」——
/// 用来实现"每满 10 条（10/20/30…）才跑一次、且同一轮数不重复跑"（用户 2026-10-22 定）。
///
/// 为什么需要这一列：旧逻辑是 `replies >= 10` 就每次对话都跑，
/// 10 条之后**每加一条回复都会重新生成一次日摘要**（浪费 token、还把缓存打散）。
pub async fn ensure_timeline_day_summary_at_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols = sqlx::query("PRAGMA table_info(timeline_nodes)").fetch_all(pool).await?;
    let has = cols.iter().any(|r| r.try_get::<String, _>(1).unwrap_or_default() == "day_summary_at");
    if !has {
        sqlx::query(
            "ALTER TABLE timeline_nodes ADD COLUMN day_summary_at INTEGER NOT NULL DEFAULT 0",
        )
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
        // v3.4「角色卡 v2」：过去 / 动机 / 目的 / 对话示例 / 行为准则（老库自动补列）
        ("background_past", "TEXT NOT NULL DEFAULT ''"),
        ("appearance", "TEXT NOT NULL DEFAULT ''"),
        ("motivation", "TEXT NOT NULL DEFAULT ''"),
        ("goal", "TEXT NOT NULL DEFAULT ''"),
        ("dialogue_examples", "TEXT NOT NULL DEFAULT ''"),
        ("behavior_rules", "TEXT NOT NULL DEFAULT ''"),
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

/// ⑤-B：给 `change_proposals` 加 `node_id`（**提案来自哪个时间线节点**）。
///
/// 为什么要它：拍板界面要能回答"这条设定变化是哪天、哪段剧情引出来的"。
/// 之前只有 `evidence`（文本），要用户自己猜来源。
pub async fn ensure_proposal_node_column(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let cols = sqlx::query("PRAGMA table_info(change_proposals)").fetch_all(pool).await?;
    if cols.is_empty() {
        return Ok(()); // 表还没建（新库早期），跳过
    }
    let has = cols
        .iter()
        .any(|r| r.try_get::<String, _>(1).unwrap_or_default() == "node_id");
    if !has {
        sqlx::query("ALTER TABLE change_proposals ADD COLUMN node_id INTEGER")
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// 第五批 ③：分支系统的数据层迁移（**幂等**，每次打开故事库都跑）。
///
/// 做三件事：
/// 1. 建 `branches` 表（分支身份：UUID + 名称 + 颜色 + 分叉点）
/// 2. 给 9 张表补 `branch_id TEXT` 列（**NULL = 主线**，无默认值 —— 现有数据因此天然全归主线）
/// 3. 建分支相关索引
///
/// 为什么 `branch_id` 不加默认值：设计口径是"`branch_id IS NULL` 就是主线专属"，
/// 若给个默认值（比如 `'main'`），"主线"就变成两种表示，查询与迁移都会多一层分支判断。
pub async fn ensure_branch_columns(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // 第五批 ④：`branches` 加 `note`（分支备注）—— 幂等迁移。
    // 用途：给分支写一句"这条线是干什么的"，切换/管理平行故事时不用靠名字硬猜。
    {
        let cols = sqlx::query("PRAGMA table_info(branches)").fetch_all(pool).await.unwrap_or_default();
        let has = cols.iter().any(|r| r.try_get::<String, _>(1).unwrap_or_default() == "note");
        let table_exists = !cols.is_empty();
        if table_exists && !has {
            let _ = sqlx::query("ALTER TABLE branches ADD COLUMN note TEXT NOT NULL DEFAULT ''")
                .execute(pool)
                .await;
        }
    }

    // 1) 分支表
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS branches (
            id               TEXT PRIMARY KEY,
            story_id         TEXT NOT NULL,
            name             TEXT NOT NULL,
            fork_node_id     INTEGER NOT NULL,
            source_branch_id TEXT,
            color            TEXT NOT NULL DEFAULT '#6B7280',
            created_at       TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                note             TEXT NOT NULL DEFAULT ''
        )",
    )
    .execute(pool)
    .await?;

    // 2) 各表补 branch_id（NULL = 主线）
    for table in [
        "timeline_nodes",
        "messages",
        "characters",
        "relations",
        "world_settings",
        "setting_snapshots",
        "story_summaries",
        "change_proposals",
        "audit_logs",
        "event_logs",
        "director_costs",
        "relation_changes",
        "stat_changes",
        "chapters",
        "foreshadows",
    ] {
        // 表可能尚未创建（老库缺表）→ 跳过，不报错
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        )
        .bind(table)
        .fetch_one(pool)
        .await?;
        if exists == 0 {
            continue;
        }
        ensure_columns(pool, table, &[("branch_id", "TEXT")]).await?;
    }

    // 3) 索引（IF NOT EXISTS 保证幂等）
    for stmt in [
        "CREATE INDEX IF NOT EXISTS idx_timeline_nodes_branch   ON timeline_nodes(branch_id)",
        "CREATE INDEX IF NOT EXISTS idx_messages_branch         ON messages(branch_id)",
        "CREATE INDEX IF NOT EXISTS idx_messages_node           ON messages(node_id)",
        "CREATE INDEX IF NOT EXISTS idx_characters_branch       ON characters(branch_id)",
        "CREATE INDEX IF NOT EXISTS idx_relations_branch        ON relations(branch_id)",
        "CREATE INDEX IF NOT EXISTS idx_world_settings_branch   ON world_settings(branch_id)",
        "CREATE INDEX IF NOT EXISTS idx_setting_snapshots_branch ON setting_snapshots(branch_id)",
        "CREATE INDEX IF NOT EXISTS idx_change_proposals_branch ON change_proposals(branch_id)",
        "CREATE INDEX IF NOT EXISTS idx_branches_story          ON branches(story_id)",
        "CREATE INDEX IF NOT EXISTS idx_director_costs_branch    ON director_costs(branch_id)",
    ] {
        // 索引涉及的列若不存在（老库缺表）会报错 —— 忽略之，迁移不因此失败
        let _ = sqlx::query(stmt).execute(pool).await;
    }
    Ok(())
}

/// 迁移：建「导演代价」账本表（2026-10-27，幂等）。
///
/// 为什么单独一张表而不是塞进 event_logs：账本要能被**逐条撤销/赦免**、要能按角色与分支
/// 求和来锁闸门 —— 这两件事在事件日志（只追加、无状态字段）上都做不了。
pub async fn ensure_director_costs_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS director_costs (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id      INTEGER,
            message_id   INTEGER,
            character_id INTEGER,
            kind         TEXT NOT NULL,
            reason       TEXT NOT NULL,
            score        INTEGER NOT NULL DEFAULT 0,
            source       TEXT NOT NULL DEFAULT 'model',  -- model / rule
            excused      INTEGER NOT NULL DEFAULT 0,     -- 1 = 赦免（留痕不计分）
            rel_delta    INTEGER NOT NULL DEFAULT 0,     -- 这笔账同时扣掉的关系强度（2026-10-27）
            created_at   TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            branch_id    TEXT
        )",
    )
    .execute(pool)
    .await?;
    // 老库（今天早些时候建的）补 rel_delta：账本要显示"这笔账同时扣了多少关系"
    if let Err(e) = ensure_columns(pool, "director_costs", &[("rel_delta", "INTEGER NOT NULL DEFAULT 0")]).await {
        eprintln!("[migrate] director_costs.rel_delta 补列失败（忽略）：{e}");
    }
    Ok(())
}

/// 分支过滤的 WHERE 片段（供各查询拼接）。
///
/// **主线 = NULL，分支 = 具体 id** —— 这是设计口径，别处不要引入 "main" 这类字面量。
pub fn branch_clause(col: &str, branch_id: &Option<String>, param_no: usize) -> String {
    match branch_id {
        // **必须用编号参数 `?N`，不能用匿名 `?`**：SQLite 会把匿名 `?` 当成 `?1`，
        // 于是"复制到新分支"这类语句里 `SELECT ..., ?1 ... WHERE branch_id = ?`
        // 会让 WHERE 也绑上新分支 id（此时新分支还没有行）→ 静默复制出 0 行。
        // 这个坑在分支系统的嵌套分支测试里踩到过。
        Some(_) => format!("{col} = ?{param_no}"),
        None => format!("{col} IS NULL"),
    }
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
// 保留未用：诊断用：列出故事库的表
#[allow(dead_code)]
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


/// 第五批 ③ 第 1 步：分支迁移的表结构测试。
///
/// 验收口径（方案 §7 步 1）：① 迁移后表结构正确 ② 重复执行不报错
/// ③ **现有数据零转换**（`branch_id` 全为 NULL = 主线）
#[cfg(test)]
mod branch_migration_tests;


#[cfg(test)]
mod pool_tests;

