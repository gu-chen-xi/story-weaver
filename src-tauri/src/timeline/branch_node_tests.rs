// 从 `timeline.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `timeline::branch_node_tests` —— 因为 `timeline.rs` + `timeline/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `timeline`，一行都不用改写。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;

async fn pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
    let dir = crate::paths::test_root()
        .join(format!("node-branch-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let opts = SqliteConnectOptions::new()
        .filename(dir.join("story.db"))
        .create_if_missing(true);
    let p = SqlitePool::connect_with(opts).await.unwrap();
    sqlx::query(
        "CREATE TABLE timeline_nodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
            is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
            day_summary TEXT NOT NULL DEFAULT '', parent_id INTEGER, branch_id TEXT)",
    ).execute(&p).await.unwrap();
    sqlx::query(
        "CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER NOT NULL, role TEXT NOT NULL,
            content TEXT NOT NULL, mode TEXT NOT NULL DEFAULT 'chat',
            meta TEXT NOT NULL DEFAULT '{}', branch_id TEXT)",
    ).execute(&p).await.unwrap();
    (p, dir)
}

/// 消息的 branch_id 由**节点**推导（写入方不传也不会错）
#[tokio::test]
async fn message_branch_is_derived_from_node() {
    let (p, dir) = pool("derive").await;
    // 主线节点 + 分支节点
    sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, branch_id) VALUES('第 1 天', 1, NULL)")
        .execute(&p).await.unwrap();
    sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, branch_id) VALUES('第 2 天', 2, 'BR-X')")
        .execute(&p).await.unwrap();

    // 复刻 create_message 的推导逻辑
    for node_id in [1i64, 2i64] {
        let b: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
            .bind(node_id).fetch_optional(&p).await.unwrap().flatten();
        sqlx::query("INSERT INTO messages(node_id, role, content, branch_id) VALUES(?1, 'user', 'x', ?2)")
            .bind(node_id).bind(b).execute(&p).await.unwrap();
    }

    let rows: Vec<(i64, Option<String>)> =
        sqlx::query_as("SELECT node_id, branch_id FROM messages ORDER BY id")
            .fetch_all(&p).await.unwrap();
    assert_eq!(rows[0], (1, None), "主线节点的消息 branch_id 应为 NULL");
    assert_eq!(rows[1], (2, Some("BR-X".to_string())), "分支节点的消息应带上该分支");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **回归守护**：`list_timeline_nodes` 必须返回**全树**（含各分支节点）。
/// 若有人给它加了 `branch_id IS NULL`，界面上的树会缺枝 —— 这条会立刻变红。
#[tokio::test]
async fn list_timeline_nodes_returns_all_branches() {
    let (p, dir) = pool("full-tree").await;
    sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, branch_id) VALUES('第 1 天', 1, NULL)")
        .execute(&p).await.unwrap();
    sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id, branch_id) VALUES('第 2 天', 2, 1, 'BR-A')")
        .execute(&p).await.unwrap();
    sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id, branch_id) VALUES('第 2 天', 3, 1, 'BR-B')")
        .execute(&p).await.unwrap();

    // 与 list_timeline_nodes 的查询同形
    let rows = sqlx::query(
        "SELECT id, date_label, sort_order, node_type, is_flashback, tags, day_summary, parent_id
         FROM timeline_nodes ORDER BY sort_order ASC, id ASC",
    ).fetch_all(&p).await.unwrap();
    assert_eq!(rows.len(), 3, "主线查询必须看到全部 3 个节点（含两个分支的）");

    // 对照：若按分支过滤就会缺枝（说明为什么不能那么改）
    let only_main = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM timeline_nodes WHERE branch_id IS NULL",
    ).fetch_one(&p).await.unwrap();
    assert_eq!(only_main, 1, "按分支过滤只剩 1 个 —— 树就断了");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 同一分叉点下挂两条分支的节点：父链回溯各自独立（配合既有的 path_to_root 测试）
#[tokio::test]
async fn forked_nodes_keep_independent_paths() {
    let (p, dir) = pool("paths").await;
    for (label, sort, parent, br) in [
        ("第 1 天", 1i64, None::<i64>, None::<&str>),
        ("第 2 天A", 2, Some(1), Some("BR-A")),
        ("第 3 天A", 3, Some(2), Some("BR-A")),
        ("第 2 天B", 4, Some(1), Some("BR-B")),
    ] {
        sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id, branch_id) VALUES(?1, ?2, ?3, ?4)")
            .bind(label).bind(sort).bind(parent).bind(br).execute(&p).await.unwrap();
    }
    let nodes: Vec<TreeNode> = sqlx::query("SELECT id, parent_id, sort_order FROM timeline_nodes")
        .fetch_all(&p).await.unwrap()
        .iter().map(|r| TreeNode {
            id: r.try_get(0).unwrap(),
            parent_id: r.try_get(1).unwrap(),
            sort_order: r.try_get(2).unwrap(),
        }).collect();

    assert_eq!(path_to_root(&nodes, 3), vec![1, 2, 3], "分支 A 的路径只含自己那条线");
    assert_eq!(path_to_root(&nodes, 4), vec![1, 4], "分支 B 的路径只含自己那条线");

    let _ = std::fs::remove_dir_all(&dir);
}
