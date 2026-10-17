//! RAG ①（方案 A）：**回忆意图识别 + 父链旧日摘要检索 + 注入【相关旧日记忆】**
//!
//! 设计口径（2026-10-17 与用户确认）：
//!
//! * **不建标签表、不建首次事件表、不改摘要提示词** —— 直接对 `day_summary` 打分；
//! * **两条触发路径**：
//!   1. **自动**：本地规则识别用户输入里的"回忆意图"（纯函数、零 API 成本、可单测）
//!   2. **手动**：界面「🔍 回顾」按钮指定**某一天** → 直接强制把那天拼进提示词
//! * **范围**：只在本分支**可见范围**内（`branch_path_ids` = 本分支节点 ∪ 父链共享历史），
//!   且只取**早于近三天原文区**、有 `day_summary` 的天
//! * **参与预算**：检索块是可变成本，超预算先砍它，绝不挤占近三天原文
//! * **`memory=false` 时完全不检索、不注入**（沿用现有开关）
//!
//! 词表目前**内置**，后续可按需扩充（用户 2026-10-17 指定了起始集合）。

use sqlx::Row;
use sqlx::SqlitePool;

/// 普通标签词（用于"日常小事"级别的回忆线索）。
///
/// 用户给的起始集合：牵手、亲吻、院子、害羞、争吵、和解 —— 后面按需补。
pub const TAG_WORDS: &[&str] = &[
    "牵手", "亲吻", "院子", "害羞", "争吵", "和解",
    // 常用补充（可删可加，不影响逻辑）
    "拥抱", "告白", "生气", "眼泪", "礼物", "做饭", "散步", "第一次一起",
];

/// 可识别的**首次事件**：`(事件键, 触发词)`。
///
/// 命中方式：触发词出现在「用户输入」或「日摘要正文」里都算。
/// 事件键目前只用于去重/说明，不落库（第一版不建事件表）。
pub const FIRST_EVENTS: &[(&str, &[&str])] = &[
    ("first_meet", &["首次相遇", "第一次见面", "初次见面", "第一次遇到"]),
    ("first_hold", &["首次牵手", "第一次牵手"]),
    ("first_kiss", &["首次亲吻", "第一次亲吻", "初吻", "第一次亲"]),
    ("first_confess", &["首次告白", "第一次告白", "第一次表白", "表白"]),
];

/// 强线索词：出现即视为"想回顾旧事"。
///
/// 取舍：**宁可多带一条摘要，也不漏**（漏了用户会觉得"AI 忘了"；
/// 多带一条最坏也只是多花一点 token）。
const STRONG_CUES: &[&str] = &[
    "第一次", "首次", "初次", "那次", "当时", "当年", "当初",
    "还记得", "记不记得", "还记不记得", "你记得",
    "上次", "上回", "之前你", "以前你", "回顾", "回想", "想起来",
];

/// 意图识别结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Intent {
    /// 是否判定为"想回顾"
    pub is_recall: bool,
    /// 命中的线索（用于界面解释"为什么带了这条记忆"）
    pub clues: Vec<String>,
    /// 提到的"第 N 天"
    pub day: Option<i64>,
    /// 命中的首次事件标签（人工可读）
    pub events: Vec<String>,
}

