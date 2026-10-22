// 从 `timeline.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `timeline::summary_selector_branch_tests` —— 因为 `timeline.rs` + `timeline/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `timeline`，一行都不用改写。

use super::*;
use std::str::FromStr;

async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
    let dir = crate::paths::test_root()
        .join("q7-selector")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("story.db");
    let opts = sqlx::sqlite::SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display()))
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    sqlx::query(
        "CREATE TABLE timeline_nodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
            is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
            day_summary TEXT, parent_id INTEGER, branch_id TEXT)",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER NOT NULL,
            role TEXT NOT NULL, content TEXT NOT NULL,
            mode TEXT NOT NULL DEFAULT 'immersion', branch_id TEXT)",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE branches (
            id TEXT PRIMARY KEY, story_id TEXT NOT NULL, name TEXT NOT NULL,
            fork_node_id INTEGER, source_branch_id TEXT, color TEXT,
            note TEXT NOT NULL DEFAULT '')",
    ).execute(&pool).await.unwrap();
    (pool, dir)
}

async fn add_node(pool: &SqlitePool, label: &str, sort: i64, branch: Option<&str>, parent: Option<i64>) -> i64 {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO timeline_nodes(date_label, sort_order, parent_id, branch_id)
         VALUES(?1, ?2, ?3, ?4) RETURNING id",
    )
    .bind(label).bind(sort).bind(parent).bind(branch)
    .fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO messages(node_id, role, content, branch_id) VALUES(?1, 'user', 'x', ?2)")
        .bind(id).bind(branch).execute(pool).await.unwrap();
    id
}

/// 主线第 1~3 天；在第 2 天分叉出分支 B，B 上第 3 天、第 4 天。
/// 在**分支 B 的第 4 天**回看：只能选 B 可见的（主线 1~2 共享 + B 的 3），
/// **绝不能碰到主线第 3 天**（那是兄弟分支独有的节点）。
#[tokio::test]
async fn selector_does_not_pick_sibling_branch_nodes() {
    let (pool, dir) = temp_pool("sibling").await;
    let m1 = add_node(&pool, "第 1 天", 1, None, None).await;
    let m2 = add_node(&pool, "第 2 天", 2, None, Some(m1)).await;
    let m3 = add_node(&pool, "第 3 天", 3, None, Some(m2)).await;   // 主线独有
    sqlx::query(
        "INSERT INTO branches(id, story_id, name, fork_node_id) VALUES('B', '1', '支线', ?1)",
    ).bind(m2).execute(&pool).await.unwrap();
    let b3 = add_node(&pool, "第 3 天", 4, Some("B"), Some(m2)).await;
    let b4 = add_node(&pool, "第 4 天", 5, Some("B"), Some(b3)).await;

    // 在分支 B 的「第 4 天」：可见 = m1, m2, b3（不含主线 m3）
    let picked = select_nodes_needing_summary(&pool, b4, &Some("B".into())).await.unwrap();
    let ids: Vec<i64> = picked.iter().map(|(id, _)| *id).collect();
    assert!(!ids.contains(&m3), "**不该选中主线独有的第 3 天**（那是兄弟分支的节点）：{picked:?}");
    assert!(ids.contains(&m1) && ids.contains(&m2) && ids.contains(&b3),
        "分叉点之前的共享历史 + 本分支节点都应被选中：{picked:?}");
    assert_eq!(ids.len(), 3, "只该有这 3 个：{picked:?}");

    // 主线视角：可见 = m1, m2, m3（不含分支的 b3/b4）
    let picked_main = select_nodes_needing_summary(&pool, m3, &None).await.unwrap();
    let main_ids: Vec<i64> = picked_main.iter().map(|(id, _)| *id).collect();
    assert!(!main_ids.contains(&b3), "主线不该选中分支的节点：{picked_main:?}");
    assert_eq!(main_ids, vec![m1, m2], "主线只看得到自己之前的节点：{picked_main:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 主线行为必须不变（回归保护）。
#[tokio::test]
async fn selector_mainline_behaviour_unchanged() {
    let (pool, dir) = temp_pool("main").await;
    let m1 = add_node(&pool, "第 1 天", 1, None, None).await;
    let m2 = add_node(&pool, "第 2 天", 2, None, Some(m1)).await;
    let m3 = add_node(&pool, "第 3 天", 3, None, Some(m2)).await;
    let picked = select_nodes_needing_summary(&pool, m3, &None).await.unwrap();
    assert_eq!(picked.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![m1, m2]);
    let _ = std::fs::remove_dir_all(&dir);
}
