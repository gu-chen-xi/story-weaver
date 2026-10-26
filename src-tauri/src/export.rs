// export.rs —— 剧情导出（MVP 已含 TXT；html/md 属最终计划，本次实装）
// 三种格式共用同一数据管道：load_story_export() 读取故事信息+过滤节点+过滤消息
// 存放位置：<项目根>/exports/story/<故事标题>_<时间戳>.txt|.md|.html（见 docs/user/数据与备份.md）
// 待办标注：
//   1) 消息级时间戳：当前不输出 created_at（聊天真实时间），改为由节点日期承载剧情时间；
//      后续修复为"剧情内时间戳"（具体方案待定）。
use sqlx::Row;
use std::path::PathBuf;



/// 导出根目录。
///
/// 第五批「路径模型」修复：原来这里是"项目根的上一级"（`cwd/..`），
/// **打包后当前目录由用户怎么启动决定**，导出会落到随机位置甚至没有写权限的地方。
/// 现在统一走 paths（`%APPDATA%\<bundle id>\exports`）。
fn project_root() -> PathBuf {
    crate::paths::exports_root()
}

/// 清洗文件名非法字符：\/:*?"<>| 及控制字符
fn clean_filename(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t' => {
                out.push('_')
            }
            _ => out.push(ch),
        }
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        "未命名故事".to_string()
    } else {
        trimmed
    }
}

/// 剧情日期后缀：time_origin（YYYY-MM-DD）为绝对时间起点，
/// 第 N 天 = 起点 + (N-1) 天，返回 "MM-DD"；time_origin 非日期格式则返回 None（降级只显示节点名）。
fn story_date_suffix(time_origin: &str, date_label: &str) -> Option<String> {
    // 兼容 YYYY-MM-DD / YYYY.M.D / YYYY/M/D / YYYY年M月D日
    let norm: String = time_origin
        .trim()
        .chars()
        .map(|ch| match ch {
            '-' | '/' | '.' | '年' | '月' => '-',
            _ => ch,
        })
        .collect();
    let parts: Vec<&str> = norm.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let (y, m, d) = (
        parts[0].trim().parse::<i64>().ok()?,
        parts[1].trim().parse::<i64>().ok()?,
        parts[2].trim().trim_end_matches('日').parse::<i64>().ok()?,
    );
    let day_no: i64 = date_label.chars().filter(|ch| ch.is_ascii_digit()).collect::<String>().parse().ok()?;
    if day_no <= 0 {
        return None;
    }
    let days = days_from_civil(y, m, d) + (day_no - 1);
    let (_, mm, dd) = civil_from_days(days);
    Some(format!("{:02}-{:02}", mm, dd))
}

/// 公历 → 1970-01-01 起算的天数（civil_from_days 的逆运算）
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

// ===== 公共数据管道：三种格式共用 =====

struct MsgExport {
    role: String,
    content: String,
    mode: String,
}

// 保留未用：id / is_flashback 暂不参与渲染（保留完整结构便于将来引用）
#[allow(dead_code)]
struct NodeExport {
    id: i64,
    date_label: String,
    node_type: String,
    is_flashback: bool,
    msgs: Vec<MsgExport>,
}

struct StoryExport {
    title: String,
    time_origin: String,
    /// 导出时刻（`YYYY-MM-DD HH:MM:SS`）—— **由 SQLite 的 localtime 提供**，
    /// 与库里的时间同一口径（旧实现硬编码 +8 小时，非 +8 时区会差几个小时）。
    exported_at: String,
    day_count: i64, // 全故事"第 N 天"节点数（插曲/回忆不计入）
    nodes: Vec<NodeExport>,
}