/// 从文本里解析"第 N 天"（支持阿拉伯数字与中文数字，1~99）。
pub fn parse_day(text: &str) -> Option<i64> {
    let chars: Vec<char> = text.chars().collect();
    for i in 0..chars.len() {
        if chars[i] != '第' {
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        // 收集数字部分（阿拉伯或中文）
        let mut num = String::new();
        while j < chars.len() && (chars[j].is_ascii_digit() || is_cn_numeral(chars[j])) {
            num.push(chars[j]);
            j += 1;
        }
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if !num.is_empty() && j < chars.len() && chars[j] == '天' {
            if let Some(n) = parse_number(&num) {
                return Some(n);
            }
        }
    }
    None
}

fn is_cn_numeral(c: char) -> bool {
    matches!(c, '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '零' | '两')
}

fn cn_digit(c: char) -> Option<i64> {
    Some(match c {
        '零' => 0, '一' => 1, '二' | '两' => 2, '三' => 3, '四' => 4,
        '五' => 5, '六' => 6, '七' => 7, '八' => 8, '九' => 9,
        _ => return None,
    })
}

/// 解析 1~99 的数字串（阿拉伯 / 中文皆可）。
fn parse_number(s: &str) -> Option<i64> {
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse::<i64>().ok();
    }
    // 中文：十 / 十X / X十 / X十Y
    let chars: Vec<char> = s.chars().collect();
    // **先处理"十"**：单个"十"=10；"十一"=11；"二十"=20；"二十三"=23
    if let Some(pos) = chars.iter().position(|c| *c == '十') {
        let hi = if pos == 0 { 1 } else { cn_digit(chars[0])? };
        let lo = if pos + 1 < chars.len() { cn_digit(chars[pos + 1])? } else { 0 };
        return Some(hi * 10 + lo);
    }
    if chars.len() == 1 {
        return cn_digit(chars[0]);
    }
    // 多个单字（如"二五"这种不该出现）→ 取第一个
    cn_digit(chars[0])
}

/// 识别回忆意图（**纯函数**，不碰数据库、不花 token）。
pub fn detect_intent(text: &str) -> Intent {
    let mut intent = Intent {
        day: parse_day(text),
        ..Default::default()
    };

    for cue in STRONG_CUES {
        if text.contains(cue) {
            intent.clues.push((*cue).to_string());
        }
    }
    // 「第 N 天」本身就是强线索
    if let Some(d) = intent.day {
        intent.clues.push(format!("第 {d} 天"));
    }
    for w in TAG_WORDS {
        if text.contains(w) {
            intent.clues.push((*w).to_string());
        }
    }
    for (_key, words) in FIRST_EVENTS {
        for w in *words {
            if text.contains(w) {
                intent.events.push((*w).to_string());
                break;
            }
        }
    }
    // 去重（保持顺序）
    intent.clues.dedup();
    intent.clues.dedup_by(|a, b| a == b);
    let mut seen = std::collections::HashSet::new();
    intent.clues.retain(|c| seen.insert(c.clone()));

    // 判定：有强线索 / 提到具体某天 / 命中首次事件 → 要回顾
    // 「只有标签词」（例如"院子"）不算 —— 那可能只是在描述当下场景。
    let has_strong = !seen.is_empty() && STRONG_CUES.iter().any(|c| text.contains(c));
    intent.is_recall = has_strong || intent.day.is_some() || !intent.events.is_empty();
    intent
}

/// 一条检索结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecallHit {
    pub node_id: i64,
    pub date_label: String,
    pub day: Option<i64>,
    pub summary: String,
    pub score: i32,
    /// 为什么带上它（界面/调试可读）
    pub why: Vec<String>,
}

