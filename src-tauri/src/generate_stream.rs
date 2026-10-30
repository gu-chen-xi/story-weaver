use serde_json::json;
pub(crate) mod prompt;
pub use prompt::*;

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

/// 尾部「上下文更新」块的首行标记（用于识别块、以及把块并入用户消息）
pub(crate) const TAIL_BLOCK_HEAD: &str = "【上下文更新】";

/// B 子项：历史保留几天的原文（更早的日子只带日摘要）。3 = 当前节点 + 前 2 个天数节点。
pub(crate) const KEEP_DAYS: usize = 3;

/// 估算 token 数：按字符类别加权（汉字 1 / 英文字母 0.25 / 空白 0.15 / 标点 0.5）。
///
/// 2026-10-28 换掉「字数 × 1.5」：英文提示词上线后，旧口径把英文按 3 倍字数虚报，
/// 中文又高估 ~50%。口径只在 [`crate::tokens`] 一处，前后端同一套系数。
fn estimate_tokens(text: &str) -> usize {
    crate::tokens::estimate(text)
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
/// 单条消息 token 的**下限**（2026-10-18 用户要求）。
///
/// 为什么要有下限：`avg` 是"当天所有消息的实测平均"，用户刚开始写时只有一两条很短的话
/// （实测跑出 30 token/条）→ `理想 N = 每日预算 ÷ 30` 会算出 **几百条/天**，
/// 那个数字既不可信、又让"保留天数"的语义失真。
///
/// 400 token ≈ 260 个汉字，接近一段正常的 AI 回复 —— 用它兜底更接近真实占用。
pub(crate) const MIN_AVG_MSG_TOKENS: usize = 400;

pub(crate) fn plan_day_limits(
    day_msg_tokens: &[Vec<usize>],
    hist_budget: usize,
    per_day_cap: usize,
) -> (Vec<usize>, usize, bool, usize) {
    let days = day_msg_tokens.len();
    if days == 0 {
        return (Vec::new(), 0, false, MIN_AVG_MSG_TOKENS);
    }
    // 实测平均，再**夹到下限 400**（见 MIN_AVG_MSG_TOKENS 的说明）
    let avg: usize = {
        let flat: Vec<usize> = day_msg_tokens.iter().flatten().copied().collect();
        if flat.is_empty() {
            MIN_AVG_MSG_TOKENS
        } else {
            (flat.iter().sum::<usize>() / flat.len()).max(1).max(MIN_AVG_MSG_TOKENS)
        }
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
        let used: usize = kept.iter().map(|(_, s)| crate::tokens::estimate(s)).sum::<usize>() + 60;
        if used <= space {
            break;
        }
        kept.remove(0);
        dropped += 1;
    }
    (kept, dropped)
}

/// 从一块 SSE JSON 里吸收 usage（上游只在最后一块带；不支持的通道完全没有）。
///
/// 2026-10-28：请求里加了 `stream_options.include_usage`，成功时最后一块是
/// `{"choices":[],"usage":{"prompt_tokens":…,"completion_tokens":…}}` —— 这里把它记下来，
/// 写进 debug_requests，用真实值校准 tokens::estimate。
fn absorb_usage(v: &serde_json::Value, prompt_tokens: &mut i64, completion_tokens: &mut i64) {
    let Some(u) = v.get("usage").filter(|u| !u.is_null()) else {
        return;
    };
    if let Some(n) = u.get("prompt_tokens").and_then(|x| x.as_i64()) {
        *prompt_tokens = n;
    }
    if let Some(n) = u.get("completion_tokens").and_then(|x| x.as_i64()) {
        *completion_tokens = n;
    }
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
mod budget_tests;

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
    // v3.4：导演监视器「当前角色卡」上的焦点角色名（示例对话第③优先级；可空）
    panel_focus: &str,
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
    // v3.3：前史/序章（存在 library.db 的 stories 表，属于故事级、不随分支变化）
    // story_id 在这里是字符串（命令层传 String），SQLite 会用 TEXT 比较 INT 主键；
    // 显式转成 i64 更稳，转不出来就当没写前史（不阻断生成）。
    let prologue = match story_id.trim().parse::<i64>() {
        Ok(sid) => crate::db::story_prologue(lib_pool, sid).await.unwrap_or_default(),
        Err(_) => String::new(),
    };
    let system = build_system_prompt(&pool, &branch, &time_origin, &prologue)
        .await
        .map_err(|e| e.to_string())?;

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

    // v3.2 ①：角色四维属性（同样限当前分支）
    //
    // 为什么"焦点角色优先、其余截断"：每个角色约 25 token，角色多时会挤占历史预算。
    // 有焦点就给焦点一人；没指定焦点（自由模式）才列出全部，但最多 8 个。
    let char_stats: Vec<String> = {
        let c_clause = crate::story_db::branch_clause("branch_id", &branch, 1);
        let c_sql = format!(
            // 情绪不再取数值（v3.4）：改由 current_status 的文字表达
            "SELECT name, stat_stamina, stat_sanity, stat_screen, COALESCE(current_status,'')
             FROM characters WHERE {} ORDER BY id",
            c_clause
        );
        let mut cq = sqlx::query(&c_sql);
        if branch.is_some() {
            cq = cq.bind(&branch);
        }
        let rows = cq.fetch_all(&pool).await.map_err(|e| e.to_string())?;
        let all: Vec<(String, i64, i64, i64, String)> = rows
            .iter()
            .filter_map(|r| {
                Some((
                    r.try_get::<String, _>(0).ok()?,
                    r.try_get::<i64, _>(1).unwrap_or(50),
                    r.try_get::<i64, _>(2).unwrap_or(50),
                    r.try_get::<i64, _>(3).unwrap_or(50),
                    r.try_get::<String, _>(4).unwrap_or_default(),
                ))
            })
            .collect();
        let focused: Vec<(String, i64, i64, i64, String)> = if !focus_role.is_empty() && focus_role != "自由" {
            all.iter().filter(|c| c.0 == focus_role).cloned().collect()
        } else {
            Vec::new()
        };
        let no_focus = focused.is_empty();
        let picked = if no_focus { all.iter().take(8).cloned().collect::<Vec<_>>() } else { focused };
        let mut v: Vec<String> = picked
            .iter()
            .map(|(n, a, b, c, d)| crate::characters::stat_value_line(n, *a, *b, *c, d))
            .collect();
        if no_focus && all.len() > 8 {
            v.push(format!("（其余 {} 个角色略）", all.len() - 8));
        }
        v
    };

    // v3.4：示例对话（只取一个角色、放尾部块）
    //
    // **踩坑修正（用户实测"示例对话没进提示词"）**：
    // `focus_role` 来自用户输入里的 `@角色名`（前端 `text.match(/@(...)/)`），
    // **不 @ 就没有焦点** → 原来 `if focus_role.is_empty() { 空 }` 直接把示例砍掉了。
    // 现在三级回落：
    //   ① 用户 @ 的那个角色；
    //   ② 否则 AI 扮演的角色（`ai_role`）—— "她该怎么说话"本来就是给 AI 看的；
    //   ③ 否则若全库只有**一个**角色写了示例，就用它（唯一解，不会张冠李戴）。
    let focus_examples: String = {
        let c_clause = crate::story_db::branch_clause("branch_id", &branch, 1);
        let all_sql = format!(
            "SELECT name, dialogue_examples FROM characters
             WHERE {} AND TRIM(dialogue_examples) <> '' ORDER BY id",
            c_clause
        );
        let mut q = sqlx::query(&all_sql);
        if branch.is_some() {
            q = q.bind(&branch);
        }
        let with_examples: Vec<(String, String)> = q
            .fetch_all(&pool)
            .await
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| {
                        Some((
                            r.try_get::<String, _>(0).ok()?,
                            r.try_get::<String, _>(1).ok()?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let names: Vec<String> = with_examples.iter().map(|(n, _)| n.clone()).collect();
        match pick_example_owner(focus_role, ai_role, panel_focus, &names) {
            Some(owner) => with_examples
                .into_iter()
                .find(|(n, _)| *n == owner)
                .map(|(_, ex)| ex)
                .unwrap_or_default(),
            None => String::new(),
        }
    };

// v3.4：世界书（触发式注入）
    //
    // 触发文本 = 本次输入 + 最近 3 条消息。为什么带上下文：用户常常"上一句提到、这一句没提"，
    // 而那恰好是最该补设定的时刻。
    // 为什么放尾部块：命中随消息变化，塞 system 会击穿前缀缓存。
    let (lore_lines, lore_skipped): (Vec<String>, usize) = {
        const LORE_BUDGET: usize = 1200;
        let trigger_text: String = {
            let mut s = user_text.to_string();
            let recent = sqlx::query_scalar::<_, String>(
                "SELECT content FROM messages WHERE node_id = ?1 ORDER BY id DESC LIMIT 3",
            )
            .bind(node_id)
            .fetch_all(&pool)
            .await
            .unwrap_or_default();
            for c in recent {
                s.push('\n');
                s.push_str(&c);
            }
            s
        };

        // 查询抽到 `world::fetch_lore_entries`：占位符编号与 bind 顺序必须一致，
        // 之前写错编号导致**主线永远查不到世界书**（详见那个函数的注释）。
        let rows = crate::world::fetch_lore_entries(&pool, &branch).await;

        // (priority, id, line)
        let mut hits: Vec<(i64, i64, String)> = Vec::new();
        for (idx, (key, value, meta_json)) in rows.iter().enumerate() {
            let id = idx as i64;
            let meta: serde_json::Value =
                serde_json::from_str(meta_json).unwrap_or_else(|_| serde_json::json!({}));
            let triggers = meta.get("triggers").and_then(|v| v.as_str()).unwrap_or("");
            let always = meta
                .get("always")
                .and_then(|v| v.as_str())
                .map(|v| v == "1")
                .unwrap_or(false);
            let priority = meta
                .get("priority")
                .and_then(|v| v.as_str())
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(50);
            // 命中词一起带进块里（用户搜的是触发词，条目名可能看不出内容）
            if let Some(hit) = crate::world::lore_hit(triggers, always, &trigger_text) {
                hits.push((priority, id, crate::world::lore_line_tagged(&key, &value, &hit)));
            }
        }

        // 优先级高的先进；同等优先级按 id（先写的先注入）
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut lines: Vec<String> = Vec::new();
        let mut used = 0usize;
        let mut skipped = 0usize;
        for (_, _, line) in hits {
            let n = line.chars().count();
            if used + n > LORE_BUDGET && !lines.is_empty() {
                skipped += 1;
                continue;
            }
            used += n;
            lines.push(line);
        }
        (lines, skipped)
    };

    // v3.4：关键记忆（一个事件一个 Markdown 文档，按分支过滤，预算 1200 字）
    // 注意：这里的 `memory` 参数是界面上的"回顾记忆"开关，与本段无关，别混。
    let memory_block = {
        let mems = crate::memory::list_for_branch(story_id, &branch);
        crate::memory::render_block(&mems, 1200).unwrap_or_default()
    };
    let tail_block = build_tail_block(mode, my_role, ai_role, focus_role, constraints, length_level, &node_type, is_flashback, director.as_deref(), &rel_values, &char_stats, &focus_examples, &memory_block, &lore_lines, lore_skipped);
    let user_text: &str = &user_text_clean;
    // 2026-10-26：这一轮涉及表白/承诺时，要求模型在最后一行输出表态标记（协议，不猜措辞）。
    let tail_block = match crate::evolution::stance_marker_kind(user_text_clean.as_str()) {
        Some(kind) => format!("{}\n{}", tail_block, crate::generate_stream::prompt::stance_hint(kind)),
        None => tail_block,
    };
    // 三维数值（2026-10-29）：**插件开着**才注入【状态变化】段 —— 默认关，零 token。
    // 注入位置在尾部块最后（和导演代价同属"软件对账协议"）。
    let stat_growth = crate::stat_change::enabled(lib_pool).await;
    let tail_block = if stat_growth {
        format!("{}\n{}", tail_block, crate::stat_change::legend())
    } else {
        tail_block
    };
    // 剧本大纲（2026-10-30 阶段②）：当前这一轮属于某章、且该章写了「本章目标」→ 注入一句方向。
    // 开关默认开（outline.inject=false 才关）；目标为空 → 整段不出现，零 token。
    let tail_block = if crate::outline::inject_enabled(lib_pool).await {
        let goal = match crate::outline::chapter_for_node(&pool, node_id).await {
            Some((_no, _title, goal)) => goal,
            None => String::new(),
        };
        match crate::outline::goal_block(&goal) {
            Some(b) => format!("{}\n{}", tail_block, b),
            None => tail_block,
        }
    } else {
        tail_block
    };

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
                // 导演代价的协议行同理（老库里可能还留着，新版存库前已剥）
                // **态度标记也要剥**（2026-10-27）：它留在历史里，模型会把它当"每轮都要写"的惯例照着抄，
                // 甚至自创表外值（实测写了「〔态度:冷淡〕」）。协议只在当轮由提示词要求，不进历史。
                (role, crate::stat_change::strip_line(&crate::memory::strip_marker(
                    &crate::director::strip_receipt(&crate::relations::strip_director(&content)),
                )))
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
    // 2026-10-28：API 实际 usage（校准 tokens::estimate 用；老库补列）
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN prompt_tokens INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN completion_tokens INTEGER NOT NULL DEFAULT 0").execute(&pool).await;

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
        "stream": true,
        // 2026-10-28：让上游在最后一块 SSE 里带 usage（OpenAI 兼容；DeepSeek 支持），
        // 用来记录真实 prompt_tokens，校准本地估算。
        "stream_options": { "include_usage": true }
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
    // 上游返回的实际用量（最后一块 SSE；没开 include_usage 或老通道就一直是 0）
    let mut prompt_tokens: i64 = 0;
    let mut completion_tokens: i64 = 0;

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
                                absorb_usage(&v, &mut prompt_tokens, &mut completion_tokens);
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
                        absorb_usage(&v, &mut prompt_tokens, &mut completion_tokens);
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
    // 导演代价（2026-10-27）：协议行〔导演代价〕类型：理由 只给软件记账用 ——
    // 存库前先剥掉，于是气泡 / 历史 / 导出 / 记忆里都不会出现它；账本另记一行（见下）。
    // 承诺结算（2026-10-30）：模型写了〔承诺〕兑现/未兑现 → 改最近一条「待兑现」的承诺记忆
    if let Some(fulfilled) = crate::promise::parse_line(&full) {
        let dir = crate::memory::memories_dir(story_id);
        match crate::memory::settle_promise_in(&dir, fulfilled) {
            Ok(Some(name)) => eprintln!("[promise] {name} → {}", if fulfilled { "已兑现" } else { "未兑现" }),
            Ok(None) => {}
            Err(e) => eprintln!("[promise] 结算失败: {e}"),
        }
    }
    let body = crate::promise::strip_line(&crate::stat_change::strip_line(&crate::director::strip_receipt(&full)));
    let msg_id: i64 = sqlx::query_scalar::<_, i64>(
        "INSERT INTO messages(node_id, role, content, mode, branch_id) VALUES(?1, 'assistant', ?2, ?3, ?4) RETURNING id",
    )
    .bind(node_id).bind(&body).bind(mode).bind(msg_branch)
    .fetch_one(&pool).await.map_err(|e| e.to_string())?;

    // 结账：**只有本轮真用了导演指令**才记（否则每轮回复都可能被关键词兜底记上一笔）
    if mode == "chat" && director.is_some() {
        if let Some(cand) = crate::director::settle(&full) {
            let cost_char = crate::director::character_id_in(&pool, &branch, ai_role).await;
            match crate::director::record_cost(&pool, &branch, node_id, Some(msg_id), cost_char, &cand).await {
                Ok(cost_id) => {
                    // 2026-10-27：记完账，顺手把「她不愿意却拗不过导演」落到关系强度上。
                    // 只扣白给/心软（见 strength_penalty），夹到 0，不进提案、不需人工确认。
                    // 前端靠 director-cost 事件弹一句「已自动把关系强度下降 N」。
                    match crate::director::apply_penalty(&pool, &branch, ai_role, my_role, &cand.kind, Some(node_id), Some(msg_id)).await {
                        Ok(Some(pen)) => {
                            let _ = sqlx::query("UPDATE director_costs SET rel_delta = ?1 WHERE id = ?2")
                                .bind(pen.delta())
                                .bind(cost_id)
                                .execute(&pool)
                                .await;
                            let _ = app.emit("director-cost", serde_json::json!({
                                "requestId": request_id,
                                "kind": cand.kind,
                                "score": cand.score,
                                "from": pen.from_name,
                                "to": pen.to_name,
                                "old": pen.old,
                                "new": pen.new,
                                "delta": pen.delta(),
                            }));
                        }
                        Ok(None) => {}
                        Err(e) => eprintln!("[director] 关系扣分失败（不影响生成）：{e}"),
                    }
                }
                Err(e) => eprintln!("[director] 导演代价记账失败（不影响生成）：{e}"),
            }
        }
    }

    // M5: 自动提取角色状态并更新状态卡（失败不影响主流程）
    let _ = crate::state_extract::auto_update_after_reply(&pool, story_id, node_id).await;

    // 记录成功快照
    let dbg_res = sqlx::query(
        "INSERT INTO debug_requests(node_id, model, system_prompt, tail_block, context_window, reserve_ratio, max_tokens, safety_margin, input_budget, day_window, day_n, n_value, avg_msg_tokens, history_json, user_text, ai_reply, estimated_tokens, n_used, status)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 'ok')"
    )
    .bind(node_id).bind(&model).bind(&system).bind(&tail_block)
    .bind(ctx_window as i64).bind(reserve_ratio).bind(max_tokens as i64).bind(safety_margin as i64).bind(input_budget as i64)
    .bind(day_window as i64).bind(&day_n_str).bind(n_value).bind(avg_msg_tokens as i64)
    .bind(&history_json).bind(user_text).bind(&full)
    .bind(total_tokens as i64).bind(chosen_n)
    .execute(&pool)
    .await;
    // 回填真实 usage（老通道 / 不支持 include_usage 时保持 0）。
    // 用 last_insert_rowid 而不是 MAX(id)：并发两次生成时不会写错行。
    if prompt_tokens > 0 || completion_tokens > 0 {
        if let Ok(res) = dbg_res {
            let _ = sqlx::query("UPDATE debug_requests SET prompt_tokens = ?1, completion_tokens = ?2 WHERE id = ?3")
                .bind(prompt_tokens)
                .bind(completion_tokens)
                .bind(res.last_insert_rowid())
                .execute(&pool)
                .await;
        }
    }

    // 三维数值（2026-10-29）：插件开着才结算；只认协议里第一条；写 stat_changes + 改 characters。
    // 失败不阻塞生成（apply 内部已吞错），前端靠 stat-change 事件弹提示。
    if stat_growth && mode == "chat" {
        let planned = crate::stat_change::settle(&full);
        if !planned.is_empty() {
            if let Some(cid) = crate::director::character_id_in(&pool, &branch, ai_role).await {
                let applied = crate::stat_change::apply(
                    &pool, &branch, node_id, msg_id, cid, &planned, "model",
                )
                .await;
                if !applied.is_empty() {
                    // 带上"第几天"：用户看到「体力 −5 → 27」时得知道是哪一天的账
                    let day_label: String = sqlx::query_scalar(
                        "SELECT COALESCE(date_label,'') FROM timeline_nodes WHERE id = ?1",
                    )
                    .bind(node_id)
                    .fetch_optional(&pool)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                    let _ = app.emit("stat-change", serde_json::json!({
                        "requestId": request_id,
                        "character": ai_role,
                        "day": day_label,
                        "changes": applied,
                    }));
                }
            }
        }
    }

    let _ = app.emit("chat-done", serde_json::json!({"requestId": request_id, "content": full}));
    Ok(())
}

/// 第五批「主生成链路」修复（《现状说明》5.2 / 5.3）的**可执行守护测试**。
///
/// 真实流式链路需要上游服务，跑不了单测。但这次改动里最容易"改回去"的是三件事，
/// 它们**可以在源码层面守护** —— 用 `include_str!` 读自己，断言关键写法还在。
/// 这不算优雅，但比"靠人记住"强得多（这类"防护栏"测试在 §5.2 里正是缺失的一环）。
#[cfg(test)]
mod stream_guards;

#[cfg(test)]
mod phase_recap_tests;

#[cfg(test)]
mod debug_cap_tests;
