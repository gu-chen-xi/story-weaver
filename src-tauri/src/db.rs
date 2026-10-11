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
    // 第三批：回复后自动分析剧情演进（纯本地规则，不调 API；关掉则只能手动点「分析」）
    ("evolution.auto_analyze", "true"),
    // 第三批 ④：每轮回复后自动评估质量（**会调 API，默认关**；手动评估不受此开关限制）
    ("evaluate.auto", "false"),
    // 合规中心：「我已阅读并了解」的时间戳（空 = 未确认；只用于界面提示，不参与任何逻辑）
    ("compliance.acknowledged_at", ""),
    // 首次启动合规提示是否已展示过（空 = 未展示 → 启动时弹一次）
    ("compliance.notice_seen", ""),
    // 用户确认过的**条款版本号**（与 src/compliance.ts 的 NOTICE_VERSION 比对；不一致就要重新确认）
    ("compliance.acknowledged_version", ""),
    ("cache.max_bytes", "5368709120"), // 5GB
    ("portrait.channel", "local"),     // 通道大类：cloud / local（沿用既有键，别新增第二套）
    // 第四批 ⑥ P1：具体用哪个适配器（见 generator::registry）
    ("portrait.generator", "local_sdwebui"),
    ("portrait.base_url", "http://127.0.0.1:7860"),
    // 云端通道（P3）：OpenAI 兼容形态用 base_url + model；自定义形态用请求地址/模板/响应路径
    ("portrait.cloud_url", ""),                  // OpenAI 形态：填到 /v1 为止
    ("portrait.cloud_model", ""),                // OpenAI 形态的模型名（可空 = 服务默认）
    ("portrait.cloud_api_key", ""),              // 云端 API Key（也支持存到系统凭据库，见前端说明）
    ("portrait.cloud_request_url", ""),          // 自定义形态：完整请求地址（可含占位符）
    ("portrait.cloud_body_template", ""),        // 自定义形态：请求体模板（JSON 文本，可含占位符）
    ("portrait.cloud_response_path", ""),        // 自定义形态：响应字段路径，如 b64:data.0.b64_json / url:output.0
    ("portrait.cloud_auth_style", "bearer"),     // bearer / api-key / none
    ("portrait.cloud_extra_headers", ""),        // 额外请求头，形如 X-Org=abc;X-Foo=bar
    ("portrait.cloud_ok_status", "200"),         // 成功状态码（少数服务用 201 等）
    ("portrait.cloud_response_raw_b64", "false"),// 响应整体就是 base64 文本（极少数服务）
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
