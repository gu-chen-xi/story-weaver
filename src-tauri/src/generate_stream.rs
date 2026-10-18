use serde_json::json;
use sqlx::{Row, SqlitePool};
use tauri::{Emitter, AppHandle};

use crate::db;
use crate::keychain;

/// 调试记录自动上限的配置键（条数）。默认 300。
pub const CFG_DEBUG_MAX_ROWS: &str = "debug.max_rows";

/// 读"调试记录最多留多少条"（读不到/非法 → 默认 300）。
pub async fn debug_max_rows(lib_pool: &SqlitePool) -> i64 {
    crate::db::get_config(lib_pool, CFG_DEBUG_MAX_ROWS)
        .await.ok().flatten()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|n| (10..=100_000).contains(n))
        .unwrap_or(300)
}

/// 只保留最新 `max_rows` 条调试记录（多出来的按 id 从旧到新删掉）。
///
/// 为什么需要（2026-10-18）：`debug_requests` 每条都存**完整 system_prompt +
/// history_json + user_text**。实测故事 7 才 177 条请求就占了 **780 KB**，
/// 且含完整设定/角色卡/剧情 —— 不设上限会一直涨到几十 MB。
/// 手动清理（数据管理页）只是治标，这里在**每次生成前**顺手清一次。
pub async fn prune_debug_requests(pool: &SqlitePool, max_rows: i64) -> Result<u64, sqlx::Error> {
    let n = sqlx::query(
        "DELETE FROM debug_requests WHERE id NOT IN \
         (SELECT id FROM debug_requests ORDER BY id DESC LIMIT ?1)",
    )
    .bind(max_rows.max(10))
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n)
}

/// 系统提示词（稳定前缀，跨轮不变，便于前缀缓存命中）
/// 顺序：基础指令 + 输出排版 → 数据字典 → 世界观 → 角色 → 方向和关系
/// 易变内容（当前模式 / 篇幅 / 数值与状态 / 聚焦 / 约束 / 时空 / 导演指令）不在这里，
/// 由 build_tail_block 拼成「上下文更新」块，插在历史之后（见调用处）
/// 第五批 ③：系统提示词的装配需要知道"当前在哪条分支" ——
/// 世界观 / 角色 / 关系这些**都要读该分支自己的版本**（主线 = NULL）。
async fn build_system_prompt(
    pool: &SqlitePool,
    branch: &Option<String>,
    time_origin: &str,
) -> Result<String, sqlx::Error> {
    let mut parts = Vec::new();
    parts.push("你是角色扮演引擎，依据当前模式工作：沉浸模式下以角色身份直接对话，旁白模式下以叙述者身份生成旁白。请用自然的文学语言继续剧情，必须自然包含角色的动作、心理、神态描写。不要输出 JSON。以角色卡设定为准；卡内未规定的细节可自由发挥，但不得改动已定义的性格、关系与世界观。\n【输出排版（供软件渲染，务必遵守）】\n- 人物台词：必须用双引号“”包住，并在台词前紧挨着写一个中括号，里面放说话人的角色名，格式如：[母亲]“你回吧。”。每条台词单独一行，台词与动作不要写在同一行。台词标记里写主名，正式回复的台词里可以用别名表示亲昵或者其他意思。\n- 动作/神态（非台词）：用（圆括号）括起、独占一行（如：（他攥紧了拳头））。连续动作时，一个动作一个括号单独包住并换行，不要挤在同一行。\n- 心理描写（只用沉浸式心声一类）：第一人称、现在时，写角色脑中真实冒出来的念头、自我说服、犹豫、口是心非；整行用星号包裹，格式如：*他怎么能这样……我分明没允许。*（软件会把星号渲染为心声样式）。语气必须贴合角色性格；心声须单独成行，不与动作、台词、旁白同行，不用中括号或引号。\n  硬性要求：角色口是心非时（尤其傲娇型），心声必须写出与台词相反或矛盾的真实想法，且角色自己未必肯承认——这是角色魅力的核心来源，不得省略、不得敷衍成旁白。\n- 旁白（叙述/环境描写）：只写场景、时间、气氛、外部事件，不进入任何人的内心；正常成段书写即可，不要用中括号或引号标记。\n- 除“台词必须带[角色名]标记”外，其余内容请保持自然发挥。".to_string());

    // 数据字典：关系强度档位 + 关系状态含义（两种「读法」，只在此处出现一次）
    parts.push(format!("\n【数据字典】\n{}\n{}", crate::relations::strength_legend(), crate::relations::state_legend()));

    // 【时间基准】—— 故事的时间锚点（用户 2026-10-17 要求加进系统提示词）。
    //
    // 为什么放在稳定前缀里：`time_origin` 建故事时定下、之后基本不动，
    // 而模型需要知道"第 N 天"对应现实里的哪一天，才能写对季节、节日、年月、年龄推进。
    // 不注入的话，模型只能看到"第 3 天"这种相对标签，无法推断绝对时间。
    //
    // 注意**只放锚点与换算规则**，不放"今天是第几天" —— 那会随节点变，
    // 一旦进了 system 就让前缀缓存失效（易变内容统一在尾部「上下文更新」块里）。
    parts.push(build_time_base_block(time_origin));

    // 世界观（稳定）—— 放在【数据字典】之后、【角色】之前
    let world_clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let world_sql = format!(
        "SELECT key, value, is_hard_rule, category, meta_json FROM world_settings WHERE {}",
        world_clause
    );
    let mut wq = sqlx::query(&world_sql);
    if let Some(b) = branch {
        wq = wq.bind(b);
    }
    let rows = wq.fetch_all(pool).await?;
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

    // 角色（稳定）—— 放在【世界观】之后。
    // 第五批 ③：读**当前分支**的角色（分支里角色是复制出的独立行，改动不会串到主线）
    let char_clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let char_sql = format!(
        "SELECT name, alias, role, faction, age, gender, one_liner, profile, speech_style, traits FROM characters WHERE {}",
        char_clause
    );
    let mut cq = sqlx::query(&char_sql);
    if let Some(b) = branch {
        cq = cq.bind(b);
    }
    let rows = cq.fetch_all(pool).await?;
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
    // 第五批 ③：关系与**两端角色**都限当前分支
    // （分支的关系指向分支自己的角色行；不限会让主线的角色名混进分支的提示词）
    let r_clause = crate::story_db::branch_clause("r.branch_id", branch, 1);
    let f_clause = crate::story_db::branch_clause("f.branch_id", branch, 2);
    let t_clause = crate::story_db::branch_clause("t.branch_id", branch, 3);
    let rel_sql = format!(
        "SELECT f.name, t.name, r.relation_type, r.direction, r.description, f.gender, f.age, t.gender, t.age
         FROM relations r
         JOIN characters f ON f.id = r.from_character_id AND {}
         JOIN characters t ON t.id = r.to_character_id AND {}
         WHERE {}",
        f_clause, t_clause, r_clause
    );
    let mut rq = sqlx::query(&rel_sql);
    if branch.is_some() {
        rq = rq.bind(branch).bind(branch).bind(branch);
    }
    let rows = rq.fetch_all(pool).await?;
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

/// B 子项：历史保留几天的原文（更早的日子只带日摘要）。3 = 当前节点 + 前 2 个天数节点。
pub(crate) const KEEP_DAYS: usize = 3;

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

/// B 子项：按天分配"每天取几条原文"，并在超预算时降级。
///
/// 规则（与 ROADMAP §6.2.1 一致）：
/// 1) 每日基准预算 = 历史区预算 ÷ 天数；理想 N = 每日基准 ÷ 单条均 token，再封顶到用户上限、且不超过当天实际条数；
/// 2) 每天至少保留 1 条（那天本来有消息的话）—— 否则"近三天"名存实亡；
/// 3) 超预算时先逐条降 N（下限 1）；仍超则从**最老的一天**开始整天移出原文区（返回要移除的天数，
///    调用方据此把那些天改用日摘要）。
///
/// 返回 `(每天条数上限, 需要移出原文区的天数, 是否发生过降条数, 单条均 token)`
pub(crate) fn plan_day_limits(
    day_msg_tokens: &[Vec<usize>],
    hist_budget: usize,
    per_day_cap: usize,
) -> (Vec<usize>, usize, bool, usize) {
    let days = day_msg_tokens.len();
    if days == 0 {
        return (Vec::new(), 0, false, 300);
    }
    let avg: usize = {
        let flat: Vec<usize> = day_msg_tokens.iter().flatten().copied().collect();
        if flat.is_empty() { 300 } else { (flat.iter().sum::<usize>() / flat.len()).max(1) }
    };
    let per_day_budget = hist_budget / days;
    let ideal = per_day_budget / avg;
    let mut limits: Vec<usize> = day_msg_tokens.iter()
        .map(|toks| ideal.min(per_day_cap.max(1)).min(toks.len()))
        .collect();
    // 每天至少 1 条（那天有消息的话）
    for (i, toks) in day_msg_tokens.iter().enumerate() {
        if !toks.is_empty() && limits[i] == 0 {
            limits[i] = 1;
        }
    }
    let sum = |lm: &[usize], ts: &[Vec<usize>]| -> usize {
        ts.iter().zip(lm.iter()).map(|(toks, n)| {
            let k = (*n).min(toks.len());
            toks[toks.len() - k..].iter().sum::<usize>()
        }).sum::<usize>()
    };
    let mut removed = 0usize;
    let mut shrunk = false;
    let mut active: Vec<Vec<usize>> = day_msg_tokens.to_vec();
    loop {
        if sum(&limits, &active) <= hist_budget {
            break;
        }
        if limits.iter().any(|n| *n > 1) {
            for n in limits.iter_mut() {
                if *n > 1 { *n -= 1; }
            }
            shrunk = true;
            continue;
        }
        if active.len() > 1 {
            active.remove(0);
            limits.remove(0);
            removed += 1;
            continue;
        }
        break;
    }
    (limits, removed, shrunk, avg)
}

/// 给每天的第一条消息加上「【第 N 天】」标记（用户 2026-10-11 要求：历史里要能看出哪几句属于哪一天）。
/// 只在每天**实际保留**的首条消息前插入标记，不额外增加消息条数、不破坏 user/assistant 交替。
pub(crate) fn mark_days(day_msgs: &[Vec<(String, String)>], day_n: &[usize], labels: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (i, msgs) in day_msgs.iter().enumerate() {
        let n = day_n.get(i).copied().unwrap_or(0);
        if n == 0 {
            continue;
        }
        let slice = if n >= msgs.len() { &msgs[..] } else { &msgs[msgs.len() - n..] };
        let label = labels.get(i).cloned().unwrap_or_default();
        for (k, (role, content)) in slice.iter().enumerate() {
            if k == 0 && !label.is_empty() {
                out.push((role.clone(), format!("【{}】\n{}", label, content)));
            } else {
                out.push((role.clone(), content.clone()));
            }
        }
    }
    out
}

/// 摘要区（【前情提要】）超预算时，从**最旧**的一条开始丢弃（保留最近的记忆）。
/// 返回（保留的摘要行, 被丢弃的条数）。
pub(crate) fn fit_recap(
    lines: &[(usize, String)],
    scaled_window: usize,
    out_reserve: usize,
    fixed_tokens: usize,
    tail_tokens: usize,
    user_input_tokens: usize,
) -> (Vec<(usize, String)>, usize) {
    let mut kept: Vec<(usize, String)> = lines.to_vec();
    let mut dropped = 0usize;
    let space = scaled_window
        .saturating_sub(out_reserve)
        .saturating_sub(fixed_tokens)
        .saturating_sub(tail_tokens)
        .saturating_sub(user_input_tokens);
    while !kept.is_empty() {
        let used: usize = kept.iter().map(|(_, s)| (s.chars().count() as f64 * 1.5).ceil() as usize).sum::<usize>() + 60;
        if used <= space {
            break;
        }
        kept.remove(0);
        dropped += 1;
    }
    (kept, dropped)
}

/// 【时间基准】块：把故事的时间起点写进系统提示词（稳定前缀）。
///
/// `time_origin` 支持两种写法：
/// * **日期**（`2026-03-01` / `2026/3/1` / `2026年3月1日`）→ 输出绝对起点 + 换算规则
/// * **非日期**（如「现代」「民国初年」）→ 作为"时代/纪元"描述输出
///
/// 空值时不输出任何内容（老故事可能没填）。
pub(crate) fn build_time_base_block(time_origin: &str) -> String {
    let raw = time_origin.trim();
    if raw.is_empty() {
        return String::new();
    }
    match parse_calendar_date(raw) {
        Some((y, m, d)) => format!(
            "\n【时间基准】\n- 第 1 天 = {:04}-{:02}-{:02}（绝对起点）；第 N 天 = 起点 + (N-1) 天。\n- 季节、节日、月份、年龄推进等一律以此换算，不要自行改动年份。\n- 只有切换到新的天数节点时时间才前进；同一天内的对话不跨日。",
            y, m, d
        ),
        None => format!(
            "\n【时间基准】\n- 故事的时代背景：{}（未给出具体日期，年月日可自由发挥但需前后一致）。\n- 只有切换到新的天数节点时时间才前进；同一天内的对话不跨日。",
            raw
        ),
    }
}

/// 解析 `YYYY-MM-DD` / `YYYY/MM/DD` / `YYYY.MM.DD` / `YYYY年M月D日`；非法返回 `None`。
pub(crate) fn parse_calendar_date(s: &str) -> Option<(i32, u32, u32)> {
    let t = s.trim();
    let sep = ['-', '/', '.', '年', '月'];
    let mut nums: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in t.chars() {
        if sep.contains(&ch) || ch == '日' {
            if !cur.is_empty() { nums.push(std::mem::take(&mut cur)); }
        } else if ch.is_ascii_digit() {
            cur.push(ch);
        } else {
            return None;
        }
    }
    if !cur.is_empty() { nums.push(cur); }
    if nums.len() != 3 { return None; }
    let y: i32 = nums[0].parse().ok()?;
    let m: u32 = nums[1].parse().ok()?;
    let d: u32 = nums[2].parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1000..=9999).contains(&y) {
        return None;
    }
    Some((y, m, d))
}

