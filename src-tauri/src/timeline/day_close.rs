//! 日结（2026-10-29）：点「下一天」时给这一天收尾 —— **摘要 + 作息三态**。
//!
//! 为什么作息不交给对话模型：`没睡好` 这种日常状态几乎每天都能成立，模型会每轮报一次，
//! 变成慢性失血（实测：一天 −5，一周掉 35）。改成**一天一次、有全天上下文**的日结，
//! 三态语义：睡好（回一点）/ 熬夜（掉体力）/ **普通的一夜什么都不写**。
//!
//! 和"攒够 10 条自动生成 / 手动点生成"的日摘要**共用一张表但提示词分开**：
//! 日结的 prompt 额外要求最后几行 `〔作息〕角色名：睡好|熬夜`，软件只认这一块并从摘要正文剥掉。

use super::*;

/// 日结提示词（和 `DAY_SUMMARY_PROMPT` 分开：多一个作息机器块，且允许"这一天没有值得写的事"）
pub const DAY_CLOSE_PROMPT: &str = "你在为一部连续剧做\"这一天\"的收尾。下面是这一天的全部剧情记录\n\
输出两部分：\n\
1) 一段 200 字以内的日摘要：只写事实（谁做了什么、关系或认知变化、留下的伏笔），
不写感想/评价/台词原文，第三人称过去时，一段话，不要分段。\n\
2) 最后单独几行，给软件的对账行（每个角色最多一行，最多 5 行）：\n\
   〔作息〕角色名：睡好\n\
   〔作息〕角色名：熬夜\n\
规则（很重要）：\n\
- 只有剧情**明确写了**\"睡着/睡到天亮/歇了一夜\"才写「睡好」；\n\
- 只有剧情**明确写了**\"整夜没睡/熬到天亮\"才写「熬夜」；\n\
- 其余情况（普通的一夜、没提睡觉）**不要写这个角色** —— 不写 = 不变；\n\
- 不要写数字、不要写理由、不要解释；\n\
- 如果这一天实在没有任何剧情，只回一句「（无）」。";

/// 作息标记前缀
pub const SLEEP_TAG: &str = "〔作息〕";

#[derive(Debug, serde::Serialize)]
pub struct CloseDayResult {
    pub day_label: String,
    /// 新写入的日摘要（已有摘要时是 None）
    pub summary: Option<String>,
    /// 这一天的作息结算（可能为空 = 普通的一夜）
    pub applied: Vec<crate::stat_change::Applied>,
    /// 跳过原因（没剧情 / 没角色等）
    pub skipped: Option<String>,
}

/// 解析日结回复里的作息行：返回 (角色名, 睡好|熬夜)
pub fn parse_sleep_lines(reply: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in reply.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix(SLEEP_TAG) else { continue };
        let Some((name, kind)) = rest.split_once(|c| c == '：' || c == ':') else { continue };
        let name = name.trim();
        let kind = kind.trim();
        if name.is_empty() {
            continue;
        }
        if kind == "睡好" || kind == "熬夜" {
            out.push((name.to_string(), kind.to_string()));
        }
    }
    out
}

/// 把作息行从摘要正文里剥掉（只剥以标记开头的整行）
pub fn strip_sleep_lines(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim().starts_with(SLEEP_TAG))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// 日结：生成/补摘要 + 结算作息。没有剧情直接跳过（**不调模型**）。
pub async fn close_day(lib_pool: &SqlitePool, story_id: &str, node_id: i64) -> Result<CloseDayResult, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let day_label: String = sqlx::query_scalar("SELECT COALESCE(date_label,'') FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let rows = sqlx::query("SELECT id, role, content, COALESCE(meta,'{}') FROM messages WHERE node_id = ?1 ORDER BY id ASC")
        .bind(node_id)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(CloseDayResult { day_label, summary: None, applied: Vec::new(), skipped: Some("这一天没有剧情，跳过日结".into()) });
    }
    let mut transcript = String::new();
    let mut last_msg = 0i64;
    // 时段标记（2026-10-29）：把这一天出现过的 早/午/晚 按顺序记下来，给日结当"时间跨度"
    let mut tods: Vec<String> = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get(0).unwrap_or(0);
        let role: String = r.try_get(1).unwrap_or_default();
        let content: String = r.try_get(2).unwrap_or_default();
        let tod = crate::timeline::parse_tod(&r.try_get::<String, _>(3).unwrap_or_default());
        if !tod.is_empty() && tods.last() != Some(&tod) {
            tods.push(tod);
        }
        last_msg = last_msg.max(id);
        if content.trim().is_empty() {
            continue;
        }
        let who = if role == "user" { "用户" } else { "AI" };
        transcript.push_str(&format!("[{who}] {}\n", crate::relations::strip_director(&content)));
    }
    if transcript.trim().is_empty() {
        return Ok(CloseDayResult { day_label, summary: None, applied: Vec::new(), skipped: Some("这一天没有正文，跳过日结".into()) });
    }
    let existing: String = sqlx::query_scalar("SELECT COALESCE(day_summary, '') FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let span = if tods.is_empty() { String::new() } else { format!("（时段：{}）", tods.join(" → ")) };
    let user_content = format!("【{day_label}】{span}的剧情记录：\n{}", transcript.chars().take(24000).collect::<String>());
    let reply = call_summary_model(lib_pool, DAY_CLOSE_PROMPT, &user_content).await?;
    let sleep = parse_sleep_lines(&reply);
    let text = strip_sleep_lines(&reply);
    let mut summary_out = None;
    if existing.trim().is_empty() && !text.is_empty() && text != "（无）" {
        let s: String = text.chars().take(600).collect();
        sqlx::query("UPDATE timeline_nodes SET day_summary = ?1 WHERE id = ?2")
            .bind(&s)
            .bind(node_id)
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        summary_out = Some(s);
    }
    let mut applied = Vec::new();
    for (name, kind) in sleep {
        let Some(cid) = crate::director::character_id_in(&pool, &branch, &name).await else { continue };
        let plan = crate::stat_change::plan_kind(&kind, "日结");
        if plan.is_empty() {
            continue;
        }
        applied.extend(crate::stat_change::apply(&pool, &branch, node_id, last_msg, cid, &plan, "summary").await);
    }
    Ok(CloseDayResult { day_label, summary: summary_out, applied, skipped: None })
}

#[cfg(test)]
mod tests;
