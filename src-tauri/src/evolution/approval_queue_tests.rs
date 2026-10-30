// 从 `evolution.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `evolution::approval_queue_tests` —— 因为 `evolution.rs` + `evolution/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `evolution`，一行都不用改写。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;
use std::str::FromStr;

/// 造一个够用的临时故事库：提案表 + 快照需要的三张设定表。
async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
    let dir = crate::paths::test_root()
        .join("approval-queue")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("story.db");
    let opts = sqlx::sqlite::SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display()))
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();

    for ddl in [
        "CREATE TABLE change_proposals (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            target_type TEXT NOT NULL, target_id INTEGER, field TEXT NOT NULL,
            old_value TEXT, new_value TEXT, reason TEXT, evidence TEXT,
            confidence REAL NOT NULL DEFAULT 0.0,
            status TEXT NOT NULL DEFAULT 'pending', note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            group_id INTEGER, branch_id TEXT, node_id INTEGER)",
        "CREATE TABLE characters (
            id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL,
            alias TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '',
            faction TEXT NOT NULL DEFAULT '', age TEXT NOT NULL DEFAULT '',
            gender TEXT NOT NULL DEFAULT '', first_appearance TEXT NOT NULL DEFAULT '',
            one_liner TEXT NOT NULL DEFAULT '', profile TEXT NOT NULL DEFAULT '',
            speech_style TEXT NOT NULL DEFAULT '', traits TEXT NOT NULL DEFAULT '',
            current_status TEXT NOT NULL DEFAULT '',
            stat_stamina INTEGER NOT NULL DEFAULT 50, stat_sanity INTEGER NOT NULL DEFAULT 50,
            stat_emotion INTEGER NOT NULL DEFAULT 50, stat_screen INTEGER NOT NULL DEFAULT 50,
                background_past TEXT NOT NULL DEFAULT '', motivation TEXT NOT NULL DEFAULT '',
                goal TEXT NOT NULL DEFAULT '', dialogue_examples TEXT NOT NULL DEFAULT '',
                behavior_rules TEXT NOT NULL DEFAULT '', appearance TEXT NOT NULL DEFAULT '',
            version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
        "CREATE TABLE relations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_character_id INTEGER NOT NULL, to_character_id INTEGER NOT NULL,
            relation_type TEXT NOT NULL DEFAULT '', direction TEXT NOT NULL DEFAULT '',
            strength INTEGER NOT NULL DEFAULT 50, state TEXT NOT NULL DEFAULT '',
            description TEXT NOT NULL DEFAULT '', evidence TEXT NOT NULL DEFAULT '',
            version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
        "CREATE TABLE world_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL DEFAULT '',
            value TEXT NOT NULL DEFAULT '', is_hard_rule INTEGER NOT NULL DEFAULT 0,
            version INTEGER NOT NULL DEFAULT 1, category TEXT NOT NULL DEFAULT '设定',
            meta_json TEXT NOT NULL DEFAULT '{}', branch_id TEXT)",
        "CREATE TABLE audit_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT, action TEXT NOT NULL, target TEXT,
            before TEXT, after TEXT, operator TEXT NOT NULL DEFAULT 'system',
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
        "CREATE TABLE setting_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
            payload_json TEXT NOT NULL, parent_version INTEGER,
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            temporary INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '',
            branch_id TEXT)",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }

    // 两个角色 + 一条关系（强度 50）+ 一条世界观
    for (i, n) in [(1, "甲"), (2, "乙")] {
        sqlx::query("INSERT INTO characters(id, name) VALUES(?1, ?2)")
            .bind(i).bind(n).execute(&pool).await.unwrap();
    }
    sqlx::query(
        "INSERT INTO relations(id, from_character_id, to_character_id, relation_type, direction, strength, state)
         VALUES(1, 1, 2, '陌生', '单向', 50, '平稳')",
    ).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO world_settings(id, key, value) VALUES(1, '地点', '城')")
        .execute(&pool).await.unwrap();
    (pool, dir)
}

/// 手动插一条关系强度提案，返回 group_id。
async fn add_strength_proposal(pool: &SqlitePool, group: i64, new_strength: i64) -> i64 {
    insert_proposal(
        pool, &None, 7, group, "relation", 1, "strength",
        "50", &new_strength.to_string(),
        "测试用", "第7天：甲关心乙", 0.9, "pending",
    ).await.unwrap()
}

async fn strength(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT strength FROM relations WHERE id = 1")
        .fetch_one(pool).await.unwrap()
}

async fn statuses(pool: &SqlitePool, group: i64) -> Vec<String> {
    sqlx::query_scalar("SELECT status FROM change_proposals WHERE group_id = ?1 ORDER BY id")
        .bind(group).fetch_all(pool).await.unwrap()
}

async fn snapshot_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM setting_snapshots")
        .fetch_one(pool).await.unwrap()
}

