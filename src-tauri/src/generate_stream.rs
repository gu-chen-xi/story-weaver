use serde_json::json;
pub(crate) mod prompt;
pub use prompt::*;
pub(crate) mod tail_view;

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

/// 流式协议的两种形态。**请求格式和行格式是同一个选择的两半**，
/// 所以用一个枚举带着走，避免"URL 换成本地了、行还按 SSE 读"这种半改。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StreamProtocol {
    /// 云端 OpenAI 兼容：SSE，行形如 data: {json}，增量在 choices[0].delta.content，
    /// 用量在最后一块的 usage.prompt_tokens / completion_tokens。
    Sse,
    /// 本地 Ollama 原生 /api/chat：NDJSON，**整行就是 JSON**，增量在 message.content，
    /// 用量在 done:true 那块（prompt_eval_count / eval_count）。
    Ndjson,
}

/// 解析**一行**流式响应，返回这行的增量文本（没有增量 → None），顺带吸收用量。
///
/// 按行切分、发事件、落库都是两套协议共用的（见调用处），差别只在这里。
/// 解析失败 / 空行 / 结束标记一律返回 None —— 流式链路不能因为一行脏数据就中断。
fn absorb_stream_line(
    line: &str,
    proto: StreamProtocol,
    prompt_tokens: &mut i64,
    completion_tokens: &mut i64,
) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    match proto {
        StreamProtocol::Sse => {
            // 容忍 "data:{...}"（无空格）与 "data: {...}"；其余 SSE 事件（event:/:ping）一律跳过
            let data = line.strip_prefix("data:")?.trim_start();
            if data == "[DONE]" {
                return None;
            }
            let v = serde_json::from_str::<serde_json::Value>(data).ok()?;
            absorb_usage(&v, prompt_tokens, completion_tokens);
            let delta = v["choices"][0]["delta"]["content"].as_str()?;
            if delta.is_empty() { None } else { Some(delta.to_string()) }
        }
        StreamProtocol::Ndjson => {
            let v = serde_json::from_str::<serde_json::Value>(line).ok()?;
            // 结束块：本地这条路**唯一**能拿到真实用量的地方（兼容端点那条拿不到）
            if v.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                if let Some(n) = v.get("prompt_eval_count").and_then(|x| x.as_i64()) {
                    *prompt_tokens = n;
                }
                if let Some(n) = v.get("eval_count").and_then(|x| x.as_i64()) {
                    *completion_tokens = n;
                }
            }
            let delta = v.get("message")?.get("content")?.as_str()?;
            if delta.is_empty() { None } else { Some(delta.to_string()) }
        }
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

/// 测试用的超时覆盖（**只认环境变量**，生产不设就是 90s）。
///
/// 为什么允许覆盖：超时链路只能用"一个只连不答的端口"来验，真等 90s 没法进 CI。
/// 取值范围夹在 1..=600 秒 —— 就算误设也不会把超时关掉。
fn connect_timeout_secs() -> u64 {
    std::env::var("DREAMWEAVER_TEST_CONNECT_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n >= 1 && *n <= 600)
        .unwrap_or(GEN_CONNECT_TIMEOUT_SECS)
}

/// 发请求，但**最多等 `secs` 秒拿到响应头**（首字节）。
///
/// 抽成独立函数是为了能单测：它正是生产链路用的那一条（`generate_reply_streaming` 的重试循环调用它），
/// 所以"连得上但对方永远不答"这条路径是被测到的，而不是另写一份测试专用逻辑。
async fn send_with_timeout(
    req: reqwest::RequestBuilder,
    secs: u64,
) -> Result<reqwest::Response, SendError> {
    match tokio::time::timeout(std::time::Duration::from_secs(secs), req.send()).await {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(e)) => Err(SendError::Network(format!("请求失败: {e}"))),
        Err(_) => Err(SendError::Timeout(format!("请求超时（{secs}s 内没有响应）—— 已中止，可重试"))),
    }
}

