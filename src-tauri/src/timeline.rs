use sqlx::sqlite::SqliteConnectOptions;
use sqlx::Row;
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::story_db::story_db_path;

async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;
    // 每次打开库都确保 tags 列存在（旧库自动迁移，幂等）
    crate::story_db::ensure_timeline_tags_column(&pool).await?;
    Ok(pool)
}

#[derive(serde::Serialize, Clone)]
pub struct TimelineNode {
    pub id: i64,
    pub date_label: String,
    pub sort_order: i64,
    pub node_type: String,
    pub is_flashback: i64,
    pub tags: String,
}

#[derive(serde::Serialize, Clone)]
pub struct Message {
    pub id: i64,
    pub node_id: i64,
    pub role: String,
    pub content: String,
    pub mode: String,
    pub created_at: String,
}

/// 获取或创建"第 1 天"节点（M2 先用最简单的单节点）。
pub async fn ensure_first_node(story_id: &str) -> Result<TimelineNode, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    // 查第一个节点
    let row = sqlx::query(
        "SELECT id, date_label, sort_order, node_type, is_flashback, tags
         FROM timeline_nodes ORDER BY sort_order LIMIT 1"
    )
    .fetch_optional(&pool)
    .await?;

    if let Some(r) = row {
        return Ok(TimelineNode {
            id: r.try_get(0)?,
            date_label: r.try_get(1)?,
            sort_order: r.try_get(2)?,
            node_type: r.try_get(3)?,
            is_flashback: r.try_get(4)?,
            tags: r.try_get(5)?,
        });
    }

    // 没有就建一个
    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback)
         VALUES('第 1 天', 1, 'day', 0) RETURNING id, date_label, sort_order, node_type, is_flashback, tags"
    )
    .fetch_one(&pool)
    .await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
    })
}

pub async fn list_messages(story_id: &str, node_id: i64) -> Result<Vec<Message>, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let rows = sqlx::query(
        "SELECT id, node_id, role, content, mode, created_at
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
) -> Result<i64, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let row = sqlx::query(
        "INSERT INTO messages(node_id, role, content, mode) VALUES(?1, ?2, ?3, ?4) RETURNING id"
    )
    .bind(node_id)
    .bind(role)
    .bind(content)
    .bind(mode)
    .fetch_one(&pool)
    .await?;
    Ok(row.try_get(0)?)
}

/// 列出某故事的全部时间线节点（按 sort_order 升序）。
pub async fn list_timeline_nodes(story_id: &str) -> Result<Vec<TimelineNode>, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let rows = sqlx::query(
        "SELECT id, date_label, sort_order, node_type, is_flashback, tags
         FROM timeline_nodes ORDER BY sort_order ASC"
    ).fetch_all(&pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(TimelineNode {
            id: r.try_get(0)?,
            date_label: r.try_get(1)?,
            sort_order: r.try_get(2)?,
            node_type: r.try_get(3)?,
            is_flashback: r.try_get(4)?,
            tags: r.try_get(5)?,
        });
    }
    Ok(out)
}

/// 创建下一天节点：sort_order 取当前最大值 + 1（保持排序），
/// 但 date_label 的"第 N 天"只数 node_type='day' 的节点数 +1（插曲不占天数序号）。
pub async fn create_next_day_node(story_id: &str) -> Result<TimelineNode, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let max_row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) FROM timeline_nodes")
        .fetch_one(&pool).await?;
    let max_order: i64 = max_row.try_get(0)?;
    let next_sort = max_order + 1;
    // 数 day 类型节点，兼容旧数据里 node_type='main' 的第一个节点；插曲/倒叙不占天数序号
    let cnt_row = sqlx::query("SELECT COUNT(*) FROM timeline_nodes WHERE node_type IN ('day', 'main')")
        .fetch_one(&pool).await?;
    let day_count: i64 = cnt_row.try_get(0)?;
    let label = format!("第 {} 天", day_count + 1);
    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback)
         VALUES(?1, ?2, 'day', 0) RETURNING id, date_label, sort_order, node_type, is_flashback, tags"
    )
    .bind(&label)
    .bind(next_sort)
    .fetch_one(&pool).await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
    })
}

/// 更新某条消息内容（修改历史剧情）。
pub async fn update_message(story_id: &str, message_id: i64, content: &str) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    sqlx::query("UPDATE messages SET content = ?1 WHERE id = ?2")
        .bind(content)
        .bind(message_id)
        .execute(&pool).await?;
    Ok(())
}

/// 创建一个插曲/倒叙节点：date_label 自动叫"插曲 N"，node_type='interlude'，is_flashback=1。
pub async fn create_interlude_node(story_id: &str) -> Result<TimelineNode, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    // 统计已有插曲数量
    let cnt_row = sqlx::query("SELECT COUNT(*) FROM timeline_nodes WHERE node_type = 'interlude'")
        .fetch_one(&pool).await?;
    let cnt: i64 = cnt_row.try_get(0)?;
    let next_interlude = cnt + 1;
    let max_row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) FROM timeline_nodes")
        .fetch_one(&pool).await?;
    let max_order: i64 = max_row.try_get(0)?;
    let label = format!("插曲 {}", next_interlude);
    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback)
         VALUES(?1, ?2, 'interlude', 1) RETURNING id, date_label, sort_order, node_type, is_flashback, tags"
    )
    .bind(&label)
    .bind(max_order + 1)
    .fetch_one(&pool).await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
    })
}

/// 删除一个时间线节点（连同其下所有消息）。至少保留一个节点。
pub async fn delete_timeline_node(story_id: &str, node_id: i64) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes").fetch_one(&pool).await?;
    if count <= 1 {
        return Err(sqlx::Error::Protocol("至少保留一个时间线节点".to_string()));
    }
    sqlx::query("DELETE FROM messages WHERE node_id = ?1").bind(node_id).execute(&pool).await?;
    sqlx::query("DELETE FROM timeline_nodes WHERE id = ?1").bind(node_id).execute(&pool).await?;
    Ok(())
}
