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

mod lexicon;
pub use lexicon::*;

mod score;
pub use score::*;

use sqlx::Row;
use sqlx::SqlitePool;

impl Default for RecallWeights {
    fn default() -> Self {
        // v3.4：entity 从 12 提到 20（= tag_word）。原因：12 < min_score 20，
            // 意味着"用户点名了一个专有名词、且它真的出现在那天摘要里"这种**最强信号**
            // 永远带不出那一天（实测：物品「显卡」写入第 4 天，回顾试算只显示命中 14 分、picked=false）。
            // 专有名词比"日常标签词"更确定，不该比它低。
            Self { event: 100, day_exact: 80, hand_tag: 25, tag_word: 20, entity: 20, name_hit: 20, name_weak: 3, min_score: 20, recency_max: 9 }
    }
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
        // 回顾也是"给模型看的旧对话"：协议行一律不带（2026-10-27，与历史同一口径）
        let content = crate::stat_change::strip_line(&crate::memory::strip_marker(
            &crate::director::strip_receipt(&crate::relations::strip_director(&content)),
        ));
        if content.trim().is_empty() {
            continue;
        }
        out.push((role, content));
    }
    Ok(out)
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