/// `send_with_timeout` 的两种失败：超时（要按固定文案报给用户）与网络错误。
#[derive(Debug)]
enum SendError {
    Timeout(String),
    Network(String),
}


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
    // 界面「🔍 回顾」选中的若干天（可多选 = 范围）；空 = 走自动检索
    recall_days: &[i64],
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

    // 2026-11-01：把「我扮演 / AI 扮演」**落库**（story 级 configs）。
    // 为什么：这两个值原来只是前端 React state，后端分析关系时完全不知道谁演谁，
    // 只能靠"名字有没有出现"猜归属（用户实测被这条挡掉过提案）。现在生成时顺手记一份，
    // 分析时当锚点用；老故事没有这两行 → 退回名字/单关系判定，行为不变。
    let _ = crate::db::set_config(&pool, "play.my_role", my_role).await;
    let _ = crate::db::set_config(&pool, "play.ai_role", ai_role).await;
    // 2026-11-10：把这一轮的扮演者写进 assistant 消息的 meta（和用户消息同一套键）——
    // 「角色视角」提示词装配器按消息取料（ROADMAP §七 第 1 步：打标）。
    let msg_meta = crate::timeline::message_meta(None, ai_role, my_role);
    // 2026-11-10（ROADMAP §七 第 2 步）：对话模式走**角色视角**，旁白永远全局。
    // 故事级开关 `prompt.roleplay_scope`：默认 role；写 global / false / off 就退回全局（一行回退）。
    let roleplay_on = mode == "chat"
        && db::get_config(lib_pool, "prompt.roleplay_scope")
            .await
            .ok()
            .flatten()
            .map(|v| !matches!(v.trim(), "global" | "false" | "0" | "off"))
            .unwrap_or(true);
    let scope = if roleplay_on {
        PromptScope { role: ai_role, my_role }
    } else {
        PromptScope::default()
    };

    let base_url = db::get_config(lib_pool, "model.base_url").await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let mut model = db::get_config(lib_pool, "model.name").await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "deepseek-v4-flash".to_string());
    let n_recent: i64 = db::get_config(lib_pool, "context.n_recent").await.map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok()).unwrap_or(10);
    let mut ctx_window: usize = db::get_config(lib_pool, "model.context_window").await.map_err(|e| e.to_string())?
        .and_then(|s| s.parse().ok()).unwrap_or(32768);
    // 2026-10-31：**主模型只有一路** —— Ollama 开着就整体替换云端（不混用），
    // 连不上或没填模型名直接报错，不静默回落（否则用户以为在跑本地）。
    let ollama_on = match crate::ollama::chat_override(lib_pool).await? {
        Some(cfg) => {
            // 注意：**不再拼 /v1** —— 本地主对话走原生 /api/chat（见下面的拼 URL 分支），
            // 兼容端点只留给 ollama::test_chat 那种探活请求。
            model = cfg.model.clone();
            ctx_window = cfg.context_window as usize;
            Some(cfg)
        }
        None => None,
    };
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

    let api_key = if ollama_on.is_some() {
        // 本地不需要 Key；Ollama 的 OpenAI 兼容层不校验
        "ollama".to_string()
    } else {
        keychain::get_api_key("openai_api_key".to_string()).map_err(|e| e.to_string())?
            .ok_or_else(|| "未设置 API Key".to_string())?
    };

    // 查当前节点类型，给系统提示词加时空提示
    let node_row = sqlx::query("SELECT node_type, is_flashback FROM timeline_nodes WHERE id = ?1")
        .bind(node_id).fetch_optional(&pool).await.map_err(|e| e.to_string())?;
    let (node_type, is_flashback): (String, i64) = match node_row {
        Some(r) => (r.try_get(0).map_err(|e: sqlx::Error| e.to_string())?, r.try_get(1).map_err(|e: sqlx::Error| e.to_string())?),
        None => ("day".to_string(), 0),
    };

    // 导演指令：从用户输入里解析出来，注入本轮尾部「上下文更新」块；历史里剥掉内联标记
    let director = crate::relations::parse_director(user_text);
    // 作者手打承诺标记（2026-10-30）：用户消息里出现〔承诺〕兑现/未兑现 → **人工结算**（来源 manual），
    // 并且这一行不能进提示词/历史（它是软件指令，不是剧情）。
    let manual_promise = crate::promise::parse_line(&crate::relations::strip_director(user_text));
    if let Some(fulfilled) = manual_promise {
        let dir = crate::memory::memories_dir(story_id);
        match crate::memory::settle_promise_in(&dir, fulfilled, "manual") {
            Ok(Some(name)) => eprintln!("[promise] 人工结算 {name} → {}", if fulfilled { "已兑现" } else { "未兑现" }),
            Ok(None) => {}
            Err(e) => eprintln!("[promise] 人工结算失败: {e}"),
        }
    }
    let user_text_clean = crate::promise::strip_line(&crate::relations::strip_director(user_text));
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
    let system = build_system_prompt(&pool, &branch, &time_origin, &prologue, scope)
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
    let mut rel_q = sqlx::query(sqlx::AssertSqlSafe(rel_sql.as_str()));
    if branch.is_some() {
        rel_q = rel_q.bind(&branch).bind(&branch).bind(&branch);
    }
    let rel_rows = rel_q.fetch_all(&pool).await.map_err(|e| e.to_string())?;
    let rel_values: Vec<String> = rel_rows.iter().filter_map(|r| {
        let from: String = r.try_get(0).ok()?;
        let to: String = r.try_get(1).ok()?;
        // 2026-11-10 角色视角：只列"她对别人"的强度 —— 别人对她的那条是别人的隐私（漏风点 ①）
        if scope.is_role() && from != scope.role.trim() {
            return None;
        }
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
        let mut cq = sqlx::query(sqlx::AssertSqlSafe(c_sql.as_str()));
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
        // 2026-11-10 角色视角：属性只给她自己（漏风点 ② —— 不能让她看到别人的体力/理智）
        let picked: Vec<(String, i64, i64, i64, String)> = if scope.is_role() {
            all.iter().filter(|c| c.0 == scope.role.trim()).cloned().collect()
        } else {
            picked
        };
        let mut v: Vec<String> = picked
            .iter()
            .map(|(n, a, b, c, d)| crate::characters::stat_value_line(n, *a, *b, *c, d))
            .collect();
        if no_focus && !scope.is_role() && all.len() > 8 {
            v.push(format!("（其余 {} 个角色略）", all.len() - 8));
        }
        // 2026-11-02「数值影响台词」：把三维翻成**本段的台词硬上限**（全正常 → 不加行）。
        // 只给 AI 扮演的那个角色；没指定扮演者时，只有单角色故事才给（多角色不猜是谁）。
        let target = if !ai_role.is_empty() && ai_role != "自由" {
            picked.iter().find(|c| c.0 == ai_role)
        } else if picked.len() == 1 {
            picked.first()
        } else {
            None
        };
        if let Some((n, st, sa, sc, _)) = target {
            if let Some(line) = crate::characters::perf_directive(n, *st, *sa, *sc) {
                v.push(line);
            }
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
        let mut q = sqlx::query(sqlx::AssertSqlSafe(all_sql.as_str()));
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
        // 2026-11-10 角色视角：示例对话只认她自己（漏风点 ③ —— 否则会落到"全库唯一有示例"的别人头上）
        let owner = if scope.is_role() {
            let r = scope.role.trim();
            if names.iter().any(|n| n == r) { Some(r.to_string()) } else { None }
        } else {
            pick_example_owner(focus_role, ai_role, panel_focus, &names)
        };
        match owner {
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
    // 这一轮是不是"要写表态标记"的轮次（表白 / 承诺）：决定【关键记忆】要不要给"改判出口"。
    // 2026-11-01 边界测试：表白轮末尾已经有 stance_hint，再叠一条同款指令会教模型写两行一模一样的标记，
    // 所以**同一轮只留一处**（详见 memory::render_block_for 的说明）。
    let stance_kind = crate::evolution::stance_marker_kind(user_text_clean.as_str());
    let memory_block = {
        let mems = crate::memory::list_for_ancestors_in(&crate::memory::memories_dir(story_id), &crate::memory::ancestors_of(story_id, &branch).await);
        // 2026-11-11 角色视角：只给她自己的 + 没有归属的（老卡 / 全局事件）
        let mems = crate::memory::filter_for_role(&mems, scope.role);
        crate::memory::render_block_for(&mems, 1200, stance_kind.is_none()).unwrap_or_default()
    };
    let tail_block = build_tail_block(mode, my_role, ai_role, focus_role, constraints, length_level, &node_type, is_flashback, director.as_deref(), &rel_values, &char_stats, &focus_examples, &memory_block, &lore_lines, lore_skipped);
    let user_text: &str = &user_text_clean;
    // 2026-10-26：这一轮涉及表白/承诺时，要求模型在最后一行输出表态标记（协议，不猜措辞）。
    let tail_block = match stance_kind {
        Some(kind) => format!("{}\n{}", tail_block, crate::generate_stream::prompt::stance_hint(kind)),
        None => tail_block,
    };
    // 三维数值（2026-10-29）：**插件开着**才注入【状态变化】段 —— 默认关，零 token。
    // 注入位置在尾部块最后（和导演代价同属"软件对账协议"）。
    // 承诺结算协议（2026-10-30）：**只在有「待兑现」承诺时**注入一行，否则零 token
    let tail_block = if crate::memory::has_pending_promise(story_id) {
        format!("{}\n{}", tail_block, crate::promise::hint())
    } else {
        tail_block
    };
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
            let rows = sqlx::query("SELECT role, content, mode, meta FROM messages WHERE node_id = ?1 ORDER BY id ASC")
                .bind(nid).fetch_all(&pool).await.map_err(|e| e.to_string())?;
            let msgs: Vec<(String, String)> = rows.iter().filter_map(|r| {
                let role: String = r.try_get(0).unwrap_or_default();
                let content: String = r.try_get(1).unwrap_or_default();
                // 2026-11-10 角色视角 / 2026-11-11 改按**配对**：只留"她俩这一段关系里的那几轮 + 旁白 + 老消息（无标记 = 全局兜底）"
                if scope.is_role() {
                    let m: String = r.try_get(2).unwrap_or_default();
                    let meta: String = r.try_get(3).unwrap_or_default();
                    if !crate::timeline::visible_in_role_view(&m, &meta, scope.role, scope.my_role) {
                        return None;
                    }
                }
                // 导演指令标记剥掉：指令只在当次的尾部块里出现一次（否则既重复又会被当成台词读）
                // 导演代价的协议行同理（老库里可能还留着，新版存库前已剥）
                // **态度标记也要剥**（2026-10-27）：它留在历史里，模型会把它当"每轮都要写"的惯例照着抄，
                // 甚至自创表外值（实测写了「〔态度:冷淡〕」）。协议只在当轮由提示词要求，不进历史。
                Some((role, crate::stat_change::strip_line(&crate::memory::strip_marker(
                    &crate::director::strip_receipt(&crate::relations::strip_director(&content)),
                ))))
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
        if !recall_days.is_empty() || intent.is_recall {
            let exclude: std::collections::HashSet<i64> =
                keep_nodes.iter().map(|(id, _)| *id).collect();
            // 单天原文上限：约 1600 字（≈800 token）。回顾要的是"当天发生了什么"，
            // 太长的天从尾部截断并注明，避免把三天原文挤掉。
            crate::recall::search(&pool, &branch, node_id, &exclude, &intent, recall_days, 3, 1600,
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

    // ===== ① 拼 URL + body：**两套协议在这里分叉** =====
    //
    // 云端（OpenAI 兼容）：{base}/chat/completions + SSE
    // 本地（Ollama 原生）：{base}/api/chat      + NDJSON
    //
    // 为什么本地必须走原生（2026-11-01 实测 + 3.13）：兼容端点 /v1/chat/completions **不收**
    // num_ctx（喂了也被忽略，实际按模型默认的 2048 跑），界面上选的 32k…1M 等于摆设；
    // 原生接口的 options.num_ctx / options.num_predict 才真生效。附带好处：done 块会带
    // prompt_eval_count / eval_count，**本地请求也能回填真实用量**（原来是 0）。
    let proto = if ollama_on.is_some() { StreamProtocol::Ndjson } else { StreamProtocol::Sse };
    let url = match &ollama_on {
        Some(cfg) => format!("{}/api/chat", cfg.base_url.trim_end_matches('/')),
        None => format!("{}/chat/completions", base_url.trim_end_matches('/')),
    };
    let body = match &ollama_on {
        Some(_) => json!({
            "model": model,
            "messages": messages_json,
            "stream": true,
            // 2026-11-03 实测：思考型模型（Qwen3.5/Qwen3 系）不关思考时，token 全进
            // `message.thinking`，`message.content` 一直是空串 —— 下面只读 content，
            // 于是 max_tokens 被思考吃光、界面报"AI 返回空内容"。这里显式关掉，
            // 与 ollama::chat_once（伏笔/校对）保持同一个口径：本地主对话要正文，不要推理。
            // 不支持思考的模型上这个字段是空操作。
            "think": false,
            // 键名与 ollama::chat_once 保持一致（原生参数只在 options 里认）
            "options": { "num_ctx": ctx_window, "num_predict": max_tokens }
        }),
        None => json!({
            "model": model,
            "messages": messages_json,
            "max_tokens": max_tokens,
            "stream": true,
            // 2026-10-28：让上游在最后一块 SSE 里带 usage（OpenAI 兼容；DeepSeek 支持），
            // 用来记录真实 prompt_tokens，校准本地估算。
            "stream_options": { "include_usage": true }
        }),
    };

    // 第五批（《现状说明》5.2）：**主生成链路原来完全没有超时** ——
    // 上游半开时请求可以永远不返回，界面永远停在"正在生成"。
    // 现在：连接超时 + 整体超时（用 timeout 包住 send）。
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(GEN_CONNECT_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;
    // 本地不送 Authorization：Ollama 不校验，还省得把一个假 key 写进调试记录
    let mut req = client.post(&url).json(&body);
    if ollama_on.is_none() {
        req = req.bearer_auth(&api_key);
    }
    // ===== 2026-11-04（用户定）：429 / 5xx / 网络错误**自动重试** =====
    // 最多重试 3 次，退避 1s / 3s / 9s；只重试「还没吐出任何正文」的阶段 ——
    // 一旦开始流式，重试会把同一段正文放两遍，所以那之后只保留已收到的部分（见下面的中止分支）。
    const RETRY_DELAYS_SECS: [u64; 3] = [1, 3, 9];
    let mut attempt = 0usize;
    let sent: Result<reqwest::Response, String> = loop {
        let mut r = client.post(&url).json(&body);
        if ollama_on.is_none() {
            r = r.bearer_auth(&api_key);
        }
        let outcome = send_with_timeout(r, connect_timeout_secs()).await;
        let detail = match outcome {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    break Ok(resp);
                }
                let text = resp.text().await.unwrap_or_default();
                let d = format!("HTTP {}: {}", status.as_u16(), text.chars().take(500).collect::<String>());
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if !retryable || attempt >= RETRY_DELAYS_SECS.len() {
                    break Err(d);
                }
                d
            }
            Err(e) => {
                let d = match e {
                    SendError::Network(m) => m,
                    SendError::Timeout(m) => m,
                };
                if attempt >= RETRY_DELAYS_SECS.len() {
                    break Err(d);
                }
                d
            }
        };
        let wait = RETRY_DELAYS_SECS[attempt];
        eprintln!("[gen] 第 {} 次失败，{wait}s 后重试：{detail}", attempt + 1);
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        attempt += 1;
    };

    let resp = match sent {
        Ok(r) => r,
        Err(why) => {
            let _ = sqlx::query(
                "INSERT INTO debug_requests(node_id, model, system_prompt, tail_block, context_window, reserve_ratio, max_tokens, safety_margin, input_budget, day_window, day_n, n_value, avg_msg_tokens, history_json, user_text, estimated_tokens, n_used, status, error) \
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, 'error', ?18)"
            )
            .bind(node_id).bind(&model).bind(&system).bind(&tail_block)
            .bind(ctx_window as i64).bind(reserve_ratio).bind(max_tokens as i64).bind(safety_margin as i64).bind(input_budget as i64)
            .bind(day_window as i64).bind(&day_n_str).bind(n_value).bind(avg_msg_tokens as i64)
            .bind(&history_json).bind(user_text)
            .bind(total_tokens as i64).bind(chosen_n).bind(&why)
            .execute(&pool).await;
            // 必须发事件，否则前端监听器不注销（《现状说明》5.3）
            let _ = app.emit("chat-error", serde_json::json!({"requestId": request_id, "detail": why}));
            return Err(why);
        }
    };

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
                            // ③ 按行切、④ 取增量、发事件：这几步**与协议无关**，
                            // 只有"这一行怎么读"分叉（见 absorb_stream_line）。
                            if let Some(delta) = absorb_stream_line(line, proto, &mut prompt_tokens, &mut completion_tokens) {
                                full.push_str(&delta);
                                let _ = app.emit("chat-token", serde_json::json!({"requestId": request_id, "delta": delta}));
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
            if let Some(delta) = absorb_stream_line(line, proto, &mut prompt_tokens, &mut completion_tokens) {
                full.push_str(&delta);
                let _ = app.emit("chat-token", serde_json::json!({"requestId": request_id, "delta": delta}));
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
            // 2026-11-04（用户定）：失败时**保留已生成的部分**，末尾追加一行「〔失败原因〕」；
// 前端把这一段渲染成红色小字（views/StoryPanel.tsx 的 failureOf）。
let marked = format!("{full}\n\n〔失败原因〕{why}");
            let msg_branch: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
                .bind(node_id).fetch_optional(&pool).await.ok().flatten().flatten();
            let _ = sqlx::query("INSERT INTO messages(node_id, role, content, mode, branch_id, meta) VALUES(?1, 'assistant', ?2, ?3, ?4, ?5)")
                .bind(node_id).bind(&marked).bind(mode).bind(msg_branch).bind(&msg_meta).execute(&pool).await;
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
        match crate::memory::settle_promise_in(&dir, fulfilled, "model") {
            Ok(Some(name)) => eprintln!("[promise] {name} → {}", if fulfilled { "已兑现" } else { "未兑现" }),
            Ok(None) => {}
            Err(e) => eprintln!("[promise] 结算失败: {e}"),
        }
    }
    let body = crate::promise::strip_line(&crate::stat_change::strip_line(&crate::director::strip_receipt(&full)));
    let msg_id: i64 = sqlx::query_scalar::<_, i64>(
        "INSERT INTO messages(node_id, role, content, mode, branch_id, meta) VALUES(?1, 'assistant', ?2, ?3, ?4, ?5) RETURNING id",
    )
    .bind(node_id).bind(&body).bind(mode).bind(msg_branch).bind(&msg_meta)
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
                    //
                    // **2026-10-31 用户定**：分是预算、可以估（收据与关键词取重）；
                    // 关系强度是事实，**只认模型自己写的收据**（source == "model"），
                    // 否则正文里恰好有「舍不得/眼泪」就会凭空扣她 5 点。
                    let penalty_ok = cand.source == "model";
                    if !penalty_ok {
                        // 关键词兜底只记账，不动关系
                    }
                    match if penalty_ok { crate::director::apply_penalty(&pool, &branch, ai_role, my_role, &cand.kind, Some(node_id), Some(msg_id)).await } else { Ok(None) } {
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
mod pair_prompt_tests;
#[cfg(test)]
mod tail_view_tests;
#[cfg(test)]
mod phase_recap_tests;

#[cfg(test)]
mod debug_cap_tests;

#[cfg(test)]
mod stream_protocol_tests;

#[cfg(test)]
mod timeout_tests {
    use super::{connect_timeout_secs, send_with_timeout, SendError};

    /// 一个「只接受连接、永远不回话」的端口 —— 真实网络里上游半开就是这样。
    async fn silent_server() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            // 接住连接后一直不写响应（把 socket 留在作用域里即可）
            let mut held = Vec::new();
            while let Ok((sock, _)) = listener.accept().await {
                held.push(sock);
            }
        });
        (port, handle)
    }

    /// E（ROADMAP #8）：超时链路真的会触发，而且报的是那句给人看的文案。
    ///
    /// 用 1 秒的覆盖值跑（生产是 90s）；断言耗时上限 10s，避免测试本身挂死。
    #[tokio::test]
    async fn request_times_out_on_silent_upstream() {
        let (port, handle) = silent_server().await;
        // 守卫禁止 `Client::new()`（防无超时客户端回潮），测试也用 builder 构造
        let client = reqwest::Client::builder().build().unwrap();
        let req = client.post(format!("http://127.0.0.1:{port}/chat/completions")).json(&serde_json::json!({"x": 1}));
        let started = std::time::Instant::now();
        let err = send_with_timeout(req, 1).await.expect_err("只接不答的端口必须超时");
        let elapsed = started.elapsed();
        handle.abort();
        match err {
            SendError::Timeout(msg) => {
                assert!(msg.contains("请求超时"), "文案变了：{msg}");
                assert!(msg.contains("1s"), "要把超时秒数说清楚：{msg}");
            }
            other => panic!("应该是超时而不是网络错误：{other:?}"),
        }
        assert!(elapsed.as_secs() < 10, "超时用了 {elapsed:?}，明显不对");
    }

    /// 连不上的端口 → 走**网络错误**分支（不是超时），两者文案必须能区分。
    #[tokio::test]
    async fn refused_connection_is_network_error_not_timeout() {
        // 绑一个端口再立刻放掉：几乎必然连不上（也有可能被别人抢到，所以只断言分支）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        // 守卫禁止 `Client::new()`（防无超时客户端回潮），测试也用 builder 构造
        let client = reqwest::Client::builder().build().unwrap();
        let req = client.post(format!("http://127.0.0.1:{port}/x")).json(&serde_json::json!({}));
        match send_with_timeout(req, 5).await {
            Err(SendError::Network(m)) => assert!(m.contains("请求失败"), "{m}"),
            Err(SendError::Timeout(m)) => panic!("不该判成超时：{m}"),
            Ok(_) => {} // 端口被抢到的极小概率：跳过
        }
    }

    /// 生产默认必须是 90s；环境变量只在 1..=600 内生效（防止误设把超时关掉）。
    #[test]
    fn default_timeout_is_90s_and_override_is_bounded() {
        std::env::remove_var("DREAMWEAVER_TEST_CONNECT_TIMEOUT_SECS");
        assert_eq!(connect_timeout_secs(), 90);
        std::env::set_var("DREAMWEAVER_TEST_CONNECT_TIMEOUT_SECS", "2");
        assert_eq!(connect_timeout_secs(), 2);
        std::env::set_var("DREAMWEAVER_TEST_CONNECT_TIMEOUT_SECS", "0");
        assert_eq!(connect_timeout_secs(), 90, "0 应该被忽略");
        std::env::set_var("DREAMWEAVER_TEST_CONNECT_TIMEOUT_SECS", "9999");
        assert_eq!(connect_timeout_secs(), 90, "超范围应该被忽略");
        std::env::remove_var("DREAMWEAVER_TEST_CONNECT_TIMEOUT_SECS");
    }
}