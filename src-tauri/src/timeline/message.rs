// 从 `timeline.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `timeline.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use message::*;` 取回，
// 外部 `crate::timeline::xxx` 的调用路径**全部不变**。

use super::*;

#[derive(serde::Serialize, Clone)]
pub struct Message {
    pub id: i64,
    pub node_id: i64,
    pub role: String,
    pub content: String,
    pub mode: String,
    pub created_at: String,
    /// 时段标记（2026-10-29）：早 / 午 / 晚，空串 = 没标。存在 messages.meta 的 JSON 里。
    #[serde(default)]
    pub tod: String,
}


/// 从 messages.meta 的 JSON 里取时段（早/午/晚）；坏数据当没标。
pub fn parse_tod(meta: &str) -> String {
    serde_json::from_str::<serde_json::Value>(meta)
        .ok()
        .and_then(|v| v.get("tod").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .filter(|s| s == "早" || s == "午" || s == "晚")
        .unwrap_or_default()
}

pub async fn list_messages(story_id: &str, node_id: i64) -> Result<Vec<Message>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let rows = sqlx::query(
        "SELECT id, node_id, role, content, mode, created_at, COALESCE(meta,'{}')
         FROM messages WHERE node_id = ?1 ORDER BY id"
    )
    .bind(node_id)
    .fetch_all(&pool)
    .await?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(Message {
            id: r.try_get(0)?,
            node_id: r.try_get(1)?,
            role: r.try_get(2)?,
            content: r.try_get(3)?,
            mode: r.try_get(4)?,
            created_at: r.try_get(5)?,
            tod: parse_tod(r.try_get::<String, _>(6).unwrap_or_default().as_str()),
        });
    }
    Ok(out)
}

pub async fn create_message(
    story_id: &str,
    node_id: i64,
    role: &str,
    content: &str,
    mode: &str,
    tod: Option<&str>,
) -> Result<i64, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    // 第五批 ③：消息带上分支（由节点推导，保证与节点一致）
    let branch: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .flatten();
    // 时段标记（2026-10-29）：只在用户消息上打，存 meta JSON；不写正文。
    let meta = match tod {
        Some(t) if t == "早" || t == "午" || t == "晚" => serde_json::json!({ "tod": t }).to_string(),
        _ => "{}".to_string(),
    };
    let row = sqlx::query(
        "INSERT INTO messages(node_id, role, content, mode, branch_id, meta) VALUES(?1, ?2, ?3, ?4, ?5, ?6) RETURNING id"
    )
    .bind(node_id)
    .bind(role)
    .bind(content)
    .bind(mode)
    .bind(branch)
    .bind(meta)
    .fetch_one(&pool)
    .await?;
    Ok(row.try_get(0)?)
}

/// 更新某条消息内容（修改历史剧情）。
/// 第五批：**删除单条消息**（用户实测反馈要的）。
///
/// 用途：请求失败时把**已经入库的那条用户消息**撤掉 ——
/// 否则重发会在历史里留下两份一样的输入（"孤儿用户消息"）。
/// 另外，取消生成留下的半截回复也可以用它清掉。
pub async fn delete_message(story_id: &str, message_id: i64) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    sqlx::query("DELETE FROM messages WHERE id = ?1")
        .bind(message_id)
        .execute(&pool)
        .await?;
    Ok(())
}

pub async fn update_message(story_id: &str, message_id: i64, content: &str) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    sqlx::query("UPDATE messages SET content = ?1 WHERE id = ?2")
        .bind(content)
        .bind(message_id)
        .execute(&pool).await?;
    Ok(())
}
