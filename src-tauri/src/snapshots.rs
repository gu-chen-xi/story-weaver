//! 请求快照（2026-10-31）：**只读**查看“当时到底发给模型了什么”。
//!
//! 为什么单独做：剧情区和「prompt 预算」都会做展示层的裁剪/分段，
//! 而 `debug_requests` 里存的是**原样**（system_prompt + history_json + user_text + tail_block + ai_reply）。
//! 这一页不看推理、不看分段，只看快照本身 —— 复制出来就能逐字对照。

use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotRow {
    pub id: i64,
    pub created_at: String,
    pub model: String,
    pub node_id: Option<i64>,
    pub status: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub estimated_tokens: i64,
    pub system_chars: i64,
    pub history_chars: i64,
    pub user_chars: i64,
    pub tail_chars: i64,
    /// 本轮输入前 40 字，列表里辨认是哪一轮
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotDetail {
    pub id: i64,
    pub created_at: String,
    pub model: String,
    pub node_id: Option<i64>,
    pub status: String,
    pub error: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub system_prompt: String,
    pub history_json: String,
    pub user_text: String,
    pub tail_block: String,
    pub ai_reply: String,
}

pub async fn list_in(pool: &SqlitePool, limit: i64) -> Result<Vec<SnapshotRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, COALESCE(created_at,''), COALESCE(model,''), node_id, COALESCE(status,''),
                prompt_tokens, completion_tokens, estimated_tokens,
                LENGTH(system_prompt), LENGTH(history_json), LENGTH(user_text), COALESCE(LENGTH(tail_block),0),
                COALESCE(user_text,'')
         FROM debug_requests ORDER BY id DESC LIMIT ?1",
    )
    .bind(limit.max(1))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let user: String = r.try_get(12).unwrap_or_default();
            let preview: String = user.chars().take(40).collect();
            SnapshotRow {
                id: r.try_get(0).unwrap_or(0),
                created_at: r.try_get(1).unwrap_or_default(),
                model: r.try_get(2).unwrap_or_default(),
                node_id: r.try_get(3).ok(),
                status: r.try_get(4).unwrap_or_default(),
                prompt_tokens: r.try_get(5).unwrap_or(0),
                completion_tokens: r.try_get(6).unwrap_or(0),
                estimated_tokens: r.try_get(7).unwrap_or(0),
                system_chars: r.try_get(8).unwrap_or(0),
                history_chars: r.try_get(9).unwrap_or(0),
                user_chars: r.try_get(10).unwrap_or(0),
                tail_chars: r.try_get(11).unwrap_or(0),
                preview,
            }
        })
        .collect())
}

pub async fn get_in(pool: &SqlitePool, id: i64) -> Result<Option<SnapshotDetail>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, COALESCE(created_at,''), COALESCE(model,''), node_id, COALESCE(status,''), COALESCE(error,''),
                prompt_tokens, completion_tokens, system_prompt, history_json, user_text,
                COALESCE(tail_block,''), COALESCE(ai_reply,'')
         FROM debug_requests WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| SnapshotDetail {
        id: r.try_get(0).unwrap_or(0),
        created_at: r.try_get(1).unwrap_or_default(),
        model: r.try_get(2).unwrap_or_default(),
        node_id: r.try_get(3).ok(),
        status: r.try_get(4).unwrap_or_default(),
        error: r.try_get(5).unwrap_or_default(),
        prompt_tokens: r.try_get(6).unwrap_or(0),
        completion_tokens: r.try_get(7).unwrap_or(0),
        system_prompt: r.try_get(8).unwrap_or_default(),
        history_json: r.try_get(9).unwrap_or_default(),
        user_text: r.try_get(10).unwrap_or_default(),
        tail_block: r.try_get(11).unwrap_or_default(),
        ai_reply: r.try_get(12).unwrap_or_default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let p = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE debug_requests (id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER, model TEXT,
                system_prompt TEXT NOT NULL, history_json TEXT NOT NULL, user_text TEXT NOT NULL, ai_reply TEXT,
                estimated_tokens INTEGER NOT NULL DEFAULT 0, n_used INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'ok', error TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')), tail_block TEXT,
                prompt_tokens INTEGER NOT NULL DEFAULT 0, completion_tokens INTEGER NOT NULL DEFAULT 0)",
        )
        .execute(&p)
        .await
        .unwrap();
        p
    }

    #[tokio::test]
    async fn lists_newest_first_with_sizes_and_preview() {
        let p = pool().await;
        sqlx::query("INSERT INTO debug_requests(system_prompt,history_json,user_text,tail_block,model) VALUES('sys','[]','第一轮输入…','尾块','m1')")
            .execute(&p).await.unwrap();
        sqlx::query("INSERT INTO debug_requests(system_prompt,history_json,user_text,tail_block,model) VALUES('sys2','[]','第二轮输入','尾块2','m1')")
            .execute(&p).await.unwrap();
        let rows = list_in(&p, 10).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].preview, "第二轮输入", "最新在前");
        assert_eq!(rows[0].system_chars, 4);
        assert_eq!(rows[0].tail_chars, 3, "尾块「尾块2」= 3 个字符");
    }

    #[tokio::test]
    async fn detail_returns_full_text() {
        let p = pool().await;
        sqlx::query("INSERT INTO debug_requests(system_prompt,history_json,user_text,tail_block,ai_reply) VALUES('S','[]','U','T','A')")
            .execute(&p).await.unwrap();
        let d = get_in(&p, 1).await.unwrap().unwrap();
        assert_eq!((d.system_prompt.as_str(), d.user_text.as_str(), d.tail_block.as_str(), d.ai_reply.as_str()), ("S", "U", "T", "A"));
        assert!(get_in(&p, 999).await.unwrap().is_none());
    }
}