/// ① 默认（need_approval=true）：**只入队，关系不该变**。
#[tokio::test]
async fn approval_on_queues_without_touching_data() {
    let (pool, dir) = temp_pool("on").await;
    let mut proposals = Vec::new();
    let mut applied = Vec::new();
    let mut notes = Vec::new();

    add_strength_proposal(&pool, 1, 90).await;
    queue_or_apply(&pool, "1", 1, true, &mut proposals, &mut applied, &mut notes)
        .await.unwrap();

    assert_eq!(strength(&pool).await, 50, "开关打开时不该改关系强度");
    assert_eq!(statuses(&pool, 1).await, vec!["pending"], "提案应停在待确认");
    assert!(applied.is_empty(), "不该记录任何已应用组");
    assert_eq!(proposals.len(), 1, "报告里应带上这条待确认提案");
    assert_eq!(proposals[0].node_id, Some(7), "提案要记住来源节点");
    assert!(notes.iter().any(|n| n.contains("待确认")), "应提示已入队（实际：{notes:?}）");

    let _ = std::fs::remove_dir_all(&dir);
}

/// ② 关掉开关：恢复旧行为 —— 立刻应用。
#[tokio::test]
async fn approval_off_restores_auto_apply() {
    let (pool, dir) = temp_pool("off").await;
    let mut proposals = Vec::new();
    let mut applied = Vec::new();
    let mut notes = Vec::new();

    add_strength_proposal(&pool, 1, 90).await;
    queue_or_apply(&pool, "1", 1, false, &mut proposals, &mut applied, &mut notes)
        .await.unwrap();

    assert_eq!(strength(&pool).await, 90, "关掉开关应立刻生效");
    assert_eq!(statuses(&pool, 1).await, vec!["applied"]);
    assert_eq!(applied, vec![1], "应记录已应用的组");

    let _ = std::fs::remove_dir_all(&dir);
}

/// ③ 批量落地：两组都落地，但**只留一个还原点**。
#[tokio::test]
async fn batch_confirm_applies_all_but_takes_one_snapshot() {
    let (pool, dir) = temp_pool("batch").await;
    add_strength_proposal(&pool, 1, 60).await;
    add_strength_proposal(&pool, 2, 80).await;
    let before = snapshot_count(&pool).await;

    let msg = confirm_proposals_batch_in(&pool, "1", vec![1, 2], None).await.unwrap();

    assert!(msg.contains("已落地 2 组"), "结果文案应说明落地 2 组（实际：{msg}）");
    assert_eq!(snapshot_count(&pool).await - before, 1, "整批只该留一个还原点");
    assert_eq!(statuses(&pool, 1).await, vec!["applied"]);
    assert_eq!(statuses(&pool, 2).await, vec!["applied"]);
    // 两组分别应用：组1 → 60，组2 → 80
    assert_eq!(strength(&pool).await, 80);

    let _ = std::fs::remove_dir_all(&dir);
}

/// ④ 批量拒绝：状态变 rejected，数据不动。
#[tokio::test]
async fn batch_reject_marks_rejected_and_keeps_data() {
    let (pool, dir) = temp_pool("reject").await;
    add_strength_proposal(&pool, 1, 60).await;
    add_strength_proposal(&pool, 2, 80).await;

    let msg = reject_proposals_batch_in(&pool, vec![1, 2]).await.unwrap();
    assert!(msg.contains("已拒绝 2 条"), "实际：{msg}");
    assert_eq!(statuses(&pool, 1).await, vec!["rejected"]);
    assert_eq!(strength(&pool).await, 50, "拒绝不该改数据");

    let _ = std::fs::remove_dir_all(&dir);
}

/// ⑤ 空选择要报错，不能静默成功。
#[tokio::test]
async fn empty_selection_is_an_error() {
    let (pool, dir) = temp_pool("empty").await;
    assert!(confirm_proposals_batch_in(&pool, "1", vec![], None).await.is_err());
    assert!(reject_proposals_batch_in(&pool, vec![]).await.is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⑥ 安全审计 2026-10-30 · F-03：排队期间关系被改过 → 落地必须**拒绝**，不能覆盖人工改动。
#[tokio::test]
async fn stale_proposal_is_rejected_instead_of_overwriting() {
    let (pool, dir) = temp_pool("stale").await;
    add_strength_proposal(&pool, 1, 90).await; // 提案记的旧值 = 50，目标 90
    // 排队期间，作者手动把强度改成了 70
    sqlx::query("UPDATE relations SET strength = 70 WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();

    let err = apply_group(&pool, "1", 1).await.unwrap_err();
    assert!(err.contains("已过期"), "应明确说提案过期（实际：{err}）");
    assert_eq!(strength(&pool).await, 70, "被拒绝时不能改数据");
    assert_eq!(statuses(&pool, 1).await, vec!["pending"], "状态应还是待确认，不能标 applied");

    let _ = std::fs::remove_dir_all(&dir);
}

/// ⑦ 旧值与现值一致时照常落地 —— 别把正常路径也拦了。
#[tokio::test]
async fn fresh_proposal_still_applies() {
    let (pool, dir) = temp_pool("fresh").await;
    add_strength_proposal(&pool, 1, 90).await;

    apply_group(&pool, "1", 1).await.unwrap();

    assert_eq!(strength(&pool).await, 90);
    assert_eq!(statuses(&pool, 1).await, vec!["applied"]);

    let _ = std::fs::remove_dir_all(&dir);
}