/// 用阶段摘要顶替它覆盖的那些"日摘要行"，压缩【前情提要】的 token 占用。
///
/// 规则（2026-10-17）：
/// * 输入 `day_lines` 按时间从旧到新，每项是 `(排序键, 天数, 行文本)`；
/// * `phases` 是 `(day_from, day_to, source_days, content)`（来自 `story_summaries`）；
/// * **最近 `keep_recent` 条日摘要不动**（保留最新细节；用户也可能刚在这几天里活动）；
/// * 更早的、被某段阶段摘要覆盖的连续日摘要 → 合并成**一行**阶段摘要，放在它原本的位置。
///
/// 这样永远不会比原来更占 token（一行 ≤ 它替换掉的多行之和），而且被压掉的记忆
/// 仍然以粗粒度的形式留在提示词里 —— 比"直接丢弃"强。
pub(crate) fn coalesce_with_phases(
    day_lines: &[(usize, i64, String)],
    phases: &[(i64, i64, i64, String)],
) -> Vec<(usize, String)> {
    if phases.is_empty() || day_lines.is_empty() {
        return day_lines.iter().map(|(i, _, s)| (*i, s.clone())).collect();
    }
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut idx = 0usize;
    while idx < day_lines.len() {
        let (sort_key, day_no, text) = &day_lines[idx];
        // 多条阶段摘要可能覆盖同一天（历史遗留 / 手动重压未清理干净）：
        // 取**区间最小**的那条（最具体），避免提示词里用了旧的大区间（用户实测踩到）。
        if let Some((from, to, _n, content)) = phases
            .iter()
            .filter(|(f, t, _, _)| *day_no >= *f && *day_no <= *t)
            .min_by_key(|(f, t, _, _)| (t - f).abs())
        {
            // 把**同一段阶段摘要覆盖的连续行**一次性收掉
            let mut end = idx;
            while end + 1 < day_lines.len() {
                let (_, d2, _) = &day_lines[end + 1];
                if *d2 >= *from && *d2 <= *to {
                    end += 1;
                } else {
                    break;
                }
            }
            let collapsed = end > idx;
            if collapsed {
                // 标签用**实际折叠的首尾天**：多条阶段摘要并存时，选中的那条可能比这一串更宽
                // （例如内容是"1~5 天"但当前只折了 3~5），直接写阶段区间会误导。
                let run_from = day_no;
                let run_to = &day_lines[end].1;
                let label = if run_from == from && *run_to == *to {
                    format!("第 {}~{} 天", from, to)
                } else {
                    format!("第 {}~{} 天（取自 {}~{} 天的阶段摘要）", run_from, run_to, from, to)
                };
                out.push((*sort_key, format!("- {}（阶段摘要）：{}", label, content.trim())));
                idx = end + 1;
                continue;
            }
        }
        out.push((*sort_key, text.clone()));
        idx += 1;
    }
    out
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
    fn recap_drops_oldest_when_over_budget() {
        let lines = vec![
            (0usize, "- 第 1 天：".to_string() + &"甲".repeat(100)),
            (1, "- 第 2 天：".to_string() + &"乙".repeat(100)),
            (2, "- 第 3 天：".to_string() + &"丙".repeat(100)),
        ];
        // 空间充足 → 一条不丢
        let (kept, dropped) = super::fit_recap(&lines, 100_000, 4800, 3000, 600, 200);
        assert_eq!(kept.len(), 3);
        assert_eq!(dropped, 0);

        // 空间只够 1 条 → 从最旧的开始丢
        let tight = 4800 + 3000 + 600 + 200 + 250; // 只剩约 250 token 给摘要
        let (kept2, dropped2) = super::fit_recap(&lines, tight, 4800, 3000, 600, 200);
        assert_eq!(dropped2, 2, "应丢掉最旧的两条");
        assert_eq!(kept2.len(), 1);
        assert!(kept2[0].1.contains("第 3 天"), "保留的应是最新的那条");
    }

    #[test]
    fn day_markers_one_per_day_on_first_kept_message() {
        let days = vec![
            vec![("user".to_string(), "第一天第一句".to_string()), ("assistant".to_string(), "第一天回复".to_string())],
            vec![("user".to_string(), "第二天第一句".to_string()), ("assistant".to_string(), "第二天回复".to_string())],
        ];
        let labels = vec!["第 1 天".to_string(), "第 2 天".to_string()];
        let out = super::mark_days(&days, &[2, 2], &labels);
        assert_eq!(out.len(), 4, "不应增加消息条数");
        assert_eq!(out[0].0, "user");
        assert!(out[0].1.starts_with("【第 1 天】"), "每天首条应带标记：{}", out[0].1);
        assert!(!out[1].1.contains("【第 1 天】"), "同一天的第二条不加标记");
        assert!(out[2].1.starts_with("【第 2 天】"), "第二天首条应带标记：{}", out[2].1);

        // 只保留每天最后 1 条时，标记应落在"实际保留的首条"上
        let out2 = super::mark_days(&days, &[1, 1], &labels);
        assert_eq!(out2.len(), 2);
        assert!(out2[0].1.starts_with("【第 1 天】"), "截取后标记应跟随保留的首条：{}", out2[0].1);
        assert!(out2[1].1.starts_with("【第 2 天】"));

        // 条数为 0 的天不产生任何消息
        let out3 = super::mark_days(&days, &[0, 2], &labels);
        assert_eq!(out3.len(), 2);
        assert!(out3[0].1.starts_with("【第 2 天】"), "被跳过的天不应产生标记");
    }

    #[test]
    fn day_limits_respect_cap_and_budget() {
        // 三天、每天 12 条、每条 100 token（共 3600），预算充足 → 封顶在用户上限 10
        let days = vec![vec![100usize; 12], vec![100; 12], vec![100; 12]];
        let (limits, removed, shrunk, _avg) = super::plan_day_limits(&days, 100_000, 10);
        assert_eq!(limits, vec![10, 10, 10], "应封顶到用户上限");
        assert_eq!(removed, 0);
        assert!(!shrunk);

        // 预算只够每天 ~3 条 → 理想 N=3000/3/100=10 与上限取小；这里把预算压到 900 → 每天 3 条
        let (limits2, removed2, _, _) = super::plan_day_limits(&days, 900, 10);
        assert_eq!(limits2, vec![3, 3, 3], "应按每日基准预算分配：{limits2:?}");
        assert_eq!(removed2, 0);

        // 条数不足时不超过当天实际条数
        let uneven = vec![vec![100usize; 2], vec![100; 12], vec![100; 5]];
        let (limits3, _, _, _) = super::plan_day_limits(&uneven, 100_000, 10);
        assert_eq!(limits3, vec![2, 10, 5], "不应超过当天实际条数");
    }

    #[test]
    fn day_limits_degrade_then_drop_oldest_day() {
        // 每天都只有 1 条、但每条 5000 token：预算 6000 → 先降到每天 1 条仍超 → 从最老的一天开始丢弃
        let days = vec![vec![5000usize], vec![5000], vec![5000]];
        let (limits, removed, shrunk, _) = super::plan_day_limits(&days, 6000, 10);
        assert!(removed >= 1, "应至少移出最老的一天（removed={removed}）");
        assert!(limits.len() <= 3);
        // 被移出的天不参与限额列表
        assert_eq!(limits.len(), days.len() - removed);
        let _ = shrunk;

        // 极端：预算为 0 → 只留最后一天，且限额不低于 1（保证当前天有上下文）
        let (limits2, removed2, _, _) = super::plan_day_limits(&days, 0, 10);
        assert_eq!(removed2, 2, "预算为 0 时应只剩最后一天");
        assert_eq!(limits2, vec![1], "最后一天至少留 1 条");
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
/// 请求**连接/首字节**超时（秒）。取值理由：真实网络下首字节通常 < 5s；
/// 超过 90s 说明上游连不上或已经挂了 —— 这时应该立刻报错，而不是让界面永远转圈。
const GEN_CONNECT_TIMEOUT_SECS: u64 = 90;
/// **流式空闲**超时（秒）：两个 chunk 之间最长允许的间隔。
/// 上游半开连接（网络中断、服务端静默不推）时，靠它把"永远不返回"变成"明确报错"。
const GEN_STREAM_IDLE_TIMEOUT_SECS: u64 = 60;

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
    // RAG ①：界面「🔍 回顾」指定的某一天（node_id）。有值时**强制**把那天拼进提示词。
    recall_day: Option<i64>,
    request_id: &str,
) -> Result<(), String> {
    // 第五批：登记取消标志 + **用一个守卫保证所有退出路径都会注销**
    // （早退/报错/取消都算）—— 否则取消表会一直涨。
    let cancel_flag = crate::gen_cancel::register(request_id);
    struct CancelGuard<'a>(&'a str);
    impl Drop for CancelGuard<'_> {
        fn drop(&mut self) {
            crate::gen_cancel::finish(self.0);
        }
    }
    let _cancel_guard = CancelGuard(request_id);
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;

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

    // A1（2026-10-18）：调试记录自动上限 —— 每次生成前先按配置清一次，
    // 避免 `debug_requests` 无限增长（每条都含完整提示词，实测 177 条就 780 KB）。
    let debug_max = debug_max_rows(lib_pool).await;
    if let Err(e) = prune_debug_requests(&pool, debug_max).await {
        eprintln!("[debug] 清理旧调试记录失败（不影响生成）：{e}");
    }

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
    // 第五批 ③：读该故事的当前分支，系统提示词按分支装配
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    // 故事的时间起点（library.db 的 stories.time_origin）：第 1 天 = 起点，第 N 天 = 起点 + (N-1) 天
    let time_origin: String = sqlx::query_scalar("SELECT time_origin FROM stories WHERE id = ?1")
        .bind(story_id)
        .fetch_optional(lib_pool).await.map_err(|e| e.to_string())?
        .unwrap_or_default();
    let system = build_system_prompt(&pool, &branch, &time_origin).await.map_err(|e| e.to_string())?;

    // 数值与状态（易变）：只取数字与状态，供尾部块使用。
    // 第五批 ③：同样限当前分支（分支里强度/状态是独立的一套）
    let v_r_clause = crate::story_db::branch_clause("r.branch_id", &branch, 1);
    let v_f_clause = crate::story_db::branch_clause("f.branch_id", &branch, 2);
    let v_t_clause = crate::story_db::branch_clause("t.branch_id", &branch, 3);
    let rel_sql = format!(
        "SELECT f.name, t.name, r.strength, r.state
         FROM relations r
         JOIN characters f ON f.id = r.from_character_id AND {}
         JOIN characters t ON t.id = r.to_character_id AND {}
         WHERE {}",
        v_f_clause, v_t_clause, v_r_clause
    );
    let mut rel_q = sqlx::query(&rel_sql);
    if branch.is_some() {
        rel_q = rel_q.bind(&branch).bind(&branch).bind(&branch);
    }
    let rel_rows = rel_q.fetch_all(&pool).await.map_err(|e| e.to_string())?;
    let rel_values: Vec<String> = rel_rows.iter().filter_map(|r| {
        let from: String = r.try_get(0).ok()?;
        let to: String = r.try_get(1).ok()?;
        let strength: i64 = r.try_get(2).ok()?;
        let state: String = r.try_get(3).ok()?;
        Some(crate::relations::rel_value_line(&from, &to, strength, &state))
    }).collect();

    let tail_block = build_tail_block(mode, my_role, ai_role, focus_role, constraints, length_level, &node_type, is_flashback, director.as_deref(), &rel_values);
    let user_text: &str = &user_text_clean;

    // ===== B 子项：跨天历史装配（近 KEEP_DAYS 天原文 + 更早的日摘要）=====
    // 1) 时间线上的"天数节点"顺序（插曲/倒叙不算天数），定位当前节点，取它及其之前的 KEEP_DAYS 个
    let day_nodes: Vec<(i64, String)> = {
        let rows = sqlx::query(
            "SELECT id, date_label FROM timeline_nodes
             WHERE node_type IN ('day', 'main') AND is_flashback = 0
             ORDER BY sort_order ASC, id ASC"
        ).fetch_all(&pool).await.map_err(|e| e.to_string())?;
        rows.iter().map(|r| (r.try_get(0).unwrap_or(0), r.try_get(1).unwrap_or_default())).collect()
    };
    // 1.5) 按**父链**（而不是"全表位置"）确定"近 KEEP_DAYS 天"。
    //
    // 为什么必须按父链：线性故事里两者结果**完全一致**（父链上的天数节点 = sort_order 在它之前的天数节点），
    // 所以这次改动对现有故事零影响；但有了分支之后，"全表位置"会把**其它分支**的天也读进来
    // （那些分支的 sort_order 可能就排在当前节点前面），串味。第 ④ 步做分支时这一步是关键前提。
    //
    // 数据异常兜底：父链走不通（缺 parent 且不是根 / 成环）时退回原来的"按位置"算法，
    // 保证"无论如何都不会因为迁移问题而改变生成内容"。
    let chain_days: Option<Vec<(i64, String)>> = {
        let tree = crate::timeline::load_tree_nodes(&story_id).await.unwrap_or_default();
        if tree.is_empty() {
            None
        } else {
            let chain = crate::timeline::ancestor_ids(&tree, node_id);
            let resolved = !chain.is_empty()
                && chain.iter().any(|id| *id == node_id)
                // 走到的链应当是"连通的"：链长度 == 站在链尾时向上能走到的节点数（由纯函数保证）
                && tree.iter().filter(|n| chain.contains(&n.id)).count() == chain.len();
            if resolved {
                let days: Vec<(i64, String)> = chain
                    .iter()
                    .filter_map(|id| day_nodes.iter().find(|(i3, _)| i3 == id).cloned())
                    .collect();
                Some(days)
            } else {
                None
            }
        }
    };
    let cur_pos = day_nodes.iter().position(|(id, _)| *id == node_id);
    let mut keep_nodes: Vec<(i64, String)> = match (&chain_days, cur_pos) {
        // 父链可用：取链上最近 KEEP_DAYS 个天数节点
        (Some(days), _) if !days.is_empty() => {
            let from = days.len().saturating_sub(KEEP_DAYS);
            days[from..].to_vec()
        }
        // 兜底（老库未迁移 / 数据异常）：退回按位置取
        (None, Some(p0)) => {
            let from = p0.saturating_sub(KEEP_DAYS - 1);
            day_nodes[from..=p0].to_vec()
        }
        // 当前节点不是天数节点（插曲 / 倒叙）→ 只带它自己
        _ => vec![(node_id, "当前时段".to_string())],
    };

    // 2) 每天的原文消息（已剥导演指令标记）
    let mut day_msgs: Vec<Vec<(String, String)>> = Vec::new();
    if memory {
        for (nid, _) in &keep_nodes {
            let rows = sqlx::query("SELECT role, content FROM messages WHERE node_id = ?1 ORDER BY id ASC")
                .bind(nid).fetch_all(&pool).await.map_err(|e| e.to_string())?;
            let msgs: Vec<(String, String)> = rows.iter().map(|r| {
                let role: String = r.try_get(0).unwrap_or_default();
                let content: String = r.try_get(1).unwrap_or_default();
                // 导演指令标记剥掉：指令只在当次的尾部块里出现一次（否则既重复又会被当成台词读）
                (role, crate::relations::strip_director(&content))
            }).collect();
            day_msgs.push(msgs);
        }
    } else {
        for _ in &keep_nodes {
            day_msgs.push(Vec::new());
        }
    }
    // 前端调用生成前已把本次用户消息存库（存的是带导演指令标记的原文），
    // 而历史读出来时已剥过标记 —— 必须拿剥过标记的正文比对，
    // 否则会误判"末尾还没有本次输入"，把同一条输入再追加一遍（请求里出现两条一样的 user）。
    if let Some(last) = day_msgs.last_mut() {
        let need_append = !last.last().map(|(role, c)| role == "user" && c == user_text).unwrap_or(false);
        if need_append {
            last.push(("user".to_string(), user_text.to_string()));
        }
    }

    // 3) 【前情提要】：更早的天（以及后面被降级踢出原文区的天）的日摘要。
    //    注意：这一块会随天数增长，所以必须计入预算（不能当成"固定成本"）。
    // 更早的天：**只取父链上的**（分支后不能把别的分支的天摘要读进来）。
    // 父链不可用时退回原算法（全表中排在 earliest_kept 之前的天数节点）。
    //
    // keep_nodes 的首个节点在"全表天数序列"里的下标 —— 下游算降级天数时用它
    // 判断"被踢出原文区的那些天"的摘要要不要进【前情提要】。
    // 必须定义在分支**之外**：两个分支产生的 recap 排序键都是全表下标，下游要统一比较。
    let earliest_kept = keep_nodes.first().and_then(|(id, _)| day_nodes.iter().position(|(i3, _)| i3 == id));
    let recap_day_list: Vec<(usize, i64, String)> = if let Some(days) = &chain_days {
        // 链上更早的天：链上的天数节点里，排除掉 keep_nodes 已带的那些。
        //
        // 注意排序键：必须用**在 day_nodes（全表天数序列）里的下标**，而不是"链上的相对下标" ——
        // 下游算降级天数时是拿它和 earliest_kept（全表下标）比较的，两套坐标系混用会算错。
        let kept_ids: Vec<i64> = keep_nodes.iter().map(|(id, _)| *id).collect();
        days.iter()
            .filter(|(id, _)| !kept_ids.contains(id))
            .filter_map(|(id, label)| {
                day_nodes
                    .iter()
                    .position(|(i3, _)| i3 == id)
                    .map(|idx| (idx, *id, label.clone()))
            })
            .collect()
    } else {
        match earliest_kept {
            Some(earliest) => day_nodes
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx < earliest)
                .map(|(idx, (id, label))| (idx, *id, label.clone()))
                .collect(),
            None => Vec::new(),
        }
    };
    let recap_lines_all: Vec<(usize, String)> = {
        let mut v = Vec::new();
        if memory {
            {
                for (idx, nid, label) in recap_day_list.iter() {
                    let nid = *nid;
                    let label = label.clone();
                    let idx = *idx;
                    let s: String = sqlx::query_scalar("SELECT COALESCE(day_summary, '') FROM timeline_nodes WHERE id = ?1")
                        .bind(nid).fetch_optional(&pool).await.map_err(|e| e.to_string())?.unwrap_or_default();
                    v.push((idx, if s.trim().is_empty() {
                        format!("- {}：（这一天还没有摘要）", label)
                    } else {
                        format!("- {}：{}", label, s.trim())
                    }));
                }
            }
        }
        v
    };
    // 3.5) 阶段摘要（C 层）：把"更早的、已被某段阶段摘要覆盖"的日摘要行合并成一行。
    //      放在 fit_recap 之前 —— 先压缩、再按预算裁剪，就能少丢东西。
    //      「最近几天」在**生成阶段**就已排除（timeline::PHASE_KEEP_RECENT_DAYS），
    //      所以这里的阶段区间与保留的日摘要细节**不会重叠**（否则提示词里会重复）。
    let recap_lines_all: Vec<(usize, String)> = {
        let phases = crate::timeline::list_phase_summaries_in(&pool, &branch).await.unwrap_or_default();
        if phases.is_empty() {
            recap_lines_all
        } else {
            let with_day: Vec<(usize, i64, String)> = recap_lines_all
                .iter()
                .map(|(i, s)| {
                    // 行格式是 "- 第 N 天：摘要"，从标签里取天数
                    let day = s
                        .strip_prefix("- ")
                        .and_then(|rest| rest.split('：').next())
                        .and_then(crate::timeline::parse_day_number)
                        .unwrap_or(0);
                    (*i, day, s.clone())
                })
                .collect();
            coalesce_with_phases(&with_day, &phases)
        }
    };
    let recap_block = |lines: &[(usize, String)]| -> String {
        if lines.is_empty() { String::new() } else {
            format!("\n\n【前情提要】（更早的剧情摘要，用于延续前文；具体细节以历史对话为准）\n{}",
                lines.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>().join("\n"))
        }
    };
    let system_with = |extra: &str| -> String { format!("{}{}", system, extra) };

    // 4) 动态 N 值（用户 2026-10-11 定的算法：**不要把 N 写死，由预算倒推**）
    //    历史可用预算 = 窗口 × 安全系数 − 输出预留 − 固定区(实测) − 摘要区(实测) − 尾部块 − 本次输入预估
    //    每日基准预算 = 历史可用预算 ÷ 保留天数
    //    N = 每日基准预算 ÷ 单条消息平均 token
    let out_reserve = max_tokens + safety_margin;
    let user_input_tokens = estimate_tokens(user_text);
    let tail_tokens_now = estimate_tokens(&tail_block);
    let fixed_tokens_now = estimate_tokens(&system);           // 基础指令/输出排版/数据字典/世界观/角色/方向和关系
    let recap_tokens_now = estimate_tokens(&recap_block(&recap_lines_all));
    let scaled_window = ((ctx_window as f64) * reserve_ratio).round() as usize;
    // 历史可用预算：先按"全部更早的天都进摘要"算一次，得到初始 N
    let budget_all_recap = scaled_window
        .saturating_sub(out_reserve)
        .saturating_sub(fixed_tokens_now)
        .saturating_sub(recap_tokens_now)
        .saturating_sub(tail_tokens_now)
        .saturating_sub(user_input_tokens);

    let day_msg_tokens: Vec<Vec<usize>> = day_msgs.iter()
        .map(|msgs| msgs.iter().map(|(_, c)| estimate_tokens(c)).collect())
        .collect();
    let (day_n, downgraded_days, cut_n, avg_msg_tokens) = plan_day_limits(&day_msg_tokens, budget_all_recap, n_recent.max(1) as usize);
    // 被降级移出原文区的天（最老的若干天）不再出现在 keep_nodes 里，其摘要会进【前情提要】
    for _ in 0..downgraded_days {
        if day_msgs.len() > 1 {
            day_msgs.remove(0);
            keep_nodes.remove(0);
        }
    }
    // 降级后重算一次：被踢出原文区的天，改由【前情提要】承载（预算仍要装得下）
    let recap_lines: Vec<(usize, String)> = recap_lines_all.iter()
        .filter(|(idx, _)| earliest_kept.map(|e| *idx < e + downgraded_days).unwrap_or(false))
        .cloned().collect();
    // ===== RAG ①：回顾检索（方案 A）=====
    // 先算检索块，再让【前情提要】按"给它留出空间"重新裁剪。
    // 优先级（文档 9.3）：近三天原文 > 检索到的旧日记忆 > 更早的常规摘要。
    let recall_hits: Vec<crate::recall::RecallHit> = if memory {
        let intent = crate::recall::detect_intent(user_text);
        // 两条触发路径：界面指定了某天（强制） / 输入被判为回忆意图（自动）
        if recall_day.is_some() || intent.is_recall {
            let exclude: std::collections::HashSet<i64> =
                keep_nodes.iter().map(|(id, _)| *id).collect();
            // 单天原文上限：约 1600 字（≈800 token）。回顾要的是"当天发生了什么"，
            // 太长的天从尾部截断并注明，避免把三天原文挤掉。
            crate::recall::search(&pool, &branch, node_id, &exclude, &intent, recall_day, 3, 1600,
            &crate::recall::load_weights(lib_pool).await)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        }
    } else {
        Vec::new() // memory=false：不检索、不注入
    };
    // 检索块是**可变成本**。注意：注入的是**当天原始对话**（比摘要大得多），
    // 所以上限放宽到输入预算的 1/4（且不超过 4000 token）——
    // 仍然只占一小部分，不会挤压近三天原文（那个优先级最高）。
    let recall_budget = (scaled_window / 4).min(4000);
    let (recall_hits, _dropped_recall) =
        crate::recall::fit_hits(recall_hits, recall_budget, estimate_tokens);
    let recall_block = crate::recall::render_block(&recall_hits);
    let recall_tokens_now = estimate_tokens(&recall_block);

    // 规则：摘要区放不下的更早摘要从**最旧**开始丢弃（保证最近的记忆一定在）
    // 注意这里把 recall 的 token 计入固定成本，于是【前情提要】会自动让位给检索块
    let (recap_lines, dropped_recap) = fit_recap(
        &recap_lines,
        scaled_window,
        out_reserve,
        fixed_tokens_now + recall_tokens_now,
        tail_tokens_now,
        user_input_tokens,
    );
    let recap_extra = recap_block(&recap_lines);
    let system = system_with(&recap_extra);
    // 【相关旧日记忆】紧接在【前情提要】之后（独立小区块，便于调试时区分来源）
    let system = if recall_block.is_empty() {
        system
    } else {
        format!("{system}{recall_block}")
    };

    // 5) 组装最终历史（按天顺序），每天首条加「【第 N 天】」标记
    let labels: Vec<String> = keep_nodes.iter().map(|(_, l)| l.clone()).collect();
    let chosen_history = mark_days(&day_msgs, &day_n, &labels);
    let chosen_n = chosen_history.len() as i64;

    // 展示用：本次生效的每日 N（同一天数下取最大，便于界面上写"现在的 N"）
    let n_value = day_n.iter().max().copied().unwrap_or(0) as i64;
    let day_window = day_msgs.len();
    let day_n_str = day_n.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",");
    let system_tokens = estimate_tokens(&system) + tail_tokens_now;

    if downgraded_days > 0 || cut_n || dropped_recap > 0 {
        let _ = app.emit("chat-info", serde_json::json!({"requestId": request_id, "detail": format!(
            "上下文预算不足：原文保留 {} 天 × 每日 {} 条{}",
            day_window,
            n_value,
            if downgraded_days > 0 { format!("，{} 天已改用摘要", downgraded_days) } else { String::new() }
        )}));
    }

    // 组装 messages：system（含【前情提要】）→ 近三天原文（带天数标记）→ 本次用户消息（尾部块 + 用户输入）
    // 块与输入合成一条 user：避免出现"两条连续 user"导致模型分不清哪段是台词；
    // 块本身不落库、每轮现拼，历史里只有用户的原文。
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
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN day_window INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN day_n TEXT NOT NULL DEFAULT ''").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN n_value INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN avg_msg_tokens INTEGER NOT NULL DEFAULT 0").execute(&pool).await;

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

    // 第五批（《现状说明》5.2）：**主生成链路原来完全没有超时** ——
    // 上游半开时请求可以永远不返回，界面永远停在"正在生成"。
    // 现在：连接超时 + 整体超时（用 timeout 包住 send）。
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(GEN_CONNECT_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(GEN_CONNECT_TIMEOUT_SECS),
        client.post(&url).bearer_auth(&api_key).json(&body).send(),
    )
    .await
    .map_err(|_| format!("请求超时（{GEN_CONNECT_TIMEOUT_SECS}s 内没有响应）—— 已中止，可重试"))?;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            let _ = sqlx::query(
                "INSERT INTO debug_requests(node_id, model, system_prompt, tail_block, context_window, reserve_ratio, max_tokens, safety_margin, input_budget, day_window, day_n, n_value, avg_msg_tokens, history_json, user_text, estimated_tokens, n_used, status, error)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, 'error', ?18)"
            )
            .bind(node_id).bind(&model).bind(&system).bind(&tail_block)
            .bind(ctx_window as i64).bind(reserve_ratio).bind(max_tokens as i64).bind(safety_margin as i64).bind(input_budget as i64)
            .bind(day_window as i64).bind(&day_n_str).bind(n_value).bind(avg_msg_tokens as i64)
            .bind(&history_json).bind(user_text)
            .bind(total_tokens as i64).bind(chosen_n).bind(format!("请求失败: {e}"))
            .execute(&pool).await;
            // **必须发事件**：前端在 done/error 时才注销监听器；旧代码这条路径直接返回，
            // 于是监听器永久泄漏、下次发送会重复正文（《现状说明》5.3）。
            let _ = app.emit("chat-error", serde_json::json!({"requestId": request_id, "detail": format!("请求失败: {e}")}));
            return Err(format!("请求失败: {e}"));
        }
    };

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let _ = sqlx::query(
            "INSERT INTO debug_requests(node_id, model, system_prompt, tail_block, context_window, reserve_ratio, max_tokens, safety_margin, input_budget, day_window, day_n, n_value, avg_msg_tokens, history_json, user_text, estimated_tokens, n_used, status, error)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, 'error', ?18)"
        )
        .bind(node_id).bind(&model).bind(&system).bind(&tail_block)
        .bind(ctx_window as i64).bind(reserve_ratio).bind(max_tokens as i64).bind(safety_margin as i64).bind(input_budget as i64)
        .bind(day_window as i64).bind(&day_n_str).bind(n_value).bind(avg_msg_tokens as i64)
        .bind(&history_json).bind(user_text)
        .bind(total_tokens as i64).bind(chosen_n).bind(format!("HTTP {}: {}", status.as_u16(), text.chars().take(500).collect::<String>()))
        .execute(&pool).await;
        let _ = app.emit("chat-error", serde_json::json!({"requestId": request_id, "detail": format!("HTTP {}: {}", status.as_u16(), text.chars().take(300).collect::<String>())}));
        return Err(format!("HTTP {}", status.as_u16()));
    }

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut full = String::new();
    let mut buf: Vec<u8> = Vec::new();

    // 第五批：这里开始记录"为什么中止" —— 读失败 / 取消都不再直接 return，
    // 而是把**已经收到的部分**保存下来并明确标记（旧行为：读到一半断开会被当成完整回复入库，
    // 用户拿到被截断的正文却没有任何提示 —— 《现状说明》5.2）。
    let mut stream_err: Option<String> = None;
    let mut cancelled = false;

    loop {
        // ① **取消检查**（每收到一个 chunk 检查一次；最坏延迟 = 空闲超时时间）
        if crate::gen_cancel::is_cancelled(&cancel_flag) {
            cancelled = true;
            break;
        }
        // ② **空闲超时**：上游半开时把"永远不返回"变成明确报错
        let next = match tokio::time::timeout(
            std::time::Duration::from_secs(GEN_STREAM_IDLE_TIMEOUT_SECS),
            stream.next(),
        )
        .await
        {
            Ok(v) => v,
            Err(_) => {
                stream_err = Some(format!(
                    "流式读取空闲超时（{GEN_STREAM_IDLE_TIMEOUT_SECS}s 没有收到数据）"
                ));
                break;
            }
        };
        let Some(chunk) = next else { break };   // 流正常结束
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                stream_err = Some(format!("读流失败: {e}"));
                break;
            }
        };
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
                                        let _ = app.emit("chat-token", serde_json::json!({"requestId": request_id, "delta": delta}));
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
                                let _ = app.emit("chat-token", serde_json::json!({"requestId": request_id, "delta": delta}));
                            }
                        }
                    }
                }
            }
        }
    }

    // ===== 第五批：中止（取消 / 读失败 / 空闲超时）的处理 =====
    // 三种情况都做到：① 给前端**明确的事件**（否则界面永远转圈且监听器不注销）
    //              ② 已经收到的部分**存下来并标记**（不丢内容、也不冒充完整回复）
    if cancelled || stream_err.is_some() {
        let why = if cancelled {
            "已被用户停止".to_string()
        } else {
            stream_err.clone().unwrap_or_else(|| "未知原因".to_string())
        };
        let partial = full.trim().to_string();
        if !partial.is_empty() {
            let marked = format!("{full}\n\n（⚠ 本段生成未完成：{why}。以上为已生成的部分）");
            let msg_branch: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
                .bind(node_id).fetch_optional(&pool).await.ok().flatten().flatten();
            let _ = sqlx::query("INSERT INTO messages(node_id, role, content, mode, branch_id) VALUES(?1, 'assistant', ?2, ?3, ?4)")
                .bind(node_id).bind(&marked).bind(mode).bind(msg_branch).execute(&pool).await;
        }
        if cancelled {
            let _ = app.emit("chat-cancelled", serde_json::json!({"requestId": request_id, "detail": why, "partial": partial}));
        } else {
            let _ = app.emit("chat-error", serde_json::json!({"requestId": request_id, "detail": why, "partial": partial}));
        }
        // 记一条失败快照，便于事后排查（失败不影响主流程）
        let _ = sqlx::query(
            "INSERT INTO debug_requests(node_id, model, system_prompt, tail_block, context_window, reserve_ratio, max_tokens, safety_margin, input_budget, day_window, day_n, n_value, avg_msg_tokens, history_json, user_text, ai_reply, estimated_tokens, n_used, status, error)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 'error', ?19)"
        )
        .bind(node_id).bind(&model).bind(&system).bind(&tail_block)
        .bind(ctx_window as i64).bind(reserve_ratio).bind(max_tokens as i64).bind(safety_margin as i64).bind(input_budget as i64)
        .bind(day_window as i64).bind(&day_n_str).bind(n_value).bind(avg_msg_tokens as i64)
        .bind(&history_json).bind(user_text).bind(&full)
        .bind(total_tokens as i64).bind(chosen_n).bind(&why)
        .execute(&pool).await;
        return Err(why);
    }

    if full.is_empty() {
        let _ = app.emit("chat-error", serde_json::json!({"requestId": request_id, "detail": "AI 返回空内容"}));
        return Err("AI 返回空内容".to_string());
    }

    // 存 AI 回复（第五批 ③：消息带上节点所属分支，保证与节点一致）
    let msg_branch: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .flatten();
    sqlx::query("INSERT INTO messages(node_id, role, content, mode, branch_id) VALUES(?1, 'assistant', ?2, ?3, ?4)")
        .bind(node_id).bind(&full).bind(mode).bind(msg_branch).execute(&pool).await.map_err(|e| e.to_string())?;

    // M5: 自动提取角色状态并更新状态卡（失败不影响主流程）
    let _ = crate::state_extract::auto_update_after_reply(&pool, story_id, node_id).await;

    // 记录成功快照
    let _ = sqlx::query(
        "INSERT INTO debug_requests(node_id, model, system_prompt, tail_block, context_window, reserve_ratio, max_tokens, safety_margin, input_budget, day_window, day_n, n_value, avg_msg_tokens, history_json, user_text, ai_reply, estimated_tokens, n_used, status)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 'ok')"
    )
    .bind(node_id).bind(&model).bind(&system).bind(&tail_block)
    .bind(ctx_window as i64).bind(reserve_ratio).bind(max_tokens as i64).bind(safety_margin as i64).bind(input_budget as i64)
    .bind(day_window as i64).bind(&day_n_str).bind(n_value).bind(avg_msg_tokens as i64)
    .bind(&history_json).bind(user_text).bind(&full)
    .bind(total_tokens as i64).bind(chosen_n)
    .execute(&pool).await;

    let _ = app.emit("chat-done", serde_json::json!({"requestId": request_id, "content": full}));
    Ok(())
}

