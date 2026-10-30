// 关键记忆（v3.4，用户 2026-10-22/23 定）。
//
// ## 它解决什么
//
// 日摘要回答"那天大致发生了什么"；**关键记忆回答"这个人与这段关系里，什么不可重来"**
// —— 表白、承诺、牺牲、背叛。这类事只发生一次，但会一直影响后面的表演：
// 模型不知道"她已经表过白了"，就会一遍又一遍地重演第一次心动。
//
// ## 三条硬规则（用户的决定）
//
// 1. **只记第一次**：每种类型在一个分支里只留一条 —— 第二次表白不再新建文档。
// 2. **一个事件一个文档**：Markdown 落盘，人类可读、可备份、可手改。
// 3. **没有"新建"入口**：由系统从剧情里提炼；用户可以**改、删**（纠错），但不能凭空造。
//
// ## 为什么用文件而不是新表
//
// 记忆天然是"文档"（用户在导出时要的是能读的 md）。放文件里：
//   * 不需要 SQLite 迁移，也没有和 rollback/分支复制的列对齐问题；
//   * 用户可以直接在外面编辑/备份，符合"数据明文是设计选择"；
//   * 分支隔离放在 front matter 的 `branch` 字段里，读取时过滤。
//
// 代价：没有事务、没有索引。所以条目数必须被规则压住（每类一条），不能无限长。

use std::path::{Path, PathBuf};

mod attitude;
pub use attitude::{parse_marker, protocol_block_start, quote_gist, reasons_text, score_reply, strip_marker, Attitude, Signal, Stance};

/// 关键记忆的**类型注册表**（扩类型的口子就在这里）。
///
/// 元组含义：`(event_type, 中文名, 默认重要度)`。
/// `event_type` 必须与 `evolution::analyze` 里识别出的事件名一致 ——
/// 加一类只要在这里加一行，抓取、命名、注入、导出的逻辑全自动跟上。
pub const MEMORY_KINDS: &[(&str, &str, i64)] = &[
    ("confession", "表白", 90),
    ("promise", "承诺", 80),
    ("sacrifice", "牺牲", 85),
    ("betrayal", "背叛", 85),
];

/// 一条记忆（从磁盘解析出来）。
#[derive(Clone, Debug)]
pub struct Memory {
    /// 文件名（不含 .md），同时用作稳定 id
    pub id: String,
    pub kind: String,
    pub label: String,
    pub day_label: String,
    pub node_id: i64,
    /// `None` = 主线
    pub branch: Option<String>,
    pub importance: i64,
    pub matched: String,
    pub created_at: String,
    /// 表白卡：AI 这一轮的态度（旧卡没有这一行 → None）
    pub stance: Option<Stance>,
    /// 表白卡：一共表白过几次
    pub attempts: usize,
    /// 表白卡：其中被拒绝几次
    pub refusals: usize,
    /// 表白卡：拒绝理由（前 3）
    pub reasons: Vec<Signal>,
    /// 表白卡：她的原话（前 1–2 句台词；靠协议标记判定时当「理由」用）
    pub quote: String,
    /// Markdown 正文（front matter 之外的部分）
    pub body: String,
}

/// 某故事的记忆目录：`<库根>/default/projects/<id>/memories`
pub fn memories_dir(story_id: &str) -> PathBuf {
    crate::paths::library_root()
        .join("default")
        .join("projects")
        .join(story_id)
        .join("memories")
}

/// 类型 → 中文名（未知类型原样返回，保证老文件也能读）
pub fn kind_label(kind: &str) -> String {
    MEMORY_KINDS
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, label, _)| label.to_string())
        .unwrap_or_else(|| kind.to_string())
}

/// 命中"要记下来"的事件类型（用于抓取）
pub fn kind_weight(kind: &str) -> Option<i64> {
    MEMORY_KINDS
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, _, imp)| *imp)
}

fn branch_key(branch: &Option<String>) -> &str {
    branch.as_deref().unwrap_or("main")
}

