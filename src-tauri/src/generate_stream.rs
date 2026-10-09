use serde_json::json;
use sqlx::{Row, SqlitePool};
use std::str::FromStr;
use sqlx::sqlite::SqliteConnectOptions;
use tauri::{Emitter, AppHandle};

use crate::story_db::story_db_path;
use crate::db;
use crate::keychain;

async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;
    let _ = crate::story_db::ensure_story_columns(&pool).await;
    Ok(pool)
}

async fn build_system_prompt(pool: &SqlitePool, node_type: &str, is_flashback: i64, mode: &str, my_role: &str, ai_role: &str, focus_role: &str, constraints: &str, length_level: &str, director: Option<&str>) -> Result<String, sqlx::Error> {
    let mut parts = Vec::new();
    parts.push("你是角色扮演引擎，依据当前模式工作：沉浸模式下以角色身份直接对话，旁白模式下以叙述者身份生成旁白。请用自然的文学语言继续剧情，必须自然包含角色的动作、心理、神态描写。不要输出 JSON。以角色卡设定为准；卡内未规定的细节可自由发挥，但不得改动已定义的性格、关系与世界观。\n【输出排版（供软件渲染，务必遵守）】\n- 人物台词：必须用双引号“”包住，并在台词前紧挨着写一个中括号，里面放说话人的角色名，格式如：[母亲]“你回吧。”。每条台词单独一行，台词与动作不要写在同一行。台词标记里写主名，正式回复的台词里可以用别名表示亲昵或者其他意思。\n- 动作/神态（非台词）：用（圆括号）括起、独占一行（如：（他攥紧了拳头））。连续动作时，一个动作一个括号单独包住并换行，不要挤在同一行。\n- 心理描写（只用沉浸式心声一类）：第一人称、现在时，写角色脑中真实冒出来的念头、自我说服、犹豫、口是心非；整行用星号包裹，格式如：*他怎么能这样……我分明没允许。*（软件会把星号渲染为心声样式）。语气必须贴合角色性格；心声须单独成行，不与动作、台词、旁白同行，不用中括号或引号。\n  硬性要求：角色口是心非时（尤其傲娇型），心声必须写出与台词相反或矛盾的真实想法，且角色自己未必肯承认——这是角色魅力的核心来源，不得省略、不得敷衍成旁白。\n- 旁白（叙述/环境描写）：只写场景、时间、气氛、外部事件，不进入任何人的内心；正常成段书写即可，不要用中括号或引号标记。\n- 除“台词必须带[角色名]标记”外，其余内容请保持自然发挥。".to_string());

    // 模式段：旁白扩写 / 沉浸对话
    if mode == "chat" {
        let my_disp = if my_role.is_empty() || my_role == "自由" { "（自由身份）".to_string() } else { format!("“{}”", my_role) };
        let ai_disp = if ai_role.is_empty() || ai_role == "自由" { "（自动选择角色库中最合适的角色）".to_string() } else { format!("“{}”", ai_role) };
        let dir_line = director.map(|d| format!("\n【导演指令】{}\n{}", crate::relations::director_legend(), d)).unwrap_or_default();
        parts.push(format!("\n【当前模式】沉浸对话：用户以{}的身份与你说话，你扮演{}回应。请始终以该角色的身份、语气直接回应，避免长篇旁白。不要替用户回答对话的内容和神态，只能回应用户角色的动作与神态。{}", my_disp, ai_disp, dir_line));
    } else {
        parts.push("\n【当前模式】旁白扩写（上帝视角）：以作者视角推进剧情，包含旁白叙述、各角色台词（[角色名]“台词”）与动作（（圆括号））。".to_string());
    }

    // 本次聚焦角色（@角色）：强调本段围绕该角色展开，不改变扮演身份
    if !focus_role.is_empty() && focus_role != "自由" {
        parts.push(format!("\n【本次聚焦】本段剧情请围绕角色“{}”展开，突出其视角、状态与内心；除非剧情需要，不要偏离该角色。", focus_role));
    }

    // 剧情约束（绝对红线）
    if !constraints.trim().is_empty() {
        let lines: Vec<String> = constraints.lines().filter(|l| !l.trim().is_empty()).map(|l| l.trim().to_string()).collect();
        if !lines.is_empty() {
            parts.push("\n【剧情约束·绝对红线】".to_string());
            for (i, l) in lines.iter().enumerate() {
                parts.push(format!("{}. {}", i + 1, l));
            }
            parts.push("以上约束优先级最高，生成内容不得违反。".to_string());
        }
    }
    // 篇幅要求
    match length_level {
        "简单" | "短" => parts.push("\n【篇幅要求·简单描写】篇幅精简，五到十行。动作、神态择要呈现，保留最能体现人物反差的那一两层；心理不可省略；环境不单独展开；台词可省缓冲，但每句独立成行、带角色名。".to_string()),
        _ => parts.push("\n【篇幅要求·详细展开】篇幅不限。动作、神态、心理、环境四项充分描写；动作分层铺开，可借具体物件或环境衬托心理；台词保留语气缓冲与停顿。".to_string()),
    }

    // 插曲/倒叙时空提示
    if node_type == "interlude" || is_flashback == 1 {
        parts.push("\n【时空提示】当前是一段插曲/倒叙回忆，请用回忆、穿插视角写作，明确提示时空与主线不同。".to_string());
    }

    let rows = sqlx::query("SELECT name, alias, role, faction, age, gender, one_liner, profile, speech_style, traits FROM characters")
        .fetch_all(pool)
        .await?;
    if !rows.is_empty() {
        parts.push("\n【角色】".to_string());
        for r in &rows {
            let name: String = r.try_get(0)?;
            let alias: String = r.try_get(1)?;
            let role: String = r.try_get(2)?;
            let faction: String = r.try_get(3)?;
            let age: String = r.try_get(4)?;
            let gender: String = r.try_get(5)?;
            let one_liner: String = r.try_get(6)?;
            let profile: String = r.try_get(7)?;
            let speech_style: String = r.try_get(8)?;
            let traits: String = r.try_get(9)?;
            let mut extras: Vec<String> = Vec::new();
            if !alias.is_empty() { extras.push(format!("别名：{}", alias)); }
            if !role.is_empty() { extras.push(format!("定位：{}", role)); }
            if !faction.is_empty() { extras.push(format!("阵营：{}", faction)); }
            if !age.is_empty() { extras.push(format!("年龄：{}", age)); }
            if !gender.is_empty() { extras.push(format!("性别：{}", gender)); }
            if !one_liner.is_empty() { extras.push(format!("身份：{}", one_liner)); }
            if !profile.is_empty() { extras.push(format!("小传：{}", profile)); }
            if !speech_style.is_empty() { extras.push(format!("说话方式：{}", speech_style)); }
            if !traits.is_empty() { extras.push(format!("性格：{}", traits)); }
            parts.push(format!("- {}：{}", name, if extras.is_empty() { "（暂无设定）".to_string() } else { extras.join("；") }));
        }
    }

    let rows = sqlx::query(
        "SELECT f.name, t.name, r.relation_type, r.direction, r.strength, r.state, r.description, f.gender, f.age, t.gender, t.age
         FROM relations r
         JOIN characters f ON f.id = r.from_character_id
         JOIN characters t ON t.id = r.to_character_id"
    ).fetch_all(pool).await?;
    if !rows.is_empty() {
        parts.push("\n【关系】".to_string());
        parts.push(crate::relations::direction_legend());
        parts.push(crate::relations::strength_legend());
        parts.push(crate::relations::state_legend());
        for r in &rows {
            let from: String = r.try_get(0)?;
            let to: String = r.try_get(1)?;
            let rel: String = r.try_get(2)?;
            let dir: String = r.try_get(3)?;
            let strength: i64 = r.try_get(4)?;
            let state: String = r.try_get(5)?;
            let description: String = r.try_get(6)?;
            let from_gender: String = r.try_get(7)?;
            let from_age: String = r.try_get(8)?;
            let to_gender: String = r.try_get(9)?;
            let to_age: String = r.try_get(10)?;
            let rel_label = if rel.trim().is_empty() { "陌生人".to_string() } else { rel.trim().to_string() };
            let is_stranger = rel.trim().is_empty() || rel.trim() == "陌生人";
            let desc = if description.trim().is_empty() { String::new() } else { format!("，说明：{}", description.trim()) };
            let band = crate::relations::strength_band(strength);
            if dir == "双向" {
                let splits = crate::relations::split_for_relation(&dir, &from, &to, &from_gender, &to_gender, &from_age, &to_age, &rel);
                for s in &splits {
                    if is_stranger {
                        parts.push(format!("- {}（强度 {}（{}），状态：{}，方向：双向）", s.text, strength, band, state));
                    } else {
                        parts.push(format!("- {}（关系：{}，强度 {}（{}），状态：{}，方向：双向{}）", s.text, rel_label, strength, band, state, desc));
                    }
                }
            } else if is_stranger {
                parts.push(format!("- {} 和 {} 是陌生人（强度 {}（{}），状态：{}，方向：单向）", from, to, strength, band, state));
            } else {
                parts.push(format!("- {} 是 {} 的「{}」（强度 {}（{}），状态：{}，方向：单向{}）", from, to, rel_label, strength, band, state, desc));
            }
        }
    }

    let rows = sqlx::query("SELECT key, value, is_hard_rule, category, meta_json FROM world_settings").fetch_all(pool).await?;
    if !rows.is_empty() {
        parts.push("\n【世界观】".to_string());
        for r in &rows {
            let key: String = r.try_get(0)?;
            let value: String = r.try_get(1)?;
            let hard: i64 = r.try_get(2)?;
            let category: String = r.try_get(3)?;
            let meta_json: String = r.try_get(4)?;
            let meta: serde_json::Value = serde_json::from_str(&meta_json).unwrap_or_else(|_| serde_json::json!({}));
            let get = |k: &str| meta.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            match category.as_str() {
                "术语" => {
                    let aliases = get("aliases");
                    let a = if aliases.is_empty() { String::new() } else { format!("（别名：{}）", aliases) };
                    parts.push(format!("- 术语「{}」：{}{}", key, value, a));
                }
                "地理" => {
                    let mut extra: Vec<String> = Vec::new();
                    let region = get("region");
                    if !region.is_empty() { extra.push(format!("区域：{}", region)); }
                    let owner = get("owner");
                    if !owner.is_empty() { extra.push(format!("归属：{}", owner)); }
                    let e = if extra.is_empty() { String::new() } else { format!("（{}）", extra.join("；")) };
                    parts.push(format!("- 地点「{}」：{}{}", key, value, e));
                }
                "势力" => {
                    let mut extra: Vec<String> = Vec::new();
                    for (k, label) in [("control_range", "控制范围"), ("scale", "规模"), ("leader", "头目"), ("attitude", "对主角态度")] {
                        let v = get(k);
                        if !v.is_empty() { extra.push(format!("{}：{}", label, v)); }
                    }
                    let e = if extra.is_empty() { String::new() } else { format!("（{}）", extra.join("；")) };
                    parts.push(format!("- 势力「{}」：{}{}", key, value, e));
                }
                "势力关系" => {
                    parts.push(format!("- 势力关系：{} 对 {}：{}", get("from"), get("to"), value));
                }
                _ => {
                    let tag = if hard == 1 { " [硬规则，不可违反]" } else { "" };
                    parts.push(format!("- {}：{}{}", key, value, tag));
                }
            }
        }
    }

    Ok(parts.join("\n"))
}

