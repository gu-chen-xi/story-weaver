//! 伏笔清单（2026-10-30 阶段③）："当时不知道"才算伏笔。
//!
//! 状态机：proposed(AI 提议，等人判) / planted(埋下) / payoff(已回收) / overdue(逾期) / dropped(废弃)。
//! - `overdue` **不落库**，按 `窗口章数` 与当前最大章序现算（改了窗口/加了新章就自动跟着变）；
//! - 拒绝 AI 提议 = `dropped`（保留行，避免重复提同一句）；
//! - 手动/自动都写 `source`，界面可区分。

use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Serialize)]
pub struct Foreshadow {
    pub id: i64,
    pub text: String,
    pub planted_chapter_id: Option<i64>,
    pub planted_order: i64,
    pub window_chapters: i64,
    pub payoff_chapter_id: Option<i64>,
    pub status: String,
    pub source: String,
    pub note: String,
    /// 现算：埋下后超过窗口章数还没回收
    pub overdue: bool,
}

pub async fn ensure_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS foreshadows (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            branch_id          TEXT,
            text               TEXT NOT NULL,
            planted_chapter_id INTEGER,
            planted_node_id    INTEGER,
            window_chapters    INTEGER NOT NULL DEFAULT 3,
            payoff_chapter_id  INTEGER,
            status             TEXT NOT NULL DEFAULT 'planted',
            source             TEXT NOT NULL DEFAULT 'manual',
            note               TEXT NOT NULL DEFAULT '',
            created_at         TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 逾期判定（纯函数，好测）：只有 planted 才可能逾期；回收了/废弃了/还是提议的不算。
pub fn is_overdue(status: &str, planted_order: i64, window_chapters: i64, max_order: i64) -> bool {
    status == "planted" && max_order - planted_order > window_chapters
}

pub async fn list_in(pool: &SqlitePool, branch: &Option<String>) -> Result<Vec<Foreshadow>, sqlx::Error> {
    ensure_table(pool).await?;
    let clause = crate::story_db::branch_clause("f.branch_id", branch, 1);
    let max_order: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(order_no),0) FROM chapters")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let sql = format!(
        "SELECT f.id, f.text, f.planted_chapter_id, COALESCE(c.order_no,0), f.window_chapters,
                f.payoff_chapter_id, f.status, f.source, f.note
         FROM foreshadows f LEFT JOIN chapters c ON c.id = f.planted_chapter_id
         WHERE {clause} ORDER BY f.id DESC"
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch { q = q.bind(b); }
    let rows = q.fetch_all(pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        let status: String = r.try_get(6).unwrap_or_else(|_| "planted".into());
        let planted_order: i64 = r.try_get(3).unwrap_or(0);
        let window: i64 = r.try_get(4).unwrap_or(3);
        out.push(Foreshadow {
            id: r.try_get(0).unwrap_or(0),
            text: r.try_get(1).unwrap_or_default(),
            planted_chapter_id: r.try_get(2).ok(),
            planted_order,
            window_chapters: window,
            payoff_chapter_id: r.try_get(5).ok(),
            overdue: is_overdue(&status, planted_order, window, max_order),
            status,
            source: r.try_get(7).unwrap_or_else(|_| "manual".into()),
            note: r.try_get(8).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// 新建（手动默认 planted；AI 提议传 status=proposed / source=ai）
pub async fn create_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    text: &str,
    planted_chapter_id: Option<i64>,
    planted_node_id: Option<i64>,
    window_chapters: i64,
    status: &str,
    source: &str,
) -> Result<i64, sqlx::Error> {
    ensure_table(pool).await?;
    let text = text.trim();
    if text.is_empty() {
        return Err(sqlx::Error::Protocol("伏笔内容不能为空".into()));
    }
    sqlx::query_scalar(
        "INSERT INTO foreshadows(branch_id, text, planted_chapter_id, planted_node_id, window_chapters, status, source)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(branch)
    .bind(text)
    .bind(planted_chapter_id)
    .bind(planted_node_id)
    .bind(window_chapters.max(1))
    .bind(status)
    .bind(source)
    .fetch_one(pool)
    .await
}

/// 改状态 / 文本 / 窗口 / 回收章
pub async fn update_in(
    pool: &SqlitePool,
    id: i64,
    status: Option<&str>,
    text: Option<&str>,
    window_chapters: Option<i64>,
    payoff_chapter_id: Option<i64>,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE foreshadows SET
            status = COALESCE(?2, status),
            text = COALESCE(?3, text),
            window_chapters = COALESCE(?4, window_chapters),
            payoff_chapter_id = COALESCE(?5, payoff_chapter_id)
         WHERE id = ?1",
    )
    .bind(id)
    .bind(status)
    .bind(text)
    .bind(window_chapters)
    .bind(payoff_chapter_id)
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

/// 删（彻底删行；拒绝 AI 提议请用 update status=dropped，别删，免得重复提）
pub async fn delete_in(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM foreshadows WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected()
        > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let p = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE chapters (id INTEGER PRIMARY KEY, order_no INTEGER)")
            .execute(&p).await.unwrap();
        sqlx::query("INSERT INTO chapters VALUES(1,1),(2,2),(3,3),(4,4),(5,5)")
            .execute(&p).await.unwrap();
        ensure_table(&p).await.unwrap();
        p
    }

    #[test]
    fn overdue_only_for_planted() {
        assert!(is_overdue("planted", 1, 3, 5), "1 → 5 超过 3 章窗口");
        assert!(!is_overdue("planted", 4, 3, 5), "才过 1 章");
        assert!(!is_overdue("payoff", 1, 3, 5), "已回收不算逾期");
        assert!(!is_overdue("dropped", 1, 3, 5));
        assert!(!is_overdue("proposed", 1, 3, 5), "提议还没埋，不算");
    }

    #[tokio::test]
    async fn create_list_and_refuse_proposal() {
        let pool = pool().await;
        let id = create_in(&pool, &None, "那把钥匙的齿形", Some(1), Some(7), 3, "planted", "manual").await.unwrap();
        let ai = create_in(&pool, &None, "她没说完的半句", Some(2), None, 3, "proposed", "ai").await.unwrap();
        let list = list_in(&pool, &None).await.unwrap();
        assert_eq!(list.len(), 2);
        let mine = list.iter().find(|f| f.id == id).unwrap();
        assert_eq!(mine.status, "planted");
        assert_eq!(mine.source, "manual");
        assert!(mine.overdue, "第 1 章埋、现在到第 5 章 → 逾期");
        let prop = list.iter().find(|f| f.id == ai).unwrap();
        assert_eq!(prop.status, "proposed");
        assert!(!prop.overdue, "提议不算逾期");
        update_in(&pool, ai, Some("dropped"), None, None, None).await.unwrap();
        assert_eq!(list_in(&pool, &None).await.unwrap().iter().find(|f| f.id == ai).unwrap().status, "dropped");
    }

    #[tokio::test]
    async fn empty_text_is_rejected() {
        let pool = pool().await;
        assert!(create_in(&pool, &None, "   ", None, None, 3, "planted", "manual").await.is_err());
    }

    #[tokio::test]
    async fn branch_scope_is_respected() {
        let pool = pool().await;
        create_in(&pool, &Some("b1".into()), "分支伏笔", None, None, 3, "planted", "manual").await.unwrap();
        assert_eq!(list_in(&pool, &None).await.unwrap().len(), 0);
        assert_eq!(list_in(&pool, &Some("b1".into())).await.unwrap().len(), 1);
    }
}
