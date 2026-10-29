//! 关系强度变化账本（2026-10-29）。
//!
//! 为什么单开一张表：塘主 2026-10-28 定 —— 关系强度「正向变化走演进（人工），负向允许自动」，
//! 而且**每次自动写入都要带 source + revoked，账本可查可撤**。
//! 之前只有 audit_logs（记了前后值，但撤不了、也查不到"是哪一笔"），这里补一张**只增不改**的账：
//! 撤销 = 反向写一行（source = undo），原行标 revoked，历史永远可追溯。
//!
//! 现值仍以 relations.strength 为准；这张表只记"怎么变成现在这个值的"。

use serde::Serialize;
use sqlx::{Row, SqlitePool};

/// 账本一行
#[derive(Debug, Clone, Serialize)]
pub struct ChangeRow {
    pub id: i64,
    pub relation_id: i64,
    /// 哪一天 / 哪条 AI 回复产生的（导演代价能带上，手动/回补可能没有）
    pub node_id: Option<i64>,
    pub message_id: Option<i64>,
    pub delta: i64,
    pub kind: String,
    pub reason: String,
    /// director_cost / pardon / evolution / user / undo
    pub source: String,
    pub revoked: i64,
    pub created_at: String,
}

/// 建表（幂等；老库自动获得）
pub async fn ensure_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS relation_changes (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            branch_id   TEXT,
            relation_id INTEGER NOT NULL,
            node_id     INTEGER,
            message_id  INTEGER,
            delta       INTEGER NOT NULL,
            kind        TEXT NOT NULL DEFAULT '',
            reason      TEXT NOT NULL DEFAULT '',
            source      TEXT NOT NULL DEFAULT 'system',
            revoked     INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )",
    )
    .execute(pool)
    .await?;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_relchg_relation ON relation_changes(relation_id)")
        .execute(pool)
        .await;
    Ok(())
}

/// 记一行（**不改数值**，只记账；调用方负责先更新 relations.strength）
pub async fn record(
    pool: &SqlitePool,
    branch: &Option<String>,
    relation_id: i64,
    node_id: Option<i64>,
    message_id: Option<i64>,
    delta: i64,
    kind: &str,
    reason: &str,
    source: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO relation_changes(branch_id, relation_id, node_id, message_id, delta, kind, reason, source)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id",
    )
    .bind(branch)
    .bind(relation_id)
    .bind(node_id)
    .bind(message_id)
    .bind(delta)
    .bind(kind)
    .bind(reason)
    .bind(source)
    .fetch_one(pool)
    .await
}

/// 某分支的账（新→旧）；给了 relation_id 就只看那一条关系
pub async fn list_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    relation_id: Option<i64>,
    limit: i64,
) -> Result<Vec<ChangeRow>, sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let limit = limit.clamp(1, 200);
    // 占位符编号要跟着"实际绑了几个"走：branch_clause 为 None 时**不占位**，
    // 写死 ?2/?3 会让 LIMIT 拿到 NULL → SQLite 报 datatype mismatch（2026-10-29 踩到）。
    let mut idx = if branch.is_some() { 2 } else { 1 };
    let rel_part = match relation_id {
        Some(_) => {
            let p = format!(" AND relation_id = ?{idx}");
            idx += 1;
            p
        }
        None => String::new(),
    };
    let sql = format!(
        "SELECT id, relation_id, node_id, message_id, delta, kind, reason, source, revoked, created_at
         FROM relation_changes WHERE {clause}{rel_part} ORDER BY id DESC LIMIT ?{idx}"
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    if let Some(rid) = relation_id {
        q = q.bind(rid);
    }
    let rows = q.bind(limit).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| ChangeRow {
            id: r.try_get(0).unwrap_or(0),
            relation_id: r.try_get(1).unwrap_or(0),
            node_id: r.try_get::<Option<i64>, _>(2).ok().flatten(),
            message_id: r.try_get::<Option<i64>, _>(3).ok().flatten(),
            delta: r.try_get(4).unwrap_or(0),
            kind: r.try_get(5).unwrap_or_default(),
            reason: r.try_get(6).unwrap_or_default(),
            source: r.try_get(7).unwrap_or_default(),
            revoked: r.try_get(8).unwrap_or(0),
            created_at: r.try_get(9).unwrap_or_default(),
        })
        .collect())
}

/// 撤销结果：新写入的撤销行 + 撤销后的强度
#[derive(Debug, Serialize)]
pub struct UndoResult {
    pub undo_id: i64,
    pub relation_id: i64,
    pub old: i64,
    pub new: i64,
}

/// 单条撤销：反向写一行 + 原行标 revoked；夹 0~100。
///
/// 不做"整天回滚"（要整段重来用快照）；撤销行本身不能再撤销。
pub async fn undo_in(pool: &SqlitePool, change_id: i64) -> Result<UndoResult, String> {
    let row = sqlx::query(
        "SELECT relation_id, delta, kind, source, revoked FROM relation_changes WHERE id = ?1",
    )
    .bind(change_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("找不到这条变化（id = {change_id}）"))?;
    let relation_id: i64 = row.try_get(0).map_err(|e| e.to_string())?;
    let delta: i64 = row.try_get(1).map_err(|e| e.to_string())?;
    let kind: String = row.try_get(2).unwrap_or_default();
    let source: String = row.try_get(3).unwrap_or_default();
    let revoked: i64 = row.try_get(4).unwrap_or(0);
    if revoked != 0 {
        return Err("这条已经撤销过了".to_string());
    }
    if source == "undo" {
        return Err("撤销行本身不能再撤销".to_string());
    }
    let old: i64 = sqlx::query_scalar("SELECT strength FROM relations WHERE id = ?1")
        .bind(relation_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("关系不存在（id = {relation_id}）"))?;
    let new = (old - delta).clamp(0, 100);
    sqlx::query("UPDATE relations SET strength = ?1, version = version + 1 WHERE id = ?2")
        .bind(new)
        .bind(relation_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let undo_id = record(pool, &None, relation_id, None, None, -delta, &kind, &format!("撤销 #{change_id}"), "undo")
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE relation_changes SET revoked = 1 WHERE id = ?1")
        .bind(change_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(UndoResult { undo_id, relation_id, old, new })
}

#[cfg(test)]
mod tests;