/// 粗估 token 数：中文 1 字 ≈ 1.5 token，英文按词数粗估。这里简单按字符数 × 1.5。
fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() as f64 * 1.5).ceil() as usize
}

/// 流式生成：逐 token emit 事件，完成后存 messages。
pub async fn generate_reply_streaming(
    app: AppHandle,
    lib_pool: &SqlitePool,
    story_id: &str,
    node_id: i64,
    user_text: &str,
    mode: &str,
    my_role: &str,
    ai_role: &str,
    focus_role: &str,
    tags: &str,
    constraints: &str,
    length_level: &str,
    memory: bool,
) -> Result<(), String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;

    let base_url = db::get_config(lib_pool, "model.base_url").await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let model = db::get_config(lib_pool, "model.name").await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "deepseek-v4-flash".to_string());
    let n_recent: i64 = db::get_config(lib_pool, "context.n_recent").await.map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok()).unwrap_or(10);
    let ctx_window: usize = db::get_config(lib_pool, "model.context_window").await.map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok()).unwrap_or(32768);
    // 预留 4800 token 给 max_tokens=4000 + 安全余量
    let budget = ctx_window.saturating_sub(4800);

    // 记录本次标签到当前节点（逗号分隔，随节点局部化标记）
    if !tags.is_empty() {
        let _ = sqlx::query("UPDATE timeline_nodes SET tags = ?1 WHERE id = ?2")
            .bind(tags).bind(node_id).execute(&pool).await;
    }

    let api_key = keychain::get_api_key("openai_api_key".to_string()).map_err(|e| e.to_string())?
        .ok_or_else(|| "未设置 API Key".to_string())?;

    // 查当前节点类型，给系统提示词加时空提示
    let node_row = sqlx::query("SELECT node_type, is_flashback FROM timeline_nodes WHERE id = ?1")
        .bind(node_id).fetch_optional(&pool).await.map_err(|e| e.to_string())?;
    let (node_type, is_flashback): (String, i64) = match node_row {
        Some(r) => (r.try_get(0).map_err(|e: sqlx::Error| e.to_string())?, r.try_get(1).map_err(|e: sqlx::Error| e.to_string())?),
        None => ("day".to_string(), 0),
    };

    // 导演指令：只存在于系统段（本次生效），历史里剥掉内联标记，避免同一件事在提示词里出现两次
    let director = crate::relations::parse_director(user_text);
    let user_text_clean = crate::relations::strip_director(user_text);
    let system = build_system_prompt(&pool, &node_type, is_flashback, mode, my_role, ai_role, focus_role, constraints, length_level, director.as_deref()).await.map_err(|e| e.to_string())?;
    let system_tokens = estimate_tokens(&system);
    let user_text: &str = &user_text_clean;

    // 拉全部历史（倒序），然后从 N=n_recent 开始往下试，直到 token 预算够；记忆上下文关闭时不带历史
    let mut all_history: Vec<(String, String)> = Vec::new();
    if memory {
        let all_rows = sqlx::query("SELECT role, content FROM messages WHERE node_id = ?1 ORDER BY id DESC")
            .bind(node_id).fetch_all(&pool).await.map_err(|e| e.to_string())?;
        all_history = all_rows.into_iter().rev().map(|r| {
            let role: String = r.try_get(0).unwrap_or_default();
            let content: String = r.try_get(1).unwrap_or_default();
            // 历史里的导演指令标记一并剥掉：指令只在当次系统段出现一次，历史留正文（否则既重复又会被当成台词读）
            (role, crate::relations::strip_director(&content))
        }).collect();
    }
    // 前端调用生成前已把本次用户消息存库，历史末尾通常就是这条；
    // 只有末尾不是同一条用户消息时才追加，避免当前输入重复进入上下文。
    let last_is_current_user = all_history
        .last()
        .map(|(role, content)| role.as_str() == "user" && content.as_str() == user_text)
        .unwrap_or(false);
    if !last_is_current_user {
        all_history.push(("user".to_string(), user_text.to_string()));
    }

    // 从 n_recent 开始尝试，逐步减小 N 直到 token 不超
    let mut chosen_n = n_recent.min(all_history.len() as i64);
    let mut chosen_history: Vec<(String, String)> = Vec::new();
    while chosen_n >= 0 {
        let slice: Vec<(String, String)> = if chosen_n as usize >= all_history.len() {
            all_history.clone()
        } else {
            all_history[all_history.len() - chosen_n as usize..].to_vec()
        };
        let hist_tokens: usize = slice.iter().map(|(_, c)| estimate_tokens(c)).sum::<usize>();
        if system_tokens + hist_tokens <= budget {
            chosen_history = slice;
            break;
        }
        chosen_n -= 1;
    }
    if chosen_n < 0 {
        chosen_history = vec![("user".to_string(), user_text.to_string())];
    }
    if chosen_n < n_recent {
        let _ = app.emit("chat-info", serde_json::json!({"detail": format!("上下文超限，自动将 N 从 {} 降到 {}", n_recent, chosen_n)}));
    }

    let mut messages_json = vec![json!({"role": "system", "content": system})];
    for (role, content) in &chosen_history {
        messages_json.push(json!({"role": role, "content": content}));
    }

    // 确保调试表存在（老 story.db 没有这张表）
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS debug_requests (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id INTEGER,
            model TEXT,
            system_prompt TEXT NOT NULL,
            history_json TEXT NOT NULL,
            user_text TEXT NOT NULL,
            ai_reply TEXT,
            estimated_tokens INTEGER NOT NULL DEFAULT 0,
            n_used INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'ok',
            error TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now', 'localtime'))
        )"
    ).execute(&pool).await;

    let history_json = serde_json::to_string(&chosen_history).unwrap_or_default();
    let total_tokens: usize = system_tokens + chosen_history.iter().map(|(_, c)| estimate_tokens(c)).sum::<usize>();

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "messages": messages_json,
        "max_tokens": 4000,
        "stream": true
    });

    let client = reqwest::Client::new();
    let resp = client.post(&url).bearer_auth(&api_key).json(&body).send().await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            let _ = sqlx::query(
                "INSERT INTO debug_requests(node_id, model, system_prompt, history_json, user_text, estimated_tokens, n_used, status, error)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'error', ?8)"
            )
            .bind(node_id).bind(&model).bind(&system).bind(&history_json).bind(user_text)
            .bind(total_tokens as i64).bind(chosen_n).bind(format!("请求失败: {e}"))
            .execute(&pool).await;
            return Err(format!("请求失败: {e}"));
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let _ = sqlx::query(
            "INSERT INTO debug_requests(node_id, model, system_prompt, history_json, user_text, estimated_tokens, n_used, status, error)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'error', ?8)"
        )
        .bind(node_id).bind(&model).bind(&system).bind(&history_json).bind(user_text)
        .bind(total_tokens as i64).bind(chosen_n).bind(format!("HTTP {}: {}", status.as_u16(), text.chars().take(500).collect::<String>()))
        .execute(&pool).await;
        let _ = app.emit("chat-error", serde_json::json!({"detail": format!("HTTP {}: {}", status.as_u16(), text.chars().take(300).collect::<String>())}));
        return Err(format!("HTTP {}", status.as_u16()));
    }

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut full = String::new();
    let mut buf: Vec<u8> = Vec::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读流失败: {e}"))?;
        buf.extend_from_slice(&chunk);
        // 按 b'\n' 找行；整行必须能转成 UTF-8 才算完整，否则保留等下次
        loop {
            match buf.iter().position(|&b| b == b'\n') {
                None => break,
                Some(idx) => {
                    let line_bytes: Vec<u8> = buf.drain(..=idx).collect();
                    // 尝试转 UTF-8；失败说明字符跨 chunk，把这行放回 buf 头部
                    match std::str::from_utf8(&line_bytes) {
                        Ok(line) => {
                            let line = line.trim();
                            if !line.starts_with("data: ") { continue; }
                            let data = &line[6..];
                            if data == "[DONE]" { continue; }
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                                if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
                                    if !delta.is_empty() {
                                        full.push_str(delta);
                                        let _ = app.emit("chat-token", serde_json::json!({"delta": delta}));
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            // 不完整 UTF-8，放回 buf 头部等下次 chunk
                            buf.splice(0..0, line_bytes);
                            break;
                        }
                    }
                }
            }
        }
    }

    // 流结束：flush 残留的未换行尾块（SSE 最后一行常无 \n，否则会丢尾）
    if !buf.is_empty() {
        let line_bytes = std::mem::take(&mut buf);
        if let Ok(line) = std::str::from_utf8(&line_bytes) {
            let line = line.trim();
            if line.starts_with("data: ") {
                let data = &line[6..];
                if data != "[DONE]" {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(delta) = v["choices"][0]["delta"]["content"].as_str() {
                            if !delta.is_empty() {
                                full.push_str(delta);
                                let _ = app.emit("chat-token", serde_json::json!({"delta": delta}));
                            }
                        }
                    }
                }
            }
        }
    }

    if full.is_empty() {
        let _ = app.emit("chat-error", serde_json::json!({"detail": "AI 返回空内容"}));
        return Err("AI 返回空内容".to_string());
    }

    // 存 AI 回复
    sqlx::query("INSERT INTO messages(node_id, role, content, mode) VALUES(?1, 'assistant', ?2, ?3)")
        .bind(node_id).bind(&full).bind(mode).execute(&pool).await.map_err(|e| e.to_string())?;

    // M5: 自动提取角色状态并更新状态卡（失败不影响主流程）
    let _ = crate::state_extract::auto_update_after_reply(&pool, story_id, node_id).await;

    // 记录成功快照
    let _ = sqlx::query(
        "INSERT INTO debug_requests(node_id, model, system_prompt, history_json, user_text, ai_reply, estimated_tokens, n_used, status)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'ok')"
    )
    .bind(node_id).bind(&model).bind(&system).bind(&history_json).bind(user_text).bind(&full)
    .bind(total_tokens as i64).bind(chosen_n)
    .execute(&pool).await;

    let _ = app.emit("chat-done", serde_json::json!({"content": full}));
    Ok(())
}