/// 给一条日摘要打分。**纯函数**，便于单测排序逻辑。
///
/// 权重口径（高 → 低）：
///   首次事件命中 100 · 提到的那一天命中 80 · 每个标签词 20（其它线索词 12）· 角色名 **3**
///
/// **关键取舍（2026-10-17 实测后调整）**：
/// 角色名几乎出现在每一条摘要里（案例数据 10 天里 10 天都有主角名），
/// 一开始给 +10 会让"当时在院子里"这类输入稳定返回同样的第 1/2/3 天 —— 纯噪声。
/// 所以：
///   ① 角色名只给 **+3**（聊胜于无的 tie-breaker）；
///   ② **必须至少有一条"内容信号"**（事件 / 那天 / 标签词）才计分，
///      只有角色名的摘要直接返回 0 —— 宁可什么都不带，也不要带一堆无关的天。
pub fn score_summary(
    summary: &str,
    day: Option<i64>,
    intent_day: Option<i64>,
    clues: &[String],
    intent_events: &[String],
    char_names: &[String],
    recency_rank: usize,
) -> (i32, Vec<String>) {
    let mut score = 0i32;
    let mut why: Vec<String> = Vec::new();
    let mut has_content_signal = false;

    // ① 首次事件（用户明确提到 or 摘要里写了"第一次…"）
    for ev in intent_events {
        if summary.contains(ev) {
            score += 100;
            has_content_signal = true;
            why.push(format!("命中首次事件「{ev}」"));
        }
    }
    for (_key, words) in FIRST_EVENTS {
        for w in *words {
            if summary.contains(w) && !why.iter().any(|x| x.contains(w)) {
                score += 100;
                has_content_signal = true;
                why.push(format!("摘要含「{w}」"));
                break;
            }
        }
    }

    // ② 用户提到的具体某天
    if let (Some(a), Some(b)) = (intent_day, day) {
        if a == b {
            score += 80;
            has_content_signal = true;
            why.push(format!("就是第 {a} 天"));
        }
    }

    // ③ 标签词 / 其它线索词
    for c in clues {
        if summary.contains(c) {
            let is_tag = TAG_WORDS.contains(&c.as_str());
            score += if is_tag { 20 } else { 12 };
            has_content_signal = true;
            why.push(format!("含「{c}」"));
        }
    }

    // ④ 角色名：**只做微弱加权**（覆盖面太广，给大了就是噪声）
    for n in char_names {
        if !n.is_empty() && summary.contains(n) {
            score += 3;
            why.push(format!("含角色「{n}」"));
        }
    }

    // 没有任何内容信号 → 直接判 0（哪怕名字全中）
    if !has_content_signal {
        return (0, Vec::new());
    }

    // ⑤ 时间接近度（越靠近当前越可能是"那次"）：0~9 的微弱加权
    score += (recency_rank.min(9)) as i32;

    // 说明去重，且只留前 4 条（界面/日志别太长）
    let mut seen = std::collections::HashSet::new();
    why.retain(|w| seen.insert(w.clone()));
    why.truncate(4);
    (score, why)
}

/// 把检索结果渲染成提示词区块（**纯函数**）。
pub fn render_block(hits: &[RecallHit]) -> String {
    if hits.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\n\n【相关旧日记忆】（根据本次提问检索到的远期摘要；用于回应「那次/当时」，具体细节仍以历史对话为准）\n",
    );
    for h in hits {
        s.push_str(&format!("- {}：{}\n", h.date_label, h.summary.trim()));
    }
    s
}

/// 按 token 预算裁剪（**纯函数**）：从头保留，直到超预算。
///
/// `cost` 是估算每条的 token 成本函数（由调用方传入 `estimate_tokens`）。
pub fn fit_hits(hits: Vec<RecallHit>, budget_tokens: usize, cost: impl Fn(&str) -> usize) -> (Vec<RecallHit>, usize) {
    let mut kept = Vec::new();
    let mut used = 0usize;
    let mut dropped = 0usize;
    for h in hits {
        let c = cost(&h.summary);
        if used + c > budget_tokens {
            dropped += 1;
            continue;
        }
        used += c;
        kept.push(h);
    }
    (kept, dropped)
}

