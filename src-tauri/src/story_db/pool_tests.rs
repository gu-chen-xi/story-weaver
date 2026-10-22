// 从 `story_db.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `story_db::pool_tests` —— 因为 `story_db.rs` + `story_db/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `story_db`，一行都不用改写。

use super::*;

/// 池测试串行锁。
///
/// 这些测试都在操作**同一份全局缓存**（清空 / 计数），并行跑会互相踩
/// （实测：断言"只建 1 个池"却看到 4）。非池测试不受影响 ——
/// 它们只会往缓存里加东西，不会清空。
fn pool_test_lock() -> &'static tokio::sync::Mutex<()> {
    static L: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    L.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn tmp_dir(tag: &str) -> PathBuf {
    let d = crate::paths::test_root()
        .join("pool-test")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn make_db(path: &PathBuf, ddl: &[&str]) {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    for s in ddl {
        sqlx::query(s).execute(&pool).await.unwrap();
    }
    pool.close().await;
}

async fn columns(pool: &SqlitePool, table: &str) -> Vec<String> {
    sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(pool)
        .await
        .unwrap()
        .iter()
        .filter_map(|r| r.try_get::<String, _>(1).ok())
        .collect()
}

async fn table_exists(pool: &SqlitePool, table: &str) -> bool {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
    )
    .bind(table)
    .fetch_one(pool)
    .await
    .unwrap();
    n > 0
}