/// 第五批「主生成链路」修复（《现状说明》5.2 / 5.3）的**可执行守护测试**。
///
/// 真实流式链路需要上游服务，跑不了单测。但这次改动里最容易"改回去"的是三件事，
/// 它们**可以在源码层面守护** —— 用 `include_str!` 读自己，断言关键写法还在。
/// 这不算优雅，但比"靠人记住"强得多（这类"防护栏"测试在 §5.2 里正是缺失的一环）。
#[cfg(test)]
mod stream_guards {
    const SRC: &str = include_str!("generate_stream.rs");

    /// 超时常量必须存在且被真正使用（不能只定义不用）
    #[test]
    fn timeouts_are_defined_and_used() {
        assert!(SRC.contains("GEN_CONNECT_TIMEOUT_SECS"), "缺少连接/首字节超时常量");
        assert!(SRC.contains("GEN_STREAM_IDLE_TIMEOUT_SECS"), "缺少流式空闲超时常量");
        // send 必须被 timeout 包住
        let send_at = SRC.find(".send(),").or_else(|| SRC.find(".send()")).expect("找不到 send()");
        let before = &SRC[..send_at];
        assert!(
            before.rfind("tokio::time::timeout").map(|i| send_at - i < 800).unwrap_or(false),
            "请求发送必须包在 tokio::time::timeout 里（否则上游半开时永远不返回）"
        );
        // 客户端必须带 connect_timeout
        assert!(SRC.contains(".connect_timeout("), "HTTP 客户端必须设置 connect_timeout");
        // 注意：这个字符串**不能直接字面量写在本测试里** —— 否则断言会匹配到自己的源码。
        // 用拼接构造出待查串，再按行扫（注释行除外）。
        let forbidden = ["reqwest::Client", "::new()"].concat();
        for (i, line) in SRC.lines().enumerate() {
            let code = line.trim();
            if code.starts_with("//") {
                continue;
            }
            // 拼接过的 needle 不会出现在本行自身，所以不会自匹配
            assert!(
                !code.contains(&forbidden),
                "第 {} 行又用回了无超时的客户端构造：{code}",
                i + 1
            );
        }
    }

