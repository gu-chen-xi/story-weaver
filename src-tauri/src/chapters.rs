//! 章节（2026-10-30）：剧本大纲的“成稿层”第一版 —— 章 ↔ 天 多对多。
//!
//! 只做**只读聚合**：章是独立实体，手动圈定它包含哪些时间线节点；
//! 字数由关联天的消息统计得出（不冗余存储），正文不在章里编辑（只读+跳转）。

use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Serialize)]
pub struct Chapter {
    pub id: i64,
    pub act: i64,
    pub order_no: i64,
    pub title: String,
    pub synopsis: String,
    pub goal: String,
    pub target_words: i64,
    pub status: String,
    /// 关联的时间线节点（按时间线顺序）
    pub node_ids: Vec<i64>,
    pub node_labels: Vec<String>,
    pub words: i64,
}

pub async fn ensure_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS chapters (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            branch_id    TEXT,
            act          INTEGER NOT NULL DEFAULT 1,
            order_no     INTEGER NOT NULL DEFAULT 1,
            title        TEXT NOT NULL DEFAULT '',
            synopsis     TEXT NOT NULL DEFAULT '',
            goal         TEXT NOT NULL DEFAULT '',
            target_words INTEGER NOT NULL DEFAULT 0,
            status       TEXT NOT NULL DEFAULT 'draft',
            created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS chapter_nodes (
            chapter_id INTEGER NOT NULL,
            node_id    INTEGER NOT NULL,
            PRIMARY KEY (chapter_id, node_id)
        )",
    )
    .execute(pool)
    .await?;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_chapter_nodes_node ON chapter_nodes(node_id)")
        .execute(pool)
        .await;
    Ok(())
}

fn branch_clause(col: &str, branch: &Option<String>, n: usize) -> String {
    crate::story_db::branch_clause(col, branch, n)
}

/// 章列表（新章在后；带关联天、字数）
pub async fn list_in(pool: &SqlitePool, branch: &Option<String>) -> Result<Vec<Chapter>, sqlx::Error> {
    ensure_table(pool).await?;
    let clause = branch_clause("branch_id", branch, 1);
    let sql = format!(
        "SELECT id, act, order_no, title, synopsis, goal, target_words, status
         FROM chapters WHERE {clause} ORDER BY order_no ASC, id ASC"
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch { q = q.bind(b); }
    let rows = q.fetch_all(pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get(0).unwrap_or(0);
        let nodes = sqlx::query(
            "SELECT n.id, COALESCE(n.date_label,'') FROM chapter_nodes cn
             JOIN timeline_nodes n ON n.id = cn.node_id
             WHERE cn.chapter_id = ?1 ORDER BY n.sort_order ASC, n.id ASC",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        let node_ids: Vec<i64> = nodes.iter().filter_map(|x| x.try_get::<i64, _>(0).ok()).collect();
        let node_labels: Vec<String> = nodes.iter().filter_map(|x| x.try_get::<String, _>(1).ok()).collect();
        let words: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(LENGTH(m.content)),0) FROM messages m
             JOIN chapter_nodes cn ON cn.node_id = m.node_id WHERE cn.chapter_id = ?1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        out.push(Chapter {
            id,
            act: r.try_get(1).unwrap_or(1),
            order_no: r.try_get(2).unwrap_or(0),
            title: r.try_get(3).unwrap_or_default(),
            synopsis: r.try_get(4).unwrap_or_default(),
            goal: r.try_get(5).unwrap_or_default(),
            target_words: r.try_get(6).unwrap_or(0),
            status: r.try_get(7).unwrap_or_else(|_| "draft".to_string()),
            node_ids,
            node_labels,
            words,
        });
    }
    Ok(out)
}

/// 新建章：`order_no` 自动取最大 +1；节点关联一并写入。
pub async fn create_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    title: &str,
    node_ids: &[i64],
) -> Result<i64, sqlx::Error> {
    ensure_table(pool).await?;
    // 章序**按分支各自编号**（2026-10-30 修）：原来全库取 MAX，分支建过章之后主线就跳号（01、03）
    let clause = branch_clause("branch_id", branch, 1);
    let sql = format!("SELECT COALESCE(MAX(order_no),0) + 1 FROM chapters WHERE {clause}");
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    if let Some(b) = branch { q = q.bind(b); }
    let next: i64 = q.fetch_one(pool).await.unwrap_or(1);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO chapters(branch_id, order_no, title) VALUES(?1, ?2, ?3) RETURNING id",
    )
    .bind(branch)
    .bind(next)
    .bind(title)
    .fetch_one(pool)
    .await?;
    set_nodes_in(pool, id, node_ids).await?;
    // 顺手把这一分支的章序归一化成 1..n（老数据里已经跳号的会被抹平）
    let _ = normalize_orders(pool, branch).await;
    Ok(id)
}

