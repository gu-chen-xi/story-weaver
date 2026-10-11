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

/// 系统提示词（稳定前缀，跨轮不变，便于前缀缓存命中）
/// 顺序：基础指令 + 输出排版 → 数据字典 → 世界观 → 角色 → 方向和关系
/// 易变内容（当前模式 / 篇幅 / 数值与状态 / 聚焦 / 约束 / 时空 / 导演指令）不在这里，
/// 由 build_tail_block 拼成「上下文更新」块，插在历史之后（见调用处）
async fn build_system_prompt(pool: &SqlitePool) -> Result<String, sqlx::Error> {
    let mut parts = Vec::new();
    parts.push("你是角色扮演引擎，依据当前模式工作：沉浸模式下以角色身份直接对话，旁白模式下以叙述者身份生成旁白。请用自然的文学语言继续剧情，必须自然包含角色的动作、心理、神态描写。不要输出 JSON。以角色卡设定为准；卡内未规定的细节可自由发挥，但不得改动已定义的性格、关系与世界观。\n【输出排版（供软件渲染，务必遵守）】\n- 人物台词：必须用双引号“”包住，并在台词前紧挨着写一个中括号，里面放说话人的角色名，格式如：[母亲]“你回吧。”。每条台词单独一行，台词与动作不要写在同一行。台词标记里写主名，正式回复的台词里可以用别名表示亲昵或者其他意思。\n- 动作/神态（非台词）：用（圆括号）括起、独占一行（如：（他攥紧了拳头））。连续动作时，一个动作一个括号单独包住并换行，不要挤在同一行。\n- 心理描写（只用沉浸式心声一类）：第一人称、现在时，写角色脑中真实冒出来的念头、自我说服、犹豫、口是心非；整行用星号包裹，格式如：*他怎么能这样……我分明没允许。*（软件会把星号渲染为心声样式）。语气必须贴合角色性格；心声须单独成行，不与动作、台词、旁白同行，不用中括号或引号。\n  硬性要求：角色口是心非时（尤其傲娇型），心声必须写出与台词相反或矛盾的真实想法，且角色自己未必肯承认——这是角色魅力的核心来源，不得省略、不得敷衍成旁白。\n- 旁白（叙述/环境描写）：只写场景、时间、气氛、外部事件，不进入任何人的内心；正常成段书写即可，不要用中括号或引号标记。\n- 除“台词必须带[角色名]标记”外，其余内容请保持自然发挥。".to_string());

    // 数据字典：关系强度档位 + 关系状态含义（两种「读法」，只在此处出现一次）
    parts.push(format!("\n【数据字典】\n{}\n{}", crate::relations::strength_legend(), crate::relations::state_legend()));

    // 世界观（稳定）—— 放在【数据字典】之后、【角色】之前
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

    // 角色（稳定）—— 放在【世界观】之后
    let rows = sqlx::query("SELECT name, alias, role, faction, age, gender, one_liner, profile, speech_style, traits FROM characters").fetch_all(pool).await?;
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
    ).fetch_all(pool).await?;
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

    Ok(parts.join("\n"))
}

/// 尾部「上下文更新」块的首行标记（用于识别块、以及把块并入用户消息）
pub(crate) const TAIL_BLOCK_HEAD: &str = "【上下文更新】";

/// 尾部「上下文更新」块（每轮单独拼装、不落库）：当前模式 / 篇幅 / 数值与状态 / 聚焦 / 约束 / 时空 / 导演指令
/// 放在历史之后，易变内容变化时不破坏前面的前缀缓存；
/// 最终由调用方**并入本次用户消息**（同一格内，块在前、用户输入在后），避免出现两条连续的 user 消息。
pub(crate) fn build_tail_block(
    mode: &str,
    my_role: &str,
    ai_role: &str,
    focus_role: &str,
    constraints: &str,
    length_level: &str,
    node_type: &str,
    is_flashback: i64,
    director: Option<&str>,
    rel_values: &[String],
) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("{}（软件自动附带的本轮运行参数，不是剧情内容、也不是用户角色的台词；优先级高于历史中的同类旧信息，如与历史冲突以本块为准。本块与后面的用户输入属于同一条消息：分隔线以上由软件生成，分隔线以下才是用户输入）", TAIL_BLOCK_HEAD));

    if mode == "chat" {
        let my_disp = if my_role.is_empty() || my_role == "自由" { "（自由身份）".to_string() } else { format!("“{}”", my_role) };
        let ai_disp = if ai_role.is_empty() || ai_role == "自由" { "（自动选择角色库中最合适的角色）".to_string() } else { format!("“{}”", ai_role) };
        parts.push(format!("【当前模式】沉浸对话：用户以{}的身份与你说话，你扮演{}回应。请始终以该角色的身份、语气直接回应，避免长篇旁白。不要替用户回答对话的内容和神态，只能回应用户角色的动作与神态。", my_disp, ai_disp));
    } else {
        parts.push("【当前模式】旁白扩写（上帝视角）：以作者视角推进剧情，包含旁白叙述、各角色台词（[角色名]“台词”）与动作（（圆括号））。".to_string());
    }

    match length_level {
        "简单" | "短" => parts.push("【篇幅要求·简单描写】篇幅精简，五到十行。动作、神态择要呈现，保留最能体现人物反差的那一两层；心理不可省略；环境不单独展开；台词可省缓冲，但每句独立成行、带角色名。".to_string()),
        _ => parts.push("【篇幅要求·详细展开】篇幅不限。动作、神态、心理、环境四项充分描写；动作分层铺开，可借具体物件或环境衬托心理；台词保留语气缓冲与停顿。".to_string()),
    }

    if !rel_values.is_empty() {
        parts.push("【数值与状态】（各关系的当前强度与状态，以本节为准，历史中的旧数值/旧状态作废）".to_string());
        for l in rel_values {
            parts.push(l.clone());
        }
    }

    if !focus_role.is_empty() && focus_role != "自由" {
        parts.push(format!("【本次聚焦】本段剧情请围绕角色“{}”展开，突出其视角、状态与内心；除非剧情需要，不要偏离该角色。", focus_role));
    }

    let clines: Vec<String> = constraints.lines().filter(|l| !l.trim().is_empty()).map(|l| l.trim().to_string()).collect();
    if !clines.is_empty() {
        parts.push("【剧情约束·绝对红线】".to_string());
        for (i, l) in clines.iter().enumerate() {
            parts.push(format!("{}. {}", i + 1, l));
        }
        parts.push("以上约束优先级最高，生成内容不得违反。".to_string());
    }

    if node_type == "interlude" || is_flashback == 1 {
        parts.push("【时空提示】当前是一段插曲/倒叙回忆，请用回忆、穿插视角写作，明确提示时空与主线不同。".to_string());
    }

    if let Some(d) = director {
        parts.push(format!("【导演指令】{}\n{}", crate::relations::director_legend(), d));
    }

    parts.join("\n")
}