/// 检索：在本分支**可见范围**内找与本次输入相关的旧日摘要。
///
/// * `visible` 来自 `timeline::branch_path_ids` —— **绝不跨分支**（2026-10-17 修过两次同类 bug）
/// * `exclude` 是"近三天原文区已经带上的节点"，那些没必要再以摘要形式重复
/// * `forced_node` 有值时（界面「🔍 回顾」指定了某一天）→ **直接强制取那一天**，忽略打分
pub async fn search(
    pool: &SqlitePool,
    branch: &Option<String>,
    current_node_id: i64,
    exclude: &std::collections::HashSet<i64>,
    intent: &Intent,
    forced_node: Option<i64>,
    top_k: usize,
) -> Result<Vec<RecallHit>, sqlx::Error> {
    let visible = crate::timeline::branch_path_ids(pool, branch).await;
    if visible.is_empty() {
        return Ok(Vec::new());
    }

    // 角色名（用于"摘要里提到了某个角色"这一档加权）
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let names_sql = format!("SELECT name FROM characters WHERE {clause} ORDER BY id");
    let mut nq = sqlx::query_scalar::<_, String>(&names_sql);
    if branch.is_some() {
        nq = nq.bind(branch.clone().unwrap_or_default());
    }
    let char_names: Vec<String> = nq.fetch_all(pool).await.unwrap_or_default();

    let rows = sqlx::query(
        "SELECT id, date_label, COALESCE(day_summary, '') FROM timeline_nodes
         ORDER BY sort_order ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;

    // ── 界面指定了某一天：强制取它，不做打分 ──
    if let Some(fid) = forced_node {
        if !visible.contains(&fid) {
            return Ok(Vec::new()); // 不在本分支可见范围 → 不跨分支翻别人那天
        }
        if let Some(r) = rows.iter().find(|r| r.try_get::<i64, _>(0).unwrap_or(-1) == fid) {
            let summary: String = r.try_get::<String, _>(2).unwrap_or_default();
            if summary.trim().is_empty() {
                return Ok(Vec::new());
            }
            let label: String = r.try_get::<String, _>(1).unwrap_or_default();
            return Ok(vec![RecallHit {
                node_id: fid,
                day: crate::timeline::parse_day_number(&label),
                date_label: label,
                summary: summary.trim().to_string(),
                score: 1000,
                why: vec!["界面指定回顾这一天".to_string()],
            }]);
        }
        return Ok(Vec::new());
    }

    // ── 自动检索 ──
    let candidates: Vec<(i64, String, String)> = rows
        .iter()
        .filter_map(|r| {
            let id = r.try_get::<i64, _>(0).ok()?;
            if !visible.contains(&id) || id == current_node_id || exclude.contains(&id) {
                return None;
            }
            let label: String = r.try_get::<String, _>(1).unwrap_or_default();
            let summary: String = r.try_get::<String, _>(2).unwrap_or_default();
            if summary.trim().is_empty() {
                return None;
            }
            Some((id, label, summary))
        })
        .collect();

    let n = candidates.len();
    let mut hits: Vec<RecallHit> = Vec::new();
    for (idx, (id, label, summary)) in candidates.iter().enumerate() {
        let day = crate::timeline::parse_day_number(label);
        // 越靠后（越接近当前）给越高的微弱加权
        let recency_rank = n.saturating_sub(idx).min(9);
        let (score, why) = score_summary(
            summary,
            day,
            intent.day,
            &intent.clues,
            &intent.events,
            &char_names,
            recency_rank,
        );
        // 最低分阈值：单个标签词命中是 20，再加温度加权 → 20 以下基本都是噪声
        if score < 20 {
            continue;
        }
        hits.push(RecallHit {
            node_id: *id,
            date_label: label.clone(),
            day,
            summary: summary.trim().to_string(),
            score,
            why,
        });
    }

    hits.sort_by(|a, b| b.score.cmp(&a.score).then(b.day.cmp(&a.day)));
    hits.truncate(top_k.max(1));
    Ok(hits)
}

/// 可供「🔍 回顾」选择的一天。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecallDay {
    pub node_id: i64,
    pub date_label: String,
    pub day: Option<i64>,
    /// 摘要开头（界面列表里预览用，截断到 ~60 字）
    pub preview: String,
}