    /// 循环里必须有取消检查 + 空闲超时
    #[test]
    fn stream_loop_checks_cancel_and_idle_timeout() {
        assert!(SRC.contains("gen_cancel::is_cancelled"), "流式循环缺少取消检查");
        assert!(SRC.contains("GEN_STREAM_IDLE_TIMEOUT_SECS"), "流式循环缺少空闲超时引用");
        assert!(SRC.contains("crate::gen_cancel::register"), "生成开始必须登记取消标志");
        assert!(SRC.contains("crate::gen_cancel::finish"), "生成结束必须注销取消标志");
    }

    /// 失败路径必须发事件（否则前端监听器永不注销 —— §5.3）
    #[test]
    fn failure_paths_emit_events() {
        for ev in ["chat-error", "chat-done", "chat-cancelled"] {
            assert!(SRC.contains(ev), "缺少事件 {ev}（前端要靠它注销监听器）");
        }
        // 请求失败分支（send 之前那次 return）必须也发 chat-error
        let fail_at = SRC.find("请求失败: {e}").expect("找不到请求失败分支");
        let tail = &SRC[fail_at..(fail_at + 900).min(SRC.len())];
        assert!(tail.contains("chat-error"), "send 失败的分支必须发 chat-error 事件");
    }

    /// 读到一半失败/被取消时：必须**保留已收到的部分并标记**，不能当成完整回复
    #[test]
    fn partial_stream_keeps_and_marks_content() {
        assert!(SRC.contains("stream_err"), "缺少读流错误的记录变量");
        assert!(SRC.contains("cancelled"), "缺少取消标记");
        assert!(
            SRC.contains("本段生成未完成"),
            "部分内容入库时必须带明确标记（否则用户拿到被截断的正文却不知道）"
        );
    }

