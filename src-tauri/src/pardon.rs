//! 赦免请求（2026-10-27）：把「赦免」从一键撤销，改成**要问过她**的流程。
//!
//! 用户定的规则：
//! * 点「赦免」不是直接免掉，而是发一条**请求**（弹窗确认 + 可补一句话）；
//! * 请求不写进剧情历史，只在对应那条 AI 回复下面挂一个**盒子**；
//! * 她在盒子里回一句（最后一行是协议标记），**同意**了「赦免」按钮才变亮，
//!   再点一次才真正生效（免分数/累计 + 关系回补**一半** —— 原谅了不代表没伤害）；
//! * 同一个 AI 消息节点最多请求 **3 次**，3 次都没同意 → 软件再扣 **5 点**关系；
//! * 每个**故事内的一天**最多 5 次（"一天"= 消息所在节点的 date_label，不是电脑日历）。
//!
//! 为什么请求不进历史：它会污染剧情（模型下一轮会把"请求"当成台词接下去），
//! 也会让历史里出现一条没有上下文的"同意"。

use serde::Serialize;
use sqlx::{Row, SqlitePool};

/// 同一个 AI 消息节点最多请求几次
pub const MAX_PER_MESSAGE: i64 = 3;
/// 同一个故事日最多请求几次
pub const MAX_PER_DAY: i64 = 5;
/// 3 次都没同意时，软件补扣的关系强度
pub const THIRD_REFUSAL_PENALTY: i64 = 5;

/// 一次赦免请求（也是界面上的一个盒子）
#[derive(Debug, Clone, Serialize)]
pub struct PardonRequest {
    pub id: i64,
    pub cost_id: i64,
    pub message_id: i64,
    pub node_id: i64,
    /// 用户补充的那句话（可空）
    pub extra: String,
    /// 她的回复全文（盒子里只显示最后一句）
    pub reply: String,
    /// pending / agreed / unclear / refused
    pub verdict: String,
    /// 这笔账的类型/理由（盒子里要写清"请求赦免什么"）
    pub kind: String,
    pub reason: String,
    /// 当时那条导演指令的原文（盒子第二行；请求时存下来，避免盒子挪位置后找不到）
    pub director: String,
    /// 谁问的、问的谁（盒子第一行）
    pub my_name: String,
    pub ai_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Agreed,
    Refused,
    Unclear,
}

impl Verdict {
    pub fn key(self) -> &'static str {
        match self {
            Verdict::Agreed => "agreed",
            Verdict::Refused => "refused",
            Verdict::Unclear => "unclear",
        }
    }
}


/// 从若干行里找协议标记（取**最后一条**，允许前后有空白/句末标点）
fn scan_marker<'a>(lines: impl Iterator<Item = &'a str>) -> Option<Verdict> {
    let mut found = None;
    for line in lines {
        if let Some(v) = verdict_of_line(line) {
            found = Some(v);
        }
    }
    found
}

/// 这一整行是不是赦免标记
fn verdict_of_line(line: &str) -> Option<Verdict> {
    let t = line
        .trim()
        .trim_matches(|c: char| c.is_whitespace() || "。.,，！!".contains(c));
    if !(t.starts_with("〔赦免") && t.ends_with("〕")) {
        return None;
    }
    let inner = t
        .trim_start_matches("〔赦免")
        .trim_end_matches("〕")
        .trim_start_matches(['：', ':', ' ', '　'])
        .trim();
    if inner.contains("不同意") || inner.contains("不原谅") {
        return Some(Verdict::Refused);
    }
    if inner.contains("同意") || inner.contains("原谅") {
        return Some(Verdict::Agreed);
    }
    None
}
/// 解析她的回答：**第一层认协议标记**，第二层按"不同意/不原谅"兜底。
///
/// 标记只在末尾协议区里认（和态度标记、导演代价同一套口径）；
/// 正文里出现「同意」两个字不算 —— 她可能只是在复述规则。
pub fn parse_verdict(reply: &str) -> Verdict {
    // 先按"末尾协议区"找（最规范的写法）。
    let start = crate::memory::protocol_block_start(reply);
    if let Some(v) = scan_marker(reply.lines().skip(start)) {
        return v;
    }
    // 兜底：模型有时把标记写在**最前面**（2026-10-28 实测：先写〔赦免:不同意〕，
    // 再在下面补理由和想说的话）。标记本身足够独特，整行匹配就不怕误判。
    if let Some(v) = scan_marker(reply.lines()) {
        return v;
    }
    // 第二层：没写标记，就看正文里有没有"否定"。
    // 注意先把所有〔…〕段剥掉：否则正文里留着的协议字样会被当成她的回答
    //（例如标记写歪了、后面还接着正文时）。
    let mut plain = String::new();
    let mut in_bracket = false;
    for ch in reply.chars() {
        match ch {
            '〔' => in_bracket = true,
            '〕' => in_bracket = false,
            c if !in_bracket => plain.push(c),
            _ => {}
        }
    }
    if plain.contains("不同意") || plain.contains("不原谅") {
        return Verdict::Refused;
    }
    if plain.contains("同意") || plain.contains("原谅") {
        return Verdict::Agreed;
    }
    Verdict::Unclear
}

