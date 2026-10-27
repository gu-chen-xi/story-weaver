//! 导演代价（2026-10-27）：**开挂也要结账**。
//!
//! 背景：导演指令是"强制剧情这样走"的软件特权（v3.7.0 给它加了强度闸门）。
//! 可它原来**没有账** —— 用户能无限次硬推"她答应了"，而关系数值、记忆、状态卡纹丝不动，
//! 于是"开挂"变成零成本，剧情越推越假。
//!
//! 这一版把"她凭什么松口"变成软件记账，四件事：
//!
//! 1. **协议行**：用了导演指令的那一轮，回复**最后一行**写 〔导演代价〕类型：理由
//!    （只认最后一行、整行、理由 ≤ MAX_REASON_CHARS 字、全角/半角冒号都收）
//! 2. **软件算分**：类型 → 分值在 COST_KINDS 里，**提示词里不出现分值**（模型不碰数字）
//! 3. **一轮只结一次账**：模板收据与关键词推断各至多一条，只留更重的那一笔（见 settle）
//! 4. **账在 director_costs 表**：带 branch_id（分支隔离），累计到 LIMIT 就锁住导演指令；
//!    账本里可以「撤销」（记错了）或「赦免」（她原谅了这次，留痕但不计分）
//!
//! 为什么不做成"人工确认"：这是**角色对这件事的真实看法**，不是用户可以拒绝的提案 ——
//! 它和「后台静默演进」那套（要人工确认）是两条线，别混在一起。

use sqlx::{Row, SqlitePool};

/// 协议标记（AI 回复最后一行）。改这里要同步改 src/lib/parse.ts 的 COST_TAG
pub const COST_TAG: &str = "〔导演代价〕";

/// 理由最长多少字（2026-10-27 用户定：24 字不够，长一点的句子才写得清）
pub const MAX_REASON_CHARS: usize = 32;

/// 单次上限 = 表里最重的那一档。**超出的分数按它夹住**（防止将来改表时忘了改上限）
pub const MAX_SINGLE: i64 = 30;

/// 累计到多少分锁住导演指令（闸门第二条件）。取 60 的理由：
/// 两次「白给」、或四次「时限」、或六次「交换」就见底 —— 够一个人开几次挂推剧情，
/// 又不至于把"硬推"当成默认操作。改这一个常量就能调松紧。
pub const LIMIT: i64 = 60;

/// 类型 → 分值表（**软件算分，模型只写类型与理由**）。
///
/// 排序即"她松口的代价"：从最像公平交易（交换）到完全没有代价（白给）。
/// 越往上，越是"人设被剧情硬拗"，所以越贵。
pub const COST_KINDS: [(&str, &str, i64); 6] = [
    ("交换", "用具体的东西或承诺换来（钱、情报、帮一次忙）", 5),
    ("条件", "她提了条件，对方先做到", 10),
    ("时限", "只答应一段时间的尝试，随时可以退回", 15),
    ("压力", "被形势或第三个人逼到没有别的选择", 20),
    ("心软", "感情上撑不住（旧情、眼泪、受伤）", 25),
    ("白给", "没有任何代价，她直接让了", 30),
];

/// 关键词兜底：模型忘了写收据时，从正文里认"她松口的方式"。
///
/// **每条类型要命中 ≥ 2 个词才算**（单个词误报太多，例如"暂时"满地都是）。
/// 兜底只补"没有收据"的那一轮，且一轮仍然只记一笔（见 settle）。
pub const KEYWORD_TYPES: [(&str, &[&str]); 5] = [
    ("交换", &["作为交换", "换作", "换来", "拿什么换", "交换"]),
    ("条件", &["除非", "如果你先", "只要你先", "答应我一个条件"]),
    ("时限", &["只此一次", "这一回", "暂时", "暂且", "过段时间"]),
    ("压力", &["没办法", "不得不", "被逼", "形势所迫", "没有别的选择"]),
    ("心软", &["心软", "舍不得", "眼泪", "旧情", "狠不下心"]),
];