/// front matter 里的一行值（简单实现：`key: value`，值不转义）
fn fm_get(block: &str, key: &str) -> Option<String> {
    for line in block.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(&format!("{key}:")) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// 解析一个记忆文件；格式不对就返回 None（**不 panic**：这目录用户会手改）
pub fn parse_memory(id: &str, raw: &str) -> Option<Memory> {
    let raw = raw.replace("\r\n", "\n");
    let body_start = raw.find("\n---");   // 结束分隔线
    let (fm, body) = match body_start {
        Some(i) => (&raw[..i], raw[i + 4..].trim_start_matches('\n').to_string()),
        None => return None,
    };
    let kind = fm_get(fm, "kind").unwrap_or_default();
    if kind.is_empty() {
        return None;
    }
    let branch_raw = fm_get(fm, "branch").unwrap_or_default();
    Some(Memory {
        id: id.to_string(),
        label: fm_get(fm, "label").unwrap_or_else(|| kind_label(&kind)),
        day_label: fm_get(fm, "day").unwrap_or_default(),
        node_id: fm_get(fm, "node_id").and_then(|v| v.parse().ok()).unwrap_or(0),
        branch: if branch_raw.is_empty() || branch_raw == "main" {
            None
        } else {
            Some(branch_raw)
        },
        importance: fm_get(fm, "importance").and_then(|v| v.parse().ok()).unwrap_or(50),
        matched: fm_get(fm, "matched").unwrap_or_default(),
        created_at: fm_get(fm, "created_at").unwrap_or_default(),
        stance: {
            let s = fm_get(fm, "stance").unwrap_or_default();
            if s.trim().is_empty() { None } else { Some(Stance::parse(&s)) }
        },
        attempts: fm_get(fm, "attempts").and_then(|v| v.parse().ok()).unwrap_or(1),
        refusals: fm_get(fm, "refusals").and_then(|v| v.parse().ok()).unwrap_or(0),
        reasons: decode_signals(&fm_get(fm, "reasons").unwrap_or_default()),
        quote: fm_get(fm, "quote").unwrap_or_default(),
        kind,
        body,
    })
}

/// 读出某故事的全部记忆（不过滤分支；调用方按需筛）
pub fn list_all(story_id: &str) -> Vec<Memory> {
    list_all_in(&memories_dir(story_id))
}

/// [`list_all`] 的注入版：直接读某个目录（测试用临时目录，避免依赖全局数据根）。
///
/// 为什么要 `_in`：`paths::data_root()` 是 `OnceLock` 缓存，测试没法靠环境变量换根；
/// 这也和仓库里 `analyze_evolution_in` / `rollback_snapshot_in` 的写法一致。
pub fn list_all_in(dir: &Path) -> Vec<Memory> {
    let mut out: Vec<Memory> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else { continue };
        let Ok(raw) = std::fs::read_to_string(&p) else { continue };
        if let Some(m) = parse_memory(stem, &raw) {
            out.push(m);
        }
    }
    // 重要度高的在前，其次新的在前
    out.sort_by(|a, b| b.importance.cmp(&a.importance).then(b.created_at.cmp(&a.created_at)));
    out
}

/// **当前分支能看到**的记忆 = 本分支 ∪ 主线（分叉前的共同经历）
pub fn list_for_branch(story_id: &str, branch: &Option<String>) -> Vec<Memory> {
    list_for_branch_in(&memories_dir(story_id), branch)
}

/// [`list_for_branch`] 的注入版
pub fn list_for_branch_in(dir: &Path, branch: &Option<String>) -> Vec<Memory> {
    list_all_in(dir)
        .into_iter()
        .filter(|m| match branch {
            None => m.branch.is_none(),
            Some(b) => m.branch.is_none() || m.branch.as_deref() == Some(b.as_str()),
        })
        .collect()
}

