// 从 `generate_stream.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `generate_stream.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use prompt::*;` 取回，
// 外部 `crate::generate_stream::xxx` 的调用路径**全部不变**。

use super::*;

/// 系统提示词（稳定前缀，跨轮不变，便于前缀缓存命中）
/// 顺序：基础指令 + 输出排版 → 数据字典 → 世界观 → 角色 → 方向和关系
/// 易变内容（当前模式 / 篇幅 / 数值与状态 / 聚焦 / 约束 / 时空 / 导演指令）不在这里，
/// 由 build_tail_block 拼成「上下文更新」块，插在历史之后（见调用处）
pub(crate) const OUTPUT_FORMAT_RULES: &str =
    "你是角色扮演引擎，依据当前模式工作：沉浸模式下以角色身份直接对话，旁白模式下以叙述者身份生成旁白。请用自然的文学语言继续剧情，必须自然包含角色的动作、心理、神态描写。不要输出 JSON。以角色卡设定为准；卡内未规定的细节可自由发挥，但不得改动已定义的性格、关系与世界观。\n【输出排版（供软件渲染，务必遵守）】\n- 人物台词：必须用双引号“”包住，并在台词前紧挨着写一个中括号，里面放说话人的角色名，格式如：[母亲]“你回吧。”。每条台词单独一行，台词与动作不要写在同一行。台词标记里写主名，正式回复的台词里可以用别名表示亲昵或者其他意思。\n- 动作/神态（非台词）：用（圆括号）括起、独占一行（如：（他攥紧了拳头））。连续动作时，一个动作一个括号单独包住并换行，不要挤在同一行。\n- 心理描写（心声）：第一人称、现在时，写角色此刻脑中真实冒出来的念头；整行用星号包裹，如：*他怎么能这样……我分明没允许。*（软件会把星号渲染为心声样式）。心声须单独成行，不与动作、台词、旁白同行，不用中括号或引号。\n  心声与台词的关系**不止「相反」一种**，按人物此刻状态自然选择，不要每条都写成口是心非：\n  - 反向（口是心非）：*有事。但我不会说给他听。*（与台词相反）\n  - 旁支（跑题）：*他袖口上沾了灰。*（与台词无关）\n  - 自欺（找台阶）：*我只是懒得争，不是在意。*（说服自己）\n  - 掐断（说到一半停）：*我本来想说——算了。*（未完成）\n  - 钝（没有念头，只有身体感）：*耳朵在响。*（空白）\n  - 算计（盘算利害）：*现在翻脸，吃亏的是我。*（评估）\n  允许不完整、允许说不出口、允许她自己也不知道自己在想什么；可以完全与对方无关（环境细节、身体感觉、旧事）——这是「活人感」的来源。\n  **忌**：写成对剧情的总结、对人物的点评，或每轮都反向控诉。\n  边界：角色处于疏离、戒备或正拒绝越界时，心声必须与台词**同向**（冷、警惕、不耐烦），不得写「其实我也想 / 心软了」这类妥协念头，也不得为越界行为提供合理化。\n- 旁白（叙述/环境描写）：只写场景、时间、气氛、外部事件，不进入任何人的内心；正常成段书写即可，不要用中括号或引号标记。\n- 除“台词必须带[角色名]标记”外，其余内容请保持自然发挥。";

