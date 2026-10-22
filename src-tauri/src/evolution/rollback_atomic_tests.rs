// 从 `evolution.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `evolution::rollback_atomic_tests` —— 因为 `evolution.rs` + `evolution/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `evolution`，一行都不用改写。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;
use std::str::FromStr;

async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
    let dir = crate::paths::test_root()
        .join("rollback-atomic")
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
                behavior_rules TEXT NOT NULL DEFAULT '',
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
    sqlx::query("INSERT INTO characters(id, name) VALUES(2, '甲')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO relations(id, from_character_id, to_character_id, strength) VALUES(2, 2, 2, 50)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO world_settings(id, key, value) VALUES(18, '地点', '城')")
        .execute(&pool).await.unwrap();
    (pool, dir)
}

async fn counts(pool: &SqlitePool) -> (i64, i64, i64) {
    let c: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters").fetch_one(pool).await.unwrap();
    let r: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM relations").fetch_one(pool).await.unwrap();
    let w: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM world_settings").fetch_one(pool).await.unwrap();
    (c, r, w)
}

/// 失败的回滚**不能留下删了一半的库**。
#[tokio::test]
async fn failed_rollback_keeps_previous_state() {
    let (pool, dir) = temp_pool("atomic").await;
    assert_eq!(counts(&pool).await, (1, 1, 1));

    // 快照 payload 里放两条同名角色，并给 name 加唯一索引 → 插入时必然 1555
    sqlx::query("CREATE UNIQUE INDEX ux_char_name ON characters(name)")
        .execute(&pool).await.unwrap();
    let bad = serde_json::json!({
        "characters": [{"id": 99, "name": "同名"}, {"id": 100, "name": "同名"}],
        "relations": [],
        "world_settings": []
    });
    sqlx::query(
        "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note)
         VALUES(1, ?1, NULL, 0, '会失败的快照')",
    ).bind(bad.to_string()).execute(&pool).await.unwrap();

    let r = rollback_snapshot_in(&pool, 1, None, String::new()).await;
    assert!(r.is_err(), "这份 payload 应该插不进去（同名冲突），实际：{r:?}");
    assert!(
        counts(&pool).await == (1, 1, 1),
        "**回滚失败后三张表必须原样还在**（之前会留下删了一半的库）"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 成功的回滚要真的替换数据。
#[tokio::test]
async fn successful_rollback_replaces_data() {
    let (pool, dir) = temp_pool("ok").await;
    let payload = serde_json::json!({
        "characters": [{"id": 2, "name": "甲改名"}],
        "relations": [{"id": 2, "from_character_id": 2, "to_character_id": 2, "strength": 77}],
        "world_settings": [{"id": 18, "key": "地点", "value": "村庄"}]
    });
    sqlx::query(
        "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note)
         VALUES(1, ?1, NULL, 0, '正常快照')",
    ).bind(payload.to_string()).execute(&pool).await.unwrap();

    rollback_snapshot_in(&pool, 1, None, String::new()).await
        .expect("这份 payload 应该能回滚成功");

    let name: String = sqlx::query_scalar("SELECT name FROM characters WHERE id = 2")
        .fetch_one(&pool).await.unwrap();
    let strength: i64 = sqlx::query_scalar("SELECT strength FROM relations WHERE id = 2")
        .fetch_one(&pool).await.unwrap();
    let v: String = sqlx::query_scalar("SELECT value FROM world_settings WHERE id = 18")
        .fetch_one(&pool).await.unwrap();
    assert_eq!((name.as_str(), strength, v.as_str()), ("甲改名", 77, "村庄"));

    let _ = std::fs::remove_dir_all(&dir);
}
