// 从 `timeline.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `timeline::summary_tests` —— 因为 `timeline.rs` + `timeline/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `timeline`，一行都不用改写。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;

/// 建一个真实临时 story.db（含 timeline_nodes.day_summary 列），返回 (pool, 目录)
async fn temp_story_db(tag: &str) -> (SqlitePool, std::path::PathBuf) {
    let dir = crate::paths::test_root().join(tag).join("projects").join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("story.db");
    let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    sqlx::query(
        "CREATE TABLE timeline_nodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
            is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
            day_summary TEXT NOT NULL DEFAULT '',
            parent_id INTEGER, branch_id TEXT)",
    ).execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER NOT NULL, role TEXT NOT NULL,
            content TEXT NOT NULL, mode TEXT NOT NULL DEFAULT 'chat', meta TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')), branch_id TEXT)",
    ).execute(&pool).await.unwrap();
    (pool, dir)
}

async fn add_node(pool: &SqlitePool, label: &str, sort: i64, summary: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, day_summary) VALUES(?1, ?2, 'day', 0, ?3) RETURNING id")
        .bind(label).bind(sort).bind(summary).fetch_one(pool).await.unwrap()
}

async fn add_msg(pool: &SqlitePool, node_id: i64, content: &str) {
    sqlx::query("INSERT INTO messages(node_id, role, content) VALUES(?1, 'user', ?2)")
        .bind(node_id).bind(content).execute(pool).await.unwrap();
}

#[test]
fn time_gaps_finds_blank_ranges() {
    // 1,2,3,4,5,8,9,10 → 只有 5→8 之间是空白（第 6、7 天）
    assert_eq!(super::time_gaps(&[1, 2, 3, 4, 5, 8, 9, 10]), vec![(5, 8)]);
    // 连续天数 → 无空白
    assert_eq!(super::time_gaps(&[1, 2, 3]), Vec::<(i64, i64)>::new());
    // 多个空白段
    assert_eq!(super::time_gaps(&[1, 4, 5, 10]), vec![(1, 4), (5, 10)]);
    // 单节点 / 空
    assert_eq!(super::time_gaps(&[3]), Vec::<(i64, i64)>::new());
    assert_eq!(super::time_gaps(&[]), Vec::<(i64, i64)>::new());
}

#[test]
fn next_day_uses_max_number_not_count() {
    // 用户报的 bug 场景：1,2,3,4,5,8,9,10 → 新增应为 11，而不是 9
    let labels: Vec<String> = ["第 1 天", "第 2 天", "第 3 天", "第 4 天", "第 5 天", "第 8 天", "第 9 天", "第 10 天"]
        .iter().map(|s| s.to_string()).collect();
    assert_eq!(super::next_day_number(&labels), 11, "应按最大天数 +1");

    // 有重复天数（真实数据里出现过两个「第 8 天」）也不受影响
    let dup: Vec<String> = ["第 1 天", "第 8 天", "第 8 天"].iter().map(|s| s.to_string()).collect();
    assert_eq!(super::next_day_number(&dup), 9);

    // 空表 → 第 1 天
    assert_eq!(super::next_day_number(&[]), 1);

    // 标签无数字 → 退回个数 +1，不 panic
    let odd: Vec<String> = ["回忆·那年夏天", "插曲"].iter().map(|s| s.to_string()).collect();
    assert_eq!(super::next_day_number(&odd), 3);
}

#[test]
fn parse_day_number_handles_variants() {
    assert_eq!(super::parse_day_number("第 5 天"), Some(5));
    assert_eq!(super::parse_day_number("第10天"), Some(10));
    assert_eq!(super::parse_day_number("Day 7"), Some(7));
    assert_eq!(super::parse_day_number("回忆·那年夏天"), None);
}

/// 硬规则：只给"当前节点之前、有剧情、还没有摘要"的节点补摘要
#[test]
fn backfill_targets_only_past_unsummarized_nodes() {
    tauri::async_runtime::block_on(async {
        let (pool, dir) = temp_story_db("summary_sel").await;

        let n1 = add_node(&pool, "第 1 天", 1, "").await;            // 有剧情、无摘要 → 应被选中
        let n2 = add_node(&pool, "第 2 天", 2, "已固化的摘要").await;  // 已有摘要 → 不选
        let n3 = add_node(&pool, "第 3 天", 3, "").await;            // 无剧情 → 不选
        let n4 = add_node(&pool, "第 4 天", 4, "").await;            // 当前节点 → 不选
        add_msg(&pool, n1, "第一天剧情").await;
        add_msg(&pool, n2, "第二天剧情").await;
        add_msg(&pool, n4, "当前节点剧情").await;

        let picked = select_nodes_needing_summary(&pool, n4, &None).await.unwrap();
        assert_eq!(picked.len(), 1, "只应选中第 1 天，实际: {picked:?}");
        assert_eq!(picked[0].0, n1);
        assert_eq!(picked[0].1, "第 1 天");

        // 倒叙节点永远不参与摘要
        let n5 = add_node(&pool, "回忆·那年夏天", 5, "").await;
        add_msg(&pool, n5, "回忆内容").await;
        sqlx::query("UPDATE timeline_nodes SET is_flashback = 1 WHERE id = ?1").bind(n5).execute(&pool).await.unwrap();
        let n6 = add_node(&pool, "第 5 天", 6, "").await;
        add_msg(&pool, n6, "第五天").await;
        let picked2 = select_nodes_needing_summary(&pool, n6, &None).await.unwrap();
        assert!(picked2.iter().all(|(id, _)| *id != n5), "倒叙节点不应参与摘要: {picked2:?}");

        let _ = std::fs::remove_dir_all(&dir);
    });
}

/// 硬规则：已有摘要 → generate_day_summary 非 force 时直接返回 None（不回改）
#[test]
fn existing_summary_is_never_regenerated() {
    tauri::async_runtime::block_on(async {
        let (pool, dir) = temp_story_db("summary_idem").await;
        let n1 = add_node(&pool, "第 1 天", 1, "").await;
        add_msg(&pool, n1, "剧情").await;
        // 直接调用会走 API；这里只验证"选择器"在摘要写入后不再返回该节点（等价于幂等）
        assert_eq!(select_nodes_needing_summary(&pool, n1, &None).await.unwrap().len(), 0, "当前节点不该被选中");
        let n2 = add_node(&pool, "第 2 天", 2, "").await;
        add_msg(&pool, n2, "剧情2").await;
        assert_eq!(select_nodes_needing_summary(&pool, n2, &None).await.unwrap().len(), 1, "第 1 天应被选中");
        sqlx::query("UPDATE timeline_nodes SET day_summary = '写好了' WHERE id = ?1").bind(n1).execute(&pool).await.unwrap();
        assert_eq!(select_nodes_needing_summary(&pool, n2, &None).await.unwrap().len(), 0, "已有摘要后不该再被选中（固化）");
        let _ = std::fs::remove_dir_all(&dir);
    });
}