/// 盒子里显示的那一句：回复的最后一个非空行
pub fn last_line_of(reply: &str) -> String {
    reply
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// 建表（幂等）
pub async fn ensure_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS pardon_requests (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            cost_id     INTEGER NOT NULL,
            message_id  INTEGER NOT NULL,
            node_id     INTEGER NOT NULL,
            extra       TEXT NOT NULL DEFAULT '',
            reply       TEXT NOT NULL DEFAULT '',
            verdict     TEXT NOT NULL DEFAULT 'pending',
            my_name     TEXT NOT NULL DEFAULT '',
            ai_name     TEXT NOT NULL DEFAULT '',
            director    TEXT NOT NULL DEFAULT '',
            branch_id   TEXT,
            created_at  TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )",
    )
    .execute(pool)
    .await?;
    // 老表（今天早些时候建的）补 director / my_name / ai_name
    let _ = sqlx::query("ALTER TABLE pardon_requests ADD COLUMN director TEXT NOT NULL DEFAULT ''").execute(pool).await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_pardon_message ON pardon_requests(message_id)")
        .execute(pool)
        .await;
    Ok(())
}

/// 同一笔账已经请求过几次（2026-10-27 改：盒子跟"当天最后一条消息"走，
/// 所以次数按**账**算，不按消息算 —— 否则同一天两笔账会共用同一个 3 次上限）。
async fn count_for_cost(pool: &SqlitePool, cost_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM pardon_requests WHERE cost_id = ?1")
        .bind(cost_id)
        .fetch_one(pool)
        .await
}

/// 同一个故事日（= 消息所在节点的 date_label）、同一个分支，已经请求过几次
async fn count_for_day(
    pool: &SqlitePool,
    branch: &Option<String>,
    day_label: &str,
) -> Result<i64, sqlx::Error> {
    let clause = crate::story_db::branch_clause("p.branch_id", branch, 2);
    let sql = format!(
        "SELECT COUNT(*) FROM pardon_requests p
         JOIN messages m ON m.id = p.message_id
         JOIN timeline_nodes n ON n.id = m.node_id
         WHERE n.date_label = ?1 AND {clause}"
    );
    let mut q = sqlx::query_scalar::<_, i64>(&sql).bind(day_label);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.fetch_one(pool).await
}

/// 这一笔账上还有没有"等她回答"的请求
async fn pending_exists(pool: &SqlitePool, cost_id: i64) -> Result<bool, sqlx::Error> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pardon_requests WHERE cost_id = ?1 AND verdict IN ('pending','unclear')",
    )
    .bind(cost_id)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

/// 这一笔账最近一次请求的判定（None = 没请求过）
pub async fn latest_verdict(pool: &SqlitePool, cost_id: i64) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT verdict FROM pardon_requests WHERE cost_id = ?1 ORDER BY id DESC LIMIT 1")
        .bind(cost_id)
        .fetch_optional(pool)
        .await
}

async fn message_content(pool: &SqlitePool, message_id: i64) -> Result<(i64, String), sqlx::Error> {
    let row = sqlx::query("SELECT COALESCE(node_id,0), COALESCE(content,'') FROM messages WHERE id = ?1")
        .bind(message_id)
        .fetch_optional(pool)
        .await?;
    Ok(match row {
        Some(r) => (r.try_get::<i64, _>(0).unwrap_or(0), r.try_get::<String, _>(1).unwrap_or_default()),
        None => (0, String::new()),
    })
}