/// 粗估 token 数：中文 1 字 ≈ 1.5 token，英文按词数粗估。这里简单按字符数 × 1.5。
fn estimate_tokens(text: &str) -> usize {
    (text.chars().count() as f64 * 1.5).ceil() as usize
}

/// 可用输入预算 = 窗口 × 系数 − （输出预留 + 安全余量）；系数超范围时回退 0.8，且结果不小于 0。
/// 抽成独立函数是为了前后端/两条通道口径一致，也便于单测。
pub(crate) fn input_budget_of(ctx_window: usize, reserve_ratio: f64, max_tokens: usize, safety_margin: usize) -> usize {
    let ratio = if reserve_ratio > 0.0 && reserve_ratio <= 1.0 { reserve_ratio } else { 0.8 };
    let scaled = ((ctx_window as f64) * ratio).round();
    let scaled = if scaled.is_finite() && scaled > 0.0 { scaled as usize } else { 0 };
    scaled.saturating_sub(max_tokens.saturating_add(safety_margin))
}

#[cfg(test)]
mod budget_tests {
    use super::input_budget_of;

    #[test]
    fn budget_formula_basic() {
        // 128k 窗口 × 0.8 = 102400，减输出预留 4000 + 余量 800 = 97600
        assert_eq!(input_budget_of(128_000, 0.8, 4000, 800), 97_600);
        // 32k 保守档：32768 × 0.8 = 26214（四舍五入）− 4800 = 21414
        assert_eq!(input_budget_of(32_768, 0.8, 4000, 800), 21_414);
        // 1M 窗口
        assert_eq!(input_budget_of(1_000_000, 0.8, 4000, 800), 795_200);
    }