    /// 第五批（《现状说明》5.3）：**每个流式事件都必须带 requestId**。
    ///
    /// 原来的事件是全局广播、不带请求编号 —— 多窗口 / 旧监听器残留 / 连发两次时，
    /// 同一批增量会被多个气泡同时追加（表现为"正文重复 / 串行"）。
    /// 这条守着"以后新增事件别忘了带编号"。
    #[test]
    fn every_emit_carries_request_id() {
        // **只扫生产代码**（本测试模块之前的部分）—— 否则断言里那个字面量会匹配到自己。
        let prod_end = SRC.find("mod stream_guards").expect("找不到 stream_guards 模块起点");
        let prod = &SRC[..prod_end];
        let all: Vec<&str> = prod.lines().collect();
        let mut checked = 0;
        for (i, line) in all.iter().enumerate() {
            if !line.contains(".emit(") {
                continue;
            }
            checked += 1;
            let seg: String = all[i..(i + 5).min(all.len())].join("\n");
            assert!(
                seg.contains("requestId"),
                "第 {} 行的 emit 没有带 requestId：\n{seg}",
                i + 1
            );
        }
        assert!(checked >= 8, "应至少检查到 8 处 emit，实际 {checked}");
    }
}

#[cfg(test)]
mod phase_recap_tests {
    use super::{coalesce_with_phases, build_time_base_block, parse_calendar_date};

