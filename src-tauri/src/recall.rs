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
mod tests;