/// 列出**本分支可见**、且有日摘要的天（倒序 = 最近的在前）。
///
/// 界面的「🔍 回顾」按钮用它弹一个"选哪一天"的列表 ——
/// 用户自己指哪天最可靠，不依赖意图识别的准确率（文档 8.1 的兜底）。
pub async fn list_days(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<RecallDay>, sqlx::Error> {
    let visible = crate::timeline::branch_path_ids(pool, branch).await;
    if visible.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT id, date_label, COALESCE(day_summary, '') FROM timeline_nodes
         ORDER BY sort_order DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get(0).unwrap_or(-1);
        if !visible.contains(&id) {
            continue;
        }
        let label: String = r.try_get(1).unwrap_or_default();
        let summary: String = r.try_get(2).unwrap_or_default();
        if summary.trim().is_empty() {
            continue; // 没摘要的天拿不出东西可拼
        }
        let preview: String = summary.trim().chars().take(60).collect();
        out.push(RecallDay {
            node_id: id,
            day: crate::timeline::parse_day_number(&label),
            date_label: label,
            preview,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_day_with_digits_and_chinese() {
        assert_eq!(parse_day("还记得第 5 天吗"), Some(5));
        assert_eq!(parse_day("第5天发生了什么"), Some(5));
        assert_eq!(parse_day("第三天那次"), Some(3));
        assert_eq!(parse_day("第十天"), Some(10));
        assert_eq!(parse_day("第  7  天"), Some(7));
        assert_eq!(parse_day("没有天数"), None);
        assert_eq!(parse_day("第 5 章"), None);
    }

    #[test]
    fn strong_cues_trigger_recall() {
        for s in ["你还记得吗", "那次在院子", "当时我说话", "我们第一次牵手的时候", "回顾一下第 2 天"] {
            assert!(detect_intent(s).is_recall, "应识别为回忆意图：{s}");
        }
    }

    #[test]
    fn plain_scene_description_is_not_recall() {
        // 只是在描述当下场景 → 不该触发检索（否则每次都多带摘要）
        for s in ["我们走进院子", "她有点害羞", "外面在下雨"] {
            assert!(!detect_intent(s).is_recall, "不该判为回忆意图：{s}");
        }
    }

    #[test]
    fn first_event_words_are_recognised() {
        let i = detect_intent("我们首次牵手的时候");
        assert!(i.is_recall);
        assert!(i.events.iter().any(|e| e.contains("牵手")), "应识别出首次牵手：{i:?}");

        let i2 = detect_intent("第一次告白那天");
        assert!(i2.is_recall);
        assert_eq!(i2.day, None);
    }

    #[test]
    fn day_mention_is_a_clue() {
        let i = detect_intent("第 8 天的时候你在做什么");
        assert_eq!(i.day, Some(8));
        assert!(i.clues.iter().any(|c| c.contains("8")));
        assert!(i.is_recall);
    }

    #[test]
    fn scoring_ranks_first_event_highest() {
        let clues = vec!["院子".to_string()];
        let (s_event, why) = score_summary(
            "两人第一次牵手", Some(5), None, &clues, &[], &[], 0,
        );
        let (s_tag, _) = score_summary("在院子里散步", Some(6), None, &clues, &[], &[], 0);
        assert!(s_event > s_tag, "首次事件应高于普通标签：{why:?}");
    }

    #[test]
    fn scoring_uses_requested_day() {
        let (s_hit, why) = score_summary("随便一句话", Some(5), Some(5), &[], &[], &[], 0);
        let (s_miss, _) = score_summary("随便一句话", Some(6), Some(5), &[], &[], &[], 0);
        assert!(s_hit > s_miss);
        assert!(why.iter().any(|w| w.contains("第 5 天")), "{why:?}");
    }

    #[test]
    fn render_skips_empty() {
        assert!(render_block(&[]).is_empty());
        let block = render_block(&[RecallHit {
            node_id: 5, date_label: "第 5 天".to_string(), day: Some(5),
            summary: "两人在院子里散步".into(), score: 100, why: vec![],
        }]);
        assert!(block.contains("【相关旧日记忆】"));
        assert!(block.contains("第 5 天：两人在院子里散步"));
    }

    #[test]
    fn fit_hits_drops_tail_when_over_budget() {
        let mk = |id: i64, text: &str| RecallHit {
            node_id: id, date_label: format!("第 {id} 天"), day: Some(id),
            summary: text.into(), score: 10, why: vec![],
        };
        let hits = vec![mk(1, "aaaa"), mk(2, "bbbb"), mk(3, "cccc")];
        let (kept, dropped) = fit_hits(hits, 8, |s| s.chars().count());
        assert_eq!(kept.len(), 2, "预算 8 只装得下两条");
        assert_eq!(dropped, 1);
    }
}
