// 从 `evolution.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `evolution::temporary_snapshot_tests` —— 因为 `evolution.rs` + `evolution/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `evolution`，一行都不用改写。

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