    fn day(i: usize, d: i64) -> (usize, i64, String) {
        (i, d, format!("- 第 {} 天：第{}天的摘要内容", d, d))
    }

    /// 被阶段摘要覆盖的"老"日摘要 → 合并成一行；最近 3 天保持原样
    #[test]
    fn old_days_collapse_into_one_phase_line() {
        // 生产情况：阶段只覆盖"更早的那一坨"（1~9），最近 3 天（10/11/12）不覆盖
        let days: Vec<(usize, i64, String)> = (1..=12).map(|d| day(d as usize, d)).collect();
        let phases = vec![(1i64, 9i64, 9i64, "阶段摘要正文".to_string())];
        let out = coalesce_with_phases(&days, &phases);

        assert_eq!(out.len(), 4, "应为 1 行阶段摘要 + 最近 3 天：{:?}", out.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>());
        assert!(out[0].1.contains("第 1~9 天（阶段摘要）"), "第一行应是阶段摘要：{}", out[0].1);
        assert!(out[1].1.contains("第 10 天") && out[3].1.contains("第 12 天"), "最近 3 天应保留日摘要原文");

        // 合并后必须更省 token（否则不该合并）
        let before: usize = days.iter().map(|(_, _, s)| s.chars().count()).sum();
        let after: usize = out.iter().map(|(_, s)| s.chars().count()).sum();
        assert!(after < before, "合并后应更短：{after} vs {before}");
    }

