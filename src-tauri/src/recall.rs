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

impl Default for RecallWeights {
    fn default() -> Self {
        // v3.4：entity 从 12 提到 20（= tag_word）。原因：12 < min_score 20，
            // 意味着"用户点名了一个专有名词、且它真的出现在那天摘要里"这种**最强信号**
            // 永远带不出那一天（实测：物品「显卡」写入第 4 天，回顾试算只显示命中 14 分、picked=false）。
            // 专有名词比"日常标签词"更确定，不该比它低。
            Self { event: 100, day_exact: 80, hand_tag: 25, tag_word: 20, entity: 20, name_hit: 20, name_weak: 3, min_score: 20, recency_max: 9 }
    }
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

/// 普通标签词（用于"日常小事"级别的回忆线索）。
///
/// 用户给的起始集合：牵手、亲吻、院子、害羞、争吵、和解 —— 后面按需补。
pub const TAG_WORDS: &[&str] = &[


    "牵手", "亲吻", "院子", "害羞", "争吵", "和解",
    // 常用补充（可删可加，不影响逻辑）
    "拥抱", "告白", "生气", "眼泪", "礼物", "做饭", "散步", "第一次一起",
];

/// 从 `meta_json` 里取别名列表（`aliases` 字段，逗号/中文逗号/顿号分隔）。
///
/// 为什么要它：召回词典原先只取 `world_settings.key`，于是
/// 「rtx显卡（别名：显卡）」这种条目，用户输入"显卡"永远匹配不上 —— 实测踩到。
pub(crate) fn split_aliases(meta_json: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(meta_json).unwrap_or(serde_json::json!({}));
    let raw = v.get("aliases").and_then(|x| x.as_str()).unwrap_or("");
    raw.split(|c| c == ',' || c == '，' || c == '、' || c == ';' || c == '；')
        .map(|s| s.trim().to_string())
        .filter(|s| s.chars().count() >= 2)
        .collect()
}

/// 召回词典里的"实体词"：实体类分类的**名称 + 别名**。
///
/// **必须只有一个实现**：`search`（真正检索）和 `explain`（「回顾试算」）原来各写了一份，
/// 结果 explain 那份忘了加 `物品`，导致"界面上加的物品，回顾试算里看不见"。
async fn push_world_terms(
    pool: &SqlitePool,
    branch: &Option<String>,
    lexicon: &mut Vec<LexTerm>,
) {
    let wcat = crate::story_db::branch_clause("branch_id", branch, 1);
    // 只取**实体类**分类（地理/术语/势力/物品）；"设定"分类的键多是容器（如"背景介绍"），
    // 拿它当检索词会命中大量无关的天（实测：`背景介绍` 一个词就能抬高所有天）。
    let wsql = format!(
        "SELECT key, meta_json FROM world_settings
         WHERE category IN ('地理','术语','势力','物品') AND {wcat} ORDER BY id"
    );
    let mut wq = sqlx::query(&wsql);
    if let Some(b) = branch {
        wq = wq.bind(b);
    }
    let rows = wq.fetch_all(pool).await.unwrap_or_default();
    for r in &rows {
        let key: String = r.try_get(0).unwrap_or_default();
        let meta: String = r.try_get(1).unwrap_or_default();
        let mut cands = vec![key.trim().to_string()];
        cands.extend(split_aliases(&meta));
        for c in cands {
            if c.chars().count() >= 2 && !lexicon.iter().any(|x| x.word == c) {
                lexicon.push(LexTerm { word: c, kind: LexKind::Entity });
            }
        }
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

/// 读某一天的原始消息（剥掉导演指令标记，跳过空消息）。
pub async fn load_day_messages(
    pool: &SqlitePool,
    node_id: i64,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    let rows = sqlx::query("SELECT role, content FROM messages WHERE node_id = ?1 ORDER BY id ASC")
        .bind(node_id)
        .fetch_all(pool)
        .await?;
    let mut out = Vec::new();
    for r in &rows {
        let role: String = r.try_get(0).unwrap_or_default();
        let content: String = r.try_get(1).unwrap_or_default();
        let content = crate::relations::strip_director(&content);
        if content.trim().is_empty() {
            continue;
        }
        out.push((role, content));
    }
    Ok(out)
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

/// 检索词的**类别**（决定用哪个权重）。
///
/// 为什么不用"数字大小"猜：C3 早期版本用 `weight >= 20` 判断"是标签词还是实体"，
/// 而角色名当时也塞了 20 → 被当成标签词计分。结果是**调 tag_word 会连带改掉角色名的分**：
/// 用户把 tag_word 20→50，分数涨了 60（1 个标签词 + 1 个角色名，各 30）而不是 30。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexKind {
    /// 内置标签词（牵手/亲吻/院子…）
    TagWord,
    /// 命名实体（地点 / 术语 / 势力名）
    Entity,
    /// 角色名（用户点名提到某人）
    Name,
}

/// 一个"检索词"及其类别。
#[derive(Debug, Clone, PartialEq)]
pub struct LexTerm {
    pub word: String,
    pub kind: LexKind,
}

/// 从「用户输入」里认出词典词（**纯函数**，A+C 的核心）。
///
/// * `input`：用户这一句原话
/// * `lexicon`：`(词, 权重)`
///
/// 只保留长度 ≥ 2 的词，避免单字（"院""手"）满屏误命中；
/// 结果按权重降序、同权重按词长降序（长词更具体，先匹配）。
pub fn match_lexicon(input: &str, lexicon: &[LexTerm]) -> Vec<LexTerm> {
    let mut out: Vec<LexTerm> = lexicon
        .iter()
        .filter(|t| t.word.chars().count() >= 2)
        .filter(|t| input.contains(t.word.as_str()))
        .cloned()
        .collect();
    // 排序：长词优先（更具体）。同类之间不需要再按分数排 —— 类别决定权重。
    out.sort_by(|a, b| b.word.chars().count().cmp(&a.word.chars().count()));
    out.dedup_by(|a, b| a.word == b.word);
    out
}

/// 从「某一天的日摘要」里派生可检索标签（**纯函数**，C 的"零成本补录"）。
///
/// 规则：标签词 + 命名实体（角色名 / 世界观名）**出现在摘要里**就算这一天带这个标签。
/// 不调模型、不改摘要、不落库 —— 每次检索现算，天然与摘要同步。
pub fn derive_day_tags(
    summary: &str,
    hand_tags: &[String],
    names: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in hand_tags {
        let s = t.trim();
        if !s.is_empty() {
            out.push(s.to_string());
        }
    }
    for w in TAG_WORDS {
        if summary.contains(w) && !out.iter().any(|x| x == w) {
            out.push((*w).to_string());
        }
    }
    for n in names {
        let s = n.trim();
        if s.chars().count() >= 2 && summary.contains(s) && !out.iter().any(|x| x == s) {
            out.push(s.to_string());
        }
    }
    out
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

/// 检索：在本分支**可见范围**内找与本次输入相关的旧日。
///
/// * `visible` 来自 `timeline::branch_path_ids` —— **绝不跨分支**
/// * `exclude` 是"近三天原文区已经带上的节点"，那些没必要再重复带
/// * `forced_node` 有值时（界面「🔍 回顾」指定了某一天）→ **直接强制取那一天**，忽略打分
/// * `per_day_chars`：单天原文的字符上限（超出从**尾部**丢弃并注明条数）
///
/// **打分用摘要、注入用原文**：摘要已被压缩，只适合当"索引"；
/// 真正给模型看的是当天原始对话（用户 2026-10-17 实测后要求改的）。
pub async fn search(
    pool: &SqlitePool,
    branch: &Option<String>,
    current_node_id: i64,
    exclude: &std::collections::HashSet<i64>,
    intent: &Intent,
    forced_node: Option<i64>,
    top_k: usize,
    per_day_chars: usize,
    // C3：权重（调用方用 `load_weights(lib_pool)` 读好传进来）
    w: &RecallWeights,
) -> Result<Vec<RecallHit>, sqlx::Error> {
    let visible = crate::timeline::branch_path_ids(pool, branch).await;
    if visible.is_empty() {
        return Ok(Vec::new());
    }

    // 角色名（仅用于打分时的微弱加权）
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let names_sql = format!("SELECT name FROM characters WHERE {clause} ORDER BY id");
    let mut nq = sqlx::query_scalar::<_, String>(&names_sql);
    if branch.is_some() {
        nq = nq.bind(branch.clone().unwrap_or_default());
    }
    let char_names: Vec<String> = nq.fetch_all(pool).await.unwrap_or_default();

    // C：词典 = 标签词(20) + 世界观名(12)。角色名另有弱加权，不重复进词典
    //    （角色名进词典会让"提到某角色"把所有含该角色的天一起抬高，噪声太大）。
    let mut lexicon: Vec<LexTerm> = TAG_WORDS.iter().map(|w| LexTerm { word: (*w).to_string(), kind: LexKind::TagWord }).collect();
    // 实体类词（地理/术语/势力/物品）+ 它们的别名，走同一份实现
    push_world_terms(pool, branch, &mut lexicon).await;
    // 角色名也算"检索词"：用户明确提到「院长」时，含院长的那些天才是候选。
    // 权重与标签词同档（20）—— 前提是**名字确实出现在用户这句话里**（match_lexicon 保证），
    // 所以不会像"摘要里出现名字就加分"那样满屏误命中。
    for n in &char_names {
        let n = n.trim().to_string();
        if n.chars().count() >= 2 && !lexicon.iter().any(|x| x.word == n) {
            lexicon.push(LexTerm { word: n, kind: LexKind::Name });
        }
    }

    // 用户这一句里出现了哪些词典词（长度 ≥2，按权重降序）
    let lex_hits = match_lexicon(&intent.text, &lexicon);

    let rows = sqlx::query(
        "SELECT id, date_label, COALESCE(day_summary, ''), COALESCE(tags, '') FROM timeline_nodes
         ORDER BY sort_order ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;

    // ── 界面指定了某一天：强制取它，不做打分 ──
    if let Some(fid) = forced_node {
        if !visible.contains(&fid) {
            return Ok(Vec::new()); // 不在本分支可见范围 → 不跨分支翻别人那天
        }
        let Some(r) = rows.iter().find(|r| r.try_get::<i64, _>(0).unwrap_or(-1) == fid) else {
            return Ok(Vec::new());
        };
        let label: String = r.try_get::<String, _>(1).unwrap_or_default();
        let summary: String = r.try_get::<String, _>(2).unwrap_or_default();
        let msgs = load_day_messages(pool, fid).await?;
        if msgs.is_empty() {
            return Ok(Vec::new());
        }
        let (msgs, omitted) = trim_day(msgs, per_day_chars);
        return Ok(vec![RecallHit {
            node_id: fid,
            day: crate::timeline::parse_day_number(&label),
            date_label: label,
            summary,
            messages: msgs,
            omitted,
            score: 1000,
            why: vec!["界面指定回顾这一天".to_string()],
        }]);
    }

    // ── 自动检索：先用**摘要**打分（摘要相当于索引），再加载命中那几天的原文 ──
    let candidates: Vec<(i64, String, String, Vec<String>)> = rows
        .iter()
        .filter_map(|r| {
            let id = r.try_get::<i64, _>(0).ok()?;
            if !visible.contains(&id) || id == current_node_id || exclude.contains(&id) {
                return None;
            }
            let label: String = r.try_get::<String, _>(1).unwrap_or_default();
            let summary: String = r.try_get::<String, _>(2).unwrap_or_default();
            let tags_raw: String = r.try_get::<String, _>(3).unwrap_or_default();
            if summary.trim().is_empty() {
                return None; // 没有摘要就没有"索引"，自动路径找不到它（手动路径仍可选）
            }
            let hand: Vec<String> = tags_raw
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            Some((id, label, summary, hand))
        })
        .collect();

    let n = candidates.len();
    let mut scored: Vec<(i32, Vec<String>, i64, String, String)> = Vec::new();
    for (idx, (id, label, summary, hand_tags)) in candidates.iter().enumerate() {
        let day = crate::timeline::parse_day_number(label);
        let recency_rank = n.saturating_sub(idx).min(9);
        // A：这一天的手打标签里，哪些词真的出现在用户这句话里
        let hand_hits: Vec<String> = hand_tags
            .iter()
            .filter(|tg| intent.text.contains(tg.as_str()))
            .cloned()
            .collect();
        let (score, why) = score_summary(summary, day, intent.day, &intent.clues, &intent.events, &char_names, recency_rank, &hand_hits, &lex_hits, w);
        // 最低分阈值：单个标签词命中是 20，再加温度加权 → 20 以下基本都是噪声
        if score < w.min_score {
            continue;
        }
        scored.push((score, why, *id, label.clone(), summary.clone()));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(top_k.max(1));

    let mut hits = Vec::new();
    for (score, why, id, label, summary) in scored {
        let msgs = load_day_messages(pool, id).await?;
        if msgs.is_empty() {
            continue;
        }
        let (msgs, omitted) = trim_day(msgs, per_day_chars);
        hits.push(RecallHit {
            node_id: id,
            day: crate::timeline::parse_day_number(&label),
            date_label: label,
            summary,
            messages: msgs,
            omitted,
            score,
            why,
        });
    }
    Ok(hits)
}

/// 试算结果的一行（「🔍 回顾试算」用）
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExplainRow {
    pub day_label: String,
    pub day: Option<i64>,
    pub score: i32,
    /// 是否会被真正带出来（>= min_score）
    pub picked: bool,
    pub why: Vec<String>,
}

/// 试算：**只算不注入**。给一句话，返回每一天的得分与理由，按分数降序。
pub async fn explain(
    pool: &SqlitePool,
    branch: &Option<String>,
    lib_pool: &SqlitePool,
    text: &str,
    current_node_id: i64,
) -> Result<Vec<ExplainRow>, sqlx::Error> {
    let w = load_weights(lib_pool).await;
    let visible = crate::timeline::branch_path_ids(pool, branch).await;
    if visible.is_empty() {
        return Ok(Vec::new());
    }
    let intent = detect_intent(text);

    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let names_sql = format!("SELECT name FROM characters WHERE {clause} ORDER BY id");
    let mut nq = sqlx::query_scalar::<_, String>(&names_sql);
    if branch.is_some() {
        nq = nq.bind(branch.clone().unwrap_or_default());
    }
    let char_names: Vec<String> = nq.fetch_all(pool).await.unwrap_or_default();

    let mut lexicon: Vec<LexTerm> = TAG_WORDS.iter().map(|x| LexTerm { word: (*x).to_string(), kind: LexKind::TagWord }).collect();
    // 与 search 共用同一份实现（原来这里复制了一份，且漏了「物品」→ 回顾试算看不见物品）
    push_world_terms(pool, branch, &mut lexicon).await;
    for n in &char_names {
        let n = n.trim().to_string();
        if n.chars().count() >= 2 && !lexicon.iter().any(|x| x.word == n) {
            lexicon.push(LexTerm { word: n, kind: LexKind::Name });
        }
    }
    let lex_hits = match_lexicon(text, &lexicon);

    let rows = sqlx::query(
        "SELECT id, date_label, COALESCE(day_summary, ''), COALESCE(tags, '') FROM timeline_nodes
         ORDER BY sort_order ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;

    let n = rows.len();
    let mut out: Vec<ExplainRow> = Vec::new();
    for (idx, r) in rows.iter().enumerate() {
        let id: i64 = r.try_get(0).unwrap_or(-1);
        if !visible.contains(&id) || id == current_node_id {
            continue;
        }
        let label: String = r.try_get(1).unwrap_or_default();
        let summary: String = r.try_get(2).unwrap_or_default();
        let tags_raw: String = r.try_get(3).unwrap_or_default();
        let hand: Vec<String> = tags_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let hand_hits: Vec<String> = hand.iter().filter(|tg| text.contains(tg.as_str())).cloned().collect();
        let day = crate::timeline::parse_day_number(&label);
        let recency_rank = n.saturating_sub(idx).min(9);

        if summary.trim().is_empty() {
            out.push(ExplainRow {
                day_label: label,
                day,
                score: 0,
                picked: false,
                why: vec!["这天没有日摘要 —— 自动检索看不到它（手动选择不受限）".to_string()],
            });
            continue;
        }
        let (score, why) = score_summary(
            &summary, day, intent.day, &intent.clues, &intent.events, &char_names,
            recency_rank, &hand_hits, &lex_hits, &w,
        );
        out.push(ExplainRow { day_label: label, day, score, picked: score >= w.min_score, why });
    }
    out.sort_by(|a, b| b.score.cmp(&a.score));
    Ok(out)
}

/// 可供「🔍 回顾」选择的一天。
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecallDay {
    pub node_id: i64,
    pub date_label: String,
    pub day: Option<i64>,
    /// 当天消息条数
    pub msg_count: i64,
    /// 当天第一句用户发言的开头（界面列表里预览用）
    pub preview: String,
    /// 是否已生成日摘要（有摘要才会被**自动**检索命中；手动选择不受限）
    pub has_summary: bool,
    /// 这一天的可检索标签：手打标签 + 从日摘要派生的（标签词/角色名）
    pub tags: Vec<String>,
}

/// 列出**本分支可见**、且有对话的天（倒序 = 最近的在前）。
///
/// 界面的「🔍 回顾」按钮用它弹一个"选哪一天"的列表 ——
/// 用户自己指哪天最可靠，不依赖意图识别的准确率（文档 8.1 的兜底）。
///
/// 注意：**不再要求"有日摘要"** —— 回顾注入的是原文，
/// 只要那天聊过就应该能回顾（摘要只影响自动检索能否命中）。
pub async fn list_days(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<RecallDay>, sqlx::Error> {
    // C：派生标签要用角色名（"这一天提到谁"）
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let names_sql = format!("SELECT name FROM characters WHERE {clause} ORDER BY id");
    let mut nq = sqlx::query_scalar::<_, String>(&names_sql);
    if branch.is_some() {
        nq = nq.bind(branch.clone().unwrap_or_default());
    }
    let char_names: Vec<String> = nq.fetch_all(pool).await.unwrap_or_default();

    let visible = crate::timeline::branch_path_ids(pool, branch).await;
    if visible.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT id, date_label, COALESCE(day_summary, ''), COALESCE(tags, '') FROM timeline_nodes
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

        let tags_raw: String = r.try_get(3).unwrap_or_default();
        let hand: Vec<String> = tags_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let msgs = load_day_messages(pool, id).await.unwrap_or_default();
        if msgs.is_empty() {
            continue; // 没聊过的天没什么可回顾
        }
        let msg_count = msgs.len() as i64;
        let first_user = msgs
            .iter()
            .find(|(role, _)| role == "user")
            .map(|(_, c)| c.as_str())
            .unwrap_or_else(|| msgs[0].1.as_str());
        let preview: String = first_user.trim().chars().take(56).collect();

        out.push(RecallDay {
            node_id: id,
            day: crate::timeline::parse_day_number(&label),
            date_label: label,
            msg_count,
            preview,
            has_summary: !summary.trim().is_empty(),
            // 手打标签 + 从日摘要派生的标签（C：零成本"补录"，界面能看见检索依据）
            tags: derive_day_tags(&summary, &hand, &char_names),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {

    /// 真实数据诊断（手动跑）：
    ///   DW_DIAG_STORY_DB=<story.db 路径> cargo test --lib diag_explain_alias -- --ignored --nocapture
    ///
    /// 验证用户的原始症状：物品「rtx显卡」（别名「显卡」）在第 4 天的日摘要里出现过，
    /// 所以 `explain("显卡")` 必须给「第 4 天」加分并在理由里体现。
    #[tokio::test]
    #[ignore]
    async fn diag_explain_alias() {
        let db = match std::env::var("DW_DIAG_STORY_DB") {
            Ok(v) => v,
            Err(_) => return,
        };
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db)
            .create_if_missing(false);
        let story = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        // lib_pool 在 explain 里只用于 `load_weights`（读不到配置就用默认值），
        // 这里直接复用故事库，免去再建一个临时库（在受限环境里建库可能失败）。
        let lib = story.clone();

        let text = std::env::var("DW_DIAG_TEXT").unwrap_or_else(|_| "显卡".to_string());
        println!("=== explain(\"{text}\") ===");
        let rows = explain(&story, &None, &lib, &text, 9999).await.unwrap();
        for r in &rows {
            println!(
                "  {:<8} score={:>5} picked={:<5} 理由={}",
                r.day_label,
                r.score,
                r.picked,
                r.why.join(" / ")
            );
        }
        story.close().await;
    }
    use super::split_aliases;

    // ===== v3.4：别名（回顾试算检测不到物品的直接原因）=====

    /// 别名要能被解析出来，且支持中英文逗号/顿号/分号
    #[test]
    fn split_aliases_handles_separators() {
        assert_eq!(split_aliases(r#"{"aliases":"显卡"}"#), vec!["显卡".to_string()]);
        // 分隔符四种都认；单字「它」应被过滤掉
        assert_eq!(
            split_aliases(r#"{"aliases":"旧表,那块表，怀表、它"}"#),
            vec!["旧表".to_string(), "那块表".to_string(), "怀表".to_string()]
        );
        // 分号/全角分号也是分隔符；单字符片段被过滤
        assert_eq!(
            split_aliases(r#"{"aliases":"旧表;那块表；怀表"}"#),
            vec!["旧表".to_string(), "那块表".to_string(), "怀表".to_string()]
        );
        assert!(
            split_aliases(r#"{"aliases":"A;B;C"}"#).is_empty(),
            "全是单字符 → 全被过滤，不能塞进词典（单字误命中率太高）"
        );
    }

    /// 没有 meta / 空别名 / 单字别名 → 不产生词（单字太容易误命中）
    #[test]
    fn split_aliases_skips_empty_and_single_char() {
        assert!(split_aliases("{}").is_empty());
        assert!(split_aliases("").is_empty());
        assert!(split_aliases(r#"{"aliases":""}"#).is_empty());
        assert!(split_aliases(r#"{"aliases":"表"}"#).is_empty(), "单字别名不该进词典");
        assert!(split_aliases(r#"{"aliases":" , ，、"}"#).is_empty());
    }
    use super::*;

    /// 回归（用户 2026-10-18 实测发现的**真正**根因）：
    /// 角色名被当成"标签词"计分，导致 **调 tag_word 连带改了角色名的分**。
    ///
    /// 复现方式：用户把 tag_word 从 20 调到 50，分数涨了 **60** 而不是 30
    /// —— 因为那句话命中 1 个标签词 + 1 个角色名，两者都吃 tag_word（2 × 30 = 60）。
    #[test]
    fn tag_word_does_not_leak_into_name_score() {
        let lex = vec![
            LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord },
            LexTerm { word: "夏婉琴".to_string(), kind: LexKind::Name },
        ];
        // 摘要里两个词都有 → 两类都会命中
        let summary = "夏婉琴在院子里牵手";
        let base_w = RecallWeights::default();
        let tuned_w = RecallWeights { tag_word: 50, ..Default::default() };

        let (base_score, _) = score_summary(summary, Some(3), None, &[], &[], &[], 0, &[], &lex, &base_w);
        let (tuned, _) = score_summary(summary, Some(3), None, &[], &[], &[], 0, &[], &lex, &tuned_w);

        assert_eq!(
            tuned - base_score, 30,
            "tag_word 20→50 只该涨 30（涨 60 = 角色名也被当成标签词了）"
        );

        // 反过来：调 name_hit 也不该影响标签词那部分
        let tuned_name = RecallWeights { name_hit: 40, ..Default::default() };
        let (by_name, _) = score_summary(summary, Some(3), None, &[], &[], &[], 0, &[], &lex, &tuned_name);
        assert_eq!(by_name - base_score, 20, "name_hit 20→40 只该涨 20");
    }

    /// 回归（用户 2026-10-18 发现）：**标签词只能计一次分**。
    ///
    /// 生产路径同时会传 `intent.clues`（detect_intent 从输入里认出的标签词）
    /// 和 `lex_hits`（C 段的词典命中）—— C3 早期版本两处都加了 `w.tag_word`，
    /// 导致"把 tag_word 从 20 调到 50，分数涨 60 而不是 30"。
    ///
    /// 之前的权重测试没抓到，是因为它只传了 lex_hits、没传 clues，走不到重复路径。
    #[test]
    fn tag_word_is_counted_exactly_once() {
        let w = RecallWeights::default();
        let clues = vec!["牵手".to_string()];                        // detect_intent 会产出这个
        let lex = vec![LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord }]; // C 段也会命中

        // 只传 clues（老路径）
        let (only_clues, _) = score_summary("两人牵手", Some(3), None, &clues, &[], &[], 0, &[], &[], &w);
        // 只传 lex_hits（新路径）
        let (only_lex, _) = score_summary("两人牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &w);
        // 两条都传（**生产实际就是这样**）
        let (both, why) = score_summary("两人牵手", Some(3), None, &clues, &[], &[], 0, &[], &lex, &w);

        assert_eq!(both, only_lex, "同时传 clues 与 lex_hits 时**不能比单传 lex 更高**：{why:?}");
        // 老的 clues 路径已经删除（它的职责被 lex_hits 接管），所以单传 clues 不再计分
        assert_eq!(only_clues, 0, "clues 不再单独计分（避免与 lex_hits 重复）");
        // 把 tag_word 提到 50：应当只涨 30，而不是 60
        let w2 = RecallWeights { tag_word: 50, ..Default::default() };
        let (both2, _) = score_summary("两人牵手", Some(3), None, &clues, &[], &[], 0, &[], &lex, &w2);
        assert_eq!(both2 - both, 30, "tag_word 20→50 只该涨 30（涨 60 说明算了两遍）");
    }

    // ── C3：权重可配置 ──
    #[test]
    fn weights_are_configurable_not_hardcoded() {
        let w0 = RecallWeights::default();
        assert_eq!((w0.event, w0.day_exact, w0.hand_tag, w0.tag_word, w0.entity, w0.min_score),
                   (100, 80, 25, 20, 20, 20), "默认值就是文档里那套（v3.4：entity 12→20）");

        let lex = vec![LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord }];
        let (base_score, _) = score_summary("两人牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &w0);

        let w1 = RecallWeights { tag_word: 50, ..Default::default() };
        let (tuned, _) = score_summary("两人牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &w1);
        assert_eq!(tuned - base_score, 30, "tag_word 20→50 应 +30");
    }

    /// 门槛也来自权重：调高 min_score 时，同样的分从"能带出"变成"带不出"
    #[test]
    fn min_score_comes_from_weights() {
        let w = RecallWeights { min_score: 999, ..Default::default() };
        let lex = vec![LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord }];
        let (score, _) = score_summary("两人牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &w);
        assert!(score < w.min_score, "门槛 999 时这条不该被带出（score={score}）");
    }

    /// JSON 往返：配置能存能读（`recall.weights` 就存这个）
    #[test]
    fn weights_round_trip_json() {
        let w = RecallWeights { tag_word: 33, min_score: 25, ..Default::default() };
        let json = serde_json::to_string(&w).unwrap();
        let back: RecallWeights = serde_json::from_str(&json).unwrap();
        assert_eq!(w, back);
        // JSON 坏了要回默认，而不是让回顾失效
        assert_eq!(serde_json::from_str::<RecallWeights>("{oops}").unwrap_or_default(), RecallWeights::default());
    }

    // ── A：手打标签参与检索（用户 2026-10-18 定）──
    #[test]
    fn hand_tag_hit_boosts_that_day() {
        // 用户把某天标了「约会」，之后说"那次约会"
        let (s, why) = score_summary("两人在院子里散步", Some(5), None, &[], &[], &[], 0, &["约会".to_string()], &[], &RecallWeights::default());
        assert!(s >= 25, "手打标签命中应给 25 分，实际 {s}");
        assert!(why.iter().any(|w| w.contains("#约会")), "应说明命中了手打标签：{why:?}");

        // 标签没被提到 → 不加分（不然每天都因标签拿到分）
        let (s2, _) = score_summary("两人在院子里散步", Some(5), None, &[], &[], &[], 0, &[], &[], &RecallWeights::default());
        assert!(s2 < 25);
    }

    // ── C：词典命中（标签词 20 / 命名实体 20 —— v3.4 起同权重）──
    #[test]
    fn lexicon_terms_score_by_weight() {
        let lex = vec![
            LexTerm { word: "牵手".into(), kind: LexKind::TagWord },
            LexTerm { word: "修女院".into(), kind: LexKind::Entity },
        ];
        let (s, why) = score_summary("两人在修女院里牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &RecallWeights::default());
        assert_eq!(s, 40, "20 + 20 = 40（v3.4 起 entity 与标签词同权重），实际 {s}");
        assert_eq!(why.len(), 2, "两条说明：{why:?}");
    }


    /// 回归锁：**只命中一个命名实体**就该够 min_score，能被真正带出来。
    ///
    /// 这是用户实测暴露的问题：物品「显卡」写进第 4 天摘要，回顾试算显示命中 14 分、
    /// 但 picked=false（entity=12 < min_score=20）→ "检测得到却带不出来"。
    #[test]
    fn single_entity_hit_reaches_min_score() {
        let w = RecallWeights::default();
        let lex = vec![LexTerm { word: "显卡".into(), kind: LexKind::Entity }];
        let (s, why) = score_summary("我还记得那块显卡", Some(4), None, &[], &[], &[], 0, &[], &lex, &w);
        assert!(s >= w.min_score, "实体单命中应过门槛：score={s} min={}", w.min_score);
        assert!(why.iter().any(|x| x.contains("显卡")), "理由里应提到命中词：{why:?}");
    }
    /// 词典词只在**用户输入里出现**才算数（纯函数 match_lexicon 负责）
    #[test]
    fn match_lexicon_only_returns_words_in_input() {
        let lex = vec![
            LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord },
            LexTerm { word: "修女院".to_string(), kind: LexKind::Entity },
            LexTerm { word: "吵架".to_string(), kind: LexKind::TagWord },
            LexTerm { word: "单".to_string(), kind: LexKind::TagWord }, // 单字应被过滤
        ];
        let got = match_lexicon("还记得在修女院那次吗", &lex);
        assert_eq!(got.len(), 1, "只有「修女院」出现在输入里：{got:?}");
        assert_eq!(got[0].word, "修女院");
        assert_eq!(got[0].kind, LexKind::Entity);
    }

    /// 派生标签：手打 + 标签词 + 角色名（去重、零成本"补录"）
    #[test]
    fn derive_day_tags_merges_hand_vocab_and_names() {
        let hand = vec!["约会".to_string()];
        let names = vec!["夏婉琴".to_string(), "宸熙".to_string()];
        let tags = derive_day_tags("夏婉琴在院子里与宸熙牵手", &hand, &names);
        assert!(tags.contains(&"约会".to_string()), "手打标签应保留");
        assert!(tags.contains(&"牵手".to_string()), "标签词命中");
        assert!(tags.contains(&"院子".to_string()), "标签词命中");
        assert!(tags.contains(&"夏婉琴".to_string()), "角色名命中");
        assert_eq!(tags.iter().filter(|x| *x == "夏婉琴").count(), 1, "不能重复");
        // 摘要里没提的人不进来
        assert!(!tags.contains(&"院长".to_string()));
    }

    #[test]
    fn parses_day_with_digits_and_chinese() {
        assert_eq!(parse_day("还记得第 5 天吗"), Some(5));
        assert_eq!(parse_day("第5天发生了什么"), Some(5));
        assert_eq!(parse_day("第三天那次"), Some(3));
        assert_eq!(parse_day("第十天"), Some(10));
        assert_eq!(parse_day("第十一天"), Some(11));
        assert_eq!(parse_day("第二十三天"), Some(23));
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
    }

    #[test]
    fn day_mention_is_a_clue() {
        let i = detect_intent("第 8 天的时候你在做什么");
        assert_eq!(i.day, Some(8));
        assert!(i.clues.iter().any(|c| c.contains("8")));
        assert!(i.is_recall);
    }

    /// 用户问了"第一次牵手"时，含该事件的摘要应显著高于只含标签词的摘要。
    #[test]
    fn scoring_ranks_first_event_highest() {
        let events = vec!["第一次牵手".to_string()];
        let (s_event, why) = score_summary("两人第一次牵手", Some(5), None, &[], &events, &[], 0, &[], &[], &RecallWeights::default());
        let (s_tag, _) = score_summary("在院子里散步", Some(5), None, &["院子".to_string()], &events, &[], 0, &[], &[], &RecallWeights::default());
        assert!(s_event > s_tag, "用户问到的事件命中应高于普通标签：{why:?}");
    }

    /// **用户没问事件时，摘要里出现"第一次X"不该给满分**（否则某一天会霸榜）。
    #[test]
    fn summary_event_without_user_ask_is_not_bonus() {
        let (s, why) = score_summary("这是第一次亲吻", Some(3), None, &[], &[], &[], 0, &[], &[], &RecallWeights::default());
        assert_eq!(s, 0, "用户没问首次事件时，摘要里的事件词不应计分：{why:?}");
    }

    /// 用户问"第 8 天"，不该被别的天靠"表白"二字抢走（实测踩到的霸榜）。
    #[test]
    fn day_match_beats_unrelated_event_word_in_summary() {
        let day_hit = score_summary("随便一句", Some(8), Some(8), &[], &[], &[], 0, &[], &[], &RecallWeights::default());
        let other = score_summary("那天他表白了", Some(12), Some(8), &[], &[], &[], 9, &[], &[], &RecallWeights::default());
        assert!(day_hit.0 > other.0, "指定的那天必须排最前：{} vs {}", day_hit.0, other.0);
    }

    #[test]
    fn scoring_uses_requested_day() {
        let (s_hit, why) = score_summary("随便一句话", Some(5), Some(5), &[], &[], &[], 0, &[], &[], &RecallWeights::default());
        let (s_miss, _) = score_summary("随便一句话", Some(6), Some(5), &[], &[], &[], 0, &[], &[], &RecallWeights::default());
        assert!(s_hit > s_miss);
        assert!(why.iter().any(|w| w.contains("第 5 天")), "{why:?}");
    }

    /// **只有角色名命中的摘要必须判 0** —— 否则"当时在院子里"会稳定带回同样的第 1/2/3 天。
    #[test]
    fn name_only_match_scores_zero() {
        let names = vec!["夏婉琴".to_string(), "宸熙".to_string()];
        let (s, why) = score_summary("夏婉琴和宸熙在吃饭", Some(2), None, &[], &[], &names, 5, &[], &[], &RecallWeights::default());
        assert_eq!(s, 0, "只有角色名不该算命中：{why:?}");
        assert!(why.is_empty());
    }

    /// 渲染的是**原始对话**，不是摘要（用户实测后要求）。
    #[test]
    fn render_uses_original_messages_not_summary() {
        assert!(render_block(&[]).is_empty());
        let hit = RecallHit {
            node_id: 1,
            date_label: "第 1 天".into(),
            day: Some(1),
            summary: "（这是摘要，不该出现在提示词里）".into(),
            messages: vec![
                ("user".into(), "宝宝 你还记得第一天的时候吗".into()),
                ("assistant".into(), "那天在下雨。".into()),
            ],
            omitted: 0,
            score: 100,
            why: vec![],
        };
        let block = render_block(&[hit]);
        assert!(block.contains("【相关旧日记忆】"));
        assert!(block.contains("原始对话"), "标题应说明是原始对话：{block}");
        assert!(block.contains("[用户] 宝宝 你还记得第一天的时候吗"), "{block}");
        assert!(block.contains("[AI] 那天在下雨。"), "{block}");
        assert!(!block.contains("这是摘要"), "摘要绝不能被渲染进去：{block}");
    }

    #[test]
    fn render_marks_omitted_count() {
        let hit = RecallHit {
            node_id: 1, date_label: "第 1 天".into(), day: Some(1), summary: String::new(),
            messages: vec![("user".into(), "甲".into())], omitted: 7, score: 10, why: vec![],
        };
        let block = render_block(&[hit]);
        assert!(block.contains("另有 7 条未列出"), "{block}");
    }

    #[test]
    fn trim_day_keeps_head_and_counts_omitted() {
        let msgs: Vec<(String, String)> = (0..5)
            .map(|i| ("user".to_string(), format!("消息{}", i)))
            .collect();
        let (kept, omitted) = trim_day(msgs, 20);
        assert!(!kept.is_empty());
        assert_eq!(kept[0].1, "消息0", "应从最早开始保留");
        assert_eq!(kept.len() + omitted, 5);
    }

    #[test]
    fn fit_hits_drops_tail_when_over_budget() {
        let mk = |id: i64, text: &str| RecallHit {
            node_id: id, date_label: format!("第 {id} 天"), day: Some(id), summary: String::new(),
            messages: vec![("user".into(), text.into())], omitted: 0, score: 10, why: vec![],
        };
        let hits = vec![mk(1, "aaaa"), mk(2, "bbbb"), mk(3, "cccc")];
        // 成本按渲染后正文估算；预算给得紧，只装得下两条
        let (kept, dropped) = fit_hits(hits, 30, |s| s.chars().count());
        assert!(kept.len() < 3, "预算紧时应丢弃末尾：kept={}", kept.len());
        assert_eq!(kept.len() + dropped, 3);
    }

    /// 真实库副本：打印每天的"可检索标签"（手打 + 派生），并模拟几个查询看命中。
    #[tokio::test]
    #[ignore]
    async fn diag_recall_tags_on_real_db() {
        use sqlx::sqlite::SqliteConnectOptions;
        let Ok(dir) = std::env::var("DW_DIAG_DB") else { eprintln!("未设置 DW_DIAG_DB"); return };
        // 注意：副本必须写**系统临时目录**，不能写真实数据目录 ——
        // 测试中断会留下几 MB 的垃圾文件在用户的 projects/ 里（2026-10-18 实测踩到）。
        let tmp = std::env::temp_dir().join("dw_diag_tags.db");
        let _ = std::fs::copy(&dir, &tmp);
        let opts = SqliteConnectOptions::new().filename(&tmp);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        for sql in ["ALTER TABLE story_summaries ADD COLUMN branch_id TEXT"] {
            let _ = sqlx::query(sql).execute(&pool).await;
        }

        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM characters ORDER BY id")
            .fetch_all(&pool).await.unwrap_or_default();
        let keys: Vec<String> = sqlx::query_scalar("SELECT key FROM world_settings WHERE category IN ('地理','术语','势力','物品') ORDER BY id")
            .fetch_all(&pool).await.unwrap_or_default();
        eprintln!("\n角色：{:?}\n世界观键：{:?}", names, keys);

        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT date_label, COALESCE(day_summary,''), COALESCE(tags,'') FROM timeline_nodes
             WHERE node_type IN ('day','main') AND is_flashback=0 ORDER BY sort_order, id")
            .fetch_all(&pool).await.unwrap();

        eprintln!("\n=== 每天的标签（手打 + 从摘要派生）===");
        for (label, summary, hand_raw) in &rows {
            let hand: Vec<String> = hand_raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let tags = derive_day_tags(summary, &hand, &names);
            eprintln!("  {label:<10} 手打={hand:?}  全部={tags:?}");
        }

        let mut lexicon: Vec<LexTerm> = TAG_WORDS.iter().map(|w| LexTerm { word: (*w).to_string(), kind: LexKind::TagWord }).collect();
        for k in &keys {
            let k = k.trim().to_string();
            if k.chars().count() >= 2 && !lexicon.iter().any(|x| x.word == k) { lexicon.push(LexTerm { word: k, kind: LexKind::Entity }); }
        }
        // 与生产一致：角色名也进词典（权重 20）
        for n in &names {
            let n = n.trim().to_string();
            if n.chars().count() >= 2 && !lexicon.iter().any(|x| x.word == n) { lexicon.push(LexTerm { word: n, kind: LexKind::Name }); }
        }

        for q in ["还记得那次牵手吗", "修女院那次", "当时院长说什么了"] {
            let hits = match_lexicon(q, &lexicon);
            eprintln!("\n查询「{q}」→ 词典命中 {:?}", hits.iter().map(|h| (h.word.clone(), h.kind)).collect::<Vec<_>>());
            // 逐天打分（不带意图事件，只看词典 + 手打标签）
            for (label, summary, hand_raw) in &rows {
                let hand: Vec<String> = hand_raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                let hand_hits: Vec<String> = hand.iter().filter(|t| q.contains(t.as_str())).cloned().collect();
                let (sc, why) = score_summary(summary, None, None, &[], &[], &names, 0, &hand_hits, &hits, &RecallWeights::default());
                if sc > 0 { eprintln!("     {label} → {sc} 分  {why:?}"); }
            }
        }
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
        let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    }

    /// 真实库副本：跑一次 `explain`，看会带哪几天、为什么（C3 的"试算"就是它）
    #[tokio::test]
    #[ignore]
    async fn diag_explain_on_real_db() {
        use sqlx::sqlite::SqliteConnectOptions;
        let Ok(dir) = std::env::var("DW_DIAG_DB") else { eprintln!("未设置 DW_DIAG_DB"); return };
        // 只读打开真实库：explain 只读不写，不需要复制
        // （复制会被正在运行的应用锁住 → "拒绝访问"，实测踩到）
        let opts = SqliteConnectOptions::new().filename(&dir).read_only(true);
        let pool = match SqlitePool::connect_with(opts).await {
            Ok(p) => p,
            Err(e) => { eprintln!("打开库失败：{e}"); return }
        };

        let lib = lib_pool_beside_story(&dir).await;
        for q in ["还记得那次牵手吗", "当时院长说什么了", "第 9 天发生什么"] {
            eprintln!("\n查询「{q}」");
            match explain(&pool, &None, &lib, q, 0).await {
                Ok(rows) => {
                    for r in rows.iter().filter(|r| r.score > 0).take(6) {
                        eprintln!(
                            "  {:<10} {:>4} 分 {:<6} {}",
                            r.day_label, r.score,
                            if r.picked { "[会带出]" } else { "[未达门槛]" },
                            r.why.join(" · ")
                        );
                    }
                }
                Err(e) => eprintln!("  ERR {e}"),
            }
        }
    }

    /// 诊断用：从 story.db 的路径推断 library.db（`<lib>/projects/<id>/story.db` → `<lib>/library.db`）
    ///
    /// 不能用 `paths::library_db()` —— 那个依赖运行时的数据根目录（start.cmd 用 env 指到项目内
    /// 的 data/library），而测试进程里没有那个 env，会指到 %APPDATA% 下的另一份。
    async fn lib_pool_beside_story(story_db: &str) -> SqlitePool {
        // <root>/default/projects/<id>/story.db → 上三级是 default，library.db 就在那儿
        let lib = std::path::Path::new(story_db)
            .parent().and_then(|p| p.parent()).and_then(|p| p.parent())
            .map(|p| p.join("library.db"))
            .expect("推断 library.db 失败");
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&lib).read_only(true);
        SqlitePool::connect_with(opts).await.unwrap_or_else(|e| panic!("连 library.db 失败（{lib:?}）：{e}"))
    }
}
