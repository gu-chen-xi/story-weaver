// 从 `generate_stream.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `generate_stream::debug_cap_tests` —— 因为 `generate_stream.rs` + `generate_stream/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `generate_stream`，一行都不用改写。

use super::prune_debug_requests;
use sqlx::sqlite::SqliteConnectOptions;

async fn temp_pool(tag: &str) -> (sqlx::SqlitePool, std::path::PathBuf) {
    let dir = crate::paths::test_root().join(format!("debug_cap_{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let opts = SqliteConnectOptions::new().filename(dir.join("t.db")).create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
    sqlx::query(
        "CREATE TABLE debug_requests(id INTEGER PRIMARY KEY AUTOINCREMENT, system_prompt TEXT NOT NULL DEFAULT '')",
    )
    .execute(&pool)
    .await
    .unwrap();
    (pool, dir)
}

/// 自动上限：超出部分按 id 从旧到新删掉，保留最新的
///
/// 注意下限是 10 条（`max_rows.max(10)`）—— 配置写错也不会把记录全清。
#[tokio::test]
async fn prune_keeps_newest_rows() {
    let (pool, dir) = temp_pool("keep").await;
    for i in 0..14 {
        sqlx::query("INSERT INTO debug_requests(system_prompt) VALUES(?1)")
            .bind(format!("p{i}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    let removed = prune_debug_requests(&pool, 10).await.unwrap();
    assert_eq!(removed, 4, "14 条留 10 条 → 删 4 条");

    let left: Vec<(i64, String)> = sqlx::query_as("SELECT id, system_prompt FROM debug_requests ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(left.len(), 10);
    assert_eq!(left[0].1, "p4", "留下的必须是最新的 10 条");
    assert_eq!(left[9].1, "p13");
    let _ = std::fs::remove_dir_all(dir);
}

/// 没超上限时一条都不删
#[tokio::test]
async fn prune_noop_when_under_limit() {
    let (pool, dir) = temp_pool("noop").await;
    for i in 0..3 {
        sqlx::query("INSERT INTO debug_requests(system_prompt) VALUES(?1)")
            .bind(format!("p{i}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    assert_eq!(prune_debug_requests(&pool, 300).await.unwrap(), 0);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM debug_requests").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 3);
    let _ = std::fs::remove_dir_all(dir);
}

/// 上限被传成 0/负数时也至少要留 10 条（防止配置写错把记录全清）
#[tokio::test]
async fn prune_has_floor_of_ten() {
    let (pool, dir) = temp_pool("floor").await;
    for i in 0..15 {
        sqlx::query("INSERT INTO debug_requests(system_prompt) VALUES(?1)")
            .bind(format!("p{i}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    prune_debug_requests(&pool, 0).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM debug_requests").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 10, "下限 10 条");
    let _ = std::fs::remove_dir_all(dir);
}
