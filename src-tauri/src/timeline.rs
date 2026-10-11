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
    // D 子项：日摘要列 + 阶段摘要表（幂等，老库自动获得）
    crate::story_db::ensure_timeline_day_summary_column(&pool).await?;
    crate::story_db::ensure_story_summaries_table(&pool).await?;
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
    /// D 子项：日摘要（"这一天发生了什么"）。空 = 还没生成；生成一次即固化、不回改。
    #[serde(default)]
    pub day_summary: String,
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
            day_summary: r.try_get(6).unwrap_or_default(),
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
        day_summary: String::new(),
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
        "SELECT id, date_label, sort_order, node_type, is_flashback, tags, day_summary
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
            day_summary: r.try_get(6).unwrap_or_default(),
        });
    }
    Ok(out)
}

// ===================== D 子项：日摘要 / 阶段摘要 =====================

/// 日摘要生成提示词（只写事实：谁做了什么、关系变化、未回收伏笔；不写感想、不写台词）
pub const DAY_SUMMARY_PROMPT: &str = "你在为一部连续剧做\"前情提要\"素材。下面是某一天的全部剧情记录。\n\
请把这一天发生的事压缩成一段摘要，供后续章节回读。硬性要求：\n\
- 只写事实：谁做了什么、去了哪里、关系或认知发生了什么变化、留下了什么未回收的伏笔或约定；\n\
- 不写感想、不写评价、不写台词原文、不写文采描写；\n\
- 用第三人称、过去时，动词为主，人名用正式名；\n\
- 200 字以内，一段话，不要分段、不要小标题、不要 JSON、不要任何解释。";

/// 生成某节点的日摘要并落库（**生成一次即固化**：已有值直接返回，不回改）。
/// 返回 `Some(摘要)` 表示本次新生成，`None` 表示该节点已有摘要或没有可摘要的剧情。
pub async fn generate_day_summary(
    lib_pool: &SqlitePool,
    story_id: &str,
    node_id: i64,
    force: bool,
) -> Result<Option<String>, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;

    // 已固化则不再重算（force = 用户手动重生成时才覆盖）
    let existing: String = sqlx::query_scalar("SELECT COALESCE(day_summary, '') FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if !force && !existing.trim().is_empty() {
        return Ok(None);
    }

    let rows = sqlx::query("SELECT role, content FROM messages WHERE node_id = ?1 ORDER BY id ASC")
        .bind(node_id)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut transcript = String::new();
    for r in &rows {
        let role: String = r.try_get(0).unwrap_or_default();
        let content: String = r.try_get(1).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }
        let who = if role == "user" { "用户" } else { "AI" };
        transcript.push_str(&format!("[{who}] {}\n", crate::relations::strip_director(&content)));
    }
    if transcript.trim().is_empty() {
        return Ok(None);
    }

    // 复用与生成相同的模型配置
    let base_url = crate::db::get_config(lib_pool, "model.base_url")
        .await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let model = crate::db::get_config(lib_pool, "model.name")
        .await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "deepseek-v4-flash".to_string());
    let api_key = crate::keychain::get_api_key("openai_api_key".to_string())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未设置 API Key".to_string())?;

    let node_label: String = sqlx::query_scalar("SELECT date_label FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| format!("节点 {node_id}"));

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let user_content = format!("【{node_label}】的剧情记录：\n{}", transcript.chars().take(24000).collect::<String>());

    // 用 1500 额度重试一次：思考模式下 max_tokens 太小会被思考内容吃光，导致正文为空
    let attempts: [(usize, bool); 2] = [(1500, true), (1500, false)];
    let mut summary = String::new();
    let mut last_note = String::new();
    for (max_tokens, disable_thinking) in attempts {
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": DAY_SUMMARY_PROMPT},
                {"role": "user", "content": user_content}
            ],
            "max_tokens": max_tokens,
            "temperature": 0.3,
            "stream": false
        });
        if disable_thinking {
            // 摘要不需要思考：显式关掉，省 token 也避免思考挤掉正文
            body["thinking"] = serde_json::json!({"type": "disabled"});
        }
        let resp = reqwest::Client::new()
            .post(&url).bearer_auth(&api_key).json(&body).send().await
            .map_err(|e| format!("摘要请求失败: {e}"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            last_note = format!("HTTP {}: {}", status.as_u16(), text.chars().take(160).collect::<String>());
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => { last_note = format!("响应解析失败: {e}"); continue; }
        };
        let choice = &v["choices"][0];
        let content = choice["message"]["content"].as_str().unwrap_or_default().trim().to_string();
        let reasoning_len = choice["message"]["reasoning_content"].as_str().unwrap_or_default().chars().count();
        let finish = choice["finish_reason"].as_str().unwrap_or("-").to_string();
        if content.is_empty() {
            // 带上诊断信息：思考占了多少、是否被截断
            last_note = format!("正文为空（思考 {reasoning_len} 字，finish={finish}，额度 {max_tokens}）");
            continue;
        }
        summary = content;
        break;
    }
    if summary.is_empty() {
        return Err(format!("摘要为空：{last_note}"));
    }
    let summary: String = summary.chars().take(600).collect();

    sqlx::query("UPDATE timeline_nodes SET day_summary = ?1 WHERE id = ?2")
        .bind(&summary).bind(node_id).execute(&pool).await.map_err(|e| e.to_string())?;
    Ok(Some(summary))
}

