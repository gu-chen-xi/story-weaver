// 从 `recall.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `recall.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use score::*;` 取回，
// 外部 `crate::recall::xxx` 的调用路径**全部不变**。

use super::*;

/// 回顾打分的权重（C3，2026-10-18：从硬编码改成**可配置**）。
///
/// 为什么可配：这些数字是拍出来的，合不合适只有实际用起来才知道 ——
/// 与其反复改代码重编译，不如留配置口子 + 「试算」入口（`recall_explain`）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecallWeights {
    /// 用户明确问了某个"首次事件"（首次牵手 / 初吻 …）
    pub event: i32,
    /// 用户直接提到「第 N 天」
    pub day_exact: i32,
    /// A：当天**手打标签**被用户提到
    pub hand_tag: i32,
    /// C：标签词（牵手/亲吻/院子…）命中
    pub tag_word: i32,
    /// C：命名实体（地点 / 术语 / 势力名）命中
    pub entity: i32,
    /// C：用户**点名提到某个角色**（与 tag_word 解耦：调标签词不该影响名字的分）
    pub name_hit: i32,
    /// 兜底：摘要里出现角色名（仅 tie-breaker）
    pub name_weak: i32,
    /// 最低分门槛：低于它的天不会被带出
    pub min_score: i32,
    /// 时间接近度加权上限
    pub recency_max: i32,
}

/// 配置键：整套权重以一个 JSON 存（改一项也存整套，避免半套配置）
pub const CFG_RECALL_WEIGHTS: &str = "recall.weights";

/// 读权重；没有配置 / JSON 坏了就回默认（**永不因为配置问题让回顾失效**）。
pub async fn load_weights(lib_pool: &SqlitePool) -> RecallWeights {
    match crate::db::get_config(lib_pool, CFG_RECALL_WEIGHTS).await {
        Ok(Some(s)) if !s.trim().is_empty() => {
            serde_json::from_str::<RecallWeights>(s.trim()).unwrap_or_default()
        }
        _ => RecallWeights::default(),
    }
}

/// 可识别的**首次事件**：`(事件键, 触发词)`。
///
/// 命中方式：触发词出现在「用户输入」或「日摘要正文」里都算。
/// 事件键目前只用于去重/说明，不落库（第一版不建事件表）。
/// 注意：**不要放裸「表白 / 告白」这类常用词**。
/// 摘要里出现"表白"只说明那天发生了表白，不代表"首次"，
/// 曾因此让某一天对所有输入都拿到 +100 而霸榜（实测踩到）。
pub const FIRST_EVENTS: &[(&str, &[&str])] = &[
    ("first_meet", &["首次相遇", "第一次见面", "初次见面", "第一次遇到"]),
    ("first_hold", &["首次牵手", "第一次牵手"]),
    ("first_kiss", &["首次亲吻", "第一次亲吻", "初吻", "第一次亲"]),
    ("first_confess", &["首次告白", "第一次告白", "第一次表白"]),
];

