use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;
use std::str::FromStr;
use std::path::PathBuf;

/// 定位库索引数据库：<项目根>/library/default/library.db
/// 开发时 cargo run 的 cwd 是 src-tauri，故回退上一级到项目根。
pub fn library_db_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("..")
        .join("library")
        .join("default")
        .join("library.db")
}

/// 初始化 library.db（不存在则创建），并建 M0 所需表。
/// 打开 library.db（不建表，供读取全局配置用）。
pub async fn library_pool() -> Result<SqlitePool, sqlx::Error> {
    let path = library_db_path();
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    SqlitePool::connect_with(opts).await
}

pub async fn init_library_db() -> Result<SqlitePool, sqlx::Error> {
    let path = library_db_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await?;

    // M0：全局配置表（模型参数/缓存上限等键值）
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS configs (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    // M1：故事索引表（在 library.db；故事的全部业务数据在各自 story.db）
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stories (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            title        TEXT NOT NULL,
            time_origin  TEXT NOT NULL,
            created_at   TEXT NOT NULL DEFAULT (datetime('now', 'localtime')),
            updated_at   TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        )",
    )
    .execute(&pool)
    .await?;

    // M0：首次启动写入默认配置（已存在则不动）
    ensure_default_configs(&pool).await?;

    Ok(pool)
}

/// 默认配置（`INSERT OR IGNORE`，可反复调用）：
/// 新增键会被补进老库，已有值不会被覆盖 —— 负责"升级后老用户也能拿到新配置项"。
pub const DEFAULT_CONFIGS: &[(&str, &str)] = &[
    ("model.provider", "openai_compatible"),
    ("model.base_url", "https://api.openai.com/v1"),
    ("model.name", "gpt-4o-mini"),
    ("context.n_recent", "10"),
    ("model.context_window", "32768"),
    // 上下文安全系数：可用预算 = 窗口 × 系数 − 各项实测占用 − 输出预留。
    // 默认 1.0（不额外打折）—— 因为各项占用与输出预留都已显式扣掉，再打折等于重复保守；
    // 想留额外余量可调到 0.8~0.9。
    ("model.context_reserve_ratio", "1.0"),
    // 单次最大输出（要预留出这么多 token 给回复）
    ("model.max_tokens", "4000"),
    // 安全余量（估算误差 / 协议开销）
    ("model.safety_margin", "800"),
    ("cache.max_bytes", "5368709120"), // 5GB
    ("portrait.channel", "cloud"),     // cloud / local
];

pub async fn ensure_default_configs(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    for (k, v) in DEFAULT_CONFIGS {
        sqlx::query("INSERT OR IGNORE INTO configs(key, value) VALUES(?1, ?2)")
            .bind(k)
            .bind(v)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// 健康检查：返回 db 路径与 configs 行数。
pub async fn health(pool: &SqlitePool) -> Result<(String, i64), sqlx::Error> {
    let row = sqlx::query("SELECT COUNT(*) FROM configs")
        .fetch_one(pool)
        .await?;
    let n: i64 = row.try_get(0)?;
    Ok((library_db_path().display().to_string(), n))
}

pub async fn get_config(pool: &SqlitePool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query("SELECT value FROM configs WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    match row {
        Some(r) => Ok(Some(r.try_get(0)?)),
        None => Ok(None),
    }
}

pub async fn set_config(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO configs(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

/// 返回全部配置（key -> value）。
pub async fn all_configs(pool: &SqlitePool) -> Result<Vec<(String, String)>, sqlx::Error> {
    let rows = sqlx::query("SELECT key, value FROM configs ORDER BY key")
        .fetch_all(pool)
        .await?;
    let mut out = Vec::new();
    for r in &rows {
        let k: String = r.try_get(0)?;
        let v: String = r.try_get(1)?;
        out.push((k, v));
    }
    Ok(out)
}

#[derive(serde::Serialize)]
pub struct StorySummary {
    pub id: i64,
    pub title: String,
    pub time_origin: String,
    pub created_at: String,
    pub updated_at: String,
}

pub async fn list_stories(pool: &SqlitePool) -> Result<Vec<StorySummary>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, title, time_origin, created_at, updated_at
         FROM stories ORDER BY updated_at DESC"
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(StorySummary {
            id: r.try_get(0)?,
            title: r.try_get(1)?,
            time_origin: r.try_get(2)?,
            created_at: r.try_get(3)?,
            updated_at: r.try_get(4)?,
        });
    }
    Ok(out)
}

pub async fn create_story(pool: &SqlitePool, title: &str, time_origin: &str) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO stories(title, time_origin) VALUES(?1, ?2) RETURNING id"
    )
    .bind(title)
    .bind(time_origin)
    .fetch_one(pool)
    .await?;
    let id: i64 = row.try_get(0)?;
    Ok(id)
}

pub async fn delete_story(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM stories WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
