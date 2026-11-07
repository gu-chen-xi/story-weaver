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
- 如果这一天实在没有任何剧情，只回一句「（无）」。\n\
\n\
格式说明：[用户] 开头的是**用户**（作者扮演的那个角色）说的话；[AI] 这一块里**可能同时涉及多个角色** —— 每一行里 [角色名] 后面紧跟的都是**那个角色**说的话；说话人切换时 [角色名] 会重新出现，**没有重新出现就表示仍是前一个角色在演**。判断「谁对谁表白」一律按这个算。\n\
另外顺手判一件事：这一天里有没有**角色主动示爱/告白**（不是用户示爱，也不是回应用户的告白）。\n\
有的话在后面单独一行：〔告白〕角色名：她说的那句原话（30 字以内）；没有就不写，最多 3 行。\n\
另外顺手记一下**这一天新出现的东西**（没有就不写，每种最多 2 条，总共最多 6 条）：\n\
〔新角色〕名字：一句话说明（他是谁、和谁有什么关系）\n\
〔新地点〕名字：一句话说明\n\
〔新术语〕词：一句话解释\n\
〔世界观〕条目：内容（世界的规则 / 习俗 / 势力，不是剧情）\n\
**硬规则**：这一天只要出现一个**不在【已有角色】里**的具名人物（有名字或有称呼，如「阿婆」「王老师」），必须写一条〔新角色〕；出现不在【已有设定】里的新地点/新术语/新规则，同样必须写一条。\n\
如果**已有条目这一天的说法变了**（身份揭穿、地点改用途、术语含义变了），写一行\n\
〔改角色〕现有角色名：新的一句话说明　／　〔改设定〕现有条目名：新的内容\n\
只写**第一次出现**的；已有的角色、地点、设定不要重复写；冒号后面不要写剧情过程。\n
";

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
    /// 这一天新抽到、已入队待确认的实体（角色 / 地点 / 术语 / 世界观条目）
    pub new_entities: Vec<String>,
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
        return Ok(CloseDayResult { day_label, summary: None, applied: Vec::new(), skipped: Some("这一天没有剧情，跳过日结".into()), new_entities: Vec::new() });
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
        return Ok(CloseDayResult { day_label, summary: None, applied: Vec::new(), skipped: Some("这一天没有正文，跳过日结".into()), new_entities: Vec::new() });
    }
    let existing: String = sqlx::query_scalar("SELECT COALESCE(day_summary, '') FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let span = if tods.is_empty() { String::new() } else { format!("（时段：{}）", tods.join(" → ")) };
    // 2026-11-05：**必须告诉模型"已经有什么"**，否则「只写第一次出现的」这句话没法执行 ——
    // 实测 deepseek-flash 会干脆一条 〔新角色〕 都不写（它不知道谁算"旧的"）。
    let known_chars: Vec<String> = crate::characters::list_characters_in(&pool, &branch)
        .await
        .map(|v| v.into_iter().map(|c| c.name).collect())
        .unwrap_or_default();
    let known_worlds: Vec<String> = crate::world::list_world_settings_in(&pool, &branch)
        .await
        .map(|v| v.into_iter().map(|w| w.key).collect())
        .unwrap_or_default();
    let known_block = format!(
        "\n【已有角色】{}\n【已有设定】{}\n（只写**不在这两份清单里**的新东西）",
        if known_chars.is_empty() { "（还没有）".to_string() } else { known_chars.join("、") },
        if known_worlds.is_empty() { "（还没有）".to_string() } else { known_worlds.join("、") },
    );
    let user_content = format!("【{day_label}】{span}的剧情记录：\n{}{}", transcript.chars().take(24000).collect::<String>(), known_block);
    let reply = call_summary_model(lib_pool, DAY_CLOSE_PROMPT, &user_content).await?;
    let sleep = parse_sleep_lines(&reply);
    // 2026-11-05（3.15）：日结顺手抽「这一天新出现的东西」→ 只入队，等人拍板
    let entities = crate::evolution::parse_new_entities(&reply);
    // 2026-11-05：日结这一步以前**不留快照**，出了问题（比如"新实体一条都没抽到"）
    // 只能靠猜。现在把这一次的请求 + 回复记进 debug_requests（开发者工具 → 请求快照能看到）。
    let _ = sqlx::query(
        "INSERT INTO debug_requests(node_id, model, system_prompt, history_json, user_text, ai_reply, status) \
         VALUES(?1, ?2, ?3, '', ?4, ?5, ?6)",
    )
    .bind(node_id)
    .bind("(日结/摘要模型)")
    .bind(DAY_CLOSE_PROMPT)
    .bind(&user_content)
    .bind(&reply)
    .bind(format!("day_close;entities={};sleep={}", entities.len(), sleep.len()))
    .execute(&pool)
    .await;
    // 2026-11-02：日结也会重写当天摘要 → 告白行同样解析、剥离、记卡
    let confessions = crate::timeline::parse_confessions(&reply);
    let text = crate::evolution::strip_new_entities(&crate::timeline::strip_confessions(&strip_sleep_lines(&reply)));
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
    if !confessions.is_empty() {
        for (name, said) in &confessions {
            let quote = format!("{name}：{said}");
            if let Err(e) = crate::memory::record_ai_confession(story_id, &branch, node_id, &day_label, &quote) {
                eprintln!("[memory] 记录她的告白失败（不影响日结）：{e}");
            }
        }
    }
    let mut applied = Vec::new();
    let mut settled: Vec<i64> = Vec::new(); // 已被「睡好 / 熬夜」明确结算过的角色
    for (name, kind) in sleep {
        let Some(cid) = crate::director::character_id_in(&pool, &branch, &name).await else { continue };
        let plan = crate::stat_change::plan_kind(&kind, "日结");
        if plan.is_empty() {
            continue;
        }
        settled.push(cid);
        applied.extend(crate::stat_change::apply(&pool, &branch, node_id, last_msg, cid, &plan, "summary").await);
    }
    // 2026-11-02：**普通的一夜** = +10 体力（用户定）。模型不用写这一行，
    // 没被「睡好/熬夜」点名的角色都按自然恢复算。apply 自带"同一天同类型只记一次"，重跑不会叠加。
    let chars = crate::characters::list_characters_in(&pool, &branch).await.unwrap_or_default();
    let plan = crate::stat_change::plan_kind("普通一夜", "日结");
    if !plan.is_empty() {
        for c in chars {
            if settled.contains(&c.id) {
                continue;
            }
            applied.extend(crate::stat_change::apply(&pool, &branch, node_id, last_msg, c.id, &plan, "summary").await);
        }
    }
    // 入队：失败不影响日结（和告白一样，只是"顺手"）
    let mut new_entities: Vec<String> = Vec::new();
    if !entities.is_empty() {
        match crate::evolution::queue_in(&pool, &branch, node_id, &day_label, &entities, &transcript).await {
            Ok(n) if n > 0 => new_entities = entities.iter().map(|e| e.name.clone()).collect(),
            Ok(_) => {}
            Err(e) => eprintln!("[evolution] 新实体入队失败（不影响日结）：{e}"),
        }
    }
    Ok(CloseDayResult { day_label, summary: summary_out, applied, skipped: None, new_entities })
}

#[cfg(test)]
mod tests;