    /// 没有阶段摘要 → 原样返回
    #[test]
    fn no_phases_keeps_everything() {
        let days: Vec<(usize, i64, String)> = (1..=5).map(|d| day(d as usize, d)).collect();
        let out = coalesce_with_phases(&days, &[]);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].1, days[0].2);
    }

    /// 阶段摘要覆盖不到这些天 → 原样返回（不能误伤）
    #[test]
    fn non_covered_days_are_untouched() {
        let days: Vec<(usize, i64, String)> = (20..=25).map(|d| day(d as usize, d)).collect();
        let phases = vec![(1i64, 12i64, 10i64, "x".to_string())];
        let out = coalesce_with_phases(&days, &phases);
        assert_eq!(out.len(), 6, "覆盖区间是 1~12，20~25 不该被动");
    }

    /// 只有一两天被覆盖时不合并（合并成一行的收益还不如留原文，且"一堆阶段行"更难读）
    #[test]
    fn single_covered_day_is_not_collapsed() {
        let days: Vec<(usize, i64, String)> = (1..=6).map(|d| day(d as usize, d)).collect();
        let phases = vec![(1i64, 1i64, 1i64, "x".to_string())];
        let out = coalesce_with_phases(&days, &phases);
        assert_eq!(out.len(), 6, "只覆盖第 1 天，不值得合并");
    }

    /// 时间基准：日期写法 → 输出绝对起点
    #[test]
    fn time_base_with_calendar_date() {
        let s = build_time_base_block("2026-03-01");
        assert!(s.contains("【时间基准】"), "应有小节标题：{s}");
        assert!(s.contains("第 1 天 = 2026-03-01"), "应含绝对起点：{s}");
        assert!(s.contains("起点 + (N-1) 天"), "应含换算规则：{s}");
    }

    /// 时间基准：中文日期写法也要认
    #[test]
    fn time_base_accepts_chinese_date() {
        assert_eq!(parse_calendar_date("2026年3月1日"), Some((2026, 3, 1)));
        assert_eq!(parse_calendar_date("2026/3/1"), Some((2026, 3, 1)));
        assert_eq!(parse_calendar_date("2026.03.01"), Some((2026, 3, 1)));
    }

    /// 时间基准：非日期 → 当作"时代背景"，不能崩、也不能编造年份
    #[test]
    fn time_base_with_era_text() {
        let s = build_time_base_block("民国初年");
        assert!(s.contains("民国初年"), "应原样带出时代描述：{s}");
        assert!(!s.contains("第 1 天 ="), "非日期时不该编造绝对日期：{s}");
    }

    /// 时间基准：空字符串 → 什么都不输出（老故事可能没填）
    #[test]
    fn time_base_empty_outputs_nothing() {
        assert_eq!(build_time_base_block(""), "");
        assert_eq!(build_time_base_block("   "), "");
    }

    /// 非法日期（月/日越界）→ 退化成"时代背景"，不静默算错
    #[test]
    fn invalid_date_falls_back_to_era() {
        assert_eq!(parse_calendar_date("2026-13-40"), None);
        assert!(build_time_base_block("2026-13-40").contains("时代背景"));
    }

    /// 在**真实库副本**上验证【前情提要】的合并效果（`#[ignore]`，手动触发；不调模型）。
    /// 跑法：DW_DIAG_DB=<story.db> cargo test --lib diag_phase_recap -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn diag_phase_recap_on_real_db() {
        use sqlx::sqlite::SqliteConnectOptions;
        let Ok(dir) = std::env::var("DW_DIAG_DB") else { eprintln!("未设置 DW_DIAG_DB"); return };
        // 注意：副本写**系统临时目录**，不写真实数据目录（测试中断会留下垃圾文件）
        let tmp = std::env::temp_dir().join("dw_diag_recap.db");
        let _ = std::fs::copy(&dir, &tmp);
        let opts = SqliteConnectOptions::new().filename(&tmp);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        let _ = sqlx::query("ALTER TABLE story_summaries ADD COLUMN branch_id TEXT").execute(&pool).await;

        // 造出与生产一致的 day_lines（第 N 天：摘要），排除最近一天（当前节点还在写）
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT date_label, COALESCE(day_summary,'') FROM timeline_nodes
             WHERE node_type IN ('day','main') AND is_flashback=0
             ORDER BY sort_order, id").fetch_all(&pool).await.unwrap();
        let mut day_lines: Vec<(usize, i64, String)> = Vec::new();
        for (i, (label, summary)) in rows.iter().enumerate() {
            if summary.trim().is_empty() { continue; }
            let Some(d) = crate::timeline::parse_day_number(label) else { continue };
            day_lines.push((i, d, format!("- {}：{}", label, summary.trim())));
        }
        let phases = crate::timeline::list_phase_summaries_in(&pool, &None).await.unwrap_or_default();

        let before: usize = day_lines.iter().map(|(_, _, s)| s.chars().count()).sum();
        let out = super::coalesce_with_phases(&day_lines, &phases);
        let after: usize = out.iter().map(|(_, s)| s.chars().count()).sum();

        eprintln!("\n=== 合并前（{} 行，{} 字）===", day_lines.len(), before);
        for (_, _, s) in &day_lines { eprintln!("  {}", s.chars().take(50).collect::<String>()); }
        eprintln!("\n=== 合并后（{} 行，{} 字）===", out.len(), after);
        for (_, s) in &out { eprintln!("  {}", s.chars().take(90).collect::<String>()); }
        eprintln!("\n  省下 {} 字（约 {} token）", before.saturating_sub(after),
                  ((before.saturating_sub(after)) as f64 * 1.5) as usize);
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
        let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    }
}

