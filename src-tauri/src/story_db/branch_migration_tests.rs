// 从 `story_db.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `story_db::branch_migration_tests` —— 因为 `story_db.rs` + `story_db/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `story_db`，一行都不用改写。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;

/// 造一个"老式"故事库：只有分支迁移**之前**的表结构（没有 branch_id、没有 branches 表）
async fn legacy_pool(tag: &str) -> SqlitePool {
    let dir = crate::paths::test_root()
        .join(format!("branch-mig-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let opts = SqliteConnectOptions::new()
        .filename(dir.join("story.db"))
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    // 只建迁移会碰到的那些表（尽量贴近老库）
    for ddl in [
        "CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
            is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
            day_summary TEXT NOT NULL DEFAULT '', parent_id INTEGER)",
        "CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER NOT NULL,
            role TEXT NOT NULL, content TEXT NOT NULL, mode TEXT NOT NULL DEFAULT 'chat',
            meta TEXT NOT NULL DEFAULT '{}')",
        "CREATE TABLE characters (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
        "CREATE TABLE relations (id INTEGER PRIMARY KEY AUTOINCREMENT, from_character_id INTEGER,
            to_character_id INTEGER)",
        "CREATE TABLE world_settings (id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT, value TEXT)",
        "CREATE TABLE setting_snapshots (id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
            payload_json TEXT NOT NULL, parent_version INTEGER, temporary INTEGER NOT NULL DEFAULT 0,
            note TEXT NOT NULL DEFAULT '')",
        "CREATE TABLE story_summaries (id INTEGER PRIMARY KEY AUTOINCREMENT, day_from INTEGER, day_to INTEGER)",
        "CREATE TABLE change_proposals (id INTEGER PRIMARY KEY AUTOINCREMENT, status TEXT NOT NULL DEFAULT 'pending')",
        "CREATE TABLE audit_logs (id INTEGER PRIMARY KEY AUTOINCREMENT, action TEXT NOT NULL)",
        "CREATE TABLE event_logs (id INTEGER PRIMARY KEY AUTOINCREMENT, event_type TEXT NOT NULL)",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    // 放几行"老数据"：迁移后它们必须还在、且 branch_id 为 NULL
    sqlx::query("INSERT INTO characters(name) VALUES('夏婉琴')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order) VALUES('第 1 天', 1)")
        .execute(&pool).await.unwrap();
    pool
}

async fn has_col(pool: &SqlitePool, table: &str, col: &str) -> bool {
    let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(pool)
        .await
        .unwrap();
    rows.iter()
        .any(|r| r.try_get::<String, _>(1).unwrap_or_default() == col)
}

