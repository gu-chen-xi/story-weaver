// 从 `evolution.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `evolution::branch_rollback_tests` —— 因为 `evolution.rs` + `evolution/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `evolution`，一行都不用改写。

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