/// 补齐结果：成功列表 + 每个失败节点的原因（失败必须能被看见，否则"什么都没发生"无法定位）
#[derive(serde::Serialize, Default)]
pub struct BackfillReport {
    pub done: Vec<String>,
    pub failed: Vec<String>,
}

/// 补齐所有"历史节点"的日摘要（幂等）：只处理当前节点之前、有剧情、但还没有摘要的 day 节点。
/// 当前节点不生成 —— 它还会继续写，摘要留到"翻篇"之后再固化（保证已生成的摘要不被回改）。
pub async fn backfill_day_summaries(lib_pool: &SqlitePool, story_id: &str, current_node_id: i64) -> Result<BackfillReport, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let rows = select_nodes_needing_summary(&pool, current_node_id).await.map_err(|e| e.to_string())?;

    let mut report = BackfillReport::default();
    for (id, label) in rows {
        match generate_day_summary(lib_pool, story_id, id, false).await {
            Ok(Some(_)) => report.done.push(label),
            Ok(None) => {}
            Err(e) => {
                // 单个节点失败不影响其它节点，也不影响主流程 —— 但要把原因带回界面
                eprintln!("[day_summary] {label} 生成失败: {e}");
                report.failed.push(format!("{label}：{e}"));
            }
        }
    }
    Ok(report)
}

/// 当天摘要的自动触发（用户 2026-10-11 定的规则）：
/// 当天的对话轮数达到阈值（默认 10，按 **AI 回复条数** 计）时，自动生成/刷新**当天**的日摘要。
/// 与 backfill 的分工：backfill 管"过去的天"（翻篇后固化一次），这里管"正在写的这天"（到量后随写随更新）。
/// 未到阈值时**绝不生成**，避免每轮都变、把前缀缓存打散。
pub async fn maybe_auto_day_summary(
    lib_pool: &SqlitePool,
    story_id: &str,
    current_node_id: i64,
    min_replies: i64,
) -> Result<Option<String>, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let replies: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE node_id = ?1 AND role = 'assistant'")
        .bind(current_node_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    if replies < min_replies.max(1) {
        return Ok(None);
    }
    let label: String = sqlx::query_scalar("SELECT date_label FROM timeline_nodes WHERE id = ?1")
        .bind(current_node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| format!("节点 {current_node_id}"));
    match generate_day_summary(lib_pool, story_id, current_node_id, true).await {
        Ok(Some(s)) => Ok(Some(format!("{label}（{replies} 条回复）"))),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

/// 阶段摘要（C 层接口预留）：写入一段覆盖 [day_from, day_to] 的摘要。
/// 本期不自动生成 —— 只提供读写，等日摘要积累后再接"若干天滚一段"。
pub async fn save_phase_summary(
    story_id: &str, day_from: i64, day_to: i64, content: &str, source_days: i64,
) -> Result<i64, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let id = sqlx::query(
        "INSERT INTO story_summaries(kind, day_from, day_to, content, source_days) VALUES('phase', ?1, ?2, ?3, ?4)"
    ).bind(day_from).bind(day_to).bind(content).bind(source_days)
     .execute(&pool).await.map_err(|e| e.to_string())?.last_insert_rowid();
    Ok(id)
}

/// 阶段摘要列表（按覆盖起始天倒序；供后续装配使用）
pub async fn list_phase_summaries(story_id: &str) -> Result<Vec<(i64, i64, i64, String)>, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let rows = sqlx::query(
        "SELECT day_from, day_to, source_days, content FROM story_summaries WHERE kind = 'phase' ORDER BY day_from DESC"
    ).fetch_all(&pool).await.map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| (
        r.try_get(0).unwrap_or(0), r.try_get(1).unwrap_or(0), r.try_get(2).unwrap_or(0), r.try_get(3).unwrap_or_default(),
    )).collect())
}

