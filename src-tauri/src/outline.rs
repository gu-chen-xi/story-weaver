//! 剧本大纲的注入（2026-10-30 阶段②）：把「本章目标」送进生成提示词。
//!
//! 拍板：**手动开关、默认开**；本章目标为空 → 整段不出现（零 token）。
//! 只注入“目标”这一句，不注入情节点/幕结构 —— 先不给模型加结构约束。

use sqlx::{Row, SqlitePool};

/// 开关键名（与前端插件/设置页共用同一个 config 键）
pub const INJECT_KEY: &str = "outline.inject";

/// 注入开关：**缺省为开**；只有显式 "false" / "0" / "off" 才关。
pub async fn inject_enabled(lib_pool: &SqlitePool) -> bool {
    match crate::db::get_config(lib_pool, INJECT_KEY).await {
        Ok(Some(v)) => !matches!(v.trim().to_lowercase().as_str(), "false" | "0" | "off"),
        _ => true,
    }
}

/// 拼“本章目标”块；目标为空/空白 → None（调用方不拼，零 token）。
pub fn goal_block(goal: &str) -> Option<String> {
    let g = goal.trim();
    if g.is_empty() {
        return None;
    }
    Some(format!(
        "【本章目标】{}\n- 这一轮按本章目标推进；**不要提前收束**，也不要把目标之外的事主动加进来。\n- 目标是方向，不是台词：不要复述它，也不要把它写成旁白。",
        g
    ))
}

/// 这个节点属于哪一章（多对多里取章序最小的那章），返回 (章序, 章标题, 目标)。
pub async fn chapter_for_node(
    pool: &SqlitePool,
    node_id: i64,
) -> Option<(i64, String, String)> {
    let row = sqlx::query(
        "SELECT c.order_no, c.title, c.goal FROM chapter_nodes cn
         JOIN chapters c ON c.id = cn.chapter_id
         WHERE cn.node_id = ?1 ORDER BY c.order_no ASC LIMIT 1",
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;
    Some((
        row.try_get(0).unwrap_or(0),
        row.try_get(1).unwrap_or_default(),
        row.try_get(2).unwrap_or_default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[test]
    fn empty_goal_injects_nothing() {
        assert!(goal_block("").is_none());
        assert!(goal_block("   \n ").is_none(), "空白也算空");
    }

    #[test]
    fn goal_block_names_the_chapter_goal_without_numbers() {
        let b = goal_block("把钥匙交出去，但不许她先开口").unwrap();
        assert!(b.contains("【本章目标】"));
        assert!(b.contains("把钥匙交出去"));
        assert!(b.contains("不要提前收束"));
        assert!(!b.chars().any(|c| c.is_ascii_digit()), "目标块不带数字");
    }

    /// 节点属于哪一章：多对多里取**章序最小**的那章（作者把一天圈进两章时的确定性口径）
    #[tokio::test]
    async fn chapter_for_node_picks_lowest_order() {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE chapters (id INTEGER PRIMARY KEY, order_no INTEGER, title TEXT, goal TEXT)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE chapter_nodes (chapter_id INTEGER, node_id INTEGER, PRIMARY KEY(chapter_id,node_id))")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO chapters VALUES(1,2,'第二','目标二'),(2,1,'第一','目标一')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO chapter_nodes VALUES(1,7),(2,7)").execute(&pool).await.unwrap();
        let got = chapter_for_node(&pool, 7).await.unwrap();
        assert_eq!(got.0, 1, "章序小的优先");
        assert_eq!(got.2, "目标一");
        assert!(chapter_for_node(&pool, 99).await.is_none());
    }

    #[tokio::test]
    async fn inject_defaults_on_and_can_be_turned_off() {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE configs (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        assert!(inject_enabled(&pool).await, "缺省是开");
        crate::db::set_config(&pool, INJECT_KEY, "false").await.unwrap();
        assert!(!inject_enabled(&pool).await);
        crate::db::set_config(&pool, INJECT_KEY, "true").await.unwrap();
        assert!(inject_enabled(&pool).await);
    }
}