/// 找这条 AI 回复**之前**的那条用户消息，取出导演指令原文
async fn director_text_before(pool: &SqlitePool, node_id: i64, message_id: i64) -> String {
    let rows = sqlx::query(
        "SELECT content FROM messages WHERE node_id = ?1 AND role = 'user' AND id < ?2 ORDER BY id DESC LIMIT 1",
    )
    .bind(node_id)
    .bind(message_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.first()
        .and_then(|r| r.try_get::<String, _>(0).ok())
        .and_then(|c| crate::relations::parse_director(&c))
        .unwrap_or_default()
}

/// 当天的历史（同一节点、截到最后 12 段；给的不是全文，只是前情）
async fn day_history(pool: &SqlitePool, node_id: i64, message_id: i64) -> String {
    let rows = sqlx::query(
        "SELECT role, content FROM messages WHERE node_id = ?1 AND id <= ?2 ORDER BY id ASC",
    )
    .bind(node_id)
    .bind(message_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    for r in &rows {
        let role: String = r.try_get(0).unwrap_or_default();
        let content: String = r.try_get(1).unwrap_or_default();
        let c = crate::stat_change::strip_line(&crate::director::strip_receipt(&crate::relations::strip_director(&content)));
        let c = crate::memory::strip_marker(&c);
        if c.trim().is_empty() {
            continue;
        }
        let clip: String = c.chars().take(300).collect();
        lines.push(format!("[{}] {}", if role == "user" { "用户" } else { "AI" }, clip));
    }
    let keep_from = lines.len().saturating_sub(12);
    lines.drain(..keep_from);
    lines.join("\n\n")
}


/// 盒子挂在哪条消息后面：**当天（这个节点）的最后一条**（没有就退回产生这笔账的那条）。
///
/// 2026-10-27 用户定：请求是"现在"发出的，盒子应该在眼前，而不是埋在
/// 产生这笔账的那条老回复下面（实测：消息 #36 的账，聊天停在 #42，盒子看不见）。
pub(crate) async fn last_message_of_node(
    pool: &SqlitePool,
    node_id: i64,
    fallback: i64,
) -> Result<i64, sqlx::Error> {
    let last: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id),0) FROM messages WHERE node_id = ?1")
        .bind(node_id)
        .fetch_one(pool)
        .await?;
    Ok(if last > 0 { last } else { fallback })
}
/// 拼请求的正文（那五段 + 补充）
fn build_request_text(
    my_name: &str,
    ai_name: &str,
    kind: &str,
    reason: &str,
    director: &str,
    her_lines: &str,
    history: &str,
    extra: &str,
) -> String {
    let mut s = String::new();
    let dir = if director.trim().is_empty() { "（未记录）" } else { director.trim() };
    let add = if extra.trim().is_empty() { "（无）" } else { extra.trim() };
    s.push_str("（这不是剧情，不要写进故事里，也不要续写场景。这一条只回答一件事。）\n");
    s.push_str(&format!("【赦免请求】用户：{my_name} 请求赦免刚刚那一笔导演代价。\n"));
    // 2026-10-28 用户定：先说清"那是当时的事"，否则模型会把指令当成现在的要求重新演一遍
    s.push_str(&format!(
        "当时「{my_name}」对你使用的导演指令是「{dir}」，注意这里是当时的情况，不是现在，现在同意与否在于你是否真的同意。\n"
    ));
    s.push_str(&format!("代价类型：{kind}；理由：{reason}\n"));
    if !her_lines.trim().is_empty() {
        s.push_str(&format!("你当时的回应（摘要）：{}\n", her_lines.trim()));
    }
    if !history.trim().is_empty() {
        s.push_str(&format!("当天发生过的（节选）：\n{}\n", history.trim()));
    }
    s.push_str("这是单独的模块判断：判断你是否原谅刚刚那条导演指令，以及你为它付出的代价。\n");
    s.push_str("回复模板：〔赦免:同意〕 / 〔赦免:不同意〕（注意：标记必须写在**最后一行**，先写下面的理由与想说的话）\n");
    s.push_str(&format!(
        "同意代表你原谅刚刚的指令，不同意代表不原谅。「{my_name}」想要补充的是：「{add}」\n"
    ));
    s.push_str(&format!("「{ai_name}」理由是：______\n"));
    s.push_str(&format!("「{ai_name}」想对「{my_name}」说：_____（可以不说）\n"));
    s.push_str("（上面几行请照着写：先写你的理由、想说的话，最后一行写〔赦免:同意〕或〔赦免:不同意〕。）\n");
    s.push_str("同意代表你原谅刚刚的指令，不同意代表不原谅。");
    s
}