/// 生成尾部块【关键记忆】。`budget` 是字符上限（默认 1200）。
///
/// 只放"一句话 + 当天"，不放全文：全文在磁盘上，模型需要细节可以自己从历史里找。
pub fn render_block(memories: &[Memory], budget: usize) -> Option<String> {
    if memories.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0usize;
    let mut skipped = 0usize;
    let mut has_refused = false;
    for m in memories {
        // 表白卡：用 `stance` + 次数 + 理由拼一行（不带正文摘要 —— 正文里有"第 N 次表白："这种重复）。
        // 旧卡没有 `stance` 时，用正文里最近一次对话**重算一次**（只影响注入，不改文件）。
        let line = if m.kind == "confession" {
            let st = effective_stance(m);
            if st == Some(Stance::Refused) {
                has_refused = true;
            }
            let mut parts = vec![format!("- {} · {}", m.day_label, m.label)];
            match st {
                Some(Stance::Refused) => {
                    parts.push(format!("：{}", "被拒绝"));
                    if m.attempts > 1 {
                        parts.push(format!("（共 {} 次表白，其中被拒 {} 次）", m.attempts, m.refusals.max(1)));
                    }
                }
                Some(Stance::Accepted) => parts.push("：已被接受（不要再演第一次表白）".to_string()),
                _ => parts.push("：对方态度未判定".to_string()),
            }
            if st == Some(Stance::Refused) {
                // 注意：reasons 里可能只有「短句否定」这种内部标记，reasons_text 会把它滤掉 →
                // 结果是**空字符串**。判断要落在「过滤之后」上，否则会留一个空壳「对方理由：」
                // （2026-10-27 用户实测：提示词里就是「被拒绝；对方理由：」后面什么都没有）。
                let rs = reasons_text(&m.reasons, 3);
                if !rs.is_empty() {
                    parts.push(format!("；对方理由：{rs}"));
                } else if !m.quote.trim().is_empty() {
                    // 靠协议标记判定的卡没有词级理由 —— 直接给她的原话
                    parts.push(format!("；她的话：「{}」", m.quote.trim()));
                }
            }
            parts.join("")
        } else {
            let one = one_line_summary(&m.body, 60);
            if one.is_empty() {
                format!("- {} · {}（重要度 {}）", m.day_label, m.label, m.importance)
            } else {
                format!("- {} · {}：{}", m.day_label, m.label, one)
            }
        };
        if used + line.chars().count() > budget {
            skipped += 1;
            continue;
        }
        used += line.chars().count();
        lines.push(line);
    }
    if lines.is_empty() {
        return None;
    }
    let mut s = String::from(
        "【关键记忆】本故事里已经发生、不可重来的事（**不要重演第一次**；可作为回忆、心结或情绪的来处）",
    );
    for l in &lines {
        s.push('\n');
        s.push_str(l);
    }
    // 这条是给"软接受"打的补丁（用户实测）：只写"被拒绝"还不够，模型会顺着往下演成默许。
    if has_refused {
        s.push_str("\n（被拒绝的表白只是「说过了」，关系没有因此确立：不要写成她已经答应、默许或「其实早就喜欢」）");
    }
    if skipped > 0 {
        s.push_str(&format!("\n（另有 {skipped} 条较早的记忆未列出）"));
    }
    Some(s)
}

/// 表白卡的"有效态度"：卡里有 `stance` 就用它；没有（旧卡/未判定）就拿正文里最近一次对话重算。
///
/// 只影响**注入的措辞**，不写回文件 —— 文件要等下一次同类事件才会被重写。
fn effective_stance(m: &Memory) -> Option<Stance> {
    match m.stance {
        Some(Stance::Refused) | Some(Stance::Accepted) => m.stance,
        _ => {
            let a = score_reply(&m.body);
            if a.stance == Stance::Unclear { m.stance } else { Some(a.stance) }
        }
    }
}