/// 第五批 ③：系统提示词的装配需要知道"当前在哪条分支" ——
/// 世界观 / 角色 / 关系这些**都要读该分支自己的版本**（主线 = NULL）。
pub(super) async fn build_system_prompt(
    pool: &SqlitePool,
    branch: &Option<String>,
    time_origin: &str,
    // v3.3：前史/序章（故事级静态设定，空串 = 没写 → 整段不出现）
    prologue: &str,
) -> Result<String, sqlx::Error> {
    let mut parts = Vec::new();
    parts.push(OUTPUT_FORMAT_RULES.to_string());

    // 数据字典：关系强度档位 + 关系状态含义（两种「读法」，只在此处出现一次）
    parts.push(format!(
        "\n【数据字典】（本提示词内的固定用法：关系强度=0-100 数值+档位名；关系态势=稳定/紧张/破裂；体力/理智/存在感=角色三维数值；当下=角色那一刻的情绪或姿态，情绪的方向从「当下」读出，不另设数值）\n{}\n{}\n{}", crate::relations::strength_legend(), crate::relations::state_legend(), crate::characters::stat_legend()));

    // 【时间基准】—— 故事的时间锚点（用户 2026-10-17 要求加进系统提示词）。
    //
    // 为什么放在稳定前缀里：`time_origin` 建故事时定下、之后基本不动，
    // 而模型需要知道"第 N 天"对应现实里的哪一天，才能写对季节、节日、年月、年龄推进。
    // 不注入的话，模型只能看到"第 3 天"这种相对标签，无法推断绝对时间。
    //
    // 注意**只放锚点与换算规则**，不放"今天是第几天" —— 那会随节点变，
    // 一旦进了 system 就让前缀缓存失效（易变内容统一在尾部「上下文更新」块里）。
    parts.push(build_time_base_block(time_origin));

    // 【前史 / 序章】（稳定，v3.3）—— 放在【世界观】之前：
    // 它是"故事开始之前发生了什么"，属于比世界观更上层的背景，模型先读它再读设定更顺。
    // 空串整段不出现（老故事升级后行为不变）。
    if let Some(block) = prologue_block(prologue) {
        parts.push(block);
    }

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
            // v3.4：世界书**只走尾部块**（命中才注入），绝不能进 system 的【世界观】。
            // 踩过的坑：`match category` 没写 "世界书" 分支 → 落进默认分支被全量渲染，
            // 于是"触发式注入"形同虚设（实测：条目每轮都在，且无视触发词）。
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
                    // v3.4：物品库（与术语同构：名称 + 说明 + 别名）
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
    }

    // 角色（稳定）—— 放在【世界观】之后。
    // 第五批 ③：读**当前分支**的角色（分支里角色是复制出的独立行，改动不会串到主线）
    let char_clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let char_sql = format!(
        "SELECT name, alias, role, faction, age, gender, one_liner, profile, speech_style, traits, background_past, motivation, goal, behavior_rules, appearance, dialogue_examples FROM characters WHERE {}",
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
            // v3.4「角色卡 v2」：过去 / 动机 / 目的 / 行为准则（示例对话不在这里 —— 见尾部块）
            let past: String = r.try_get(10).unwrap_or_default();
            let motive: String = r.try_get(11).unwrap_or_default();
            let goal: String = r.try_get(12).unwrap_or_default();
            let rules: String = r.try_get(13).unwrap_or_default();
            // v3.4：外貌（作者设定，截断 300 字 —— 外貌通常短，且不该挤掉小传/动机）
            let look: String = r.try_get(14).unwrap_or_default();
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
            // 作者设定字段：截断到 400 字（超了就标注"节略"，避免一个角色吃穿预算）
            for (label, raw, cap) in [("外貌", &look, 300usize), ("过去", &past, 400), ("动机", &motive, 400), ("目的", &goal, 400), ("行为准则", &rules, 400)] {
                if let Some(v) = clip_field(raw, cap) {
                    extras.push(format!("{}：{}", label, v));
                }
            }
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
        // 配合度段由 `relations::coop_legend()` 生成 —— 档位名与边界来自 STRENGTH_BANDS，
    // 不再手写（手写那份曾写错档位名与边界「客气/熟识 0-19」，真相是「相识/亲近/交心 0-20」，
    // 两套映射并存 → 模型表现成"疏离档却放行"）。
    parts.push(format!("\n{}", crate::relations::coop_legend()));
    parts.push(format!("\n{}", crate::relations::footing_rule()));

    // 情绪 × 强度 × 理智（2026-10-22）：三个变量各管一件事，避免"崩溃一律沉默"与"该拒却软"
    parts.push(
        "\n【情绪表演】情绪只定方向；强度定藏露；理智定语量。三者冲突时**以强度优先** ——          疏离档下，任何情绪都不外放给对象看。\n\
         - 情绪（从「当下」读）：往哪个方向炸 —— 内爆还是外放；\n\
         - 关系强度：给谁看 —— 疏离＝挡住，亲近＝摊开；\n\
         - 理智：还剩多少语言 —— 高＝能说完，低＝只剩短句。\n\
         同一个「崩溃」的四种写法：\n\
         - 强度低 · 理智低：转身、挡脸、话断在半截，不哭给你看；\n\
         - 强度低 · 理智高：答非所问、语气平得反常，事后才失态；\n\
         - 强度高 · 理智低：当场散架，说不完整的话，抓住人不放；\n\
         - 强度高 · 理智高：一句一句说完，说得很清楚，越清楚越吓人。\n\
         崩溃不一定哭，狂怒不一定吼 —— 往反方向演，人物才立得住；理智低时说的，才是平时不肯说的那句真话。"
            .to_string(),
    );


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
    char_stats: &[String],
    // v3.4：焦点角色的"示例对话"（空串 = 不输出该段）
    focus_examples: &str,
    // v3.4：关键记忆（前端已拼好整段文本；空串 = 不输出该段）
    memory_block: &str,
    // v3.4：世界书命中的行（已按优先级排好；空 = 不输出该段）
    lore_lines: &[String],
    lore_skipped: usize,
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

    // v3.2 ①：这一段原来只有"关系强度与状态"，现在补上**角色四维属性**。
    // 单独两组的理由：关系是两人之间的、属性是单个角色的，混在一起模型容易串。
    if !rel_values.is_empty() || !char_stats.is_empty() {
        parts.push("【数值与当下】（本节为当前真值，以本节为准；历史里的旧数值、旧关系态势作废）".to_string());
        if !char_stats.is_empty() {
            parts.push("角色属性（只作当下与倾向的参考，靠动作/语气间接表现，不要报数字）：".to_string());
            for l in char_stats {
                parts.push(l.clone());
            }
        }
        if !rel_values.is_empty() {
            parts.push("关系强度与状态：".to_string());
            for l in rel_values {
                parts.push(l.clone());
            }
        }
    }

    // v3.4：世界书放在【示例对话】之前 —— 先补"她背后的世界"，再教"她怎么说话"
    if let Some(block) = crate::world::lore_block(lore_lines, lore_skipped) {
        parts.push(block);
    }

    // v3.4：关键记忆放在世界书之后、示例对话之前 ——
    // 先补"她背后的世界"，再提"你们之间发生过什么"，最后才教"她怎么说话"。
    if !memory_block.trim().is_empty() {
        parts.push(memory_block.to_string());
    }

    // v3.4：示例对话放在【本次聚焦】之前 —— 先给"她怎么说话"，再说"这一段围绕她"
    if let Some(block) = examples_block(focus_examples) {
        parts.push(block);
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

/// v3.3：【前史】块。空白 → `None`（整段不出现），否则去掉首尾空白后包一段。
///
/// 抽成纯函数的原因：这段是"稳定前缀"的一部分，改错会让所有故事的提示词都变；
/// 用单测锁住"空串不出块 / 有内容才出块 / 首尾空白被裁掉"这三件事最省事。
pub(crate) fn prologue_block(prologue: &str) -> Option<String> {
    let p = prologue.trim();
    if p.is_empty() {
        return None;
    }
    Some(format!(
        "\n【前史】（故事开始之前发生过的事，属于已确立的背景，不是本轮剧情；与后文设定冲突时以前史为准）\n{}",
        p
    ))
}

/// 截断单个作者字段：空白 → None；超长 → 截断并标注「节略」。
///
/// 为什么要截断：角色卡是手写自由文本，用户可能贴几千字小传进去。
/// 一个角色 400 字 × 10 个角色 = 4000 字 ≈ 6000 token，能直接把预算吃穿。
pub(crate) fn clip_field(raw: &str, max_chars: usize) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let n = s.chars().count();
    if n <= max_chars {
        return Some(s.to_string());
    }
    let cut: String = s.chars().take(max_chars).collect();
    Some(format!("{}…（原文 {} 字，已节略）", cut, n))
}

/// 【示例对话】块（进尾部块，只给焦点角色）。
///
/// 为什么放尾部而不是 system：
///   ① 它体积最大（几段对话），放 system 会推高固定区、挤占历史；
///   ② 它是"这一轮希望对话像什么样"的运行时提示，随焦点角色变化；
///   ③ 尾部块本来就不吃前缀缓存，放这里最合适。
/// 决定"用谁的示例对话"。
///
/// 顺序：`@提到的角色` → `AI 扮演的角色` → **全库唯一**写过示例的角色 → 都不用。
///
/// 为什么最后那档要加"唯一"限制：如果有多个角色都写了示例，随便挑一个会张冠李戴
/// （把 A 的说话方式套到 B 身上，比不给示例更糟）。
pub(crate) fn pick_example_owner(
    focus: &str,
    ai: &str,
    panel_focus: &str,
    having_examples: &[String],
) -> Option<String> {
    let valid = |s: &str| !s.is_empty() && s != "自由";
    for cand in [focus, ai, panel_focus] {
        if valid(cand) && having_examples.iter().any(|n| n == cand) {
            return Some(cand.to_string());
        }
    }
    // 都不满足 → 不给示例。用户明确要求："示例可以为空"，
    // 比"随便挑一个角色的说话方式"安全（那会把 A 的语气套到 B 身上）。
    None
}

pub(crate) fn examples_block(raw: &str) -> Option<String> {
    let body = clip_field(raw, 800)?;
    Some(format!(
        "【示例对话】（以下对话只用于**模仿语气、用词、停顿与反应方式**，不要照抄内容、也不要直接接续它）\n{}",
        body
    ))
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

#[cfg(test)]
mod prompt_rules_tests;