/// 让模型写一条"是否原谅"的回答（非流式、短回复）
async fn call_model(lib_pool: &SqlitePool, system: &str, user: &str) -> Result<String, String> {
    let base_url = crate::db::get_config(lib_pool, "model.base_url")
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let model = crate::db::get_config(lib_pool, "model.name")
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "deepseek-v4-flash".to_string());
    let api_key = crate::keychain::get_api_key("openai_api_key".to_string())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未设置 API Key".to_string())?;
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "max_tokens": 400,
        "temperature": 0.8,
        "stream": false,
        "thinking": {"type": "disabled"}
    });
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("赦免请求失败: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("赦免请求失败 HTTP {}: {}", status.as_u16(), text.chars().take(200).collect::<String>()));
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("赦免响应解析失败: {e}"))?;
    Ok(v["choices"][0]["message"]["content"].as_str().unwrap_or_default().trim().to_string())
}

/// 发一次赦免请求。返回这次请求（含她的回答）。
///
/// 失败不扣次数（请求根本没发出去）；次数在**插入成功后**才算。
pub async fn request_pardon(
    lib_pool: &SqlitePool,
    story_id: &str,
    cost_id: i64,
    extra: &str,
    ai_name: &str,
    my_name: &str,
) -> Result<PardonRequest, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_table(&pool).await.map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);

    let cost = sqlx::query(
        "SELECT id, COALESCE(message_id,0), COALESCE(node_id,0), kind, reason, excused FROM director_costs WHERE id = ?1",
    )
    .bind(cost_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("找不到这笔账（id = {cost_id}）"))?;
    let message_id: i64 = cost.try_get(1).map_err(|e| e.to_string())?;
    let node_id: i64 = cost.try_get(2).map_err(|e| e.to_string())?;
    let kind: String = cost.try_get(3).map_err(|e| e.to_string())?;
    let reason: String = cost.try_get(4).map_err(|e| e.to_string())?;
    let excused: i64 = cost.try_get(5).map_err(|e| e.to_string())?;
    if excused != 0 {
        return Err("这笔账已经赦免过了".to_string());
    }
    if message_id <= 0 {
        return Err("这笔账没挂在任何消息上，无法请求赦免（老数据）".to_string());
    }
    if pending_exists(&pool, cost_id).await.map_err(|e| e.to_string())? {
        return Err("已经发过一次请求，等她的回答".to_string());
    }
    let used = count_for_cost(&pool, cost_id).await.map_err(|e| e.to_string())?;
    if used >= MAX_PER_MESSAGE {
        return Err(format!("这笔账已经请求过 {MAX_PER_MESSAGE} 次，不能再问了"));
    }
    let day_label: String = sqlx::query_scalar("SELECT COALESCE(date_label,'') FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let day_used = count_for_day(&pool, &branch, &day_label).await.map_err(|e| e.to_string())?;
    if day_used >= MAX_PER_DAY {
        return Err(format!("「{day_label}」这一天已经问过 {MAX_PER_DAY} 次了，明天再说"));
    }

    // 盒子挂在**当天（这个节点）最后一条消息**后面（2026-10-27 用户定）：
    // 请求是"现在"发出的，位置应该在读者眼前，而不是埋在产生这笔账的那条老回复下面。
    let box_msg_id = last_message_of_node(&pool, node_id, message_id).await.map_err(|e| e.to_string())?;

    let (_, her_reply) = message_content(&pool, message_id).await.map_err(|e| e.to_string())?;
    let her_lines = crate::memory::quote_gist(&her_reply, 80);
    let director = director_text_before(&pool, node_id, message_id).await;
    let history = day_history(&pool, node_id, message_id).await;

    let time_origin: String = sqlx::query_scalar("SELECT COALESCE(time_origin,'') FROM stories WHERE id = ?1")
        .bind(story_id)
        .fetch_optional(lib_pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let prologue = match story_id.trim().parse::<i64>() {
        Ok(sid) => crate::db::story_prologue(lib_pool, sid).await.unwrap_or_default(),
        Err(_) => String::new(),
    };
    let mut system = crate::generate_stream::prompt::build_system_prompt(&pool, &branch, &time_origin, &prologue)
        .await
        .map_err(|e| e.to_string())?;
    system.push_str("\n【本次任务的边界】这是一次元对话（不是剧情）：用户不是在跟你演，而是在请求你原谅刚刚那条导演指令。按你角色的性格决定原不原谅；不要续写场景，不要输出旁白，只写一两句态度 + 最后一行协议标记。");

    let user = build_request_text(my_name, ai_name, &kind, &reason, &director, &her_lines, &history, extra);
    let reply = call_model(lib_pool, &system, &user).await?;
    let verdict = parse_verdict(&reply);

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO pardon_requests(cost_id, message_id, node_id, extra, reply, verdict, my_name, ai_name, director, branch_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) RETURNING id",
    )
    .bind(cost_id)
    .bind(box_msg_id)
    .bind(node_id)
    .bind(extra)
    .bind(&reply)
    .bind(verdict.key())
    .bind(my_name)
    .bind(ai_name)
    .bind(&director)
    .bind(&branch)
    .fetch_one(&pool)
    .await
    .map_err(|e| e.to_string())?;

    if verdict != Verdict::Agreed {
        let _ = penalize_if_still_refused(&pool, &branch, cost_id, ai_name, my_name).await;
    }

    let created_at: String = sqlx::query_scalar("SELECT created_at FROM pardon_requests WHERE id = ?1")
        .bind(id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    Ok(PardonRequest {
        id, cost_id, message_id: box_msg_id, node_id,
        extra: extra.to_string(), reply, verdict: verdict.key().to_string(),
        kind, reason, director, my_name: my_name.to_string(), ai_name: ai_name.to_string(), created_at,
    })
}


/// 3 次都没同意 → 软件再扣 5 点关系（她的耐心用完了）。
///
/// 抽成函数是为了能单测：真正发请求要调 API，而"第 3 次还没同意"这个判断不该靠联网验证。
pub(crate) async fn penalize_if_still_refused(
    pool: &SqlitePool,
    branch: &Option<String>,
    cost_id: i64,
    ai_name: &str,
    my_name: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let n = count_for_cost(pool, cost_id).await?;
    let any_agreed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pardon_requests WHERE cost_id = ?1 AND verdict = 'agreed'",
    )
    .bind(cost_id)
    .fetch_one(pool)
    .await?;
    if n >= MAX_PER_MESSAGE && any_agreed == 0 {
        let meta = crate::director::BumpMeta {
            source: "pardon_refused",
            kind: "赦免被拒 3 次",
            reason: "赦免请求被拒 3 次",
            node_id: None,
            message_id: None,
        };
        if let Some(b) = crate::director::bump_strength(
            pool, branch, ai_name, my_name,
            -THIRD_REFUSAL_PENALTY, meta,
        ).await? {
            return Ok(Some(b.old - b.new));
        }
    }
    Ok(None)
}
/// 确认赦免：**必须先有"同意"**。执行内容：excused=1 + 关系回补一半（rel_delta/2）。
pub async fn apply_pardon(story_id: &str, cost_id: i64, ai_name: &str, my_name: &str) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_table(&pool).await.map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    // 先用当前解析器把这条账下的判定刷一遍（历史行可能是"标记写最前面"被判 unclear）
    if let Ok(rows) = sqlx::query("SELECT id, reply, verdict FROM pardon_requests WHERE cost_id = ?1")
        .bind(cost_id).fetch_all(&pool).await
    {
        for r in &rows {
            let id: i64 = r.try_get(0).unwrap_or(0);
            let reply: String = r.try_get(1).unwrap_or_default();
            let now = r.try_get::<String, _>(2).unwrap_or_default();
            let v = parse_verdict(&reply).key();
            if now != v {
                let _ = sqlx::query("UPDATE pardon_requests SET verdict = ?1 WHERE id = ?2")
                    .bind(v).bind(id).execute(&pool).await;
            }
        }
    }
    let agreed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pardon_requests WHERE cost_id = ?1 AND verdict = 'agreed'",
    )
    .bind(cost_id)
    .fetch_one(&pool)
    .await
    .map_err(|e| e.to_string())?;
    if agreed == 0 {
        return Err("她还没有同意，不能赦免".to_string());
    }
    let row = sqlx::query("SELECT COALESCE(rel_delta,0), excused FROM director_costs WHERE id = ?1")
        .bind(cost_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("找不到这笔账（id = {cost_id}）"))?;
    let rel_delta: i64 = row.try_get(0).map_err(|e| e.to_string())?;
    let excused: i64 = row.try_get(1).map_err(|e| e.to_string())?;
    if excused != 0 {
        return Ok("这笔账已经赦免过了".to_string());
    }
    sqlx::query("UPDATE director_costs SET excused = 1 WHERE id = ?1")
        .bind(cost_id)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let back = rel_delta / 2;
    if back > 0 {
        let meta = crate::director::BumpMeta {
            source: "pardon",
            kind: "赦免回补",
            reason: "赦免成功（回补一半）",
            node_id: None,
            message_id: None,
        };
        let _ = crate::director::bump_strength(&pool, &branch, ai_name, my_name, back, meta).await;
    }
    Ok(format!("已赦免：这笔账不再计入累计{}", if back > 0 { format!("，关系回补 {back} 点") } else { String::new() }))
}