    #[test]
    fn budget_formula_guards() {
        // 系数非法 → 回退 0.8，不产生 NaN/panic
        assert_eq!(input_budget_of(1_000_000, 0.0, 4000, 800), 795_200);
        assert_eq!(input_budget_of(1_000_000, 1.5, 4000, 800), 795_200);
        assert_eq!(input_budget_of(1_000_000, f64::NAN, 4000, 800), 795_200);
        // 窗口小于预留 → 下溢为 0，不 panic
        assert_eq!(input_budget_of(1000, 0.8, 4000, 800), 0);
        assert_eq!(input_budget_of(0, 0.8, 4000, 800), 0);
    }
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
    // 上下文预算三件套（A 子项，2026-10-10）：系数 / 单次最大输出 / 安全余量
    let reserve_ratio: f64 = db::get_config(lib_pool, "model.context_reserve_ratio").await.map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok()).unwrap_or(0.8);
    let max_tokens: usize = db::get_config(lib_pool, "model.max_tokens").await.map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok()).unwrap_or(4000);
    let safety_margin: usize = db::get_config(lib_pool, "model.safety_margin").await.map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok()).unwrap_or(800);
    // 实际可用输入预算 = 窗口 × 系数 − 回复预留（max_tokens + 安全余量）
    // 注意：历史区预算还要再减 system 段与本次输入，那一步在装配时算（见下）
    let budget = input_budget_of(ctx_window, reserve_ratio, max_tokens, safety_margin);

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

    // 导演指令：从用户输入里解析出来，注入本轮尾部「上下文更新」块；历史里剥掉内联标记
    let director = crate::relations::parse_director(user_text);
    let user_text_clean = crate::relations::strip_director(user_text);
    let system = build_system_prompt(&pool).await.map_err(|e| e.to_string())?;

    // 数值与状态（易变）：只取数字与状态，供尾部块使用
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

    let tail_block = build_tail_block(mode, my_role, ai_role, focus_role, constraints, length_level, &node_type, is_flashback, director.as_deref(), &rel_values);
    let system_tokens = estimate_tokens(&system) + estimate_tokens(&tail_block);
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
    // 前端调用生成前已把本次用户消息存库（存的是**带导演指令标记**的原文），
    // 而历史里读出来时已用 strip_director 剥过标记 —— 必须拿剥过标记的正文去比对，
    // 否则会误判"末尾还没有本次输入"，把同一条输入再追加一遍（表现为请求里出现两条一样的 user）。
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

    // 组装 messages：system（稳定前缀）→ 历史 → 本次用户消息（尾部块 + 用户输入，同一格）
    // 块与输入合成一条 user：避免出现"两条连续 user"导致模型分不清哪段是台词；
    // 块本身仍然不落库、每轮现拼，历史里只有用户的原文。
    let mut messages_json = vec![json!({"role": "system", "content": system})];
    for (role, content) in &chosen_history {
        messages_json.push(json!({"role": role, "content": content}));
    }
    let last_user_content = if user_text.trim().is_empty() {
        tail_block.clone()
    } else {
        format!("{}\n\n——— 以下为用户输入 ———\n{}", tail_block, user_text)
    };
    messages_json.push(json!({"role": "user", "content": last_user_content}));

    // 确保调试表存在（老 story.db 没有这张表）
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS debug_requests (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id INTEGER,
            model TEXT,
            system_prompt TEXT NOT NULL,
            tail_block TEXT,
            context_window INTEGER NOT NULL DEFAULT 0,
            reserve_ratio REAL NOT NULL DEFAULT 0,
            max_tokens INTEGER NOT NULL DEFAULT 0,
            safety_margin INTEGER NOT NULL DEFAULT 0,
            input_budget INTEGER NOT NULL DEFAULT 0,
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
    // 老库补列（幂等）
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN tail_block TEXT").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN context_window INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN reserve_ratio REAL NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN max_tokens INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN safety_margin INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN input_budget INTEGER NOT NULL DEFAULT 0").execute(&pool).await;

    let history_json = serde_json::to_string(&chosen_history).unwrap_or_default();
    // 估算 token：system_tokens 已含 system + 尾部块，这里再加历史（块与输入并入同一条 user，块只算一次）
    let total_tokens: usize = system_tokens + chosen_history.iter().map(|(_, c)| estimate_tokens(c)).sum::<usize>();
    // 快照记录本次实际用的预算口径（前端不再自己硬编码算，直接读这里）
    let input_budget = budget;

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "messages": messages_json,
        "max_tokens": max_tokens,
        "stream": true
    });

    let client = reqwest::Client::new();
    let resp = client.post(&url).bearer_auth(&api_key).json(&body).send().await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            let _ = sqlx::query(
                "INSERT INTO debug_requests(node_id, model, system_prompt, tail_block, context_window, reserve_ratio, max_tokens, safety_margin, input_budget, history_json, user_text, estimated_tokens, n_used, status, error)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'error', ?14)"
            )
            .bind(node_id).bind(&model).bind(&system).bind(&tail_block)
            .bind(ctx_window as i64).bind(reserve_ratio).bind(max_tokens as i64).bind(safety_margin as i64).bind(input_budget as i64)
            .bind(&history_json).bind(user_text)
            .bind(total_tokens as i64).bind(chosen_n).bind(format!("请求失败: {e}"))
            .execute(&pool).await;
            return Err(format!("请求失败: {e}"));
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let _ = sqlx::query(
            "INSERT INTO debug_requests(node_id, model, system_prompt, tail_block, context_window, reserve_ratio, max_tokens, safety_margin, input_budget, history_json, user_text, estimated_tokens, n_used, status, error)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 'error', ?14)"
        )
        .bind(node_id).bind(&model).bind(&system).bind(&tail_block)
        .bind(ctx_window as i64).bind(reserve_ratio).bind(max_tokens as i64).bind(safety_margin as i64).bind(input_budget as i64)
        .bind(&history_json).bind(user_text)
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
        "INSERT INTO debug_requests(node_id, model, system_prompt, tail_block, context_window, reserve_ratio, max_tokens, safety_margin, input_budget, history_json, user_text, ai_reply, estimated_tokens, n_used, status)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'ok')"
    )
    .bind(node_id).bind(&model).bind(&system).bind(&tail_block)
    .bind(ctx_window as i64).bind(reserve_ratio).bind(max_tokens as i64).bind(safety_margin as i64).bind(input_budget as i64)
    .bind(&history_json).bind(user_text).bind(&full)
    .bind(total_tokens as i64).bind(chosen_n)
    .execute(&pool).await;

    let _ = app.emit("chat-done", serde_json::json!({"content": full}));
    Ok(())
}
