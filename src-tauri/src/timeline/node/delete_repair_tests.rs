// 删节点 / 悬空父链的测试（v3.5.2）。
//
// 为什么必须有：用户实测踩到过「时间线只剩前半段」——
// 删掉第 4 天之后，第 5 天的 parent_id 还指着那个已删除的 id，
// 整条后半段从根不可达。前端只能靠兜底逻辑打补丁，数据层才是病根。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use std::str::FromStr;

async fn mem_pool() -> SqlitePool {
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::from_str("sqlite::memory:").unwrap().create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE timeline_nodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date_label TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0,
            node_type TEXT NOT NULL DEFAULT 'day',
            is_flashback INTEGER NOT NULL DEFAULT 0,
            tags TEXT NOT NULL DEFAULT '',
            day_summary TEXT NOT NULL DEFAULT '',
            day_summary_at INTEGER NOT NULL DEFAULT 0,
            parent_id INTEGER,
            branch_id TEXT
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER, role TEXT, content TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE branches (id TEXT PRIMARY KEY, story_id TEXT, name TEXT, fork_node_id INTEGER)")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

async fn add(pool: &SqlitePool, label: &str, sort: i64, parent: Option<i64>, branch: Option<&str>) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO timeline_nodes(date_label, sort_order, parent_id, branch_id) VALUES(?1,?2,?3,?4) RETURNING id",
    )
    .bind(label)
    .bind(sort)
    .bind(parent)
    .bind(branch)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// 删中间一天：它的子节点要接到**祖父**上（链不能断）
#[tokio::test]
async fn delete_middle_node_reparents_children_to_grandparent() {
    let pool = mem_pool().await;
    let d1 = add(&pool, "第 1 天", 1, None, None).await;
    let d2 = add(&pool, "第 2 天", 2, Some(d1), None).await;
    let d3 = add(&pool, "第 3 天", 3, Some(d2), None).await;
    let d4 = add(&pool, "第 4 天", 4, Some(d3), None).await;

    // 删第 3 天 → 第 4 天应接到第 2 天
    delete_timeline_node_in(&pool, d3).await.unwrap();
    let p4: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM timeline_nodes WHERE id = ?1")
        .bind(d4)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(p4, Some(d2), "第 4 天应接到祖父（第 2 天）");

    // 悬空行数应为 0
    let dangling: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM timeline_nodes n WHERE n.parent_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM timeline_nodes p WHERE p.id = n.parent_id)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(dangling, 0, "删完不该留下悬空父链");
}

/// 删分叉点（无分支引用）：两个子节点都接到祖父，不会丢
#[tokio::test]
async fn delete_forkish_node_keeps_all_children() {
    let pool = mem_pool().await;
    let d1 = add(&pool, "第 1 天", 1, None, None).await;
    let d2 = add(&pool, "第 2 天", 2, Some(d1), None).await;
    let a = add(&pool, "第 2 天", 3, Some(d2), Some("B1")).await;
    let b = add(&pool, "第 2 天", 4, Some(d2), Some("B2")).await;

    delete_timeline_node_in(&pool, d2).await.unwrap();
    for id in [a, b] {
        let p: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM timeline_nodes WHERE id = ?1")
            .bind(id).fetch_one(&pool).await.unwrap();
        assert_eq!(p, Some(d1), "子节点都应接到祖父，而不是悬空");
    }
}

/// 被分支当作分叉点 → 仍然拒绝删除（这条是既有的安全规则，不能被新逻辑绕过）
#[tokio::test]
async fn delete_still_rejected_when_node_is_a_fork_point() {
    let pool = mem_pool().await;
    let d1 = add(&pool, "第 1 天", 1, None, None).await;
    let d2 = add(&pool, "第 2 天", 2, Some(d1), None).await;
    let _d3 = add(&pool, "第 3 天", 3, Some(d2), None).await;
    sqlx::query("INSERT INTO branches(id, story_id, name, fork_node_id) VALUES('B1','1','分支A',?1)")
        .bind(d2).execute(&pool).await.unwrap();
    let err = delete_timeline_node_in(&pool, d2).await.unwrap_err();
    assert!(format!("{err:?}").contains("分叉点"), "应拒绝：{err:?}");
}

/// 自愈：历史库里悬空的 parent_id 按 sort_order 接回前一个节点
#[tokio::test]
async fn repair_reattaches_dangling_parents_in_order() {
    let pool = mem_pool().await;
    let d1 = add(&pool, "第 1 天", 1, None, None).await;
    let d2 = add(&pool, "第 2 天", 2, Some(d1), None).await;
    let d3 = add(&pool, "第 3 天", 3, Some(d2), None).await;
    // 模拟"第 4 天被删、第 5 天悬空"：parent 指向不存在的 999
    let d5 = add(&pool, "第 5 天", 5, Some(999), None).await;

    let fixed = repair_dangling_parents(&pool).await.unwrap();
    assert_eq!(fixed, 1, "应修一行");
    let p5: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM timeline_nodes WHERE id = ?1")
        .bind(d5).fetch_one(&pool).await.unwrap();
    assert_eq!(p5, Some(d3), "第 5 天应接回第 3 天（sort_order 比它小的最近一个）");

    // 幂等：再跑一次不该再改
    assert_eq!(repair_dangling_parents(&pool).await.unwrap(), 0);
}

/// 自愈不该动正常数据
#[tokio::test]
async fn repair_is_noop_on_healthy_data() {
    let pool = mem_pool().await;
    let d1 = add(&pool, "第 1 天", 1, None, None).await;
    let _d2 = add(&pool, "第 2 天", 2, Some(d1), None).await;
    assert_eq!(repair_dangling_parents(&pool).await.unwrap(), 0);
}

/// 分支节点的悬空同样修，但只在**同一条线内**找父（不能把主线节点接到分支下）
#[tokio::test]
async fn repair_stays_within_the_same_line() {
    let pool = mem_pool().await;
    let _m1 = add(&pool, "第 1 天", 1, None, None).await;
    let m2 = add(&pool, "第 2 天", 2, Some(1), None).await;
    let b1 = add(&pool, "第 8 天", 8, Some(999), Some("B1")).await;
    let b2 = add(&pool, "第 9 天", 9, Some(b1), Some("B1")).await;

    repair_dangling_parents(&pool).await.unwrap();
    let pb1: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM timeline_nodes WHERE id = ?1")
        .bind(b1).fetch_one(&pool).await.unwrap();
    // 同分支里只有 b1 自己 → 找不到候选，保持原样（不跨线乱接）
    assert_eq!(pb1, Some(999), "同线内没有候选时不该乱接");
    let pb2: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM timeline_nodes WHERE id = ?1")
        .bind(b2).fetch_one(&pool).await.unwrap();
    assert_eq!(pb2, Some(b1), "正常的分支父子不该被动");
    assert_eq!(m2, 2);
}