/// 取正文的第一句（去掉 Markdown 装饰）作为一句话摘要
pub fn one_line_summary(body: &str, max_chars: usize) -> String {
    let plain: String = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('>'))
        .collect::<Vec<_>>()
        .join(" ");
    let plain = plain.replace("**", "").replace('`', "");
    let chars: Vec<char> = plain.chars().collect();
    if chars.len() <= max_chars {
        return plain;
    }
    chars[..max_chars].iter().collect::<String>() + "…"
}

fn now_stamp() -> String {
    // 与库里其它时间同源：SQLite 的 localtime（这里没连接池，退回系统时间格式化）
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs() as i64 + 8 * 3600;   // 与项目其它手工时间一致（+8）
            let days = secs.div_euclid(86400);
            let rem = secs.rem_euclid(86400);
            let (y, m, d) = civil_from_days(days);
            format!(
                "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
                rem / 3600,
                (rem % 3600) / 60,
                rem % 60
            )
        })
        .unwrap_or_default()
}

/// 抓取要记的事件并落盘（每种类型、每个分支一条）。
///
/// `events` 来自 `evolution::analyze::extract_events`；`text` 是**这一轮**的拼装文本
/// （`evolution::memory_round` 给的「用户那条消息 + AI 回复」）；`ai_reply` 是这一轮
/// **AI 的回复原文**，只给它打态度分（用户 2026-10-26 定：判定只看 AI 说了什么）。
///
/// 两类写法：
///   * **表白**：累计一张卡（`attempts` 次数、`stance` 被拒/被接受、`reasons` 前 3 理由），
///     正文只留**最近一次**的对话（更早的交给「🔍 回顾」）；后来接受了就在同一张卡上改判。
///   * 其它（承诺 / 牺牲 / 背叛）：仍按"每种只记第一次"。
pub fn capture_first_events(
    story_id: &str,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    text: &str,
    ai_reply: &str,
    events: &[(String, String)],
) -> Result<Vec<String>, String> {
    capture_first_events_in(&memories_dir(story_id), branch, node_id, day_label, text, ai_reply, events)
}