/// 强线索词：出现即视为"想回顾旧事"。
///
/// 取舍：**宁可多带一条摘要，也不漏**（漏了用户会觉得"AI 忘了"；
/// 多带一条最坏也只是多花一点 token）。
pub(super) const STRONG_CUES: &[&str] = &[
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
    /// 原始输入文本（A/C 用：判断"用户到底提了哪个词"）
    pub text: String,
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

pub(super) fn is_cn_numeral(c: char) -> bool {
    matches!(c, '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '零' | '两')
}

pub(super) fn cn_digit(c: char) -> Option<i64> {
    Some(match c {
        '零' => 0, '一' => 1, '二' | '两' => 2, '三' => 3, '四' => 4,
        '五' => 5, '六' => 6, '七' => 7, '八' => 8, '九' => 9,
        _ => return None,
    })
}

/// 解析 1~99 的数字串（阿拉伯 / 中文皆可）。
pub(super) fn parse_number(s: &str) -> Option<i64> {
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
    intent.text = text.to_string();

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
///
/// **关键设计（2026-10-17 用户实测后修正）**：
/// 回顾要注入的是**当天的原始对话**，不是日摘要。
/// 摘要只是"索引"（用来打分找哪一天），真正喂给模型的是原文 ——
/// 因为摘要已经被压缩过、细节全没了，而【前情提要】里本来就全是摘要，
/// 再塞摘要等于没回顾。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecallHit {
    pub node_id: i64,
    pub date_label: String,
    pub day: Option<i64>,
    /// 打分用的摘要（**不注入**，只用于打分与"为什么选它"）
    pub summary: String,
    /// 当天原始对话：(角色 user/assistant, 已剥导演标记的正文)
    pub messages: Vec<(String, String)>,
    /// 因为长度上限被省略的消息条数（渲染时会注明）
    pub omitted: usize,
    pub score: i32,
    /// 为什么带上它（界面/调试可读）
    pub why: Vec<String>,
}

impl RecallHit {
    /// 渲染成提示词里的一段（纯函数，可单测）。
    pub fn render(&self) -> String {
        let mut s = format!("- {}：\n", self.date_label);
        for (role, content) in &self.messages {
            let who = if role == "user" { "用户" } else { "AI" };
            s.push_str(&format!("  [{who}] {}\n", content.trim()));
        }
        if self.omitted > 0 {
            s.push_str(&format!("  （当天另有 {} 条未列出）\n", self.omitted));
        }
        s
    }
}

/// 按**字符数**上限裁剪一天的消息：**从最早往后保留**（回顾通常是在问"当时发生了什么"，
/// 开头更重要），超出部分从尾部丢弃并记录条数。
pub fn trim_day(messages: Vec<(String, String)>, max_chars: usize) -> (Vec<(String, String)>, usize) {
    let mut kept = Vec::new();
    let mut used = 0usize;
    let mut omitted = 0usize;
    for (role, content) in messages {
        let cost = content.chars().count() + 4;
        if used + cost > max_chars {
            omitted += 1;
            continue;
        }
        used += cost;
        kept.push((role, content));
    }
    (kept, omitted)
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
    // A：这一天的**手打标签**里，哪些词在用户输入中出现了
    hand_hits: &[String],
    // C：从词典（标签词/角色名/世界观名）里认出、且出现在用户输入中的词（带权重）
    lex_hits: &[LexTerm],
    // C3：权重（可配置）
    w: &RecallWeights,
) -> (i32, Vec<String>) {
    let mut score = 0i32;
    let mut why: Vec<String> = Vec::new();
    let mut has_content_signal = false;

    // ① 首次事件：**只在用户确实问了那个事件**时才计分。
    //
    // 反例（实测踩到）：用户问"当时在院子里"，某天摘要里恰好有"表白"两个字，
    // 就拿到 +100 霸榜 —— 但用户问的根本不是表白。
    // 所以这里只对 `intent_events`（从用户输入解析出来的）做匹配。
    for ev in intent_events {
        if summary.contains(ev) {
            score += w.event;
            has_content_signal = true;
            why.push(format!("命中首次事件「{ev}」"));
        }
    }

    // ② 用户提到的具体某天
    if let (Some(a), Some(b)) = (intent_day, day) {
        if a == b {
            score += w.day_exact;
            has_content_signal = true;
            why.push(format!("就是第 {a} 天"));
        }
    }

    // ②.5 A：**手打标签**命中 —— 用户自己给那天标过词，是最可靠的人工信号
    //
    // 例：用户把第 5 天标成「#约会」，之后说"那次约会" → 直接命中第 5 天。
    // 原来这些标签只显示、不参与检索（用户 2026-10-18 指出该问题）。
    for tag in hand_hits {
        score += w.hand_tag;
        has_content_signal = true;
        why.push(format!("那天标了「#{tag}」"));
    }

    // ②.6 C：**词典命中**（标签词 20 / 命名实体 12）
    //
    // 为什么实体只给 12：角色名/地点名覆盖面广，给大了就是噪声（角色名单独还有 +3 的弱加权）。
    // 这一档的价值在"用户提到的具体物件/地点/术语"——不再是内置 14 个词的死名单。
    for lt in lex_hits {
        if summary.contains(lt.word.as_str()) {
            score += match lt.kind {
                LexKind::TagWord => w.tag_word,
                LexKind::Entity => w.entity,
                LexKind::Name => w.name_hit,
            };
            has_content_signal = true;
            why.push(format!("含「{}」", lt.word));
        }
    }

    // ③ 【已删除】原本这里还要单独给"标签词"计一次分。
    //
    // C3 引入 `lex_hits`（词典 = 标签词 + 实体名 + 角色名）之后，这段就成了**重复计分**：
    // 用户说"那次牵手"，标签词「牵手」会先在上面的 C 段落加一次 `w.tag_word`，
    // 这里再加一次 → 同一个词算两遍。实测表现：把 tag_word 从 20 调到 50，
    // 分数涨了 **60**（2×30）而不是 30 —— 用户发现的。
    //
    // 保留的历史理由（依旧成立）：
    // 为什么不像更早那样把"第一次 / 当时 / 那次"也拿来和摘要匹配：
    // 那些词是**意图触发词**，不是内容指纹 —— 摘要里出现"当时"毫无信息量，
    // 只会让某一天对所有输入都得分（实测里"当时在院子里"因此带回一条无关的天）。
    let _ = clues; // 参数保留（调用方仍传），但不再参与计分

    // ④ 角色名：**只做微弱加权**（覆盖面太广，给大了就是噪声）
    for n in char_names {
        if !n.is_empty() && summary.contains(n) {
            score += w.name_weak;
            why.push(format!("含角色「{n}」"));
        }
    }

    // 没有任何内容信号 → 直接判 0（哪怕名字全中）
    if !has_content_signal {
        return (0, Vec::new());
    }

    // ⑤ 时间接近度（越靠近当前越可能是"那次"）：0~9 的微弱加权
    score += ((recency_rank as i32).min(w.recency_max)).max(0);

    // 说明去重，且只留前 4 条（界面/日志别太长）
    let mut seen = std::collections::HashSet::new();
    why.retain(|w| seen.insert(w.clone()));
    why.truncate(4);
    (score, why)
}

/// 把检索结果渲染成提示词区块（**纯函数**）。
///
/// 区块标题明确写"原始对话"，避免模型把它当摘要复述；
/// 也避免模型以为这是"更早的剧情摘要"（那是【前情提要】的语义）。
pub fn render_block(hits: &[RecallHit]) -> String {
    if hits.is_empty() {
        return String::new();
    }
    let mut s = String::from(
        "\n\n【相关旧日记忆】（根据本次提问恢复的**当时原始对话**，用于回应「那次/当时/还记得」；以此为准，不要改写）\n",
    );
    for h in hits {
        s.push_str(&h.render());
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
        // 成本按**实际要注入的原文**算，不是摘要
        let c = cost(&h.render());
        if used + c > budget_tokens {
            dropped += 1;
            continue;
        }
        used += c;
        kept.push(h);
    }
    (kept, dropped)
}