/// 一条收据（或一条关键词推断）
#[derive(Debug, Clone, PartialEq)]
pub struct Receipt {
    pub kind: String,
    pub reason: String,
    /// 表外类型 = 0（**不记账**，只把协议行从正文里删掉）
    pub score: i64,
}

/// 入账用的一条：收据 + 来源（model = 模型自己写的收据，rule = 关键词推断）
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub kind: String,
    pub reason: String,
    pub score: i64,
    pub source: &'static str,
}

/// 类型名 → 分值。表外类型返回 0（不是报错：模型偶尔会自创类型，别让它把整轮搞挂）
pub fn score_of(kind: &str) -> i64 {
    let k = kind.trim();
    COST_KINDS
        .iter()
        .find(|(name, _, _)| *name == k)
        .map(|(_, _, s)| *s)
        .unwrap_or(0)
}

/// 末尾**协议区**：从最后一行往上数，最多 3 个非空行（保持原顺序）。
///
/// 为什么不"只认最后一行"（2026-10-27 实测）：本轮同时触发了两条协议 ——
/// `〔导演代价〕条件：…` 和 `〔态度:接受〕`。两条都要求"最后一行"，模型只能把态度标记放最后，
/// 于是收据被顶掉、**账没记**（用户截图里那张记忆卡就是证据）。现在协议区允许 2~3 行共存，
/// 各按自己的标记抓，谁在前谁在后都行。
/// 这一整行是不是导演代价的协议行（只做形状判断，不解析内容）
pub fn is_cost_line(line: &str) -> bool {
    line.trim_start().starts_with(COST_TAG)
}

/// 末尾协议区的起始行号（0 起）：从最后一行往上，跳过空行 / 代价行 / 态度行，
/// 遇到第一条正文行就停。没有协议行时返回 `lines().count()`。
///
/// 这样"标记后面还接着正文"就不算协议（正文里写到这个词也不会被误读）；
/// 而"态度行 + 代价行"两条并存时，两条都在协议区里，各抓各的。
fn protocol_block_start(text: &str) -> usize {
    crate::memory::protocol_block_start(text)
}

/// 回复的**最后一行**（去掉尾部空行后）。留着给别的用途（正文中间的同名标记不是协议）。
pub fn last_line(text: &str) -> &str {
    text.trim_end().rsplit('\n').next().unwrap_or("").trim()
}

/// 取第一个冒号（全角/半角都收）并把它切开
fn split_colon(s: &str) -> Option<(&str, &str)> {
    for (i, ch) in s.char_indices() {
        if ch == '：' || ch == ':' {
            return Some((&s[..i], &s[i + ch.len_utf8()..]));
        }
    }
    None
}

/// 按收据协议解析回复的最后一行。
///
/// 返回 None 的情况（= 空收据，不入账）：最后一行不是协议行 / 没有冒号 /
/// 理由是空的 / 理由超过 MAX_REASON_CHARS 字。
/// **表外类型返回 Some，但 score = 0** —— 协议行仍然要从正文里删掉。
pub fn parse_receipt(reply: &str) -> Option<Receipt> {
    let start = protocol_block_start(reply);
    for line in reply.lines().skip(start) {
        if !is_cost_line(line) {
            continue;
        }
        let Some(rest) = line.trim().strip_prefix(COST_TAG) else { continue };
        let Some((kind, reason)) = split_colon(rest) else { continue };
        let kind = kind.trim().to_string();
        let reason = reason.trim().to_string();
        if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARS {
            continue;
        }
        return Some(Receipt { score: score_of(&kind), kind, reason });
    }
    None
}