/// [`capture_first_events`] 的注入版（`dir` = 记忆目录）。
pub fn capture_first_events_in(
    dir: &Path,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    text: &str,
    ai_reply: &str,
    events: &[(String, String)],
) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // 2026-10-27：**元指令与协议都先剥掉**再进卡片 ——
    // 用户那条可能带 `〔导演指令〕…` 整块，AI 那条可能带 `〔导演代价〕…` 收据；
    // 原样抄进"最近一次/她的原话"会让人以为那是台词（用户实测截图里就出现了）。
    // 注意：态度标记（〔态度:…〕）仍然**保留**，下面要靠它判定接受/拒绝。
    let text_clean = crate::stat_change::strip_line(&crate::director::strip_receipt(&crate::relations::strip_director_anywhere(text)));
    let reply_clean = crate::stat_change::strip_line(&crate::director::strip_receipt(ai_reply));
    let text = text_clean.as_str();
    let ai_reply = reply_clean.as_str();
    // v3.4 修（2026-10-26）：`text` 就是**这一轮**（用户那条消息 + AI 回复）—— 原样落盘，
    // 不再取"命中词 ±80 字"的窗口（那个窗口会把上一轮 AI 的半句话一起卷进来）。
    // 协议标记（〔态度:接受/拒绝〕）只给软件判定用，不能写进记忆正文
    let snippet = strip_marker(text);
    if snippet.is_empty() {
        return Ok(Vec::new());
    }

    // 已有记忆：主线（branch = None）算"分叉前的共同历史"，分支里不再重复记同一类。
    let existing = list_all_in(dir);
    let find = |kind: &str| -> Option<&Memory> {
        existing.iter().find(|m| {
            m.kind == kind && (m.branch.is_none() || m.branch.as_deref() == branch.as_deref())
        })
    };

    let mut made: Vec<String> = Vec::new();
    for (kind, matched) in events {
        let Some(importance) = kind_weight(kind) else { continue };
        let label = kind_label(kind);

        // ===== 表白：累计一张卡 =====
        if kind == "confession" {
            let att = score_reply(ai_reply);
            // 判定优先级：**标记 > 打分 > 未定**（标记是模型自己写的协议，最可信）
            let stance_now = parse_marker(ai_reply).unwrap_or(att.stance);
            // 「她的原话」（前 1–2 句台词）：靠协议标记判定时没有词级理由，用它更可信
            let quote_now = quote_gist(ai_reply, 60);
            match find("confession") {
                None => {
                    let reasons = encode_signals(&att.signals);
                    let refusals_now = if stance_now == Stance::Refused { 1 } else { 0 };
                    let body = confession_body(1, day_label, stance_now, &att.signals, refusals_now, &snippet, &label, &quote_now);
                    let front = confession_front(
                        branch, node_id, day_label, importance, matched, 1,
                        refusals_now,
                        stance_now, att.score, &reasons, &quote_now,
                    );
                    let id = next_id(dir, &label);
                    write_memory(dir, &id, &front, &body)?;
                    made.push(format!("{label}（第 1 次）"));
                }
                Some(m) => {
                    let raw = std::fs::read_to_string(dir.join(format!("{}.md", m.id)))
                        .map_err(|e| e.to_string())?;
                    let fm_end = raw.find("\n---").unwrap_or(0);
                    let fm = &raw[..fm_end];
                    let prev_attempts: usize = fm_get(fm, "attempts").and_then(|v| v.parse().ok()).unwrap_or(1);
                    let prev_refusals: usize = fm_get(fm, "refusals").and_then(|v| v.parse().ok()).unwrap_or(0);
                    let prev_stance = Stance::parse(&fm_get(fm, "stance").unwrap_or_default());
                    let prev_signals = decode_signals(&fm_get(fm, "reasons").unwrap_or_default());
                    let attempts = prev_attempts + 1;
                    let refusals = prev_refusals + if stance_now == Stance::Refused { 1 } else { 0 };
                    // 后来接受了就**在这张卡上改判**（用户 2026-10-26 定），不再新开一条；
                    // 已经接受的卡不会被后来的拒绝翻回去（否则提示词会自相矛盾）。
                    let stance = if stance_now == Stance::Accepted {
                        Stance::Accepted
                    } else if prev_stance == Stance::Accepted {
                        Stance::Accepted
                    } else if stance_now == Stance::Refused {
                        Stance::Refused
                    } else {
                        prev_stance
                    };
                    let merged = merge_signals(&prev_signals, &att.signals);
                    let reasons = encode_signals(&merged);
                    let first_day = fm_get(fm, "day").unwrap_or_else(|| day_label.to_string());
                    let body = confession_body(attempts, &first_day, stance, &merged, refusals, &snippet, &label, &quote_now);
                    let front = confession_front(
                        branch, node_id, &first_day, importance, matched, attempts, refusals, stance, att.score, &reasons, &quote_now,
                    );
                    write_memory(dir, &m.id, &front, &body)?;
                    made.push(format!("{label}（第 {attempts} 次）"));
                }
            }
            continue;
        }

        // ===== 其余三类：只记第一次；**承诺例外**（2026-10-30 用户选 A：允许多条，各自带状态） =====
        if kind != "promise" && find(kind).is_some() {
            continue;
        }
        let body = format!(
            "# {day_label} · {label}\n\n{snippet}\n\n> 由系统从剧情里提炼（命中「{matched}」）。这是这个分支里第一次发生，后续同类事件不再单独记录。\n"
        );
        let promise_line = if kind == "promise" { "promise_state: 待兑现\n" } else { "" };
        let front = format!(
            "---\nkind: {kind}\nlabel: {label}\nday: {day_label}\nnode_id: {node_id}\nbranch: {}\nimportance: {importance}\n{promise_line}created: {}\n---\n",
            branch_key(branch),
            now_stamp()
        );
        let id = next_id(dir, &label);
        write_memory(dir, &id, &front, &body)?;
        made.push(label);
    }
    Ok(made)
}


