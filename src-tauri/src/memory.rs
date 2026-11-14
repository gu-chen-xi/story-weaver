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
    // 2026-11-02：**她主动表白**（区别于"用户表白 → 她接受/拒绝"那张卡）
    ("ai_confession", "她的告白", 88),
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
    /// 表白卡：**后来改判接受**的那一天（2026-11-01）。被拒之后又答应时记下来，
    /// 提示词里就能写"第 9 天她已经接受了"，模型不会误以为当初就没拒过。
    pub accepted_day: Option<String>,
    /// 表白卡：一共表白过几次
    pub attempts: usize,
    /// 表白卡：其中被拒绝几次
    pub refusals: usize,
    /// 表白卡：拒绝理由（前 3）
    pub reasons: Vec<Signal>,
    /// 表白卡：她的原话（前 1–2 句台词；靠协议标记判定时当「理由」用）
    pub quote: String,
    /// 归属的**参与者**（2026-11-15）：一张卡可能牵涉两个人（"a 向 b 表白"两边都该记得）。
    /// 空 = 全局（老卡 / 旁观事件，任何视角都可见）。front matter 的 `characters:` 一行（逗号分隔）；
    /// 老文件只有 `character:` 单值 —— 照样读，视为只含一个元素的集合（零迁移）。
    pub characters: Vec<String>,
    /// Markdown 正文（front matter 之外的部分）
    /// 承诺卡：待兑现 / 已兑现 / 未兑现（其它类型为空串）
    pub promise_state: String,
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

/// front matter 里「参与者」那一行：去重、跳过空与「自由」；一个都没有 → 不写（= 全局）。
fn characters_line(roles: &[&str]) -> String {
    let mut list: Vec<&str> = Vec::new();
    for r in roles {
        let s = r.trim();
        if !s.is_empty() && s != "自由" && !list.contains(&s) {
            list.push(s);
        }
    }
    if list.is_empty() { String::new() } else { format!("characters: {}\n", list.join(",")) }
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
        promise_state: fm_get(fm, "promise_state").unwrap_or_default(),
        matched: fm_get(fm, "matched").unwrap_or_default(),
        created_at: fm_get(fm, "created_at").unwrap_or_default(),
        stance: {
            let s = fm_get(fm, "stance").unwrap_or_default();
            if s.trim().is_empty() { None } else { Some(Stance::parse(&s)) }
        },
        accepted_day: {
            let d = fm_get(fm, "accepted_day").unwrap_or_default();
            if d.trim().is_empty() { None } else { Some(d.trim().to_string()) }
        },
        attempts: fm_get(fm, "attempts").and_then(|v| v.parse().ok()).unwrap_or(1),
        refusals: fm_get(fm, "refusals").and_then(|v| v.parse().ok()).unwrap_or(0),
        reasons: decode_signals(&fm_get(fm, "reasons").unwrap_or_default()),
        quote: fm_get(fm, "quote").unwrap_or_default(),
        characters: {
            // 新格式 `characters: a,b`；老格式 `character: a`（兼容读，不写回）
            let mut list: Vec<String> = Vec::new();
            let raw = fm_get(fm, "characters").unwrap_or_default();
            for part in raw.split([',', '，', ';', '；']) {
                let s = part.trim();
                if !s.is_empty() {
                    list.push(s.to_string());
                }
            }
            if list.is_empty() {
                let legacy = fm_get(fm, "character").unwrap_or_default();
                let s = legacy.trim();
                if !s.is_empty() {
                    list.push(s.to_string());
                }
            }
            list.dedup();
            list
        },
        kind,
        body,
    })
}

/// 角色视角的过滤（2026-11-11）：`role` 为空 → 全给（旁白 / 全局）；
/// 非空 → 只留"属于她"和"没有归属（全局）"的卡。老卡没有 `character:` 行 → 视为全局。
pub fn filter_for_role(memories: &[Memory], role: &str) -> Vec<Memory> {
    let r = role.trim();
    if r.is_empty() {
        return memories.to_vec();
    }
    memories
        .iter()
        .filter(|m| m.characters.is_empty() || m.characters.iter().any(|c| c == r))
        .cloned()
        .collect()
}

