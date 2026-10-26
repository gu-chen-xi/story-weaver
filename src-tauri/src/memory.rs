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
    for m in memories {
        let one = one_line_summary(&m.body, 60);
        let line = if one.is_empty() {
            format!("- {} · {}（重要度 {}）", m.day_label, m.label, m.importance)
        } else {
            format!("- {} · {}：{}", m.day_label, m.label, one)
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
    if skipped > 0 {
        s.push_str(&format!("\n（另有 {skipped} 条较早的记忆未列出）"));
    }
    Some(s)
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

/// 抓取"第一次"事件并落盘（每种类型、每个分支只写一次）。
///
/// `events` 来自 `evolution::analyze::extract_events`；`text` 是**这一轮**的拼装文本
/// （`evolution::memory_round` 给的「用户那条消息 + AI 回复」），原样落进记忆正文。
/// 返回这次新建的记忆的标签（用于提示"已记下 xxx"）。
pub fn capture_first_events(
    story_id: &str,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    text: &str,
    events: &[(String, String)],
) -> Result<Vec<String>, String> {
    capture_first_events_in(&memories_dir(story_id), branch, node_id, day_label, text, events)
}

/// [`capture_first_events`] 的注入版（`dir` = 记忆目录）。
pub fn capture_first_events_in(
    dir: &Path,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    text: &str,
    events: &[(String, String)],
) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    // 已有记忆：按 (kind) 记下"本分支已记过哪几类" —— 只记第一次
    // 注意：主线（branch = None）的记忆算"分叉前的共同历史"，**分支里不再重复记同一类**
    // （否则同一件事会在分支视角里出现两次）。
    let existing: Vec<(String, Option<String>)> = list_all_in(dir)
        .into_iter()
        .map(|m| (m.kind, m.branch))
        .collect();
    let seen = |kind: &str| {
        existing.iter().any(|(k, b)| {
            k == kind && (b.is_none() || b.as_deref() == branch.as_deref())
        })
    };

    let mut made: Vec<String> = Vec::new();
    for (kind, matched) in events {
        let Some(importance) = kind_weight(kind) else { continue };
        if seen(kind) {
            continue;   // 只记第一次
        }
        let label = kind_label(kind);
        // v3.4 修（2026-10-26）：`text` 现在就是**这一轮**（用户那条消息 + AI 回复，
        // 由 `evolution::memory_round` 拼好）。直接整段落进记忆，不再取"命中词 ±80 字"
        // 的窗口 —— 那个窗口会把上一轮 AI 的半句话一起卷进来。
        let snippet = text.trim().to_string();
        if snippet.is_empty() {
            continue;
        }
        // 序号 = 现有文件数 + 1（稳定、可读、便于排序）
        let seq = std::fs::read_dir(dir).map(|r| r.count()).unwrap_or(0) + 1;
        let id = format!("{seq:03}-{label}");
        let body = format!(
            "# {day_label} · {label}\n\n{snippet}\n\n> 由系统从剧情里提炼（命中「{matched}」）。\
             这是这个分支里第一次发生，后续同类事件不再单独记录。\n"
        );
        let content = format!(
            "---\nkind: {kind}\nlabel: {label}\nday: {day_label}\nnode_id: {node_id}\nbranch: {}\nimportance: {importance}\nmatched: {matched}\ncreated_at: {}\n---\n\n{body}",
            branch_key(branch),
            now_stamp()
        );
        std::fs::write(dir.join(format!("{id}.md")), content).map_err(|e| e.to_string())?;
        made.push(label);
    }
    Ok(made)
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