/// 结算最近一条「待兑现」的承诺（2026-10-30，用户选 A：多条承诺各自带状态）。
///
/// 返回被改状态的那条文件名（没有待兑现的返回 None —— 不新建、不猜）。
/// 状态只有三种：待兑现 / 已兑现 / 未兑现（未兑现是**正式状态**，之后补兑现可再改）。
pub fn settle_promise_in(dir: &Path, fulfilled: bool) -> Result<Option<String>, String> {
    if !dir.exists() {
        return Ok(None);
    }
    let mut best: Option<(i64, std::path::PathBuf, String)> = None;
    for e in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = e.map_err(|e| e.to_string())?.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else { continue };
        if !name.contains("承诺") || !name.ends_with(".md") {
            continue;
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let fm_end = raw.find("\n---").unwrap_or(0);
        let fm = &raw[..fm_end];
        if fm_get(fm, "promise_state").as_deref() != Some("待兑现") {
            continue;
        }
        let seq: i64 = name.split('-').next().and_then(|s| s.parse().ok()).unwrap_or(0);
        if best.as_ref().map(|(s, _, _)| seq > *s).unwrap_or(true) {
            best = Some((seq, path, raw));
        }
    }
    let Some((_, path, raw)) = best else { return Ok(None) };
    let new_state = if fulfilled { "已兑现" } else { "未兑现" };
    let updated = raw.replacen("promise_state: 待兑现", &format!("promise_state: {new_state}"), 1);
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    Ok(path.file_name().and_then(|s| s.to_str()).map(|s| s.to_string()))
}

/// 下一个文件名：`001-表白` 这种（序号 = 现有文件数 + 1）
fn next_id(dir: &Path, label: &str) -> String {
    let seq = std::fs::read_dir(dir).map(|r| r.count()).unwrap_or(0) + 1;
    format!("{seq:03}-{label}")
}

fn write_memory(dir: &Path, id: &str, front: &str, body: &str) -> Result<(), String> {
    std::fs::write(dir.join(format!("{id}.md")), format!("{front}\n{body}")).map_err(|e| e.to_string())
}

/// 表白卡的 front matter（含累计字段）
#[allow(clippy::too_many_arguments)]
fn confession_front(
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    importance: i64,
    matched: &str,
    attempts: usize,
    refusals: usize,
    stance: Stance,
    score: i32,
    reasons: &str,
    quote: &str,
) -> String {
    format!(
        "---\nkind: confession\nlabel: 表白\nday: {day_label}\nnode_id: {node_id}\nbranch: {}\nimportance: {importance}\nmatched: {matched}\nattempts: {attempts}\nrefusals: {refusals}\nstance: {}\nscore: {score}\nreasons: {reasons}\nquote: {quote}\ncreated_at: {}\n---\n",
        branch_key(branch),
        stance.key(),
        now_stamp()
    )
}

/// 表白卡的正文：第一行是**给提示词看的摘要**（`render_block` 取第一句），后面只留最近一次对话。
fn confession_body(
    attempts: usize,
    first_day: &str,
    stance: Stance,
    signals: &[Signal],
    refusals: usize,
    snippet: &str,
    label: &str,
    quote: &str,
) -> String {
    let summary = match stance {
        Stance::Refused => {
            let rs = reasons_text(signals, 3);
            // 有词级理由写理由；靠协议标记判定时没有理由，写「她的原话」；都没有才只报次数
            if !rs.is_empty() {
                format!("第 {attempts} 次{label}：**被拒绝**（累计被拒 {refusals} 次）。对方理由：{rs}")
            } else if !quote.trim().is_empty() {
                format!("第 {attempts} 次{label}：**被拒绝**（累计被拒 {refusals} 次）。她的话：「{}」", quote.trim())
            } else {
                format!("第 {attempts} 次{label}：**被拒绝**（累计被拒 {refusals} 次）。")
            }
        }
        Stance::Accepted => format!(
            "第 {attempts} 次{label}：**已被接受**（此前被拒 {refusals} 次）。此后不要再演「第一次表白」"
        ),
        Stance::Unclear => format!("第 {attempts} 次{label}：对方态度未判定。"),
    };
    format!(
        "# {first_day} · {label}\n\n{summary}\n\n最近一次：\n\n{snippet}\n\n> 由系统从剧情里提炼。更早的几次不在这里，需要时用「🔍 回顾」。\n"
    )
}