/// **同一路径复用同一个池**：第二次调用不该新建池。
#[tokio::test]
async fn reuses_pool_for_same_path() {
    let _guard = pool_test_lock().lock().await;
    clear_pool_cache().await;
    let dir = tmp_dir("reuse");
    let db = dir.join("story.db");
    make_db(&db, &["CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY)"]).await;

    let before = pool_creations(&db);
    let p1 = open_story_pool_at(db.clone()).await.unwrap();
    let mid = pool_creations(&db);
    let p2 = open_story_pool_at(db.clone()).await.unwrap();
    let after = pool_creations(&db);

    assert_eq!(mid - before, 1, "第一次应新建 1 个池，实际 {}", mid - before);
    assert_eq!(after - mid, 0, "第二次应走缓存，不该再新建池（实际又新建了 {} 个）", after - mid);
    // 两个句柄都能用
    let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes")
        .fetch_one(&p1)
        .await
        .unwrap();
    let _: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes")
        .fetch_one(&p2)
        .await
        .unwrap();

    clear_pool_cache().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// **每个路径各有一份缓存**，互不干扰。
///
/// 语义重点：缓存键是**路径**（不是 story_id），否则同一 id 在不同数据根下会串。
/// 这里各自打开两次，都只应建 1 个池 —— 说明两份缓存是独立的。
///
/// 注：不去断言 `pool_cache_len()` 的**绝对值**，因为 `cargo test` 并行跑，
/// 别的测试也会往缓存里加东西（实测断言 =2 会看到 4）。
#[tokio::test]
async fn each_path_has_its_own_cache_entry() {
    let _guard = pool_test_lock().lock().await;
    clear_pool_cache().await;
    let dir = tmp_dir("distinct");
    let a = dir.join("a.db");
    let b = dir.join("b.db");
    make_db(&a, &["CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY)"]).await;
    make_db(&b, &["CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY)"]).await;

    let a0 = pool_creations(&a);
    let b0 = pool_creations(&b);
    let _pa1 = open_story_pool_at(a.clone()).await.unwrap();
    let _pb1 = open_story_pool_at(b.clone()).await.unwrap();
    let _pa2 = open_story_pool_at(a.clone()).await.unwrap();
    let _pb2 = open_story_pool_at(b.clone()).await.unwrap();

    assert_eq!(pool_creations(&a) - a0, 1, "a.db 两次打开应只建 1 个池");
    assert_eq!(pool_creations(&b) - b0, 1, "b.db 两次打开应只建 1 个池");

    clear_pool_cache().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// **清空缓存后应重建**（测试隔离依赖这个行为）。
#[tokio::test]
async fn clear_cache_forces_rebuild() {
    let _guard = pool_test_lock().lock().await;
    clear_pool_cache().await;
    let dir = tmp_dir("clear");
    let db = dir.join("story.db");
    make_db(&db, &["CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY)"]).await;

    let _ = open_story_pool_at(db.clone()).await.unwrap();
    let n1 = pool_creations(&db);
    clear_pool_cache().await;
    let _ = open_story_pool_at(db.clone()).await.unwrap();
    let n2 = pool_creations(&db);
    assert_eq!(n2 - n1, 1, "清空缓存后应重新建池");

    clear_pool_cache().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// **核心价值**：不管从哪个模块进来，迁移都会跑齐（原来是 6 种不同集合）。
///
/// 造一个"很老"的库：只有部分表、且缺各版本新增的列。
/// 打开后应补齐 **全部** 迁移（并集）。
#[tokio::test]
async fn all_migrations_run_regardless_of_entry_point() {
    let _guard = pool_test_lock().lock().await;
    clear_pool_cache().await;
    let dir = tmp_dir("migrations");
    let db = dir.join("story.db");
    make_db(&db, &[
        // 很老的 timeline_nodes：缺 tags / day_summary / parent_id
        "CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY, date_label TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
            is_flashback INTEGER NOT NULL DEFAULT 0)",
        // 很老的 characters：缺 A/D/G 那一批列
        "CREATE TABLE characters (id INTEGER PRIMARY KEY, name TEXT NOT NULL,
            profile TEXT NOT NULL DEFAULT '')",
        "CREATE TABLE relations (id INTEGER PRIMARY KEY, from_character_id INTEGER NOT NULL,
            to_character_id INTEGER NOT NULL, type TEXT NOT NULL DEFAULT '',
            direction TEXT NOT NULL DEFAULT '', strength INTEGER NOT NULL DEFAULT 50,
            state TEXT NOT NULL DEFAULT '')",
        "CREATE TABLE world_settings (id INTEGER PRIMARY KEY, key TEXT NOT NULL DEFAULT '',
            value TEXT NOT NULL DEFAULT '', is_hard_rule INTEGER NOT NULL DEFAULT 0)",
        // 老快照表：缺 temporary
        "CREATE TABLE setting_snapshots (id INTEGER PRIMARY KEY, version INTEGER NOT NULL,
            payload_json TEXT NOT NULL DEFAULT '')",
    ]).await;

    let pool = open_story_pool_at(db.clone()).await.unwrap();

    // ① timeline 三列
    let tl = columns(&pool, "timeline_nodes").await;
    for c in ["tags", "day_summary", "parent_id"] {
        assert!(tl.iter().any(|x| x == c), "timeline_nodes 缺列 {c}（实际 {tl:?}）");
    }
    // ② characters 扩展列
    let ch = columns(&pool, "characters").await;
    for c in ["alias", "role", "faction", "age", "gender", "first_appearance",
              "one_liner", "speech_style", "traits", "stat_stamina"] {
        assert!(ch.iter().any(|x| x == c), "characters 缺列 {c}");
    }
    // ③ relations / world_settings 扩展列
    let rel = columns(&pool, "relations").await;
    assert!(rel.iter().any(|x| x == "description"), "relations 缺 description");
    assert!(rel.iter().any(|x| x == "branch_id"), "relations 缺 branch_id");
    let ws = columns(&pool, "world_settings").await;
    assert!(ws.iter().any(|x| x == "category"), "world_settings 缺 category");

    // ④ 分支表 + 快照 temporary
    assert!(table_exists(&pool, "branches").await, "缺 branches 表");
    let snap = columns(&pool, "setting_snapshots").await;
    assert!(snap.iter().any(|x| x == "temporary"), "setting_snapshots 缺 temporary");

    // ⑤ 阶段摘要表
    assert!(table_exists(&pool, "story_summaries").await, "缺 story_summaries 表");

    clear_pool_cache().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// **量化收益**：缓存省掉了什么。