/// 回归（2026-10-18 用户实测：全新安装后新建故事立刻报错）：
/// **新建库必须自带 parent_id / branch_id**，否则：
///   ❌ 加载角色失败: no such column: branch_id
///   ❌ 加载时间线失败: no such column: parent_id
///
/// 根因：`init_story_db` 只跑了 `ensure_story_columns` 就把池子登记进缓存，
/// 之后 `open_story_pool` 命中缓存**不再补迁移** → 缺列永远补不上。
/// 老库没暴露，是因为它们早被别的路径迁移过。
///
/// 这里调用的是**生产同一个入口**（`init_story_db_at`），
/// 所以一旦有人把里面的 `apply_all_story_migrations` 删掉，这个测试立刻变红。
#[tokio::test]
async fn fresh_story_db_has_all_modern_columns() {
    let dir = crate::paths::test_root().join("fresh_schema");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("story.db");

    let pool = init_story_db_at(path).await.unwrap();

    for (table, col) in [
        ("timeline_nodes", "parent_id"),
        ("timeline_nodes", "tags"),
        ("timeline_nodes", "day_summary"),
        ("characters", "branch_id"),
        // v3.4「角色卡 v2」
        ("characters", "background_past"),
        ("characters", "motivation"),
        ("characters", "goal"),
        ("characters", "dialogue_examples"),
        ("characters", "behavior_rules"),
        ("world_settings", "branch_id"),
        ("relations", "branch_id"),
        ("story_summaries", "branch_id"),
    ] {
        assert!(
            has_col(&pool, table, col).await,
            "新建库缺列：{table}.{col} —— 这正是「全新安装后加载失败」的根因"
        );
    }

    pool.close().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// v3.4：**老库**（characters 只有旧列）跑一次迁移，必须补上角色卡 v2 的 5 列。
///
/// 为什么单独测：`add column` 是在 `ensure_story_columns` 里做的，
/// 而它只在**打开 story.db 时**执行（进入故事才打开）。所以"升级用户"这条路径
/// 只靠新库测试覆盖不到 —— 必须造一个旧 schema 的库来跑。
#[tokio::test]
async fn migration_adds_card_v2_columns_to_legacy_db() {
    let dir = crate::paths::test_root().join("cardv2-legacy");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("story.db");
    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();

    // 老 schema：只有 v3.3 及以前的列
    sqlx::query(
        "CREATE TABLE characters (
            id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
            profile TEXT NOT NULL DEFAULT '', alias TEXT NOT NULL DEFAULT '',
            role TEXT NOT NULL DEFAULT '', faction TEXT NOT NULL DEFAULT '',
            age TEXT NOT NULL DEFAULT '', gender TEXT NOT NULL DEFAULT '',
            first_appearance TEXT NOT NULL DEFAULT '', one_liner TEXT NOT NULL DEFAULT '',
            speech_style TEXT NOT NULL DEFAULT '', traits TEXT NOT NULL DEFAULT '',
            current_status TEXT NOT NULL DEFAULT '',
            stat_stamina INTEGER NOT NULL DEFAULT 50, stat_sanity INTEGER NOT NULL DEFAULT 50,
            stat_emotion INTEGER NOT NULL DEFAULT 50, stat_screen INTEGER NOT NULL DEFAULT 50,
            version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO characters(name, profile) VALUES('老角色', '旧小传')")
        .execute(&pool)
        .await
        .unwrap();

    assert!(!has_col(&pool, "characters", "background_past").await, "迁移前不该有");
    assert!(!has_col(&pool, "characters", "appearance").await, "迁移前不该有 appearance");

    // 与"进入故事"同一条路径
    apply_all_story_migrations(&pool).await;

    for col in ["background_past", "motivation", "goal", "dialogue_examples", "behavior_rules", "appearance"] {
        assert!(has_col(&pool, "characters", col).await, "老库迁移后缺列：{col}");
    }
    // 老数据不许丢，新列默认空串
    let (name, past): (String, String) =
        sqlx::query_as("SELECT name, background_past FROM characters WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(name, "老角色", "老数据应保留");
    assert_eq!(past, "", "新列默认空串");

    pool.close().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// 迁移后：branches 表存在；10 张表都有 branch_id 列
#[tokio::test]
async fn migration_creates_branches_table_and_branch_columns() {
    let pool = legacy_pool("cols").await;
    assert!(!has_col(&pool, "characters", "branch_id").await, "迁移前不该有 branch_id");

    ensure_branch_columns(&pool).await.unwrap();

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='branches'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "应建出 branches 表");

    for table in [
        "timeline_nodes", "messages", "characters", "relations", "world_settings",
        "setting_snapshots", "story_summaries", "change_proposals", "audit_logs", "event_logs",
    ] {
        assert!(has_col(&pool, table, "branch_id").await, "{table} 应有 branch_id 列");
    }
}

/// **最要紧的一条**：老数据零转换 —— 迁移后所有既有行 branch_id 必须是 NULL（= 主线）
#[tokio::test]
async fn migration_leaves_existing_rows_as_main_branch() {
    let pool = legacy_pool("data").await;
    ensure_branch_columns(&pool).await.unwrap();

    let c: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE branch_id IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(c, 1, "老角色必须仍归主线（branch_id IS NULL）");
    let name: String = sqlx::query_scalar("SELECT name FROM characters WHERE branch_id IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "夏婉琴", "数据内容不能变");

    let t: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes WHERE branch_id IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(t, 1, "老节点必须仍归主线");
}

/// 幂等：重复执行不报错、不重复建表
#[tokio::test]
async fn migration_is_idempotent() {
    let pool = legacy_pool("idem").await;
    ensure_branch_columns(&pool).await.unwrap();
    ensure_branch_columns(&pool).await.unwrap();
    ensure_branch_columns(&pool).await.unwrap();
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='branches'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "branches 表只该有一个");
    // 索引也只该有一份
    let idx: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_characters_branch'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(idx, 1, "索引不该重复创建");
}

/// 缺表的老库不能让迁移失败（表不存在时跳过该表）
#[tokio::test]
async fn migration_tolerates_missing_tables() {
    let dir = crate::paths::test_root()
        .join("branch-mig-sparse");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let opts = SqliteConnectOptions::new()
        .filename(dir.join("story.db"))
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    sqlx::query("CREATE TABLE characters (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    // 只有一张表，其余全缺 —— 迁移应成功，且只给存在的表加列
    ensure_branch_columns(&pool).await.unwrap();
    assert!(has_col(&pool, "characters", "branch_id").await);
}

/// `branch_clause` 的口径：主线是 IS NULL，分支是 = ?
#[test]
fn branch_clause_uses_null_for_main() {
    assert_eq!(branch_clause("branch_id", &None, 2), "branch_id IS NULL");
    assert_eq!(
        branch_clause("branch_id", &Some("abc".to_string()), 2),
        "branch_id = ?2"
    );
}
/// 真机验证（默认跳过）：对**真实故事库**跑分支迁移，打印迁移前后的结构。
/// 跑法：`cargo test --lib live_branch_migration -- --ignored --nocapture`
///
/// 为什么必须单独跑一次：迁移只在"打开故事库"时执行。真实库上跑一遍才能确认
/// ① 加列成功 ② **既有数据一行不少、且全部归主线（branch_id IS NULL）**
#[tokio::test]
#[ignore]
async fn live_branch_migration() {
    let db = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..").join("library").join("default").join("projects").join("7").join("story.db");
    if !db.exists() {
        println!("真实库不存在，跳过：{}", db.display());
        return;
    }
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display())).unwrap();
    let pool = SqlitePool::connect_with(opts).await.unwrap();

    // 迁移前：各表行数（用来证明"零丢失"）
    let tables = ["characters", "relations", "world_settings", "setting_snapshots",
                  "timeline_nodes", "messages", "change_proposals", "audit_logs", "event_logs"];
    let mut before = Vec::new();
    for tb in tables {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {tb}"))
            .fetch_one(&pool).await.unwrap_or(-1);
        before.push((tb, n));
    }
    println!("  迁移前行数：{before:?}");

    ensure_branch_columns(&pool).await.unwrap();
    ensure_branch_columns(&pool).await.unwrap(); // 幂等再跑一次

    let mut ok = true;
    for (tb, n0) in &before {
        let n1: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {tb}"))
            .fetch_one(&pool).await.unwrap_or(-2);
        let nulls: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {tb} WHERE branch_id IS NULL"))
            .fetch_one(&pool).await.unwrap_or(-3);
        let mark = if n1 == *n0 && nulls == n1 { "OK" } else { ok = false; "!!" };
        println!("    {tb:<20} 行数 {n0} → {n1}　归主线 {nulls}　{mark}");
    }
    assert!(ok, "迁移后行数必须不变、且所有既有行归主线（branch_id IS NULL）");

    let nb: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM branches").fetch_one(&pool).await.unwrap();
    println!("  branches 表：存在，当前 {nb} 条（迁移期应为 0）");
    println!("  ✔ 真机迁移通过：结构已就位、既有数据零转换");
}