/// 按「关系组」分卡（2026-11-15）：一组 = 参与者的**集合**（顺序不重要，a×b 与 b×a 同一组）。
///
/// 返回顺序：组内卡片数多的在前；同数量按组名。
/// 无归属的卡自成一组（key 为空 Vec，界面上叫「全局」）。
pub fn group_by_participants(memories: &[Memory]) -> Vec<(Vec<String>, Vec<Memory>)> {
    let mut groups: Vec<(Vec<String>, Vec<Memory>)> = Vec::new();
    for m in memories {
        let mut key: Vec<String> = m.characters.clone();
        key.sort();
        key.dedup();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, list)) => list.push(m.clone()),
            None => groups.push((key, vec![m.clone()])),
        }
    }
    groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.join(",").cmp(&b.0.join(","))));
    groups
}

/// 只取「这两个人」那一组（集合相等；顺序无关）。查不到返回空 —— 调用方据此显示"还没有"。
pub fn cards_of_pair(memories: &[Memory], a: &str, b: &str) -> Vec<Memory> {
    let mut want = vec![a.trim().to_string(), b.trim().to_string()];
    want.retain(|s| !s.is_empty());
    want.sort();
    want.dedup();
    memories
        .iter()
        .filter(|m| {
            let mut have = m.characters.clone();
            have.sort();
            have.dedup();
            have == want
        })
        .cloned()
        .collect()
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


/// 关键记忆的**祖先链可见性**（2026-10-30 用户定）。
///
/// 旧逻辑是"branch 相等"：嵌套分支读不到父分支的承诺（用户实测：4 层分支看不到 6 分支第 7 天的承诺）。
/// 新逻辑：**记忆的分支在当前分支的祖先链上就可见**；兄弟分支互不可见，主线只看到共享段。
pub fn branch_visible(mem_branch: &Option<String>, ancestors: &[Option<String>]) -> bool {
    match mem_branch {
        None => true, // 主线 = 共享段，所有分支都继承
        Some(b) => ancestors.iter().any(|a| a.as_deref() == Some(b.as_str())),
    }
}

/// 当前分支的祖先链（含自己；主线为 None）。
pub async fn ancestors_of(story_id: &str, branch: &Option<String>) -> Vec<Option<String>> {
    let Ok(pool) = crate::story_db::open_story_pool(story_id).await else { return vec![branch.clone()] };
    let mut out: Vec<Option<String>> = Vec::new();
    let mut cur = branch.clone();
    loop {
        out.push(cur.clone());
        let Some(id) = cur.clone() else { break };
        let parent: Option<String> = sqlx::query_scalar("SELECT source_branch_id FROM branches WHERE id = ?1")
            .bind(&id)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten()
            .flatten();
        if parent.is_none() {
            break;
        }
        cur = parent;
        if out.len() > 64 {
            break; // 防御：脏数据成环
        }
    }
    out
}

/// 按祖先链过滤（注入与界面共用）
pub fn list_for_ancestors_in(dir: &Path, ancestors: &[Option<String>]) -> Vec<Memory> {
    list_all_in(dir)
        .into_iter()
        .filter(|m| branch_visible(&m.branch, ancestors))
        .collect()
}

/// 生成尾部块【关键记忆】。`budget` 是字符上限（默认 1200）。
///
/// 只放"一句话 + 当天"，不放全文：全文在磁盘上，模型需要细节可以自己从历史里找。
pub fn render_block(memories: &[Memory], budget: usize) -> Option<String> {
    render_block_for(memories, budget, true)
}

/// render_block 的带开关版：allow_flip_hint 控制**要不要给「被拒之后改判」的协议出口**。
///
/// 为什么要这个开关（2026-11-01 边界测试）：那一轮如果本身就是表白轮，尾部块末尾已经有
/// stance_hint 在教模型写〔态度:接受〕/〔态度:拒绝〕；再叠一条同款指令，模型可能写成两行
/// 一样的标记（解析只认第一行，但那是运气不是设计）。所以**同一轮只留一处**教它写标记：
/// 表白轮用 stance_hint，普通轮才用这里这条。
pub fn render_block_for(memories: &[Memory], budget: usize, allow_flip_hint: bool) -> Option<String> {
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
                Some(Stance::Accepted) => {
                    let when = match m.accepted_day.as_deref() {
                        Some(d) => format!("{d}已接受"),
                        None => "已被接受".to_string(),
                    };
                    parts.push(format!("：{when}（不要再演第一次表白）"));
                }
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
            // 她的告白（2026-11-02）：直接给她的原话，模型才知道"她已经说过一次"
            if m.kind == "ai_confession" {
                let q = m.quote.trim();
                lines.push(if q.is_empty() {
                    format!("- {} · {}", m.day_label, m.label)
                } else {
                    format!("- {} · {}：{}", m.day_label, m.label, q)
                });
                used += lines.last().map(|l| l.chars().count()).unwrap_or(0);
                continue;
            }
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
    // 2026-11-01 补上**反向出口**（用户指出的逻辑漏洞）：被拒之后她真的可能改主意，
    // 而那种轮次不是"表白轮"，原本根本不会要求表态标记 → 卡片永远停在"被拒绝"，
    // 提示词还在一遍遍说"不要写成她已经答应"，演员被自己锁死。所以给一条明确的协议出口。
    if has_refused {
        // ① 这条**永远给**：防止模型"软接受"（PUA 式的默许、其实早就喜欢）
        s.push_str("\n（被拒绝的表白只是「说过了」，关系没有因此确立：不要写成她已经答应、默许或「其实早就喜欢」）。");
        // ② 反向出口**只在非表白轮给**：表白轮末尾的 stance_hint 已经在教它写标记，
        //    同一轮两条同款指令会让模型写出两行一模一样的标记（2026-11-01 边界测试）
        if allow_flip_hint {
            s.push_str("\n（如果**这一轮**剧情里她真的改变了主意、接受了其中一次表白，请在回复最后单独一行原样输出〔态度:接受〕，软件会据此把这张卡改成「已接受」；没有接受就不要写这一行，也不要提前替她答应。）");
        }
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

/// 带**归属角色**的版本（2026-11-11）：角色视角下只给她自己的记忆卡。
pub fn capture_first_events_as(
    story_id: &str,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    text: &str,
    ai_reply: &str,
    events: &[(String, String)],
    roles: &[&str],
) -> Result<Vec<String>, String> {
    capture_first_events_in_as(&memories_dir(story_id), branch, node_id, day_label, text, ai_reply, events, roles)
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
    capture_first_events_in_as(dir, branch, node_id, day_label, text, ai_reply, events, &[])
}

/// 真正干活的那个：`role` = 这一轮 AI 演的是谁（写进卡片的 `character:`；空 = 全局）。
pub fn capture_first_events_in_as(
    dir: &Path,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    text: &str,
    ai_reply: &str,
    events: &[(String, String)],
    roles: &[&str],
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

    // ===== 2026-11-01：**被拒之后的改判**（用户指出的逻辑漏洞）=====
    // 原来只有"这一轮又检测到表白事件"时才会改判（见下面的 confession 分支）。
    // 现实是：被拒之后过几天，她在**普通对话**里松口了 —— 那一轮没有表白事件，
    // 于是卡永远停在"被拒绝"，提示词还在一遍遍说"不要写成她已经答应"，演员被自己锁死。
    // 现在：只要模型按【关键记忆】里那条协议写了〔态度:接受〕，就把那张卡改成已接受。
    if parse_marker(ai_reply) == Some(Stance::Accepted)
        && !events.iter().any(|(k, _)| k == "confession")
    {
        if let Some(m) = find("confession") {
            if effective_stance(m) != Some(Stance::Accepted) {
                flip_confession_to_accepted(dir, &m.id, day_label)?;
                made.push("表白（改判为已接受）".to_string());
            }
        }
    }

    // 这一轮的指纹（2026-11-01）：用来分辨"同一轮被重复分析"和"真的又表白了一次"。
    // 为什么需要：evolution 的「立即分析」可以对同一个节点反复跑，原来每跑一次 attempts +1、
    // 拒绝次数 +1（用户实测：一张卡从 4 次涨到 10 次），数字就不再是"她拒绝过几次"。
    let round_key = round_fingerprint(text, ai_reply);

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
                    let accepted_day = if stance_now == Stance::Accepted { day_label.to_string() } else { String::new() };
                    let front = confession_front(
                        branch, node_id, day_label, importance, matched, 1,
                        refusals_now,
                        stance_now, &accepted_day, &round_key, att.score, &reasons, &quote_now,
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
                    // **同一轮重复分析不再累加**：只在"这一轮和上次记的指纹不同"时才算新的一次
                    let same_round = !fm_get(fm, "last_round").unwrap_or_default().is_empty()
                        && fm_get(fm, "last_round").unwrap_or_default() == round_key;
                    let attempts = prev_attempts + if same_round { 0 } else { 1 };
                    let refusals = prev_refusals + if !same_round && stance_now == Stance::Refused { 1 } else { 0 };
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
                    // 后来接受的那一天：只在**刚变成已接受**时写入，之后不再覆盖
                    let prev_accepted = fm_get(fm, "accepted_day").unwrap_or_default();
                    let accepted_day = if stance == Stance::Accepted {
                        if prev_accepted.trim().is_empty() { day_label.to_string() } else { prev_accepted.clone() }
                    } else {
                        String::new()
                    };
                    let body = confession_body(attempts, &first_day, stance, &merged, refusals, &snippet, &label, &quote_now);
                    let front = confession_front(
                        branch, node_id, &first_day, importance, matched, attempts, refusals, stance,
                        &accepted_day, &round_key, att.score, &reasons, &quote_now,
                    );
                    write_memory(dir, &m.id, &front, &body)?;
                    made.push(format!("{label}（第 {attempts} 次）"));
                }
            }
            continue;
        }

        // ===== 她的告白（2026-11-02）：她主动说的那句，只记第一次 =====
        if kind == "ai_confession" {
            if find(kind).is_some() {
                continue; // 一个分支只留一条
            }
            // 用**她的原话**当正文（不是整轮）：这张卡要保留"她是怎么说的"
            let quote = quote_gist(ai_reply, 80);
            let body = format!(
                "# {day_label} · {label}\n\n{quote}\n\n> 由系统从剧情里提炼：**她主动说的**。此后不要再让她「第一次」表白，也不要把这张卡当成用户表白。\n"
            );
            let front = format!(
                "---\nkind: {kind}\nlabel: {label}\nday: {day_label}\nnode_id: {node_id}\nbranch: {}\n{}importance: {importance}\nmatched: {matched}\nquote: {quote}\ncreated_at: {}\n---\n",
                branch_key(branch),
                characters_line(roles),
                now_stamp()
            );
            let id = next_id(dir, &label);
            write_memory(dir, &id, &front, &body)?;
            made.push(label);
            continue;
        }

        // ===== 其余三类：只记第一次；**承诺例外**（2026-10-30 用户选 A：允许多条，各自带状态） =====
        if kind != "promise" && find(kind).is_some() {
            continue;
        }
        // 卡片正文末尾那句话按类型分（2026-10-30）：承诺允许多条，不能说"后续同类不再记录"
        let tail = if kind == "promise" {
            "这条承诺会各自带状态（待兑现 / 已兑现 / 未兑现），结算在记忆卡上点徽标也能改。"
        } else {
            "这是这个分支里第一次发生，后续同类事件不再单独记录。"
        };
        let body = format!(
            "# {day_label} · {label}\n\n{snippet}\n\n> 由系统从剧情里提炼（命中「{matched}」）。{tail}\n"
        );
        let promise_line = if kind == "promise" { "promise_state: 待兑现\n" } else { "" };
        let front = format!(
            "---\nkind: {kind}\nlabel: {label}\nday: {day_label}\nnode_id: {node_id}\nbranch: {}\n{}importance: {importance}\n{promise_line}created: {}\n---\n",
            branch_key(branch),
            characters_line(roles),
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
/// 有没有「待兑现」的承诺（决定要不要给模型注入兑现协议；没有就零 token）。
pub fn has_pending_promise(story_id: &str) -> bool {
    let dir = memories_dir(story_id);
    let Ok(rd) = std::fs::read_dir(&dir) else { return false };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !name.contains("承诺") || !name.ends_with(".md") {
            continue;
        }
        if let Ok(raw) = std::fs::read_to_string(e.path()) {
            let fm_end = raw.find("\n---").unwrap_or(0);
            if fm_get(&raw[..fm_end], "promise_state").as_deref() == Some("待兑现") {
                return true;
            }
        }
    }
    false
}

/// 手动改承诺状态（人工补兑现 / 改回待兑现）。返回是否找到这张卡。
pub fn set_promise_state_in(dir: &Path, id: &str, state: &str) -> Result<bool, String> {
    if !matches!(state, "待兑现" | "已兑现" | "未兑现") {
        return Err("状态只能是 待兑现 / 已兑现 / 未兑现".into());
    }
    let path = dir.join(format!("{id}.md"));
    if !path.exists() {
        return Ok(false);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let updated = if raw.contains("promise_state:") {
        let mut out = String::new();
        for line in raw.lines() {
            if line.trim_start().starts_with("promise_state:") {
                out.push_str(&format!("promise_state: {state}\n"));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    } else {
        return Err("这张卡没有承诺状态字段".into());
    };
    std::fs::write(&path, updated).map_err(|e| e.to_string())?;
    Ok(true)
}

pub fn settle_promise_in(dir: &Path, fulfilled: bool, source: &str) -> Result<Option<String>, String> {
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
    // 记录是谁判的：model（模型写的标记）/ manual（作者手打或点徽标）
    let mut updated = raw.replacen("promise_state: 待兑现", &format!("promise_state: {new_state}"), 1);
    if !updated.contains("promise_source:") {
        updated = updated.replacen(&format!("promise_state: {new_state}"), &format!("promise_state: {new_state}\npromise_source: {source}"), 1);
    }
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

/// 一轮对话的指纹（FNV-1a 64）：只用来判断"是不是同一轮被重复分析"。
///
/// 不用哈希库：这里不需要抗碰撞，只需要**稳定**（同一轮每次都算出同一个值）且够短。
/// 同一轮重跑分析 → 指纹相同 → 不再把表白次数 / 被拒次数 +1。
fn round_fingerprint(text: &str, ai_reply: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes().chain([0u8]).chain(ai_reply.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// 记录「她的告白」（日摘要协议用，2026-11-02）：**一个分支只留第一条**。
///
/// 为什么从日摘要来：表白说法补不完，交给"每天必做"的日摘要顺手判一次，
/// 比每轮对话都判便宜得多，也不额外加一次模型请求。
pub fn record_ai_confession(
    story_id: &str,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    quote: &str,
) -> Result<bool, String> {
    record_ai_confession_in(&memories_dir(story_id), branch, node_id, day_label, quote)
}

/// 带**归属角色**的版本（2026-11-11）：日结里知道是谁告白，就记在谁名下。
pub fn record_ai_confession_as(
    story_id: &str,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    quote: &str,
    roles: &[&str],
) -> Result<bool, String> {
    record_ai_confession_in_as(&memories_dir(story_id), branch, node_id, day_label, quote, roles)
}

/// [`record_ai_confession`] 的注入版（测试用临时目录）。
pub fn record_ai_confession_in(
    dir: &Path,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    quote: &str,
) -> Result<bool, String> {
    record_ai_confession_in_as(dir, branch, node_id, day_label, quote, &[])
}

pub fn record_ai_confession_in_as(
    dir: &Path,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    quote: &str,
    roles: &[&str],
) -> Result<bool, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let existing = list_all_in(dir);
    let dup = existing.iter().any(|mm| {
        mm.kind == "ai_confession"
            && (mm.branch.is_none() || mm.branch.as_deref() == branch.as_deref())
    });
    if dup {
        return Ok(false);
    }
    let label = kind_label("ai_confession");
    let importance = kind_weight("ai_confession").unwrap_or(88);
    let q = quote.trim();
    let body = format!(
        "# {day_label} · {label}\n\n{q}\n\n> 由系统从日摘要里提炼：**她主动说的**。此后不要再让她「第一次」表白，也不要把这张卡当成用户表白。\n"
    );
    let front = format!(
        "---\nkind: ai_confession\nlabel: {label}\nday: {day_label}\nnode_id: {node_id}\nbranch: {}\n{}importance: {importance}\nmatched: 日摘要\nquote: {q}\ncreated_at: {}\n---\n",
        branch_key(branch),
        characters_line(roles),
        now_stamp()
    );
    let id = next_id(dir, &label);
    write_memory(dir, &id, &front, &body)?;
    Ok(true)
}

/// 把一张"被拒绝"的表白卡改判成"已接受"（2026-11-01）。
///
/// **只改 front matter 的两行**：`stance` 和 `accepted_day`。次数、理由、她当时的原话、
/// 正文全部原样保留 —— 那是历史事实，抹掉就分不清"当初被拒过"和"其实没拒过"了。
fn flip_confession_to_accepted(dir: &Path, id: &str, day_label: &str) -> Result<(), String> {
    let path = dir.join(format!("{id}.md"));
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let raw = raw.replace("\r\n", "\n");
    let mut lines: Vec<String> = raw.lines().map(|s| s.to_string()).collect();
    // front matter = 第一行 --- 到下一行 --- 之间
    let mut seen = 0usize;
    let mut end: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        if l.trim() == "---" {
            seen += 1;
            if seen == 2 {
                end = Some(i);
                break;
            }
        }
    }
    let Some(end) = end else {
        return Err(format!("记忆卡 {id} 的 front matter 坏了，没法改判"));
    };
    let body_lines = lines.split_off(end); // body_lines[0] 是收尾的 ---
    let mut front = lines;
    front.retain(|l| !l.starts_with("accepted_day:")); // 只留一条
    for l in front.iter_mut() {
        if l.starts_with("stance:") {
            *l = "stance: accepted".to_string();
        }
    }
    front.push(format!("accepted_day: {day_label}"));
    let mut out = front.join("\n");
    out.push('\n');
    out.push_str(&body_lines.join("\n"));
    out.push('\n');
    std::fs::write(&path, out).map_err(|e| e.to_string())
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
    accepted_day: &str,
    last_round: &str,
    score: i32,
    reasons: &str,
    quote: &str,
) -> String {
    format!(
        "---\nkind: confession\nlabel: 表白\nday: {day_label}\nnode_id: {node_id}\nbranch: {}\nimportance: {importance}\nmatched: {matched}\nattempts: {attempts}\nrefusals: {refusals}\nstance: {}\naccepted_day: {accepted_day}\nlast_round: {last_round}\nscore: {score}\nreasons: {reasons}\nquote: {quote}\ncreated_at: {}\n---\n",
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
        let hit = settle_promise_in(&dir, false, "model").unwrap().unwrap();
        assert_eq!(hit, "003-承诺.md", "只结算最近一条");
        let raw = std::fs::read_to_string(dir.join("003-承诺.md")).unwrap();
        assert!(raw.contains("promise_state: 未兑现"));
        let raw2 = std::fs::read_to_string(dir.join("002-承诺.md")).unwrap();
        assert!(raw2.contains("promise_state: 待兑现"), "更早那条不动");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod ancestor_visibility_tests {
    use super::branch_visible;

    fn s(x: &str) -> Option<String> { Some(x.to_string()) }

    /// 三层嵌套能看见祖先的；兄弟分支互不可见；主线是共享段（2026-10-30）
    #[test]
    fn nested_sees_ancestors_but_not_siblings() {
        let chain = vec![s("b3"), s("b2"), s("b1"), None]; // 3 层 → 2 层 → 1 层 → 主线
        assert!(branch_visible(&None, &chain), "主线共享段");
        assert!(branch_visible(&s("b1"), &chain), "父分支");
        assert!(branch_visible(&s("b2"), &chain), "祖父分支");
        assert!(branch_visible(&s("b3"), &chain), "自己");
        assert!(!branch_visible(&s("bx"), &chain), "兄弟分支不可见");
        assert!(!branch_visible(&s("b1"), &[s("bx"), None]), "别人的祖先链不算");
    }
}

#[cfg(test)]
mod role_owner_tests {
    use super::*;

    fn card(fm_extra: &str) -> String {
        format!("---\nkind: promise\nlabel: 承诺\nday: 第 1 天\nnode_id: 1\nbranch: main\n{fm_extra}importance: 80\n---\n\n# 第 1 天 · 承诺\n\n她说会回来。\n")
    }

    #[test]
    fn parses_character_from_front_matter() {
        let m = parse_memory("001", &card("character: 夏婉琴\n")).expect("应能解析");
        assert_eq!(m.characters, vec!["夏婉琴".to_string()]);
    }

    #[test]
    fn old_cards_without_character_are_global() {
        let m = parse_memory("001", &card("")).expect("应能解析");
        assert!(m.characters.is_empty(), "老卡没有 character 行 → 全局");
        // 空白值也算没写
        let m2 = parse_memory("002", &card("character:   \n")).expect("应能解析");
        assert!(m2.characters.is_empty(), "空白值也算没写");
    }

    #[test]
    fn filter_keeps_own_and_global_only() {
        let own = parse_memory("001", &card("character: 夏婉琴\n")).unwrap();
        let other = parse_memory("002", &card("character: 王老师\n")).unwrap();
        let global = parse_memory("003", &card("")).unwrap();
        let all = vec![own, other, global];

        // 旁白 / 全局：全给
        assert_eq!(filter_for_role(&all, "").len(), 3);
        // 角色视角：自己的 + 没有归属的
        let mine = filter_for_role(&all, "夏婉琴");
        assert_eq!(mine.len(), 2);
        assert!(mine.iter().any(|m| m.id == "001"));
        assert!(mine.iter().any(|m| m.id == "003"));
        assert!(!mine.iter().any(|m| m.id == "002"), "别人的卡不该出现在她的视角里");
    }

    #[test]
    fn capture_writes_character_into_the_card() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.tmp/mem-role-tests")
            .join(format!("{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let events = vec![("promise".to_string(), "答应你".to_string())];
        let made = capture_first_events_in_as(&dir, &None, 1, "第 1 天", "我答应你，明天回来。", "好，我等你。", &events, &["夏婉琴"]).unwrap();
        assert!(!made.is_empty(), "应该写下一条承诺卡");
        let all = list_all_in(&dir);
        let card = all.iter().find(|m| m.kind == "promise").expect("应有承诺卡");
        assert_eq!(card.characters, vec!["夏婉琴".to_string()], "写卡时要带上 characters:");
        // 空角色 → 不写这一行（老文件格式不变）
        let dir2 = dir.join("global");
        std::fs::create_dir_all(&dir2).unwrap();
        capture_first_events_in_as(&dir2, &None, 2, "第 2 天", "我答应你，后天回来。", "好。", &events, &[]).unwrap();
        let all2 = list_all_in(&dir2);
        assert!(all2.iter().all(|m| m.characters.is_empty()), "空角色不该写 characters 行");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn character_line_skips_free_and_blank() {
        assert_eq!(characters_line(&[]), "");
        assert_eq!(characters_line(&["   "]), "");
        assert_eq!(characters_line(&["自由"]), "");
        assert_eq!(characters_line(&[" 夏婉琴 "]), "characters: 夏婉琴\n");
        // 两个人 + 去重 + 跳过自由
        assert_eq!(characters_line(&["夏婉琴", "顾宸熙", "夏婉琴", "自由"]), "characters: 夏婉琴,顾宸熙\n");
    }
}

#[cfg(test)]
mod multi_participant_tests {
    use super::*;

    fn card(fm_extra: &str) -> String {
        format!("---\nkind: promise\nlabel: 承诺\nday: 第 1 天\nnode_id: 1\nbranch: main\n{fm_extra}importance: 80\n---\n\n# 第 1 天 · 承诺\n\n他说会回来。\n")
    }

    #[test]
    fn reads_comma_separated_participants() {
        let m = parse_memory("001", &card("characters: 夏婉琴,顾宸熙\n")).expect("应能解析");
        assert_eq!(m.characters, vec!["夏婉琴".to_string(), "顾宸熙".to_string()]);
        // 中文逗号/分号也认
        let m2 = parse_memory("002", &card("characters: 夏婉琴，顾宸熙；王老师\n")).unwrap();
        assert_eq!(m2.characters.len(), 3);
    }

    #[test]
    fn legacy_single_character_still_parses() {
        // 2026-11-11 写的卡只有 character: 单值 —— 必须照样读，视为一个元素的集合
        let m = parse_memory("003", &card("character: 夏婉琴\n")).expect("老卡要能读");
        assert_eq!(m.characters, vec!["夏婉琴".to_string()]);
        // 有 characters 时忽略老的 character
        let m2 = parse_memory("004", &card("character: 王老师\ncharacters: 夏婉琴,顾宸熙\n")).unwrap();
        assert_eq!(m2.characters, vec!["夏婉琴".to_string(), "顾宸熙".to_string()]);
    }

    #[test]
    fn both_participants_see_the_card_others_dont() {
        let both = parse_memory("005", &card("characters: 夏婉琴,顾宸熙\n")).unwrap();
        let none = parse_memory("006", &card("")).unwrap();
        let all = vec![both, none];

        // 她俩都看得到（"顾宸熙向夏婉琴表白"这种两边都该记得）
        for who in ["夏婉琴", "顾宸熙"] {
            let seen = filter_for_role(&all, who);
            assert_eq!(seen.len(), 2, "{who} 应看到两张（自己的 + 全局的）");
        }
        // 没参与的第三个人只看得到全局卡
        let outsider = filter_for_role(&all, "王老师");
        assert_eq!(outsider.len(), 1, "别人的事不该出现在她视角里");
    }

    #[test]
    fn writes_both_names_into_front_matter() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.tmp/mem-multi-tests")
            .join(format!("{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let events = vec![("promise".to_string(), "答应你".to_string())];
        capture_first_events_in_as(&dir, &None, 1, "第 1 天", "我答应你。", "好。", &events, &["夏婉琴", "顾宸熙"]).unwrap();
        let all = list_all_in(&dir);
        let c = all.iter().find(|m| m.kind == "promise").expect("应有承诺卡");
        assert_eq!(c.characters, vec!["夏婉琴".to_string(), "顾宸熙".to_string()]);
        // 两个人都在场时，从任一方的视角都能取到
        assert_eq!(filter_for_role(&all, "夏婉琴").len(), 1);
        assert_eq!(filter_for_role(&all, "顾宸熙").len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn card(id: &str, chars: &str) -> Memory {
        let extra = if chars.is_empty() { "" } else { "chars" };
        let _ = extra;
        let raw = format!("---\nkind: promise\nlabel: 承诺\nday: 第 {id} 天\nnode_id: 1\nbranch: main\n{}importance: 80\n---\n\n正文\n",
            if chars.is_empty() { String::new() } else { format!("characters: {chars}\n") });
        parse_memory(id, &raw).unwrap()
    }

    #[test]
    fn same_pair_regardless_of_order() {
        let a = card("1", "夏婉琴,顾宸熙");
        let all = vec![a.clone()];
        assert_eq!(cards_of_pair(&all, "夏婉琴", "顾宸熙").len(), 1);
        assert_eq!(cards_of_pair(&all, "顾宸熙", "夏婉琴").len(), 1, "顺序不重要");
        // 有第三个人参与的卡不算这一对
        let three = card("2", "夏婉琴,顾宸熙,王老师");
        let all2 = vec![three];
        assert!(cards_of_pair(&all2, "夏婉琴", "顾宸熙").is_empty());
        // 没有这组的卡 → 空（界面据此显示"还没有"）
        assert!(cards_of_pair(&[card("3", "")], "夏婉琴", "顾宸熙").is_empty());
    }

    #[test]
    fn groups_are_keyed_by_participant_set() {
        let all = vec![
            card("1", "夏婉琴,顾宸熙"),
            card("2", "顾宸熙,夏婉琴"),
            card("3", "夏婉琴,王老师"),
            card("4", ""),
        ];
        let g = group_by_participants(&all);
        assert_eq!(g.len(), 3, "a×b（两张合并一组）+ a×c + 全局");
        let ab = g.iter().find(|(k, _)| k == &vec!["夏婉琴".to_string(), "顾宸熙".to_string()]);
        assert_eq!(ab.expect("应有 a×b 组").1.len(), 2, "顺序不同也算同一组");
        assert!(g.iter().any(|(k, _)| k.is_empty()), "无归属的卡自成一组");
    }
}