/// 把协议行从正文里删掉（存库 / 历史 / 导出 / 气泡都不带它）。
///
/// 只删**最后一行且以标记开头**的那种；正文中间的同名文本原样保留。
/// 删完变空（模型只写了收据）时退回原文 —— 宁可多一行，也不要存一条空消息。
pub fn strip_receipt(text: &str) -> String {
    let start = protocol_block_start(text);
    let drop: std::collections::HashSet<usize> = text
        .lines()
        .enumerate()
        .filter(|(i, l)| *i >= start && is_cost_line(l))
        .map(|(i, _)| i)
        .collect();
    if drop.is_empty() {
        return text.to_string();
    }
    let kept: Vec<&str> = text
        .lines()
        .enumerate()
        .filter(|(i, _)| !drop.contains(i))
        .map(|(_, l)| l)
        .collect();
    let out = kept.join("\n").trim_end().to_string();
    if out.trim().is_empty() {
        text.to_string()
    } else {
        out
    }
}

/// 命中位置所在的那一句（用它当关键词兜底的理由）
fn sentence_of(text: &str, at: usize) -> String {
    let mut start = 0usize;
    for (i, ch) in text.char_indices() {
        if i >= at {
            break;
        }
        if matches!(ch, '。' | '！' | '？' | '!' | '?' | '\n') {
            start = i + ch.len_utf8();
        }
    }
    let mut end = text.len();
    for (i, ch) in text.char_indices() {
        if i < at {
            continue;
        }
        if matches!(ch, '。' | '！' | '？' | '!' | '?' | '\n') {
            end = i;
            break;
        }
    }
    text[start..end].trim().to_string()
}

fn clip_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