/// 整组替换章的关联天（空数组 = 清空）。
///
/// **独占**（2026-10-30 用户定）：一天只属于一章 —— 加进来时先把这天从别的章里摘掉。
/// 表结构仍是多对多（方便回滚/迁移），但语义上不允许一天挂两章。
pub async fn set_nodes_in(pool: &SqlitePool, chapter_id: i64, node_ids: &[i64]) -> Result<(), sqlx::Error> {
    for nid in node_ids {
        let _ = sqlx::query("DELETE FROM chapter_nodes WHERE node_id = ?1 AND chapter_id <> ?2")
            .bind(nid)
            .bind(chapter_id)
            .execute(pool)
            .await;
    }
    sqlx::query("DELETE FROM chapter_nodes WHERE chapter_id = ?1")
        .bind(chapter_id)
        .execute(pool)
        .await?;
    for nid in node_ids {
        let _ = sqlx::query("INSERT OR IGNORE INTO chapter_nodes(chapter_id, node_id) VALUES(?1, ?2)")
            .bind(chapter_id)
            .bind(nid)
            .execute(pool)
            .await;
    }
    Ok(())
}


/// 把某条分支的章序归一化成 1..n（按现有 order_no、id 排序，相对顺序不变）。
/// 用途：分支各自编号后，老数据的跳号（01、03）会被抹平；删章后也不会留洞。
pub async fn normalize_orders(pool: &SqlitePool, branch: &Option<String>) -> Result<(), sqlx::Error> {
    let clause = branch_clause("branch_id", branch, 1);
    let sql = format!("SELECT id FROM chapters WHERE {clause} ORDER BY order_no ASC, id ASC");
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    if let Some(b) = branch { q = q.bind(b); }
    let ids: Vec<i64> = q.fetch_all(pool).await.unwrap_or_default();
    for (i, id) in ids.iter().enumerate() {
        let _ = sqlx::query("UPDATE chapters SET order_no = ?1 WHERE id = ?2")
            .bind((i as i64) + 1)
            .bind(id)
            .execute(pool)
            .await;
    }
    Ok(())
}
/// 改元信息（标题/状态/摘要/目标/目标字数/幕/章序）。正文不在章里改。
pub async fn update_in(
    pool: &SqlitePool,
    id: i64,
    title: Option<&str>,
    status: Option<&str>,
    synopsis: Option<&str>,
    goal: Option<&str>,
    target_words: Option<i64>,
    act: Option<i64>,
    order_no: Option<i64>,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE chapters SET
            title = COALESCE(?2, title),
            status = COALESCE(?3, status),
            synopsis = COALESCE(?4, synopsis),
            goal = COALESCE(?5, goal),
            target_words = COALESCE(?6, target_words),
            act = COALESCE(?7, act),
            order_no = COALESCE(?8, order_no)
         WHERE id = ?1",
    )
    .bind(id)
    .bind(title)
    .bind(status)
    .bind(synopsis)
    .bind(goal)
    .bind(target_words)
    .bind(act)
    .bind(order_no)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

/// 删章：只删章与关联，**不删任何剧情**。
pub async fn delete_in(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    // 先记住它属于哪条分支：删完要把这条分支的章序补齐（不留洞）
    // 用 Option<Option<String>> 取：branch_id 是 NULL 时不能被解成空串（否则归一化会筛错分支）
    let branch: Option<String> = match sqlx::query("SELECT branch_id FROM chapters WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(row)) => row.try_get::<Option<String>, _>(0).unwrap_or(None),
        _ => None,
    };
    sqlx::query("DELETE FROM chapter_nodes WHERE chapter_id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    let removed = sqlx::query("DELETE FROM chapters WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected() > 0;
    if removed {
        let _ = normalize_orders(pool, &branch).await;
    }
    Ok(removed)
}

/// 还没归章的“天”（有消息的 day 节点）
pub async fn unassigned_days(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<(i64, String)>, sqlx::Error> {
    let clause = branch_clause("n.branch_id", branch, 1);
    let sql = format!(
        "SELECT n.id, COALESCE(n.date_label,'') FROM timeline_nodes n
         WHERE {clause} AND COALESCE(n.node_type,'day') = 'day'
           AND EXISTS (SELECT 1 FROM messages m WHERE m.node_id = n.id)
           AND NOT EXISTS (SELECT 1 FROM chapter_nodes cn WHERE cn.node_id = n.id)
         ORDER BY n.sort_order ASC, n.id ASC"
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch { q = q.bind(b); }
    let rows = q.fetch_all(pool).await.unwrap_or_default();
    Ok(rows
        .iter()
        .map(|r| (r.try_get(0).unwrap_or(0), r.try_get(1).unwrap_or_default()))
        .collect())
}

#[cfg(test)]
mod tests;
