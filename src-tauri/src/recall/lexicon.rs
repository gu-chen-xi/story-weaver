// 从 `recall.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `recall.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use lexicon::*;` 取回，
// 外部 `crate::recall::xxx` 的调用路径**全部不变**。

use super::*;

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
pub(super) async fn push_world_terms(
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
