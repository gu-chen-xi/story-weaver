// 从 `world.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `world::lore_query_tests` —— 因为 `world.rs` + `world/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `world`，一行都不用改写。

/// **回归锁**：老写法（`?2` 只 bind 一次）在主线查不到，新写法查得到。
///
/// 这是用户实测（记录 #39 提到"怀表"但世界书没注入）的根因：
/// sqlx 的 bind 按**调用顺序**落到 1、2… 号位，而 SQL 要的是 `?2` —— 参数错位后
/// 查询返回 0 行且**不报错**（被 `unwrap_or_default()` 吞掉），界面上完全看不出。
#[tokio::test]
async fn old_bind_shape_returns_nothing_on_main_branch() {
    let pool = pool_with_rows().await;

    // 老写法：category = ?2，clause 从 ?1 起 —— 主线时 clause 不含占位符
    let old_clause = crate::story_db::branch_clause("branch_id", &None, 1);
    let old_sql = format!(
        "SELECT key FROM world_settings WHERE category = ?2 AND {} ORDER BY id",
        old_clause
    );
    let old = sqlx::query(&old_sql)
        .bind(CATEGORY_LORE)
        .fetch_all(&pool)
        .await;
    match &old {
        Ok(rows) => assert_eq!(rows.len(), 0, "老写法在主线上必须是 0 行（这正是那个 bug）"),
        // 报错也算"老写法不工作"，两种都接受
        Err(_) => {}
    }

    let new = fetch_lore_entries(&pool, &None).await;
    assert_eq!(new.len(), 1, "新写法必须查得到：{new:?}");
}
use super::*;
use sqlx::sqlite::SqlitePoolOptions;

async fn pool_with_rows() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE world_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT, value TEXT,
            category TEXT, meta_json TEXT DEFAULT '{}', branch_id TEXT)",
    )
    .execute(&pool)
    .await
    .unwrap();
    // 一条世界书（主线）+ 一条物品（主线）+ 一条分支世界书
    for (k, v, cat, br) in [
        ("大肥鱼送的", "表的故事", CATEGORY_LORE, None::<&str>),
        ("怀表", "物品说明", "物品", None),
        ("分支里的书", "分支内容", CATEGORY_LORE, Some("b1")),
    ] {
        sqlx::query(
            "INSERT INTO world_settings(key, value, category, meta_json, branch_id)
             VALUES(?1, ?2, ?3, '{\"triggers\":\"怀表\"}', ?4)",
        )
        .bind(k)
        .bind(v)
        .bind(cat)
        .bind(br)
        .execute(&pool)
        .await
        .unwrap();
    }
    pool
}

/// **回归锁**：主线（`branch = None`）必须能查到世界书条目。
///
/// 这条测的就是那个"占位符 ?2 但只 bind 一次"的 bug ——
/// 修之前这里会返回 0 条（而且不报错，只看界面永远发现不了）。
#[tokio::test]
async fn main_branch_finds_lore_entries() {
    let pool = pool_with_rows().await;
    let rows = fetch_lore_entries(&pool, &None).await;
    assert_eq!(rows.len(), 1, "主线应恰好查到 1 条世界书：{rows:?}");
    assert_eq!(rows[0].0, "大肥鱼送的");
    assert!(rows[0].2.contains("triggers"), "meta_json 要原样带出来");
}

/// 分支只查自己那份（不串主线）
#[tokio::test]
async fn branch_scoped_lore_is_isolated() {
    let pool = pool_with_rows().await;
    let rows = fetch_lore_entries(&pool, &Some("b1".to_string())).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "分支里的书");
}

/// 物品不属于世界书，不该被这个函数取到
#[tokio::test]
async fn other_categories_are_not_returned() {
    let pool = pool_with_rows().await;
    let rows = fetch_lore_entries(&pool, &None).await;
    assert!(rows.iter().all(|(k, _, _)| k != "怀表"), "物品不该进世界书结果");
}