/// 列出某故事（当前分支）的赦免请求，界面按 message_id 挂到对应回复下面
pub async fn list_requests(story_id: &str, node_id: Option<i64>) -> Result<Vec<PardonRequest>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_table(&pool).await.map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    list_requests_in(&pool, &branch, node_id).await.map_err(|e| e.to_string())
}

/// [`list_requests`] 的注入版（测试直接给 pool）。
///
/// 注意分支条件必须写 **p.branch_id**：这里 LEFT JOIN 了 director_costs，
/// 它也有 branch_id —— 只写 branch_id 会 ambiguous（2026-10-28 实测：盒子一直不显示就是这个）。
pub(crate) async fn list_requests_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    node_id: Option<i64>,
) -> Result<Vec<PardonRequest>, sqlx::Error> {
    let clause = crate::story_db::branch_clause("p.branch_id", branch, 1);
    let cols = "p.id, p.cost_id, p.message_id, p.node_id, p.extra, p.reply, p.verdict,
                p.my_name, p.ai_name, p.created_at,
                COALESCE(c.kind,''), COALESCE(c.reason,''), p.director";
    let sql = match node_id {
        Some(_) => format!(
            "SELECT {cols} FROM pardon_requests p LEFT JOIN director_costs c ON c.id = p.cost_id
             WHERE {clause} AND p.node_id = ?2 ORDER BY p.id ASC"
        ),
        None => format!(
            "SELECT {cols} FROM pardon_requests p LEFT JOIN director_costs c ON c.id = p.cost_id
             WHERE {clause} ORDER BY p.id ASC"
        ),
    };
    let mut q = sqlx::query(&sql);
    if let Some(b) = &branch {
        q = q.bind(b);
    }
    if let Some(nid) = node_id {
        q = q.bind(nid);
    }
    let rows = q.fetch_all(pool).await?;
    let mut out: Vec<PardonRequest> = rows
        .iter()
        .map(|r| PardonRequest {
            id: r.try_get(0).unwrap_or(0),
            cost_id: r.try_get(1).unwrap_or(0),
            message_id: r.try_get(2).unwrap_or(0),
            node_id: r.try_get(3).unwrap_or(0),
            extra: r.try_get(4).unwrap_or_default(),
            reply: r.try_get(5).unwrap_or_default(),
            verdict: r.try_get(6).unwrap_or_else(|_| "pending".to_string()),
            my_name: r.try_get(7).unwrap_or_default(),
            ai_name: r.try_get(8).unwrap_or_default(),
            created_at: r.try_get(9).unwrap_or_default(),
            kind: r.try_get(10).unwrap_or_default(),
            reason: r.try_get(11).unwrap_or_default(),
            director: r.try_get(12).unwrap_or_default(),
        })
        .collect();
    // 2026-10-28：历史行里可能有"标记写在最前面"被判成 unclear 的 —— 读的时候用新解析器重算一次
    for r in &mut out {
        let v = parse_verdict(&r.reply).key();
        if r.verdict != v {
            let _ = sqlx::query("UPDATE pardon_requests SET verdict = ?1 WHERE id = ?2")
                .bind(v).bind(r.id).execute(pool).await;
            r.verdict = v.to_string();
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