/// 理由序列 → front matter 里的一行 JSON（我们自己解析，不引 yaml 依赖）
fn encode_signals(signals: &[Signal]) -> String {
    let arr: Vec<serde_json::Value> = signals
        .iter()
        .take(3)
        .map(|s| serde_json::json!({ "p": s.phrase, "w": s.weight, "c": s.count }))
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string())
}

/// front matter 的 reasons 一行 → 理由序列（坏了就当空）
fn decode_signals(raw: &str) -> Vec<Signal> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim()) else {
        return Vec::new();
    };
    let Some(arr) = v.as_array() else { return Vec::new() };
    arr.iter()
        .filter_map(|it| {
            Some(Signal {
                phrase: it.get("p")?.as_str()?.to_string(),
                weight: it.get("w")?.as_i64()? as i32,
                count: it.get("c")?.as_u64()? as usize,
            })
        })
        .collect()
}

/// 合并两批理由（同名累加次数、权重取大），按强度排序后只留前 3
fn merge_signals(old: &[Signal], new: &[Signal]) -> Vec<Signal> {
    let mut out: Vec<Signal> = old.to_vec();
    for s in new {
        if let Some(e) = out.iter_mut().find(|x| x.phrase == s.phrase) {
            e.count += s.count;
            e.weight = e.weight.max(s.weight);
        } else {
            out.push(s.clone());
        }
    }
    out.sort_by(|a, b| {
        b.weight.abs().cmp(&a.weight.abs())
            .then(b.count.cmp(&a.count))
            .then(a.phrase.cmp(&b.phrase))
    });
    out.truncate(3);
    out
}

/// 改正文（只允许改 front matter 之后的正文；元数据不动，避免用户误改坏 id/kind）
pub fn update_body(story_id: &str, id: &str, new_body: &str) -> Result<(), String> {
    update_body_in(&memories_dir(story_id), id, new_body)
}

/// [`update_body`] 的注入版
pub fn update_body_in(dir: &Path, id: &str, new_body: &str) -> Result<(), String> {
    let p = dir.join(format!("{id}.md"));
    let raw = std::fs::read_to_string(&p).map_err(|e| format!("读不到记忆 {id}：{e}"))?;
    let raw = raw.replace("\r\n", "\n");
    let Some(i) = raw.find("\n---") else {
        return Err("记忆文件格式不对（缺少 front matter 结束线）".to_string());
    };
    let head = &raw[..i + 4];
    std::fs::write(&p, format!("{head}\n\n{}\n", new_body.trim())).map_err(|e| e.to_string())
}

/// 删一条记忆（用户纠错用）
pub fn delete(story_id: &str, id: &str) -> Result<(), String> {
    delete_in(&memories_dir(story_id), id)
}

/// [`delete`] 的注入版
pub fn delete_in(dir: &Path, id: &str) -> Result<(), String> {
    let p = dir.join(format!("{id}.md"));
    std::fs::remove_file(&p).map_err(|e| format!("删不掉记忆 {id}：{e}"))
}

/// 导出：`<exports>/key-memories/<故事>_<时间戳>/` 下一条记忆一个 md。
///
/// 用户要求：保存位置和"故事导出"一致，但**用文件夹包着**，文件夹名叫 `key-memories`。
/// 返回导出目录的绝对路径（提示里给用户看）。
pub fn export_all(story_id: &str, story_title: &str) -> Result<(String, usize), String> {
    export_all_in(&crate::paths::exports_root(), &memories_dir(story_id), story_title)
}