/// 关键词兜底：没有收据时，看正文里反复出现的"松口方式"。
///
/// 保守到什么程度：**同一类型要命中 ≥ 2 个不同的词**；命中数相同时取更重的那档。
/// 宁可漏记（模型下一轮还会写收据），也不要凭一个"暂时"就往账上记一笔。
pub fn infer_from_keywords(reply: &str) -> Option<Receipt> {
    let mut best: Option<(usize, i64, &'static str, String)> = None;
    for (kind, words) in KEYWORD_TYPES.iter() {
        let mut hits = 0usize;
        let mut first_at: Option<usize> = None;
        for w in words.iter() {
            if let Some(pos) = reply.find(w) {
                hits += 1;
                if first_at.map(|p| pos < p).unwrap_or(true) {
                    first_at = Some(pos);
                }
            }
        }
        if hits < 2 {
            continue;
        }
        let score = score_of(kind);
        let better = match &best {
            None => true,
            Some((h, s, _, _)) => hits > *h || (hits == *h && score > *s),
        };
        if better {
            let reason = first_at
                .map(|p| clip_chars(&sentence_of(reply, p), MAX_REASON_CHARS))
                .unwrap_or_default();
            best = Some((hits, score, kind, reason));
        }
    }
    best.map(|(_, score, kind, reason)| Receipt {
        kind: kind.to_string(),
        reason: if reason.is_empty() { format!("（正文里提到「{kind}」）") } else { reason },
        score,
    })
}

/// **一轮只结一次账**：模板收据与关键词推断各至多一条，只留更重的那一笔。
///
/// 为什么"取最重"而不是"收据优先"：模型自报轻了（写「交换」，正文却是"她心一软就答应了"）
/// 时，按重的算才守得住"开挂有代价"；反过来模型自报重了也没什么可亏的。
/// 0 分（表外类型）不入账 —— 那不是一笔账，只是模型写歪了的协议行。
pub fn settle(reply: &str) -> Option<Candidate> {
    let from_model = parse_receipt(reply).map(|r| Candidate {
        kind: r.kind,
        reason: r.reason,
        score: r.score.clamp(0, MAX_SINGLE),
        source: "model",
    });
    // 关键词兜底只看**正文**：协议行里的理由也是模型写的，让它参与推断
    // 等于自己给自己加码（而且会把「心软」这种词重复计一次）。
    let body = strip_receipt(reply);
    let from_rule = infer_from_keywords(&body).map(|r| Candidate {
        kind: r.kind,
        reason: r.reason,
        score: r.score.clamp(0, MAX_SINGLE),
        source: "rule",
    });
    let picked = match (from_model, from_rule) {
        (Some(a), Some(b)) => Some(if b.score > a.score { b } else { a }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
    picked.filter(|c| c.score > 0)
}

/// 注入尾部块的【导演代价】段（只在用了导演指令的那一轮出现）。
///
/// 三条硬约束：
/// * **不给分值**：模型只写类型，分数由软件按 COST_KINDS 算（"模型不碰数字"）；
/// * **不给 JSON**：只给一个照抄的模板行（JSON 会让模型自由发挥字段名）；
/// * 正文不能出现【】—— 提示词预览按【】切段，会出现假分段。
pub fn cost_legend() -> String {
    let mut s = String::from("【导演代价】（给软件的对账行，不是剧情内容；本轮用过导演指令，必须写）");
    s.push_str("\n- 位置：回复末尾单独一行（如果这一轮也要写〔态度:…〕，两行都写：态度行在前，这一行在后）。");
    s.push_str("\n- 格式：〔导演代价〕类型：理由");
    s.push_str("\n- 类型只能是：");
    let names: Vec<String> = COST_KINDS
        .iter()
        .map(|(name, hint, _)| format!("{name}（{hint}）"))
        .collect();
    s.push_str(&names.join(" / "));
    s.push_str("\n- 理由一句话写清「她凭什么松口」，不超过 32 个字；不要写数字，不要写成她的心声或对白。");
    s.push_str("\n- 这一行会被软件读走并从正文里删掉（用户看不到）；如果真的没有付代价，就老实写「白给」。");
    s
}

// ===== 账本（director_costs 表）=====

/// 账本里的一行（前端直接渲染：类型 / 理由 / 分值 / 天）
#[derive(Debug, serde::Serialize)]
pub struct CostRow {
    pub id: i64,
    pub node_id: Option<i64>,
    /// 这一笔发生在哪天（拿不到就空串）
    pub day_label: String,
    pub character_id: Option<i64>,
    pub kind: String,
    pub reason: String,
    pub score: i64,
    /// model = 模型写的收据；rule = 关键词推断
    pub source: String,
    pub excused: i64,
    pub created_at: String,
}

/// 类型表的只读副本（界面用来解释"为什么这条记这么多分"）
#[derive(Debug, serde::Serialize)]
pub struct KindRow {
    pub name: String,
    pub hint: String,
    pub score: i64,
}

/// 账本快照：最近 N 条 + 累计（只算没被赦免的）
#[derive(Debug, serde::Serialize)]
pub struct Ledger {
    pub rows: Vec<CostRow>,
    pub total: i64,
    pub limit: i64,
    pub max_single: i64,
    pub kinds: Vec<KindRow>,
}

/// 当前分支（和全项目同一个口径：stories.current_branch_id，None = 主线）
async fn current_branch(story_id: &str) -> Option<String> {
    crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None)
}

/// AI 扮演者 → 角色 id（当前分支里按名字找；「自由」/空/查无此人 → None）
pub async fn character_id_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    name: &str,
) -> Option<i64> {
    let n = name.trim();
    if n.is_empty() || n == "自由" {
        return None;
    }
    let clause = crate::story_db::branch_clause("branch_id", branch, 2);
    let sql = format!("SELECT id FROM characters WHERE name = ?1 AND {} LIMIT 1", clause);
    let mut q = sqlx::query_scalar::<_, i64>(&sql).bind(n);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.fetch_optional(pool).await.ok().flatten()
}

/// 记一笔账。0 分不写（见 settle 的口径），返回新行 id
pub async fn record_cost(
    pool: &SqlitePool,
    branch: &Option<String>,
    node_id: i64,
    message_id: Option<i64>,
    character_id: Option<i64>,
    cand: &Candidate,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO director_costs(node_id, message_id, character_id, kind, reason, score, source, branch_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id",
    )
    .bind(node_id)
    .bind(message_id)
    .bind(character_id)
    .bind(&cand.kind)
    .bind(&cand.reason)
    .bind(cand.score.clamp(0, MAX_SINGLE))
    .bind(cand.source)
    .bind(branch)
    .fetch_one(pool)
    .await
}

/// 某分支的全部账（新→旧）。**取全量再在内存里筛人**：
/// 一个分支的账本撑死几百行，比拼两套占位符编号可靠得多（E-DB-11 就是编号拼错）。
pub async fn list_rows_in(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<CostRow>, sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let sql = format!(
        "SELECT id, node_id, character_id, kind, reason, score, source, excused, created_at,
                COALESCE((SELECT date_label FROM timeline_nodes WHERE id = node_id), '')
         FROM director_costs WHERE {} ORDER BY id DESC",
        clause
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| CostRow {
            id: r.try_get(0).unwrap_or(0),
            // 注意 try_get::<i64> 在读 NULL 时会**静默给 0**（sqlx-sqlite 的老毛病，见 E-DB-08），
            // 所以可空列必须显式按 Option 解 —— 否则 "没指定角色" 会被读成 "角色 0"。
            node_id: r.try_get::<Option<i64>, _>(1).ok().flatten(),
            character_id: r.try_get::<Option<i64>, _>(2).ok().flatten(),
            kind: r.try_get(3).unwrap_or_default(),
            reason: r.try_get(4).unwrap_or_default(),
            score: r.try_get(5).unwrap_or(0),
            source: r.try_get(6).unwrap_or_else(|_| "model".to_string()),
            excused: r.try_get(7).unwrap_or(0),
            created_at: r.try_get(8).unwrap_or_default(),
            day_label: r.try_get(9).unwrap_or_default(),
        })
        .collect())
}