///
/// 关键点：`SqlitePool::connect_with` 本身是懒的（默认 min_connections=0），
/// 所以"建池"并不贵；**贵的是每次都要重跑一遍迁移探测**
/// （`PRAGMA table_info` × 若干 + 可能 `ALTER TABLE`）。
/// 旧实现在 13 个模块里各留一份 `open_story_pool`，谁进去都重跑一遍。
///
/// 这里不写硬断言（不同机器波动大），只打印数字。
#[tokio::test]
async fn cache_skips_repeated_migrations() {
    let _g = pool_test_lock().lock().await;
    clear_pool_cache().await;
    let dir = tmp_dir("perf");
    let db = dir.join("story.db");
    make_db(&db, &[
        "CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY, date_label TEXT NOT NULL)",
        "CREATE TABLE characters (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
        "CREATE TABLE relations (id INTEGER PRIMARY KEY, from_character_id INTEGER)",
        "CREATE TABLE world_settings (id INTEGER PRIMARY KEY, key TEXT NOT NULL)",
        "CREATE TABLE setting_snapshots (id INTEGER PRIMARY KEY, version INTEGER)",
    ]).await;

    const N: usize = 60;

    // A. 走缓存（真实路径）
    let t0 = std::time::Instant::now();
    for _ in 0..N {
        let _ = open_story_pool_at(db.clone()).await.unwrap();
    }
    let cached = t0.elapsed().as_secs_f64() * 1000.0;

    // B. 每次都重跑迁移（等价旧行为）
    let t1 = std::time::Instant::now();
    for _ in 0..N {
        let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display()))
            .unwrap();
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        apply_all_story_migrations(&pool).await;
    }
    let uncached = t1.elapsed().as_secs_f64() * 1000.0;

    println!(
        "[perf] {N} 次打开：每次重跑迁移 {uncached:.1} ms  →  走缓存 {cached:.1} ms  （{:.1}x）",
        uncached / cached.max(0.001)
    );
    // 只要求"不更慢"
    assert!(cached <= uncached * 1.5 + 5.0, "缓存不该比不缓存慢");

    clear_pool_cache().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⑤-B：老库（`change_proposals` 没有 `node_id`）打开后要自动补上这一列。
#[tokio::test]
async fn proposal_node_column_is_added_to_old_db() {
    clear_pool_cache().await;
    let dir = tmp_dir("proposal-node");
    let db = dir.join("story.db");
    // 故意用**没有** node_id 的老结构
    make_db(&db, &[
        "CREATE TABLE change_proposals (
            id INTEGER PRIMARY KEY AUTOINCREMENT, target_type TEXT NOT NULL,
            target_id INTEGER, field TEXT NOT NULL, old_value TEXT, new_value TEXT,
            reason TEXT, evidence TEXT, confidence REAL NOT NULL DEFAULT 0.0,
            status TEXT NOT NULL DEFAULT 'pending', note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            group_id INTEGER, branch_id TEXT)",
    ]).await;

    let pool = open_story_pool_at(db.clone()).await.unwrap();
    let cols = columns(&pool, "change_proposals").await;
    assert!(cols.iter().any(|c| c == "node_id"), "应补上 node_id 列（实际 {cols:?}）");

    // 幂等：再来一次不应报错
    ensure_proposal_node_column(&pool).await.unwrap();

    clear_pool_cache().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// 库文件不存在时不**悄悄**建空库（与原 13 个副本行为一致）。
#[tokio::test]
async fn missing_file_is_an_error_not_an_empty_db() {
    let _guard = pool_test_lock().lock().await;
    clear_pool_cache().await;
    let dir = tmp_dir("missing");
    let db = dir.join("nope.db");
    let r = open_story_pool_at(db.clone()).await;
    assert!(r.is_err(), "库不存在时应报错，而不是建出一个空库");
    assert!(!db.exists(), "不该创建文件");

    clear_pool_cache().await;
    let _ = std::fs::remove_dir_all(&dir);
}
