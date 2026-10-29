// 关系账本测试（2026-10-29）。
// 每条对应塘主定的一条规矩：可查、可撤、夹取、分支隔离、撤销行不可再撤。

use super::*;
use sqlx::sqlite::SqlitePoolOptions;

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE relations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_character_id INTEGER, to_character_id INTEGER,
            direction TEXT DEFAULT '单向', strength INTEGER DEFAULT 50,
            state TEXT DEFAULT '稳定', version INTEGER DEFAULT 1, branch_id TEXT)",
    )
    .execute(&pool)
    .await
    .unwrap();
    ensure_table(&pool).await.unwrap();
    sqlx::query("INSERT INTO relations(id, from_character_id, to_character_id, strength) VALUES(1, 1, 2, 50)")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

#[tokio::test]
async fn record_then_list_roundtrip() {
    let pool = pool().await;
    let id = record(&pool, &None, 1, Some(7), Some(80), -10, "白给", "她没反抗", "director_cost")
        .await
        .unwrap();
    let rows = list_in(&pool, &None, None, 10).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].relation_id, 1);
    assert_eq!(rows[0].delta, -10);
    assert_eq!(rows[0].node_id, Some(7));
    assert_eq!(rows[0].message_id, Some(80));
    assert_eq!(rows[0].source, "director_cost");
    assert_eq!(rows[0].revoked, 0);
}

#[tokio::test]
async fn undo_reverses_strength_and_marks_the_original() {
    let pool = pool().await;
    // 先模拟一次扣分（数值已由 bump_strength 改过）
    sqlx::query("UPDATE relations SET strength = 40 WHERE id = 1").execute(&pool).await.unwrap();
    let id = record(&pool, &None, 1, None, None, -10, "白给", "她没反抗", "director_cost")
        .await
        .unwrap();
    let res = undo_in(&pool, id).await.unwrap();
    assert_eq!((res.old, res.new), (40, 50), "撤销要回到原值");
    let s: i64 = sqlx::query_scalar("SELECT strength FROM relations WHERE id = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(s, 50);
    let rows = list_in(&pool, &None, None, 10).await.unwrap();
    assert_eq!(rows.len(), 2, "撤销要留一行（只增不改）");
    assert_eq!(rows[0].source, "undo");
    assert_eq!(rows[0].delta, 10);
    assert_eq!(rows[1].revoked, 1, "原行标记已撤销");
}

#[tokio::test]
async fn undo_is_rejected_twice() {
    let pool = pool().await;
    let id = record(&pool, &None, 1, None, None, -5, "心软", "", "director_cost")
        .await
        .unwrap();
    undo_in(&pool, id).await.unwrap();
    assert!(undo_in(&pool, id).await.is_err(), "同一条不能撤两次");
}

#[tokio::test]
async fn undo_clamps_to_zero_and_hundred() {
    let pool = pool().await;
    // 现在 50，撤销一次 -80 的扣分 → 应该只到 100 而不是 130
    let id = record(&pool, &None, 1, None, None, -80, "受伤", "", "director_cost")
        .await
        .unwrap();
    let res = undo_in(&pool, id).await.unwrap();
    assert_eq!(res.new, 100, "夹到 100");
}

#[tokio::test]
async fn branch_scope_is_respected() {
    let pool = pool().await;
    record(&pool, &Some("b1".to_string()), 1, None, None, -5, "心软", "", "director_cost")
        .await
        .unwrap();
    assert_eq!(list_in(&pool, &None, None, 10).await.unwrap().len(), 0, "主线看不到分支的账");
    assert_eq!(list_in(&pool, &Some("b1".to_string()), None, 10).await.unwrap().len(), 1);
}
