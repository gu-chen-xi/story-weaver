// 从 `evolution.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `evolution::rollback_sql_tests` —— 因为 `evolution.rs` + `evolution/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `evolution`，一行都不用改写。

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;

/// 造一个只含 world_settings 的临时库（文件库 —— :memory: 下每个连接是各自独立的库）
async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
    let dir = crate::paths::test_root()
        .join(format!("rollback-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let opts = SqliteConnectOptions::new()
        .filename(dir.join("story.db"))
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    sqlx::query(
        "CREATE TABLE world_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL, value TEXT NOT NULL,
            is_hard_rule INTEGER NOT NULL DEFAULT 0, version INTEGER NOT NULL DEFAULT 1,
            category TEXT NOT NULL DEFAULT '设定', meta_json TEXT NOT NULL DEFAULT '{}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    (pool, dir)
}

/// 与生产代码同形的插入语句；`bind_missing_as_zero` 控制用哪种写法，便于对照
async fn insert_world(pool: &SqlitePool, items: &[serde_json::Value], bind_missing_as_zero: bool) -> Result<(), String> {
    for w in items {
        let id = if bind_missing_as_zero {
            Some(w.get("id").and_then(|v| v.as_i64()).unwrap_or(0))
        } else {
            w.get("id").and_then(|v| v.as_i64())
        };
        sqlx::query(
            "INSERT INTO world_settings(id, key, value, is_hard_rule, version, category, meta_json)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(id)
        .bind(w.get("key").and_then(|v| v.as_str()).unwrap_or(""))
        .bind(w.get("value").and_then(|v| v.as_str()).unwrap_or(""))
        .bind(w.get("is_hard_rule").and_then(|v| v.as_i64()).unwrap_or(0))
        .bind(w.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
        .bind(w.get("category").and_then(|v| v.as_str()).unwrap_or("设定"))
        .bind(w.get("meta_json").and_then(|v| v.as_str()).unwrap_or("{}"))
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 老式 payload（**没有 id**）+ 现在的写法 → 必须成功，且自增 id 不冲突
#[tokio::test]
async fn missing_ids_fall_back_to_autoincrement() {
    let (pool, _dir) = temp_pool("ok").await;
    let items = vec![
        serde_json::json!({"key": "背景介绍", "value": "A"}),
        serde_json::json!({"key": "修女宿舍", "value": "B"}),
        serde_json::json!({"key": "客房", "value": "C"}),
    ];
    insert_world(&pool, &items, false).await.expect("缺失 id 时应能正常插入");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM world_settings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 3, "三条都应插入成功");
    // id 必须是互不相同的正数（自增分配）
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM world_settings ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(ids, vec![1, 2, 3], "应由数据库自增分配，而不是全为 0");
}

/// 对照：**旧写法**（缺失当 0）在同一条数据上必然失败 —— 复现用户报的 1555
#[tokio::test]
async fn old_binding_reproduces_unique_constraint_error() {
    let (pool, _dir) = temp_pool("bad").await;
    let items = vec![
        serde_json::json!({"key": "背景介绍", "value": "A"}),
        serde_json::json!({"key": "修女宿舍", "value": "B"}),
    ];
    let err = insert_world(&pool, &items, true).await.unwrap_err();
    assert!(
        err.contains("UNIQUE constraint failed") || err.contains("1555"),
        "旧写法应当复现主键冲突，实际错误：{err}"
    );
}

/// 新式 payload（带 id）仍应按 id 原样恢复 —— 修复不能把"有 id"的情形搞坏
#[tokio::test]
async fn explicit_ids_are_still_restored_as_is() {
    let (pool, _dir) = temp_pool("withid").await;
    let items = vec![
        serde_json::json!({"id": 7, "key": "修女宿舍", "value": "B"}),
        serde_json::json!({"id": 9, "key": "客房", "value": "C"}),
    ];
    insert_world(&pool, &items, false).await.unwrap();
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM world_settings ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(ids, vec![7, 9], "带 id 时应原样恢复（回滚要保持版本一致）");
}