/// [`export_all`] 的注入版（`exports_root` / `mem_dir` 都可注入）
pub fn export_all_in(
    exports_root: &Path,
    mem_dir: &Path,
    story_title: &str,
) -> Result<(String, usize), String> {
    let all = list_all_in(mem_dir);
    let dir = exports_root
        .join("key-memories")
        .join(format!("{}_{}", clean_filename(story_title), ts_compact()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut n = 0usize;
    let mut index = String::from("# 关键记忆 · 目录\n\n");
    for m in &all {
        let file = format!("{}.md", m.id);
        let content = format!(
            "# {} · {}\n\n{}\n\n---\n\n- 类型：{}（重要度 {}）\n- 当天：{}\n- 分支：{}\n- 记录时间：{}\n",
            m.day_label,
            m.label,
            m.body,
            m.label,
            m.importance,
            m.day_label,
            if m.branch.is_none() { "主线".to_string() } else { m.branch.clone().unwrap_or_default() },
            m.created_at
        );
        std::fs::write(dir.join(&file), content).map_err(|e| e.to_string())?;
        index.push_str(&format!("- [{} · {}]({})\n", m.day_label, m.label, file));
        n += 1;
    }
    if n == 0 {
        index.push_str("（这个分支还没有关键记忆）\n");
    }
    std::fs::write(dir.join("INDEX.md"), index).map_err(|e| e.to_string())?;
    Ok((dir.display().to_string(), n))
}

/// 清洗文件名非法字符（与 export.rs 同规则；这里独立一份，避免跨模块耦合）
fn clean_filename(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => out.push('_'),
            c if (c as u32) < 0x20 => out.push('_'),
            c => out.push(c),
        }
    }
    let t = out.trim().trim_matches('_').to_string();
    if t.is_empty() { "story".to_string() } else { t }
}

/// 秒级时间戳 `20261023_134500`
fn ts_compact() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 + 8 * 3600)
        .unwrap_or(0);
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}_{:02}{:02}{:02}", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// 记忆目录（供上层判断"有没有记忆"）
pub fn has_any(story_id: &str) -> bool {
    !list_all(story_id).is_empty()
}

/// 目录是否存在（测试用）
pub fn dir_exists(story_id: &str) -> bool {
    Path::new(&memories_dir(story_id)).is_dir()
}
/// 天数（1970-01-01 起算）→ 公历年月日（Howard Hinnant 的 civil_from_days）。
///
/// 为什么自带一份：`portraits` / `export` 里各有一份私有实现，
/// 提到公共模块要动它们的可见性；这里只要能格式化时间戳，重复 10 行算法比扩大可见性更安全。
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod memory_tests;

#[cfg(test)]
mod promise_multi_tests {
    use super::*;

    /// 多条承诺各自带状态；结算只动**最近一条待兑现**（用户 2026-10-30 选 A）
    #[test]
    fn settle_only_touches_latest_pending_promise() {
        let dir = std::path::PathBuf::from(".tmp").join(format!("memtest-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mk = |id: &str, state: &str| {
            std::fs::write(
                dir.join(format!("{id}.md")),
                format!("---\nkind: promise\nlabel: 承诺\npromise_state: {state}\n---\n\n# 第 1 天 · 承诺\n"),
            ).unwrap();
        };
        mk("001-承诺", "已兑现");
        mk("002-承诺", "待兑现");
        mk("003-承诺", "待兑现");
        let hit = settle_promise_in(&dir, false).unwrap().unwrap();
        assert_eq!(hit, "003-承诺.md", "只结算最近一条");
        let raw = std::fs::read_to_string(dir.join("003-承诺.md")).unwrap();
        assert!(raw.contains("promise_state: 未兑现"));
        let raw2 = std::fs::read_to_string(dir.join("002-承诺.md")).unwrap();
        assert!(raw2.contains("promise_state: 待兑现"), "更早那条不动");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
