use serde_json::json;
use sqlx::{Row, SqlitePool};

use crate::db;
use crate::keychain;


/// 读故事的角色、关系、世界观，组装系统提示词（稳定前缀，跨轮不变，便于前缀缓存命中）。
/// 顺序与流式版一致：基础指令 + 输出排版 → 数据字典 → 世界观 → 角色 → 方向和关系；
/// 易变内容（当前模式 / 篇幅 / 数值与状态 / 导演指令）由 build_tail_block 拼成「上下文更新」块，插在历史之后。
// 保留未用：旧的非流式生成路径（已被 generate_stream 取代）
#[allow(dead_code)]
async fn build_system_prompt(pool: &SqlitePool) -> Result<String, sqlx::Error> {
    let mut parts = Vec::new();

    parts.push("你是角色扮演引擎，依据当前模式工作：沉浸模式下以角色身份直接对话，旁白模式下以叙述者身份生成旁白。请用自然的文学语言继续剧情，必须自然包含角色的动作、心理、神态描写。不要输出 JSON。以角色卡设定为准；卡内未规定的细节可自由发挥，但不得改动已定义的性格、关系与世界观。\n【输出排版（供软件渲染，务必遵守）】\n- 人物台词：必须用双引号“”包住，并在台词前紧挨着写一个中括号，里面放说话人的角色名，格式如：[母亲]“你回吧。”。每条台词单独一行，台词与动作不要写在同一行。台词标记里写主名，正式回复的台词里可以用别名表示亲昵或者其他意思。\n- 动作/神态（非台词）：用（圆括号）括起、独占一行（如：（他攥紧了拳头））。连续动作时，一个动作一个括号单独包住并换行，不要挤在同一行。\n- 心理描写（只用沉浸式心声一类）：第一人称、现在时，写角色脑中真实冒出来的念头、自我说服、犹豫、口是心非；整行用星号包裹，格式如：*他怎么能这样……我分明没允许。*（软件会把星号渲染为心声样式）。语气必须贴合角色性格；心声须单独成行，不与动作、台词、旁白同行，不用中括号或引号。\n  硬性要求：角色口是心非时（尤其傲娇型），心声必须写出与台词相反或矛盾的真实想法，且角色自己未必肯承认——这是角色魅力的核心来源，不得省略、不得敷衍成旁白。\n- 旁白（叙述/环境描写）：只写场景、时间、气氛、外部事件，不进入任何人的内心；正常成段书写即可，不要用中括号或引号标记。\n- 除“台词必须带[角色名]标记”外，其余内容请保持自然发挥。".to_string());

    // 数据字典：关系强度档位 + 关系状态含义（两种「读法」，只在此处出现一次）
    parts.push(format!("\n【数据字典】\n{}\n{}", crate::relations::strength_legend(), crate::relations::state_legend()));

    // 世界观
    let rows = sqlx::query("SELECT key, value, is_hard_rule, category, meta_json FROM world_settings")
        .fetch_all(pool)
        .await?;
    if !rows.is_empty() {
        parts.push("\n【世界观】".to_string());
        for r in &rows {
            let key: String = r.try_get(0)?;
            let value: String = r.try_get(1)?;
            let hard: i64 = r.try_get(2)?;
            let category: String = r.try_get(3)?;
            let meta_json: String = r.try_get(4)?;
            // 世界书只走尾部块，不进 system 的世界观段
            if crate::world::is_tail_only(&category) {
                continue;
            }
            let meta: serde_json::Value = serde_json::from_str(&meta_json).unwrap_or_else(|_| serde_json::json!({}));
            let get = |k: &str| meta.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            match category.as_str() {
                "术语" => {
                    let aliases = get("aliases");
                    let a = if aliases.is_empty() { String::new() } else { format!("（别名：{}）", aliases) };
                    parts.push(format!("- 术语「{}」：{}{}", key, value, a));
                }
                "物品" => {
                    parts.push(crate::world::item_line(&key, &value, &get("aliases")));
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
    // 角色（稳定）
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

    // 方向和关系（稳定）：只放方向 + 关系类型 + 说明；强度与状态在尾部「数值与状态」段
    let rows = sqlx::query(
        "SELECT f.name, t.name, r.relation_type, r.direction, r.description, f.gender, f.age, t.gender, t.age
         FROM relations r
         JOIN characters f ON f.id = r.from_character_id
         JOIN characters t ON t.id = r.to_character_id"
    )
    .fetch_all(pool)
    .await?;
    if !rows.is_empty() {
        parts.push("\n【方向和关系】".to_string());
        for r in &rows {
            let from: String = r.try_get(0)?;
            let to: String = r.try_get(1)?;
            let rel: String = r.try_get(2)?;
            let dir: String = r.try_get(3)?;
            let description: String = r.try_get(4)?;
            let from_gender: String = r.try_get(5)?;
            let from_age: String = r.try_get(6)?;
            let to_gender: String = r.try_get(7)?;
            let to_age: String = r.try_get(8)?;
            let is_stranger = rel.trim().is_empty() || rel.trim() == "陌生人";
            if dir == "双向" {
                let splits = crate::relations::split_for_relation(&dir, &from, &to, &from_gender, &to_gender, &from_age, &to_age, &rel);
                for s in &splits {
                    parts.push(crate::relations::rel_direction_line(&s.text, &dir, &description));
                }
            } else if is_stranger {
                parts.push(crate::relations::rel_direction_line(&format!("{} 和 {} 是陌生人", from, to), &dir, &description));
            } else {
                parts.push(crate::relations::rel_direction_line(&format!("{} 是 {} 的「{}」", from, to, rel.trim()), &dir, &description));
            }
        }
    }

    }

    Ok(parts.join("\n"))
}

/// 生成剧情回复：组装上下文 → 调 DeepSeek → 存消息 → 返回内容。
// 保留未用：旧的非流式生成（同上）
#[allow(dead_code)]
pub async fn generate_reply(
    lib_pool: &SqlitePool,
    story_id: &str,
    node_id: i64,
    user_text: &str,
) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;

    // 读配置
    let base_url = db::get_config(lib_pool, "model.base_url")
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let model = db::get_config(lib_pool, "model.name")
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "deepseek-v4-flash".to_string());
    let n_recent: i64 = db::get_config(lib_pool, "context.n_recent")
        .await
        .map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    // 上下文预算三件套（与流式通道同口径）：系数 / 单次最大输出 / 安全余量
    let ctx_window: usize = db::get_config(lib_pool, "model.context_window")
        .await
        .map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok())
        .unwrap_or(32768);
    let reserve_ratio: f64 = db::get_config(lib_pool, "model.context_reserve_ratio")
        .await
        .map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.8);
    let max_tokens: usize = db::get_config(lib_pool, "model.max_tokens")
        .await
        .map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok())
        .unwrap_or(4000);
    let safety_margin: usize = db::get_config(lib_pool, "model.safety_margin")
        .await
        .map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok())
        .unwrap_or(800);
    // 非流式通道不做逐条裁剪（备用通道，默认只取 n_recent 条），但预算口径与流式保持一致
    let _budget = crate::generate_stream::input_budget_of(ctx_window, reserve_ratio, max_tokens, safety_margin);

    let api_key = keychain::get_api_key("openai_api_key".to_string())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未设置 API Key".to_string())?;

    // 系统提示词（稳定前缀）
    let system = build_system_prompt(&pool).await.map_err(|e| e.to_string())?;

    // 导演指令（从本次输入解析；历史里剥掉标记）
    let director = crate::relations::parse_director(user_text);
    let user_text_clean = crate::relations::strip_director(user_text);
    let user_text: &str = &user_text_clean;

    // 数值与状态（易变）：供尾部「上下文更新」块使用
    let rel_rows = sqlx::query(
        "SELECT f.name, t.name, r.strength, r.state
         FROM relations r
         JOIN characters f ON f.id = r.from_character_id
         JOIN characters t ON t.id = r.to_character_id"
    ).fetch_all(&pool).await.map_err(|e| e.to_string())?;
    let rel_values: Vec<String> = rel_rows.iter().filter_map(|r| {
        let from: String = r.try_get(0).ok()?;
        let to: String = r.try_get(1).ok()?;
        let strength: i64 = r.try_get(2).ok()?;
        let state: String = r.try_get(3).ok()?;
        Some(crate::relations::rel_value_line(&from, &to, strength, &state))
    }).collect();
    // 非流式是备用通道：模式/扮演/篇幅按默认值拼尾部块
    let tail_block = crate::generate_stream::build_tail_block("chat", "", "", "", "", "简单", "day", 0, director.as_deref(), &rel_values, &[], "", &[], 0);

    // B 子项：跨天装配 —— 近 KEEP_DAYS 个天数节点各取 n_recent 条原文；更早的天取日摘要进【前情提要】
    let day_nodes: Vec<(i64, String)> = sqlx::query(
        "SELECT id, date_label FROM timeline_nodes
         WHERE node_type IN ('day', 'main') AND is_flashback = 0
         ORDER BY sort_order ASC, id ASC"
    ).fetch_all(&pool).await.map_err(|e| e.to_string())?
     .iter().map(|r| (r.try_get(0).unwrap_or(0), r.try_get(1).unwrap_or_default())).collect();
    let cur_pos = day_nodes.iter().position(|(id, _)| *id == node_id);
    let keep: Vec<(i64, String)> = match cur_pos {
        Some(p0) => {
            let from = p0.saturating_sub(crate::generate_stream::KEEP_DAYS - 1);
            day_nodes[from..=p0].to_vec()
        }
        None => vec![(node_id, "当前时段".to_string())],
    };

    let mut history: Vec<(String, String)> = Vec::new();
    for (nid, label) in &keep {
        let rows = sqlx::query("SELECT role, content FROM messages WHERE node_id = ?1 ORDER BY id DESC LIMIT ?2")
            .bind(nid).bind(n_recent)
            .fetch_all(&pool).await.map_err(|e| e.to_string())?;
        let mut day: Vec<(String, String)> = rows.into_iter().rev().map(|r| {
            let role: String = r.try_get(0).unwrap_or_default();
            let content: String = r.try_get(1).unwrap_or_default();
            (role, crate::relations::strip_director(&content))
        }).collect();
        // 每天首条加「【第 N 天】」标记（与流式通道一致）
        if let Some(first) = day.first_mut() {
            first.1 = format!("【{}】\n{}", label, first.1);
        }
        history.append(&mut day);
    }

    // 【前情提要】：更早的天（不含当前节点）的日摘要
    let earliest_kept = keep.first().and_then(|(id, _)| day_nodes.iter().position(|(i2, _)| i2 == id));
    let mut recap_lines: Vec<String> = Vec::new();
    if let (Some(earliest), Some(cur)) = (earliest_kept, cur_pos) {
        for (idx, (nid, label)) in day_nodes.iter().enumerate() {
            if idx >= earliest || idx == cur { continue; }
            let s: String = sqlx::query_scalar("SELECT COALESCE(day_summary, '') FROM timeline_nodes WHERE id = ?1")
                .bind(nid).fetch_optional(&pool).await.map_err(|e| e.to_string())?.unwrap_or_default();
            if !s.trim().is_empty() {
                recap_lines.push(format!("- {}：{}", label, s.trim()));
            }
        }
    }
    // 前端调用生成前已把本次用户消息存库，历史末尾通常就是这条；
    // 只有末尾不是同一条用户消息时才追加，避免当前输入重复进入上下文。
    let last_is_current_user = history
        .last()
        .map(|(role, content)| role.as_str() == "user" && content.as_str() == user_text)
        .unwrap_or(false);
    if !last_is_current_user {
        history.push(("user".to_string(), user_text.to_string()));
    }

    let system = if recap_lines.is_empty() {
        system
    } else {
        format!("{}\n\n【前情提要】（更早的剧情摘要，用于延续前文；具体细节以历史对话为准）\n{}", system, recap_lines.join("\n"))
    };

    // 组装 messages：system（稳定前缀）→ 历史 → 本次用户消息（尾部块 + 用户输入，同一格）
    let mut messages_json = vec![json!({"role": "system", "content": system})];
    for (role, content) in &history {
        messages_json.push(json!({"role": role, "content": content}));
    }
    let last_user_content = if user_text.trim().is_empty() {
        tail_block.clone()
    } else {
        format!("{}\n\n——— 以下为用户输入 ———\n{}", tail_block, user_text)
    };
    messages_json.push(json!({"role": "user", "content": last_user_content}));

    // 调 API
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "messages": messages_json,
        "max_tokens": max_tokens,
        "stream": false
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读响应失败: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status.as_u16(), text.chars().take(300).collect::<String>()));
    }

    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("解析 JSON 失败: {e}"))?;
    let reply = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    if reply.is_empty() {
        return Err("AI 返回空内容".to_string());
    }

    Ok(reply)
}
