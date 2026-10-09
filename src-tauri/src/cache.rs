// M7：缓存管理——占用展示（cache_entries）、上限配置（默认 5GB）、手动清空、自动清理最久未用。

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::Row;
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::db;
use crate::story_db::story_db_path;

const DEFAULT_LIMIT_GB: i64 = 5;

async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    SqlitePool::connect_with(opts).await
}

#[derive(serde::Serialize)]
pub struct CacheEntry {
    pub path: String,
    pub size: i64,
    pub type_: String,
    pub last_access: String,
}

pub async fn list_cache_entries(story_id: &str) -> Result<Vec<CacheEntry>, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let rows = sqlx::query("SELECT path, size, type, last_access FROM cache_entries ORDER BY last_access DESC LIMIT 200")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(CacheEntry {
            path: r.try_get(0).map_err(|e| e.to_string())?,
            size: r.try_get(1).map_err(|e| e.to_string())?,
            type_: r.try_get(2).map_err(|e| e.to_string())?,
            last_access: r.try_get(3).map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

/// 总占用（字节）。
pub async fn cache_usage(story_id: &str) -> Result<i64, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let row = sqlx::query("SELECT COALESCE(SUM(size), 0) FROM cache_entries")
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.try_get::<i64, _>(0).map_err(|e| e.to_string())?)
}

/// 手动清空缓存记录（MVP：仅清元数据表）。
pub async fn clear_cache(story_id: &str) -> Result<String, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let r = sqlx::query("DELETE FROM cache_entries").execute(&pool).await.map_err(|e| e.to_string())?;
    Ok(format!("已清理 {} 条缓存记录", r.rows_affected()))
}

/// 读取缓存上限（GB），存于 library.db configs（key=cache.limit）。
pub async fn get_cache_limit() -> Result<i64, String> {
    let pool = crate::db::library_pool().await.map_err(|e| e.to_string())?;
    match db::get_config(&pool, "cache.limit").await.map_err(|e| e.to_string())? {
        Some(v) => v.parse::<i64>().map_err(|_| "cache.limit 配置无效".to_string()),
        None => Ok(DEFAULT_LIMIT_GB),
    }
}

pub async fn set_cache_limit(gb: i64) -> Result<String, String> {
    if !(1..=1000).contains(&gb) {
        return Err("上限需在 1~1000 GB 之间".into());
    }
    let pool = crate::db::library_pool().await.map_err(|e| e.to_string())?;
    db::set_config(&pool, "cache.limit", &gb.to_string()).await.map_err(|e| e.to_string())?;
    Ok(format!("缓存上限已设为 {} GB", gb))
}

/// 自动清理：占用超过上限时，按 last_access 最久未用优先清理，直到低于上限。
pub async fn auto_cleanup_cache(story_id: &str) -> Result<String, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let limit_bytes = get_cache_limit().await? * 1024 * 1024 * 1024;
    let usage: i64 = cache_usage(story_id).await?;
    if usage <= limit_bytes {
        return Ok(format!("占用 {:.2} GB，未超上限 {} GB，无需清理", usage as f64 / 1073741824.0, get_cache_limit().await?));
    }
    // 按最久未用删除，直到低于上限（每次删一条最旧的）
    let mut removed = 0i64;
    loop {
        let cur: i64 = sqlx::query("SELECT COALESCE(SUM(size), 0) FROM cache_entries")
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?
            .try_get::<i64, _>(0)
            .map_err(|e| e.to_string())?;
        if cur <= limit_bytes {
            break;
        }
        let del = sqlx::query(
            "DELETE FROM cache_entries WHERE id IN (SELECT id FROM cache_entries ORDER BY last_access ASC LIMIT 1)",
        )
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
        if del.rows_affected() == 0 {
            break;
        }
        removed += 1;
    }
    Ok(format!("已按最久未用清理 {} 条缓存记录", removed))
}