/// 账本快照：最近 limit 条（按角色筛）+ 累计分（只算没赦免的，含不在最近 N 条里的）
pub async fn ledger_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    character_id: Option<i64>,
    limit: i64,
) -> Result<Ledger, sqlx::Error> {
    let all = list_rows_in(pool, branch).await?;
    let mine: Vec<CostRow> = all
        .into_iter()
        .filter(|r| r.character_id == character_id)
        .collect();
    let total = mine
        .iter()
        .filter(|r| r.excused == 0)
        .map(|r| r.score)
        .sum();
    let rows = mine.into_iter().take(limit.max(1) as usize).collect();
    Ok(Ledger {
        rows,
        total,
        limit: LIMIT,
        max_single: MAX_SINGLE,
        kinds: COST_KINDS
            .iter()
            .map(|(name, hint, score)| KindRow {
                name: name.to_string(),
                hint: hint.to_string(),
                score: *score,
            })
            .collect(),
    })
}

/// 赦免一笔：**留痕，但不计入累计**（"她原谅了这次"）
pub async fn excuse_in(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    let n = sqlx::query("UPDATE director_costs SET excused = 1 WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n > 0)
}

/// 撤销一笔：**删掉**（记错了 / 规则判歪了）
pub async fn delete_in(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    let n = sqlx::query("DELETE FROM director_costs WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n > 0)
}

// ===== 对外入口（story_id + 当前分支 + AI 扮演者）=====

/// 账本（界面用）
pub async fn ledger(story_id: &str, character_name: &str, limit: i64) -> Result<Ledger, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let branch = current_branch(story_id).await;
    let cid = character_id_in(&pool, &branch, character_name).await;
    ledger_in(&pool, &branch, cid, limit).await.map_err(|e| e.to_string())
}

pub async fn excuse(story_id: &str, cost_id: i64) -> Result<(), String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let hit = excuse_in(&pool, cost_id).await.map_err(|e| e.to_string())?;
    if !hit {
        return Err(format!("没找到这笔账（id = {cost_id}）"));
    }
    Ok(())
}

pub async fn delete(story_id: &str, cost_id: i64) -> Result<(), String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let hit = delete_in(&pool, cost_id).await.map_err(|e| e.to_string())?;
    if !hit {
        return Err(format!("没找到这笔账（id = {cost_id}）"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