#[cfg(test)]
mod debug_cap_tests {
    use super::prune_debug_requests;
    use sqlx::sqlite::SqliteConnectOptions;

    async fn temp_pool(tag: &str) -> (sqlx::SqlitePool, std::path::PathBuf) {
        let dir = crate::paths::test_root().join(format!("debug_cap_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = SqliteConnectOptions::new().filename(dir.join("t.db")).create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE debug_requests(id INTEGER PRIMARY KEY AUTOINCREMENT, system_prompt TEXT NOT NULL DEFAULT '')",
        )
        .execute(&pool)
        .await
        .unwrap();
        (pool, dir)
    }

    /// 自动上限：超出部分按 id 从旧到新删掉，保留最新的
    ///
    /// 注意下限是 10 条（`max_rows.max(10)`）—— 配置写错也不会把记录全清。
    #[tokio::test]
    async fn prune_keeps_newest_rows() {
        let (pool, dir) = temp_pool("keep").await;
        for i in 0..14 {
            sqlx::query("INSERT INTO debug_requests(system_prompt) VALUES(?1)")
                .bind(format!("p{i}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        let removed = prune_debug_requests(&pool, 10).await.unwrap();
        assert_eq!(removed, 4, "14 条留 10 条 → 删 4 条");

        let left: Vec<(i64, String)> = sqlx::query_as("SELECT id, system_prompt FROM debug_requests ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(left.len(), 10);
        assert_eq!(left[0].1, "p4", "留下的必须是最新的 10 条");
        assert_eq!(left[9].1, "p13");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 没超上限时一条都不删
    #[tokio::test]
    async fn prune_noop_when_under_limit() {
        let (pool, dir) = temp_pool("noop").await;
        for i in 0..3 {
            sqlx::query("INSERT INTO debug_requests(system_prompt) VALUES(?1)")
                .bind(format!("p{i}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        assert_eq!(prune_debug_requests(&pool, 300).await.unwrap(), 0);
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM debug_requests").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// 上限被传成 0/负数时也至少要留 10 条（防止配置写错把记录全清）
    #[tokio::test]
    async fn prune_has_floor_of_ten() {
        let (pool, dir) = temp_pool("floor").await;
        for i in 0..15 {
            sqlx::query("INSERT INTO debug_requests(system_prompt) VALUES(?1)")
                .bind(format!("p{i}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        prune_debug_requests(&pool, 0).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM debug_requests").fetch_one(&pool).await.unwrap();
        assert_eq!(n, 10, "下限 10 条");
        let _ = std::fs::remove_dir_all(dir);
    }
}