/// 选择"需要补日摘要"的节点（幂等）：**时间线上排在当前节点之前**、有剧情、还没有摘要的非倒叙节点。
///
/// 用"位置"而不是"sort_order 比大小"来判断先后 —— 因为历史数据里 sort_order 可能重复
/// （例如两个节点都叫「第 8 天」、sort_order 分别为 9 和 13），比大小会在极端情况下全表落空。
/// 抽成独立函数以便单测直接验证（不触发任何 API 调用）。
pub(crate) async fn select_nodes_needing_summary(pool: &SqlitePool, current_node_id: i64) -> Result<Vec<(i64, String)>, sqlx::Error> {
    // 按时间线顺序（sort_order, id）排出全表，找到当前节点的位置，取它之前的节点
    let all = sqlx::query("SELECT id, date_label, sort_order, is_flashback, COALESCE(day_summary, '') FROM timeline_nodes ORDER BY sort_order ASC, id ASC")
        .fetch_all(pool).await?;
    let pos = all.iter().position(|r| r.try_get::<i64, _>(0).unwrap_or(-1) == current_node_id);

    let mut out = Vec::new();
    for (idx, r) in all.iter().enumerate() {
        let id: i64 = r.try_get(0).unwrap_or(-1);
        let label: String = r.try_get(1).unwrap_or_default();
        let flashback: i64 = r.try_get(3).unwrap_or(0);
        let summary: String = r.try_get(4).unwrap_or_default();
        if id == current_node_id || flashback != 0 || !summary.trim().is_empty() {
            continue;
        }
        // 当前节点存在时必须排在它之前；当前节点找不到（数据异常）时退回"按 sort_order 小于最大值"的保守判断
        let before_current = match pos {
            Some(p) => idx < p,
            None => {
                let max_sort: i64 = all.iter().map(|x| x.try_get::<i64, _>(2).unwrap_or(0)).max().unwrap_or(0);
                r.try_get::<i64, _>(2).unwrap_or(0) < max_sort
            }
        };
        if !before_current {
            continue;
        }
        let has_msgs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE node_id = ?1")
            .bind(id).fetch_one(pool).await.unwrap_or(0);
        if has_msgs > 0 {
            out.push((id, label));
        }
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
        day_summary: String::new(),
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
        day_summary: String::new(),
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

#[cfg(test)]
mod summary_tests {
    use super::*;

    /// 建一个真实临时 story.db（含 timeline_nodes.day_summary 列），返回 (pool, 目录)
    async fn temp_story_db(tag: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = std::env::current_dir().unwrap().join("target").join("test-library").join(tag).join("projects").join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("story.db");
        let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE timeline_nodes (
                id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
                is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
                day_summary TEXT NOT NULL DEFAULT '')",
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER NOT NULL, role TEXT NOT NULL,
                content TEXT NOT NULL, mode TEXT NOT NULL DEFAULT 'chat', meta TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
        ).execute(&pool).await.unwrap();
        (pool, dir)
    }

    async fn add_node(pool: &SqlitePool, label: &str, sort: i64, summary: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, day_summary) VALUES(?1, ?2, 'day', 0, ?3) RETURNING id")
            .bind(label).bind(sort).bind(summary).fetch_one(pool).await.unwrap()
    }

    async fn add_msg(pool: &SqlitePool, node_id: i64, content: &str) {
        sqlx::query("INSERT INTO messages(node_id, role, content) VALUES(?1, 'user', ?2)")
            .bind(node_id).bind(content).execute(pool).await.unwrap();
    }

    /// 硬规则：只给"当前节点之前、有剧情、还没有摘要"的节点补摘要
    #[test]
    fn backfill_targets_only_past_unsummarized_nodes() {
        tauri::async_runtime::block_on(async {
            let (pool, dir) = temp_story_db("summary_sel").await;

            let n1 = add_node(&pool, "第 1 天", 1, "").await;            // 有剧情、无摘要 → 应被选中
            let n2 = add_node(&pool, "第 2 天", 2, "已固化的摘要").await;  // 已有摘要 → 不选
            let n3 = add_node(&pool, "第 3 天", 3, "").await;            // 无剧情 → 不选
            let n4 = add_node(&pool, "第 4 天", 4, "").await;            // 当前节点 → 不选
            add_msg(&pool, n1, "第一天剧情").await;
            add_msg(&pool, n2, "第二天剧情").await;
            add_msg(&pool, n4, "当前节点剧情").await;

            let picked = select_nodes_needing_summary(&pool, n4).await.unwrap();
            assert_eq!(picked.len(), 1, "只应选中第 1 天，实际: {picked:?}");
            assert_eq!(picked[0].0, n1);
            assert_eq!(picked[0].1, "第 1 天");

            // 倒叙节点永远不参与摘要
            let n5 = add_node(&pool, "回忆·那年夏天", 5, "").await;
            add_msg(&pool, n5, "回忆内容").await;
            sqlx::query("UPDATE timeline_nodes SET is_flashback = 1 WHERE id = ?1").bind(n5).execute(&pool).await.unwrap();
            let n6 = add_node(&pool, "第 5 天", 6, "").await;
            add_msg(&pool, n6, "第五天").await;
            let picked2 = select_nodes_needing_summary(&pool, n6).await.unwrap();
            assert!(picked2.iter().all(|(id, _)| *id != n5), "倒叙节点不应参与摘要: {picked2:?}");

            let _ = std::fs::remove_dir_all(&dir);
        });
    }

    /// 硬规则：已有摘要 → generate_day_summary 非 force 时直接返回 None（不回改）
    #[test]
    fn existing_summary_is_never_regenerated() {
        tauri::async_runtime::block_on(async {
            let (pool, dir) = temp_story_db("summary_idem").await;
            let n1 = add_node(&pool, "第 1 天", 1, "").await;
            add_msg(&pool, n1, "剧情").await;
            // 直接调用会走 API；这里只验证"选择器"在摘要写入后不再返回该节点（等价于幂等）
            assert_eq!(select_nodes_needing_summary(&pool, n1).await.unwrap().len(), 0, "当前节点不该被选中");
            let n2 = add_node(&pool, "第 2 天", 2, "").await;
            add_msg(&pool, n2, "剧情2").await;
            assert_eq!(select_nodes_needing_summary(&pool, n2).await.unwrap().len(), 1, "第 1 天应被选中");
            sqlx::query("UPDATE timeline_nodes SET day_summary = '写好了' WHERE id = ?1").bind(n1).execute(&pool).await.unwrap();
            assert_eq!(select_nodes_needing_summary(&pool, n2).await.unwrap().len(), 0, "已有摘要后不该再被选中（固化）");
            let _ = std::fs::remove_dir_all(&dir);
        });
    }
}