async fn load_story_export(story_id: &str, node_ids: &[i64], mode_filter: &str) -> Result<StoryExport, String> {
    // 1. 故事标题 / 时间起点（library.db）
    let lib_pool = crate::db::library_pool().await.map_err(|e| e.to_string())?;
    let row = sqlx::query("SELECT title, time_origin FROM stories WHERE id = ?1")
        .bind(story_id)
        .fetch_optional(&lib_pool)
        .await
        .map_err(|e| e.to_string())?;
    let (title, time_origin) = match row {
        Some(r) => (
            r.try_get::<String, _>(0).map_err(|e| e.to_string())?,
            r.try_get::<String, _>(1).map_err(|e| e.to_string())?,
        ),
        None => return Err("故事不存在".to_string()),
    };

    // 2. 全部节点（按 sort_order）
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // 导出时刻：交给 SQLite 算本地时间（不自己加减时区）
    let exported_at: String = sqlx::query_scalar("SELECT datetime('now','localtime')")
        .fetch_one(&pool)
        .await
        .unwrap_or_default();
    let nodes = sqlx::query(
        "SELECT id, date_label, sort_order, node_type, is_flashback
         FROM timeline_nodes ORDER BY sort_order"
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;

    // 3. 全故事天数（插曲/回忆不计入）
    let day_count = nodes
        .iter()
        .filter(|n| {
            let t: String = n.try_get(3).unwrap_or_default();
            t != "interlude" && t != "flashback"
        })
        .count() as i64;

    // 4. 过滤节点 + 读消息 + 按模式过滤
    let mut out_nodes = Vec::new();
    for n in &nodes {
        let id: i64 = n.try_get(0).map_err(|e| e.to_string())?;
        if !node_ids.is_empty() && !node_ids.contains(&id) {
            continue;
        }
        let date_label: String = n.try_get(1).map_err(|e| e.to_string())?;
        let node_type: String = n.try_get(3).map_err(|e| e.to_string())?;
        let is_flashback: bool = n.try_get(4).map_err(|e| e.to_string())?;
        let msgs = sqlx::query(
            "SELECT id, role, content, mode, created_at FROM messages WHERE node_id = ?1 ORDER BY id"
        )
        .bind(id)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
        let mut out_msgs = Vec::new();
        for m in &msgs {
            let role: String = m.try_get(1).map_err(|e| e.to_string())?;
            let content: String = m.try_get(2).map_err(|e| e.to_string())?;
            let m_mode: String = m.try_get(3).map_err(|e| e.to_string())?;
            let keep = match mode_filter {
                "narr" => m_mode == "narr",
                "chat" => m_mode == "chat",
                _ => true,
            };
            if !keep {
                continue;
            }
            // 2026-10-26：协议标记〔态度:接受/拒绝〕只给软件判定用，导出文件里不出现
    out_msgs.push(MsgExport { role, content: crate::memory::strip_marker(&content), mode: m_mode });
        }
        out_nodes.push(NodeExport { id, date_label, node_type, is_flashback, msgs: out_msgs });
    }

    Ok(StoryExport { title, time_origin, exported_at, day_count, nodes: out_nodes })
}

fn scope_label_or_default(scope_label: &str) -> String {
    if scope_label.trim().is_empty() {
        "全部节点 · 全部".to_string()
    } else {
        scope_label.trim().to_string()
    }
}

// ===== TXT 渲染 =====

fn render_txt(e: &StoryExport, scope_label: &str) -> String {
    let mut sb = String::new();
    sb.push_str(&format!("《{}》\n", e.title));
    sb.push_str(&format!("时间起点：{}\n", e.time_origin));
    sb.push_str(&format!("故事有 {} 天\n", e.day_count));
    sb.push_str(&format!("导出时间：{}\n", e.exported_at));
    sb.push_str(&format!("导出范围：{}\n", scope_label_or_default(scope_label)));
    sb.push_str("＝".repeat(28).as_str());
    sb.push('\n');

    let mut any = false;
    for n in &e.nodes {
        let prefix = if n.node_type == "interlude" || n.node_type == "flashback" {
            format!("【🎭 {}】", n.date_label)
        } else {
            match story_date_suffix(&e.time_origin, &n.date_label) {
                Some(suf) => format!("【{} · {}】", n.date_label, suf),
                None => format!("【{}】", n.date_label),
            }
        };
        sb.push('\n');
        sb.push_str(&prefix);
        sb.push('\n');
        for m in &n.msgs {
            let who = if m.role == "user" { "你" } else { "AI" };
            sb.push_str(&format!("[{}]\n", who));
            sb.push_str(&m.content);
            sb.push('\n');
            sb.push('\n');
            any = true;
        }
    }
    if !any {
        sb.push_str("（暂无剧情内容）\n");
    }
    // 末尾附「AI 生成」声明：文件离开软件后仍带着标识
    sb.push('\n');
    sb.push_str(AI_NOTICE_TXT);
    sb
}

/// 导出文件末尾的「AI 生成」告知。
///
/// 为什么必须有：AI 生成合成内容带标识是法规要求（《人工智能生成合成内容标识办法》2025-09-01 施行）。
/// 导出的剧情会离开本软件（被转发、上传、投稿），**标识必须跟着文件走** —— 只放在界面上是不够的。
const AI_NOTICE_TXT: &str =
    "────────────────────────────\n\
【AI 生成内容声明】\n\
本作品由 AI 剧情创作软件辅助生成，属人工智能生成合成内容。\n\
按《人工智能生成合成内容标识办法》，对外发布时请保留本声明并显式标注「AI 生成」。\n\
请勿删除本声明。\n";

// 注意：这里**不带前导 `---`** —— 节点循环末尾已经推过一条分隔线，再带一条会出现连续两条。
const AI_NOTICE_MD: &str =
    "> **【AI 生成内容声明】** 本作品由 AI 剧情创作软件辅助生成，属人工智能生成合成内容。\n\
> 按《人工智能生成合成内容标识办法》，对外发布时请保留本声明并显式标注「AI 生成」。\n\
> 请勿删除本声明。\n";


// ===== Markdown 渲染 =====

/// 整段加引用前缀（每行 >，空行保留为 > 延续引用）
fn block_quote(s: &str) -> String {
    s.lines()
        .map(|l| format!("> {}", l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 解析对话消息的 [角色名] 前缀：([角色名], 去除前缀后的文本)；无前缀返回 (None, 原文)
fn parse_dialog(content: &str) -> (Option<String>, String) {
    if let Some(rest) = content.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            return (Some(rest[..end].trim().to_string()), rest[end + 1..].trim().to_string());
        }
    }
    (None, content.to_string())
}

fn render_md(e: &StoryExport, scope_label: &str) -> String {
    let mut sb = String::new();
    sb.push_str(&format!("# 《{}》\n\n", e.title));
    sb.push_str(&format!("- 时间起点：{}\n", e.time_origin));
    sb.push_str(&format!("- 故事有 {} 天\n", e.day_count));
    sb.push_str(&format!("- 导出范围：{}\n", scope_label_or_default(scope_label)));
    sb.push_str(&format!("- 导出时间：{}\n\n---\n\n", e.exported_at));

    let mut any = false;
    for n in &e.nodes {
        let head = if n.node_type == "interlude" || n.node_type == "flashback" {
            format!("## 🎭 {}\n\n", n.date_label)
        } else {
            match story_date_suffix(&e.time_origin, &n.date_label) {
                Some(suf) => format!("## {}（{}）\n\n", n.date_label, suf),
                None => format!("## {}\n\n", n.date_label),
            }
        };
        sb.push_str(&head);
        for m in &n.msgs {
            if m.role == "user" {
                sb.push_str(&format!("**你**\n\n{}\n\n", block_quote(&m.content)));
            } else if m.mode == "narr" {
                sb.push_str(&format!("**旁白**\n\n{}\n\n", m.content));
            } else {
                // 对话 → 引用块，带角色名
                let (who, text) = parse_dialog(&m.content);
                let who = who.unwrap_or_else(|| "对话".to_string());
                sb.push_str(&format!("**{}**\n\n{}\n\n", who, block_quote(&text)));
            }
            any = true;
        }
        sb.push_str("---\n\n");
    }
    if !any {
        sb.push_str("（暂无剧情内容）\n");
    }
    // 末尾附「AI 生成」声明：文件离开软件后仍带着标识。
    // 前面的分隔线由节点循环提供（每个节点后一条），所以常量里不再自带，避免连续两条 `---`
    sb.push_str(AI_NOTICE_MD);
    sb
}

// ===== HTML 渲染（浅色阅读主题，单文件无 JS） =====

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn html_body(text: &str) -> String {
    html_escape(text).replace('\n', "<br>")
}

fn render_html(e: &StoryExport, scope_label: &str) -> String {
    let mut sb = String::new();
    sb.push_str("<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"utf-8\">\n");
    sb.push_str(&format!("<title>《{}》</title>\n", html_escape(&e.title)));
    sb.push_str("<style>");
    sb.push_str(
        "body{font-family:'Segoe UI','PingFang SC','Microsoft YaHei',sans-serif;max-width:860px;margin:0 auto;padding:40px 24px 60px;background:#faf9f7;color:#2b2f36;line-height:1.8;}\n\
         header{border-bottom:1px solid #e3e0da;padding-bottom:18px;margin-bottom:28px;}\n\
         h1{font-size:26px;margin:0 0 10px;}\n\
         .meta{color:#7a7f87;font-size:13px;}\n\
         .node{margin-bottom:34px;}\n\
         .node h2{font-size:17px;border-left:4px solid #d9a441;padding-left:10px;margin:0 0 14px;color:#333;}\n\
         .msg{margin-bottom:14px;}\n\
         .tag{display:inline-block;font-size:12px;font-weight:600;padding:2px 9px;border-radius:10px;margin-bottom:5px;}\n\
         .msg.user .tag{background:#e8eef7;color:#3d6ba8;}\n\
         .msg.narr .tag{background:#efece6;color:#8a7a55;}\n\
         .msg.chat .tag{background:#f7e8d8;color:#b26a1f;}\n\
         .body{font-size:14.5px;}\n\
         .msg.user .body{color:#555c66;}\n\
         blockquote{margin:0;padding:8px 14px;border-left:3px solid #e0b27a;background:#fbf6ef;border-radius:0 8px 8px 0;color:#5a4632;}\n\
         footer{margin-top:40px;padding-top:14px;border-top:1px solid #e3e0da;color:#a5a29c;font-size:12px;}"
    );
    sb.push_str("</style>\n</head>\n<body>\n");
    sb.push_str(&format!(
        "<header><h1>《{}》</h1><p class=\"meta\">时间起点：{} · 故事有 {} 天 · 导出范围：{} · 导出时间：{}</p></header>\n",
        html_escape(&e.title),
        html_escape(&e.time_origin),
        e.day_count,
        html_escape(&scope_label_or_default(scope_label)),
        &e.exported_at
    ));

    let mut any = false;
    for n in &e.nodes {
        let head = if n.node_type == "interlude" || n.node_type == "flashback" {
            format!("🎭 {}", html_escape(&n.date_label))
        } else {
            match story_date_suffix(&e.time_origin, &n.date_label) {
                Some(suf) => format!("{}（{}）", html_escape(&n.date_label), suf),
                None => html_escape(&n.date_label),
            }
        };
        sb.push_str(&format!("<section class=\"node\"><h2>{}</h2>\n", head));
        for m in &n.msgs {
            if m.role == "user" {
                sb.push_str(&format!(
                    "<div class=\"msg user\"><span class=\"tag\">你</span><div class=\"body\">{}</div></div>\n",
                    html_body(&m.content)
                ));
            } else if m.mode == "narr" {
                sb.push_str(&format!(
                    "<div class=\"msg narr\"><span class=\"tag\">旁白</span><div class=\"body\">{}</div></div>\n",
                    html_body(&m.content)
                ));
            } else {
                let (who, text) = parse_dialog(&m.content);
                let who = html_escape(&who.unwrap_or_else(|| "对话".to_string()));
                sb.push_str(&format!(
                    "<div class=\"msg chat\"><span class=\"tag\">{}</span><blockquote>{}</blockquote></div>\n",
                    who,
                    html_body(&text)
                ));
            }
            any = true;
        }
        sb.push_str("</section>\n");
    }
    if !any {
        sb.push_str("<p>（暂无剧情内容）</p>\n");
    }
    // 末尾附「AI 生成」声明（法规要求标识跟着内容走；只放在界面里不够）
    sb.push_str(
        "<footer>\n\
  <p><strong>【AI 生成内容声明】</strong>本作品由 <b>AI 剧情创作软件</b> 辅助生成，属人工智能生成合成内容。</p>\n\
  <p>按《人工智能生成合成内容标识办法》，对外发布时请保留本声明并显式标注「AI 生成」。<b>请勿删除本声明。</b></p>\n\
</footer>\n</body>\n</html>\n",
    );
    sb
}

// ===== 导出入口 =====

fn write_export_file(
    title: &str,
    ext: &str,
    content: &str,
    minute_level: bool,
    stamp: &str, // 导出时刻（`e.exported_at`），**由 SQLite localtime 提供**
) -> Result<String, String> {
    let export_dir = project_root().join("exports").join("story");
    std::fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;
    let ts = if minute_level {
        ts_compact_minute(stamp)
    } else {
        stamp.replace(['-', ':', ' '], "").replace('.', "_")
    };
    let file_name = format!("{}_{}.{}", clean_filename(title), ts, ext);
    let path = export_dir.join(file_name);
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// 导出 TXT（原有行为 + 头部新增"故事有 N 天"；文件名秒级时间戳）。
pub async fn export_story_txt(story_id: &str, node_ids: &[i64], mode_filter: &str, scope_label: &str) -> Result<String, String> {
    let e = load_story_export(story_id, node_ids, mode_filter).await?;
    let body = render_txt(&e, scope_label);
    write_export_file(&e.title, "txt", &body, false, &e.exported_at)
}

/// 导出 Markdown（对话加工为引用块；文件名分钟级时间戳）。
pub async fn export_story_md(story_id: &str, node_ids: &[i64], mode_filter: &str, scope_label: &str) -> Result<String, String> {
    let e = load_story_export(story_id, node_ids, mode_filter).await?;
    let body = render_md(&e, scope_label);
    write_export_file(&e.title, "md", &body, true, &e.exported_at)
}

/// 导出 HTML（浅色阅读主题单文件；文件名分钟级时间戳）。
pub async fn export_story_html(story_id: &str, node_ids: &[i64], mode_filter: &str, scope_label: &str) -> Result<String, String> {
    let e = load_story_export(story_id, node_ids, mode_filter).await?;
    let body = render_html(&e, scope_label);
    write_export_file(&e.title, "html", &body, true, &e.exported_at)
}

/// 分钟级紧凑时间戳："2026-09-27 18:30:25" → "20260927_1830"
fn ts_compact_minute(stamp: &str) -> String {
    let digits: String = stamp.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 12 {
        format!("{}_{}", &digits[..8], &digits[8..12])
    } else {
        digits
    }
}

// 说明（2026-10-18）：原来这里有一个 `chrono_local_now()`，用 `SystemTime + 8*3600` 手工格式化。
// 它假定用户在中国时区 —— 非 +8 时区导出的"导出时间"会和节点时间差几个小时，
// 同一个文件里出现两套时间口径。现在改成**由 SQLite 的 `datetime('now','localtime')` 提供**
// （见 `StoryExport::exported_at`），与库里的时间完全同源。

/// 天数 → 公历（1970-01-01 起算）
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests;
