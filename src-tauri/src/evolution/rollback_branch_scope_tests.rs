// 从 `evolution.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `evolution::rollback_branch_scope_tests` —— 因为 `evolution.rs` + `evolution/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `evolution`，一行都不用改写。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;
use std::str::FromStr;

/// 主线 3 个角色 + 分支 3 个角色（id 故意错开，模拟真实数据）
async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
    let dir = crate::paths::test_root()
        .join("rollback-scope")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("story.db");
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display()))
        .unwrap()
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();

    for ddl in [
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
        "CREATE TABLE setting_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
            payload_json TEXT NOT NULL, parent_version INTEGER,
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            temporary INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '',
            branch_id TEXT)",
        "CREATE TABLE audit_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT, action TEXT NOT NULL, target TEXT,
            before TEXT, after TEXT, operator TEXT NOT NULL DEFAULT 'system',
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }

    // 主线：id 2/3/4；分支 B：id 27/28/29
    for (id, name, br) in [
        (2i64, "甲", None),
        (3, "乙", None),
        (4, "丙", None),
        (27, "甲(分支)", Some("B")),
        (28, "乙(分支)", Some("B")),
        (29, "丙(分支)", Some("B")),
    ] {
        sqlx::query("INSERT INTO characters(id, name, branch_id) VALUES(?1, ?2, ?3)")
            .bind(id).bind(name).bind(br).execute(&pool).await.unwrap();
    }
    (pool, dir)
}

async fn char_ids(pool: &SqlitePool, branch: &Option<String>) -> Vec<i64> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let sql = format!("SELECT id FROM characters WHERE {clause} ORDER BY id");
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    if let Some(b) = branch { q = q.bind(b); }
    q.fetch_all(pool).await.unwrap()
}

/// ① 快照只装**本分支**的行。
#[tokio::test]
async fn snapshot_payload_is_branch_scoped() {
    let (pool, dir) = temp_pool("scope").await;
    assert_eq!(char_ids(&pool, &None).await, vec![2, 3, 4]);
    assert_eq!(char_ids(&pool, &Some("B".into())).await, vec![27, 28, 29]);

    let v = make_snapshot(&pool, false, Some("主线"), None).await.unwrap();
    let raw: String = sqlx::query_scalar(
        "SELECT payload_json FROM setting_snapshots WHERE version = ?1",
    )
    .bind(v).fetch_one(&pool).await.unwrap();
    let payload: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let ids: Vec<i64> = payload["characters"].as_array().unwrap()
        .iter().filter_map(|c| c["id"].as_i64()).collect();
    assert_eq!(ids, vec![2, 3, 4], "主线快照不该含分支的角色（原来含 27/28/29，导致回滚撞主键）");

    let vb = make_snapshot(&pool, false, Some("分支"), Some("B".into())).await.unwrap();
    let pb: String = sqlx::query_scalar("SELECT payload_json FROM setting_snapshots WHERE version = ?1")
        .bind(vb).fetch_one(&pool).await.unwrap();
    let pb: serde_json::Value = serde_json::from_str(&pb).unwrap();
    let idsb: Vec<i64> = pb["characters"].as_array().unwrap()
        .iter().filter_map(|c| c["id"].as_i64()).collect();
    assert_eq!(idsb, vec![27, 28, 29], "分支快照应只含分支自己的角色");

    let _ = std::fs::remove_dir_all(&dir);
}

/// ② **老快照**（payload 里混着别的分支的行）回滚时不能报错，也不能破坏别的分支。
///
/// 这是用户报的那个 1555 错误的回归测试。
#[tokio::test]
async fn rollback_skips_rows_belonging_to_other_branch() {
    let (pool, dir) = temp_pool("legacy").await;

    // 手搓一个"老格式"快照：主线 3 条 + 分支 3 条全塞进去
    let legacy = serde_json::json!({
        "characters": [
            {"id": 2, "name": "甲"},
            {"id": 3, "name": "乙"},
            {"id": 4, "name": "丙"},
            {"id": 27, "name": "甲(分支)"},
            {"id": 28, "name": "乙(分支)"},
            {"id": 29, "name": "丙(分支)"}
        ],
        "relations": [],
        "world_settings": []
    });
    sqlx::query(
        "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note)
         VALUES(1, ?1, NULL, 0, '老格式')",
    ).bind(legacy.to_string()).execute(&pool).await.unwrap();

    // 模拟 rollback_snapshot 的前半段：只删主线
    sqlx::query("DELETE FROM characters WHERE branch_id IS NULL")
        .execute(&pool).await.unwrap();

    // 修复前这里会 `UNIQUE constraint failed: characters.id`
    let mut conn = pool.acquire().await.unwrap();
    restore_settings_from_payload(&mut conn, &None, &legacy)
        .await
        .expect("老快照回滚不该撞主键");
    drop(conn);

    assert_eq!(char_ids(&pool, &None).await, vec![2, 3, 4], "主线应恢复成 3 个角色");
    assert_eq!(char_ids(&pool, &Some("B".into())).await, vec![27, 28, 29], "分支的行必须原样保留");

    let _ = std::fs::remove_dir_all(&dir);
}

/// ③ 同分支残留（删除没覆盖到）时，宁可换 id 也不能让回滚整体失败。
#[tokio::test]
async fn rollback_falls_back_to_autoincrement_when_id_taken() {
    let (pool, dir) = temp_pool("taken").await;
    let payload = serde_json::json!({
        "characters": [{"id": 2, "name": "甲(新)"}],
        "relations": [],
        "world_settings": []
    });
    // 故意不删 2，模拟"删除没覆盖到"
    let mut conn = pool.acquire().await.unwrap();
    restore_settings_from_payload(&mut conn, &None, &payload)
        .await
        .expect("id 被占时应退化为自增，而不是报错");
    drop(conn);

    let names: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM characters WHERE branch_id IS NULL ORDER BY id")
        .fetch_all(&pool).await.unwrap();
    assert!(names.iter().any(|(_, n)| n == "甲(新)"), "内容应插进去了（实际 {names:?}）");
    assert_eq!(names.len(), 4, "原有 3 条 + 新插 1 条");

    let _ = std::fs::remove_dir_all(&dir);
}
