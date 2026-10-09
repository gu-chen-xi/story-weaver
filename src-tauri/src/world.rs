use sqlx::sqlite::SqliteConnectOptions;
use sqlx::Row;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use crate::story_db::story_db_path;

async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;
    let _ = crate::story_db::ensure_story_columns(&pool).await;
    Ok(pool)
}

#[derive(serde::Serialize)]
pub struct WorldSetting {
    pub id: i64,
    pub key: String,
    pub value: String,
    pub is_hard_rule: i64,
    pub version: i64,
    pub category: String,
    pub meta_json: String,
}

pub async fn list_world_settings(story_id: &str) -> Result<Vec<WorldSetting>, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let rows = sqlx::query(
        "SELECT id, key, value, is_hard_rule, version, category, meta_json FROM world_settings ORDER BY id",
    )
    .fetch_all(&pool)
    .await?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(WorldSetting {
            id: r.try_get(0)?,
            key: r.try_get(1)?,
            value: r.try_get(2)?,
            is_hard_rule: r.try_get(3)?,
            version: r.try_get(4)?,
            category: r.try_get(5)?,
            meta_json: r.try_get(6)?,
        });
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_world_setting(
    story_id: &str,
    key: &str,
    value: &str,
    is_hard_rule: bool,
    category: &str,
    meta_json: &str,
) -> Result<i64, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let row = sqlx::query(
        "INSERT INTO world_settings(key, value, is_hard_rule, category, meta_json) VALUES(?1, ?2, ?3, ?4, ?5) RETURNING id",
    )
    .bind(key)
    .bind(value)
    .bind(if is_hard_rule { 1 } else { 0 })
    .bind(category)
    .bind(meta_json)
    .fetch_one(&pool)
    .await?;
    Ok(row.try_get(0)?)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_world_setting(
    story_id: &str,
    id: i64,
    key: &str,
    value: &str,
    is_hard_rule: bool,
    category: &str,
    meta_json: &str,
) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    sqlx::query(
        "UPDATE world_settings SET key=?1, value=?2, is_hard_rule=?3, category=?4, meta_json=?5, version=version+1 WHERE id=?6",
    )
    .bind(key)
    .bind(value)
    .bind(if is_hard_rule { 1 } else { 0 })
    .bind(category)
    .bind(meta_json)
    .bind(id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn delete_world_setting(story_id: &str, id: i64) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    sqlx::query("DELETE FROM world_settings WHERE id = ?1")
        .bind(id)
        .execute(&pool)
        .await?;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct WorldConflict {
    pub title: String,
    pub level: String,
    pub left_id: i64,
    pub right_id: i64,
    pub left_label: String,
    pub left_text: String,
    pub left_source: String,
    pub right_label: String,
    pub right_text: String,
    pub right_source: String,
}

/// 冲突检测（当前范围）：只检测「地图与地点」的坐标重复；
/// 同一坐标 (x,y) 被两个及以上地点使用即视为冲突。
/// 设定条目的冲突检测暂未覆盖，前端标记为「后期修正」。
pub async fn detect_world_conflicts(story_id: &str) -> Result<Vec<WorldConflict>, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let rows = sqlx::query("SELECT id, key, value, meta_json FROM world_settings WHERE category = '地理' ORDER BY id")
        .fetch_all(&pool)
        .await?;
    let mut groups: HashMap<String, Vec<(i64, String, String)>> = HashMap::new();
    for r in &rows {
        let id: i64 = r.try_get(0)?;
        let name: String = r.try_get(1)?;
        let desc: String = r.try_get(2)?;
        let meta_json: String = r.try_get(3)?;
        let meta: serde_json::Value = serde_json::from_str(&meta_json).unwrap_or_else(|_| serde_json::json!({}));
        let x = meta.get("x").and_then(|v| v.as_str()).unwrap_or("50").trim().to_string();
        let y = meta.get("y").and_then(|v| v.as_str()).unwrap_or("50").trim().to_string();
        groups.entry(format!("{},{}", x, y)).or_default().push((id, name, desc));
    }
    let mut out = Vec::new();
    for (coord, list) in groups {
        if list.len() < 2 {
            continue;
        }
        let a = &list[0];
        let b = &list[1];
        out.push(WorldConflict {
            title: format!("地点坐标冲突：「{}」与「{}」都在 ({})", a.1, b.1, coord),
            level: "提示".to_string(),
            left_id: a.0,
            right_id: b.0,
            left_label: format!("地图与地点 · {}", a.1),
            left_text: a.2.clone(),
            left_source: format!("world_settings #{} · 坐标 {}", a.0, coord),
            right_label: format!("地图与地点 · {}", b.1),
            right_text: b.2.clone(),
            right_source: format!("world_settings #{} · 坐标 {}", b.0, coord),
        });
    }
    Ok(out)
}
