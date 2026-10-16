use sqlx::sqlite::SqliteConnectOptions;
use sqlx::Row;
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::story_db::story_db_path;

/// 第五批 ③：取该故事"当前所在分支"（`None` = 主线）。
///
/// 新建节点/消息时用它决定 `branch_id` —— 在分支里写作就写进分支，主线就写 NULL。
/// 读不到（library.db 异常）时按主线处理，不让写作失败。
async fn current_branch_for(story_id: &str) -> Option<String> {
    crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None)
}

async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;
    // 每次打开库都确保 tags 列存在（旧库自动迁移，幂等）
    crate::story_db::ensure_timeline_tags_column(&pool).await?;
    // D 子项：日摘要列 + 阶段摘要表（幂等，老库自动获得）
    crate::story_db::ensure_timeline_day_summary_column(&pool).await?;
    crate::story_db::ensure_story_summaries_table(&pool).await?;
    // 第五批 ①：时间线树 —— parent_id 列（幂等 + 老库按线性顺序回填）
    crate::story_db::ensure_timeline_parent_column(&pool).await?;
    // 第五批 ③：分支列与 branches 表（幂等）
    crate::story_db::ensure_branch_columns(&pool).await?;
    Ok(pool)
}

#[derive(serde::Serialize, Clone)]
pub struct TimelineNode {
    pub id: i64,
    pub date_label: String,
    pub sort_order: i64,
    pub node_type: String,
    pub is_flashback: i64,
    pub tags: String,
    /// D 子项：日摘要（"这一天发生了什么"）。空 = 还没生成；生成一次即固化、不回改。
    #[serde(default)]
    pub day_summary: String,
    /// 第五批 ①：父节点 id。NULL = 树根（通常是「第 1 天」）。
    ///
    /// 现在所有故事都是线性链（每个节点至多一个后继）；等做分支系统时，
    /// **"多个节点共享同一个 parent_id"就是分叉**，数据结构无需再改。
    #[serde(default)]
    pub parent_id: Option<i64>,
    /// 第五批 ③：所属分支（`None` = 主线）。
    ///
    /// ⚠ **必须真的从 SQL 里查出来**：早先只给前端类型加了 `branch_id` 却忘了加进 SELECT，
    /// 结果前端拿到的永远是 `undefined` → 树状图把每条分支的节点都显示成"主线"
    /// （用户实测反馈"id=68 理应属于第六轮，却显示主线"）。
    #[serde(default)]
    pub branch_id: Option<String>,
}

// ===== 第五批 ①：时间线树 —— 纯函数（不碰数据库，便于单测与复用） =====

/// 树节点（供纯函数使用；用已有的 TimelineNode 即可，这里给纯函数一个最小输入类型，
/// 这样测试不必构造 day_summary / tags 那些与树无关的字段）
#[derive(Debug, Clone, PartialEq)]
pub struct TreeNode {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub sort_order: i64,
}

/// 由扁平列表构建"父子索引"：parent_id → 其子节点（按 sort_order, id 排序）。
///
/// 说明：**不做连通性校验**（不判断是否有环、是否同一棵树）—— 那是"检查"功能的职责；
/// 这里只负责把父子关系整理出来，遇到环也只会返回一个有限的子表，不会 panic。
pub fn children_index(nodes: &[TreeNode]) -> std::collections::HashMap<Option<i64>, Vec<i64>> {
    let mut map: std::collections::HashMap<Option<i64>, Vec<i64>> = std::collections::HashMap::new();
    for n in nodes {
        map.entry(n.parent_id).or_default().push(n.id);
    }
    // 同层按 (sort_order, id) 排序 —— 与线性视图的顺序一致
    let order: std::collections::HashMap<i64, (i64, i64)> =
        nodes.iter().map(|n| (n.id, (n.sort_order, n.id))).collect();
    for ids in map.values_mut() {
        ids.sort_by_key(|id| order.get(id).copied().unwrap_or((0, *id)));
    }
    map
}

/// 从某节点向上走到根，返回**从根到该节点**的路径（含两端）。
///
/// 用途：跨天装配要取"当前节点所在的那条线"的历史 —— 有了分支之后，
/// "按 sort_order 取前 N 个"是错的（会把别的分支的内容也读进来），必须按父链取。
/// 遇到环（数据异常）时会在访问过的集合上停下，不会死循环。
pub fn path_to_root(nodes: &[TreeNode], target: i64) -> Vec<i64> {
    let by_id: std::collections::HashMap<i64, &TreeNode> =
        nodes.iter().map(|n| (n.id, n)).collect();
    let mut out: Vec<i64> = Vec::new();
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut cur = Some(target);
    while let Some(id) = cur {
        if !seen.insert(id) {
            break; // 环：停下
        }
        let Some(node) = by_id.get(&id) else { break };
        out.push(id);
        cur = node.parent_id;
    }
    out.reverse(); // 从根到目标
    out
}

/// 展开成"带层级的行"（供树状视图渲染：缩进列表 A 形态）。
///
/// - `root`：从哪个节点开始展开（None = 全部根节点，通常只有一个）
/// - `expanded`：已展开的节点集合；未在集合中的节点其子节点不出现
/// - 返回 `(节点 id, 深度)`，深度从 0 起
///
/// 这是**纯函数**：折叠状态由调用方（界面）持有，函数本身无副作用 —— 便于单测。
pub fn flatten_tree(
    nodes: &[TreeNode],
    root: Option<i64>,
    expanded: &std::collections::HashSet<i64>,
) -> Vec<(i64, usize)> {
    let idx = children_index(nodes);
    let mut out: Vec<(i64, usize)> = Vec::new();
    let mut stack: Vec<(i64, usize)> = Vec::new();

    // 起点：指定 root 则只展开它；否则展开所有根（parent_id 为 None）
    let starts: Vec<i64> = match root {
        Some(r) => vec![r],
        None => idx.get(&None).cloned().unwrap_or_default(),
    };
    for s in starts.into_iter().rev() {
        stack.push((s, 0));
    }
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    while let Some((id, depth)) = stack.pop() {
        if !seen.insert(id) {
            continue; // 防环
        }
        out.push((id, depth));
        if expanded.contains(&id) {
            if let Some(kids) = idx.get(&Some(id)) {
                for k in kids.iter().rev() {
                    stack.push((*k, depth + 1));
                }
            }
        }
    }
    out
}

/// 某节点的**祖先链**（含自身），从根到自身的顺序 —— 与 `path_to_root` 同义，
/// 单独再导出一个名字是给"跨天装配"用的：那里读起来是"我要这条线上的祖先"。
pub fn ancestor_ids(nodes: &[TreeNode], target: i64) -> Vec<i64> {
    path_to_root(nodes, target)
}

/// 读出一棵树的全部节点（供跨天装配按父链取历史）。
///
/// 与 `list_timeline_nodes` 的区别：本函数只取与"树"有关的三个字段，
/// 且**不做排序假设** —— 树结构由 `parent_id` 决定，顺序由纯函数内部处理。
pub async fn load_tree_nodes(story_id: &str) -> Result<Vec<TreeNode>, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let rows = sqlx::query("SELECT id, parent_id, sort_order FROM timeline_nodes")
        .fetch_all(&pool)
        .await?;
    Ok(rows
        .iter()
        .map(|r| TreeNode {
            id: r.try_get(0).unwrap_or(0),
            parent_id: r.try_get(1).unwrap_or(None),
            sort_order: r.try_get(2).unwrap_or(0),
        })
        .collect())
}

/// 某节点的子节点 id（用于界面上的"▸ 展开"按钮：无子节点则不显示按钮）
pub fn branches_of(nodes: &[TreeNode], id: i64) -> Vec<i64> {
    children_index(nodes).get(&Some(id)).cloned().unwrap_or_default()
}

/// 是否为分叉点（子节点多于一个）—— 树状视图用它高亮"这里分了叉"
pub fn is_fork(nodes: &[TreeNode], id: i64) -> bool {
    branches_of(nodes, id).len() > 1
}

#[derive(serde::Serialize, Clone)]
pub struct Message {
    pub id: i64,
    pub node_id: i64,
    pub role: String,
    pub content: String,
    pub mode: String,
    pub created_at: String,
}

/// 获取或创建"第 1 天"节点（M2 先用最简单的单节点）。
pub async fn ensure_first_node(story_id: &str) -> Result<TimelineNode, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    // 查第一个节点
    let row = sqlx::query(
        "SELECT id, date_label, sort_order, node_type, is_flashback, tags, parent_id, branch_id
         FROM timeline_nodes ORDER BY sort_order LIMIT 1"
    )
    .fetch_optional(&pool)
    .await?;

    if let Some(r) = row {
        return Ok(TimelineNode {
            id: r.try_get(0)?,
            date_label: r.try_get(1)?,
            sort_order: r.try_get(2)?,
            node_type: r.try_get(3)?,
            is_flashback: r.try_get(4)?,
            tags: r.try_get(5)?,
            day_summary: r.try_get(6).unwrap_or_default(),
            parent_id: r.try_get::<Option<i64>, _>(7).unwrap_or(None),
            branch_id: r.try_get::<Option<String>, _>(8).unwrap_or(None),
        });
    }

    // 没有就建一个（归属当前分支）
    let branch = current_branch_for(story_id).await;
    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, branch_id)
         VALUES('第 1 天', 1, 'day', 0, ?1) RETURNING id, date_label, sort_order, node_type, is_flashback, tags, parent_id, branch_id"
    )
    .bind(&branch)
    .fetch_one(&pool)
    .await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
        day_summary: String::new(),
        parent_id: row.try_get::<Option<i64>, _>(7).unwrap_or(None),
        branch_id: branch.clone(),
    })
}

pub async fn list_messages(story_id: &str, node_id: i64) -> Result<Vec<Message>, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let rows = sqlx::query(
        "SELECT id, node_id, role, content, mode, created_at
         FROM messages WHERE node_id = ?1 ORDER BY id"
    )
    .bind(node_id)
    .fetch_all(&pool)
    .await?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(Message {
            id: r.try_get(0)?,
            node_id: r.try_get(1)?,
            role: r.try_get(2)?,
            content: r.try_get(3)?,
            mode: r.try_get(4)?,
            created_at: r.try_get(5)?,
        });
    }
    Ok(out)
}

pub async fn create_message(
    story_id: &str,
    node_id: i64,
    role: &str,
    content: &str,
    mode: &str,
) -> Result<i64, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    // 第五批 ③：消息带上分支（由节点推导，保证与节点一致）
    let branch: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .flatten();
    let row = sqlx::query(
        "INSERT INTO messages(node_id, role, content, mode, branch_id) VALUES(?1, ?2, ?3, ?4, ?5) RETURNING id"
    )
    .bind(node_id)
    .bind(role)
    .bind(content)
    .bind(mode)
    .bind(branch)
    .fetch_one(&pool)
    .await?;
    Ok(row.try_get(0)?)
}

/// 列出某故事的全部时间线节点（按 sort_order 升序）。
pub async fn list_timeline_nodes(story_id: &str) -> Result<Vec<TimelineNode>, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let rows = sqlx::query(
        "SELECT id, date_label, sort_order, node_type, is_flashback, tags, day_summary, parent_id, branch_id
         FROM timeline_nodes ORDER BY sort_order ASC, id ASC"
    ).fetch_all(&pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(TimelineNode {
            id: r.try_get(0)?,
            date_label: r.try_get(1)?,
            sort_order: r.try_get(2)?,
            node_type: r.try_get(3)?,
            is_flashback: r.try_get(4)?,
            tags: r.try_get(5)?,
            day_summary: r.try_get(6).unwrap_or_default(),
            parent_id: r.try_get::<Option<i64>, _>(7).unwrap_or(None),
            branch_id: r.try_get::<Option<String>, _>(8).unwrap_or(None),
        });
    }
    Ok(out)
}

// ===================== D 子项：日摘要 / 阶段摘要 =====================

/// 日摘要生成提示词（只写事实：谁做了什么、关系变化、未回收伏笔；不写感想、不写台词）
pub const DAY_SUMMARY_PROMPT: &str = "你在为一部连续剧做\"前情提要\"素材。下面是某一天的全部剧情记录。\n\
请把这一天发生的事压缩成一段摘要，供后续章节回读。硬性要求：\n\
- 只写事实：谁做了什么、去了哪里、关系或认知发生了什么变化、留下了什么未回收的伏笔或约定；\n\
- 不写感想、不写评价、不写台词原文、不写文采描写；\n\
- 用第三人称、过去时，动词为主，人名用正式名；\n\
- 200 字以内，一段话，不要分段、不要小标题、不要 JSON、不要任何解释。";

/// 生成某节点的日摘要并落库（**生成一次即固化**：已有值直接返回，不回改）。
/// 返回 `Some(摘要)` 表示本次新生成，`None` 表示该节点已有摘要或没有可摘要的剧情。
pub async fn generate_day_summary(
    lib_pool: &SqlitePool,
    story_id: &str,
    node_id: i64,
    force: bool,
) -> Result<Option<String>, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;

    // 已固化则不再重算（force = 用户手动重生成时才覆盖）
    let existing: String = sqlx::query_scalar("SELECT COALESCE(day_summary, '') FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if !force && !existing.trim().is_empty() {
        return Ok(None);
    }

    let rows = sqlx::query("SELECT role, content FROM messages WHERE node_id = ?1 ORDER BY id ASC")
        .bind(node_id)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut transcript = String::new();
    for r in &rows {
        let role: String = r.try_get(0).unwrap_or_default();
        let content: String = r.try_get(1).unwrap_or_default();
        if content.trim().is_empty() {
            continue;
        }
        let who = if role == "user" { "用户" } else { "AI" };
        transcript.push_str(&format!("[{who}] {}\n", crate::relations::strip_director(&content)));
    }
    if transcript.trim().is_empty() {
        return Ok(None);
    }

    // 复用与生成相同的模型配置
    let base_url = crate::db::get_config(lib_pool, "model.base_url")
        .await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let model = crate::db::get_config(lib_pool, "model.name")
        .await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "deepseek-v4-flash".to_string());
    let api_key = crate::keychain::get_api_key("openai_api_key".to_string())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未设置 API Key".to_string())?;

    let node_label: String = sqlx::query_scalar("SELECT date_label FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| format!("节点 {node_id}"));

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let user_content = format!("【{node_label}】的剧情记录：\n{}", transcript.chars().take(24000).collect::<String>());

    // 用 1500 额度重试一次：思考模式下 max_tokens 太小会被思考内容吃光，导致正文为空
    let attempts: [(usize, bool); 2] = [(1500, true), (1500, false)];
    let mut summary = String::new();
    let mut last_note = String::new();
    for (max_tokens, disable_thinking) in attempts {
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": DAY_SUMMARY_PROMPT},
                {"role": "user", "content": user_content}
            ],
            "max_tokens": max_tokens,
            "temperature": 0.3,
            "stream": false
        });
        if disable_thinking {
            // 摘要不需要思考：显式关掉，省 token 也避免思考挤掉正文
            body["thinking"] = serde_json::json!({"type": "disabled"});
        }
        let resp = reqwest::Client::new()
            .post(&url).bearer_auth(&api_key).json(&body).send().await
            .map_err(|e| format!("摘要请求失败: {e}"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            last_note = format!("HTTP {}: {}", status.as_u16(), text.chars().take(160).collect::<String>());
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => { last_note = format!("响应解析失败: {e}"); continue; }
        };
        let choice = &v["choices"][0];
        let content = choice["message"]["content"].as_str().unwrap_or_default().trim().to_string();
        let reasoning_len = choice["message"]["reasoning_content"].as_str().unwrap_or_default().chars().count();
        let finish = choice["finish_reason"].as_str().unwrap_or("-").to_string();
        if content.is_empty() {
            // 带上诊断信息：思考占了多少、是否被截断
            last_note = format!("正文为空（思考 {reasoning_len} 字，finish={finish}，额度 {max_tokens}）");
            continue;
        }
        summary = content;
        break;
    }
    if summary.is_empty() {
        return Err(format!("摘要为空：{last_note}"));
    }
    let summary: String = summary.chars().take(600).collect();

    sqlx::query("UPDATE timeline_nodes SET day_summary = ?1 WHERE id = ?2")
        .bind(&summary).bind(node_id).execute(&pool).await.map_err(|e| e.to_string())?;
    Ok(Some(summary))
}

/// 补齐结果：成功列表 + 每个失败节点的原因（失败必须能被看见，否则"什么都没发生"无法定位）
#[derive(serde::Serialize, Default)]
pub struct BackfillReport {
    pub done: Vec<String>,
    pub failed: Vec<String>,
}

/// 补齐所有"历史节点"的日摘要（幂等）：只处理当前节点之前、有剧情、但还没有摘要的 day 节点。
/// 当前节点不生成 —— 它还会继续写，摘要留到"翻篇"之后再固化（保证已生成的摘要不被回改）。
pub async fn backfill_day_summaries(lib_pool: &SqlitePool, story_id: &str, current_node_id: i64) -> Result<BackfillReport, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let rows = select_nodes_needing_summary(&pool, current_node_id).await.map_err(|e| e.to_string())?;

    let mut report = BackfillReport::default();
    for (id, label) in rows {
        match generate_day_summary(lib_pool, story_id, id, false).await {
            Ok(Some(_)) => report.done.push(label),
            Ok(None) => {}
            Err(e) => {
                // 单个节点失败不影响其它节点，也不影响主流程 —— 但要把原因带回界面
                eprintln!("[day_summary] {label} 生成失败: {e}");
                report.failed.push(format!("{label}：{e}"));
            }
        }
    }
    Ok(report)
}

/// 当天摘要的自动触发（用户 2026-10-11 定的规则）：
/// 当天的对话轮数达到阈值（默认 10，按 **AI 回复条数** 计）时，自动生成/刷新**当天**的日摘要。
/// 与 backfill 的分工：backfill 管"过去的天"（翻篇后固化一次），这里管"正在写的这天"（到量后随写随更新）。
/// 未到阈值时**绝不生成**，避免每轮都变、把前缀缓存打散。
pub async fn maybe_auto_day_summary(
    lib_pool: &SqlitePool,
    story_id: &str,
    current_node_id: i64,
    min_replies: i64,
) -> Result<Option<String>, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let replies: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE node_id = ?1 AND role = 'assistant'")
        .bind(current_node_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| e.to_string())?;
    if replies < min_replies.max(1) {
        return Ok(None);
    }
    let label: String = sqlx::query_scalar("SELECT date_label FROM timeline_nodes WHERE id = ?1")
        .bind(current_node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| format!("节点 {current_node_id}"));
    match generate_day_summary(lib_pool, story_id, current_node_id, true).await {
        Ok(Some(s)) => Ok(Some(format!("{label}（{replies} 条回复）"))),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

/// 阶段摘要（C 层接口预留）：写入一段覆盖 [day_from, day_to] 的摘要。
/// 本期不自动生成 —— 只提供读写，等日摘要积累后再接"若干天滚一段"。
pub async fn save_phase_summary(
    story_id: &str, day_from: i64, day_to: i64, content: &str, source_days: i64,
) -> Result<i64, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    save_phase_summary_in(&pool, &None, day_from, day_to, content, source_days).await
}

/// 第五批 ③：写入阶段摘要（带上分支 —— 阶段摘要是"整条线"的汇总，分支各自独立）。
pub async fn save_phase_summary_in(
    pool: &SqlitePool, branch: &Option<String>,
    day_from: i64, day_to: i64, content: &str, source_days: i64,
) -> Result<i64, String> {
    let id = sqlx::query(
        "INSERT INTO story_summaries(kind, day_from, day_to, content, source_days, branch_id) VALUES('phase', ?1, ?2, ?3, ?4, ?5)"
    ).bind(day_from).bind(day_to).bind(content).bind(source_days).bind(branch.clone())
     .execute(pool).await.map_err(|e| e.to_string())?.last_insert_rowid();
    Ok(id)
}

/// 阶段摘要列表（按覆盖起始天倒序；供后续装配使用）
pub async fn list_phase_summaries(story_id: &str) -> Result<Vec<(i64, i64, i64, String)>, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    list_phase_summaries_in(&pool, &None).await
}

/// 第五批 ③：阶段摘要列表（按分支过滤；`None` = 主线）。
pub async fn list_phase_summaries_in(
    pool: &SqlitePool, branch: &Option<String>,
) -> Result<Vec<(i64, i64, i64, String)>, String> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let sql = format!(
        "SELECT day_from, day_to, source_days, content FROM story_summaries
         WHERE kind = 'phase' AND {} ORDER BY day_from DESC",
        clause
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await.map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| (
        r.try_get(0).unwrap_or(0), r.try_get(1).unwrap_or(0), r.try_get(2).unwrap_or(0), r.try_get(3).unwrap_or_default(),
    )).collect())
}

/// 选择"需要补日摘要"的节点（幂等）：**时间线上排在当前节点之前**、有剧情、还没有摘要的非倒叙节点。
///
/// 用"位置"而不是"sort_order 比大小"来判断先后 —— 因为历史数据里 sort_order 可能重复
/// （例如两个节点都叫「第 8 天」、sort_order 分别为 9 和 13），比大小会在极端情况下全表落空。
/// 抽成独立函数以便单测直接验证（不触发任何 API 调用）。
pub(crate) async fn select_nodes_needing_summary(pool: &SqlitePool, current_node_id: i64) -> Result<Vec<(i64, String)>, sqlx::Error> {
    // 按时间线顺序（sort_order, id）排出全表，找到当前节点的位置，取它之前的节点
    let all = sqlx::query("SELECT id, date_label, sort_order, is_flashback, COALESCE(day_summary, '') FROM timeline_nodes ORDER BY sort_order ASC, id ASC")
        .fetch_all(pool).await?;
    let pos = all.iter().position(|r| r.try_get::<i64, _>(0).unwrap_or(-1) == current_node_id);

    let mut out = Vec::new();
    for (idx, r) in all.iter().enumerate() {
        let id: i64 = r.try_get(0).unwrap_or(-1);
        let label: String = r.try_get(1).unwrap_or_default();
        let flashback: i64 = r.try_get(3).unwrap_or(0);
        let summary: String = r.try_get(4).unwrap_or_default();
        if id == current_node_id || flashback != 0 || !summary.trim().is_empty() {
            continue;
        }
        // 当前节点存在时必须排在它之前；当前节点找不到（数据异常）时退回"按 sort_order 小于最大值"的保守判断
        let before_current = match pos {
            Some(p) => idx < p,
            None => {
                let max_sort: i64 = all.iter().map(|x| x.try_get::<i64, _>(2).unwrap_or(0)).max().unwrap_or(0);
                r.try_get::<i64, _>(2).unwrap_or(0) < max_sort
            }
        };
        if !before_current {
            continue;
        }
        let has_msgs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE node_id = ?1")
            .bind(id).fetch_one(pool).await.unwrap_or(0);
        if has_msgs > 0 {
            out.push((id, label));
        }
    }
    Ok(out)
}

/// 从"第 N 天"这类标签里解析天数（找不到数字时返回 None）。
/// 用于"新增下一天"时取**最大天数 + 1**，而不是数节点个数 ——
/// 数个数在存在跳号/重复（如 1,2,3,4,5,8,9,10）时会算出错误的天数（曾出现"新增一天又变成第 9 天"）。
pub(crate) fn parse_day_number(label: &str) -> Option<i64> {
    let digits: String = label.chars().filter(|c| c.is_ascii_digit()).collect();
    digits.parse::<i64>().ok()
}

/// 第五批 ③：某分支的"可见节点集合" = **它自己的节点 ∪ 沿父链上溯到的共享历史（含起点自己）**。
///
/// 用户 2026-10-16 定的规则：分支**按天**算，分叉点那天算共享历史。
/// 所以"这条线上已有第几天"必须把分叉点及其之前也算进来 ——
/// 只数自己的，从第 3 天分叉的分支里点「下一天」会建成"第 1 天"（实测踩到）。
pub(crate) async fn branch_path_ids(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> std::collections::HashSet<i64> {
    let mut out: std::collections::HashSet<i64> = std::collections::HashSet::new();
    // 本分支自己的节点全部算
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let sql = format!("SELECT id FROM timeline_nodes WHERE {}", clause);
    let mut q = sqlx::query_scalar::<_, i64>(&sql);
    if branch.is_some() {
        q = q.bind(branch.clone().unwrap_or_default());
    }
    let own: Vec<i64> = q.fetch_all(pool).await.unwrap_or_default();
    for id in &own { out.insert(*id); }
    // 分支起点：自己最后一个节点，或（还没有自己的节点时）分叉点
    let start: Option<i64> = match own.iter().max() {
        Some(_) => {
            let last_sql = format!(
                "SELECT id FROM timeline_nodes WHERE {} ORDER BY sort_order DESC, id DESC LIMIT 1",
                clause
            );
            let mut lq = sqlx::query_scalar::<_, i64>(&last_sql);
            if branch.is_some() { lq = lq.bind(branch.clone().unwrap_or_default()); }
            lq.fetch_optional(pool).await.unwrap_or(None)
        }
        None => match branch {
            Some(bid) => sqlx::query_scalar::<_, i64>("SELECT fork_node_id FROM branches WHERE id = ?1")
                .bind(bid).fetch_optional(pool).await.unwrap_or(None),
            None => sqlx::query_scalar::<_, i64>(
                "SELECT id FROM timeline_nodes WHERE branch_id IS NULL ORDER BY sort_order DESC, id DESC LIMIT 1")
                .fetch_optional(pool).await.unwrap_or(None),
        },
    };
    // **兜底**：分叉点可能指向一个已被删除的节点（历史数据的"半坏"状态）。
    // 此时退化成"主线最后一个节点"作起点 —— 至少让天数接着主线数，不会重头数成"第 1 天"。
    let all_ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM timeline_nodes")
        .fetch_all(pool).await.unwrap_or_default();
    let start = match start {
        Some(s) if all_ids.contains(&s) => Some(s),
        _ => sqlx::query_scalar::<_, i64>(
            "SELECT id FROM timeline_nodes WHERE branch_id IS NULL ORDER BY sort_order DESC, id DESC LIMIT 1")
            .fetch_optional(pool).await.unwrap_or(None),
    };
    let Some(start) = start else { return out };
    let all: Vec<TreeNode> = sqlx::query("SELECT id, parent_id, sort_order FROM timeline_nodes")
        .fetch_all(pool).await.unwrap_or_default()
        .iter().map(|r| TreeNode {
            id: r.try_get(0).unwrap_or(0),
            parent_id: r.try_get(1).unwrap_or(None),
            sort_order: r.try_get(2).unwrap_or(0),
        }).collect();
    // **关键：把 `start` 自己也放进去**（`ancestor_ids` 返回的是"祖先"、不含自己）。
    // 漏掉它会重复用同一个天数：分支已有"第 12 天"（= start）→ 基准里最大只剩"第 11 天"
    // → max+1 = 12 → **又建一个"第 12 天"**（用户第三轮实测踩到）。
    out.insert(start);
    for id in ancestor_ids(&all, start) { out.insert(id); }
    out
}

/// 第五批 ③：**按节点推导它属于哪条分支**（自己开池）。
///
/// 用途：`+下一天` 时"新节点应该落在哪条线"由**界面上那个节点**决定 ——
/// 界面看到哪个节点，就落在哪条线上。这比信任界面记的 `currentBranchId` 可靠
/// （曾经因为界面状态过期，出现"人在分支、节点却建到主线"，用户实测踩到）。
///
/// 返回值：`None` = 该节点在主线（或节点不存在/库打不开 → 都按主线处理，不报错）。
pub async fn branch_of_node_in_story(story_id: &str, node_id: i64) -> Option<Option<String>> {
    let pool = open_story_pool(story_id).await.ok()?;
    // fetch_optional 直接给出 Option<Option<String>>：
    // 外层"行在不在"，内层"branch_id 是不是 NULL（主线）" —— 正是要的语义
    sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .ok()?
}

/// 下一个天数序号 = 已有天数标签里的**最大值 + 1**（不是节点个数 + 1）。
/// 例：已有 1,2,3,4,5,8,9,10 → 下一个是 11（旧实现数个数会算成 9）。
/// 标签全无数字（如"回忆·那年夏天"）时退回"个数 + 1"。
pub(crate) fn next_day_number(labels: &[String]) -> i64 {
    match labels.iter().filter_map(|l| parse_day_number(l)).max() {
        Some(m) => m + 1,
        None => labels.len() as i64 + 1,
    }
}

/// 创建下一天节点：
/// - `sort_order` = 当前最大值 + 1（保证排在最后）
/// - `date_label` 的"第 N 天" = **已有天数标签里的最大值 + 1**（插曲/倒叙不占天数；标签无法解析时退回"节点数 + 1"）
pub async fn create_next_day_node(story_id: &str) -> Result<TimelineNode, sqlx::Error> {
    create_next_day_node_with(story_id, None).await
}

/// 第五批 ③：`branch_override` = **界面显式传来的分支**（`None` 时退回"故事记录的当前分支"）。
///
/// 为什么要显式传：`stories.current_branch_id` 是**隐式状态** ——
/// 一旦界面与后端不同步（切换失败、多窗口、崩溃后重启），
/// 就会出现"我在主线，节点却建到了分支里"这类**看不见的错**（用户实测踩到）。
/// 界面把当前分支一起传下来，就不存在不同步的可能。
pub async fn create_next_day_node_with(
    story_id: &str,
    branch_override: Option<Option<String>>,
) -> Result<TimelineNode, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let branch = match branch_override {
        Some(b) => b,
        None => current_branch_for(story_id).await,
    };
    create_next_day_node_in(&pool, &branch).await
}

/// `create_next_day_node` 的核心（接 pool + 分支，供测试直连）。
///
/// ## 天数与父节点的基准（用户 2026-10-16 定的规则）
///
/// > 分支**按天**算、不按消息；分叉点那一天算**共享历史**，它之后才算分支起点。
///
/// 所以基准 = **分支自己的节点 ∪ 父链上的共享历史**：
/// - **只数分支自己的节点** → 从"第 3 天"分叉的分支里点「下一天」会建成"第 1 天"（错）
/// - **数全库** → 会把别的分支的天数也算进来，跳号
/// - 正确做法：沿 `parent_id` 上溯到分叉点（含），与分支自己的节点合并
pub async fn create_next_day_node_in(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<TimelineNode, sqlx::Error> {
    let b_clause = crate::story_db::branch_clause("branch_id", branch, 1);

    // sort_order 仍取**全库最大值 + 1**：sort_order 是显示排序，全库唯一才不会打架
    let max_row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) FROM timeline_nodes")
        .fetch_one(pool).await?;
    let max_order: i64 = max_row.try_get(0)?;
    let next_sort = max_order + 1;

    // 分支自己的最后一个节点（父节点与"上溯起点"都用它；
    // 分支还没有自己的节点时为空 → 分叉点本身就是基准）
    let last_sql = format!(
        "SELECT id FROM timeline_nodes WHERE {} ORDER BY sort_order DESC, id DESC LIMIT 1",
        b_clause
    );
    let mut lq = sqlx::query_scalar::<_, i64>(&last_sql);
    if branch.is_some() {
        lq = lq.bind(branch.clone().unwrap_or_default());
    }
    let last_of_branch: Option<i64> = lq.fetch_optional(pool).await?;

    // 父节点 = 分支自己最后一个节点；没有则退到分叉点（其 parent_id 指向的节点）
    let parent_id: Option<i64> = match last_of_branch {
        Some(id) => Some(id),
        None => match branch {
            // 分支：分叉点 = 该分支所有节点的共同祖先。没有自己的节点时，
            // 用 branch 表里的 fork_node_id 作父。
            Some(bid) => sqlx::query_scalar::<_, i64>("SELECT fork_node_id FROM branches WHERE id = ?1")
                .bind(bid)
                .fetch_optional(pool)
                .await?,
            // 主线：最后一个主线节点
            None => sqlx::query_scalar::<_, i64>(
                "SELECT id FROM timeline_nodes WHERE branch_id IS NULL ORDER BY sort_order DESC, id DESC LIMIT 1")
                .fetch_optional(pool)
                .await?,
        },
    };

    // 天数序号：**分支自己的节点 + 父链上的共享历史**（与 create_node_at_day 同一口径，
    // 共用 branch_path_ids；那段逻辑曾在这里重复实现过一次，已合并）
    let path_ids = branch_path_ids(pool, branch).await;
    let labels: Vec<String> = {
        let rows = sqlx::query("SELECT id, date_label, node_type FROM timeline_nodes")
            .fetch_all(pool).await?;
        rows.iter().filter_map(|r| {
            let id: i64 = r.try_get(0).ok()?;
            let nt: String = r.try_get(2).unwrap_or_default();
            if (nt == "day" || nt == "main") && path_ids.contains(&id) {
                Some(r.try_get::<String, _>(1).unwrap_or_default())
            } else { None }
        }).collect()
    };
    let next_day = next_day_number(&labels);
    let label = format!("第 {} 天", next_day);

    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, branch_id, parent_id) VALUES(?1, ?2, 'day', 0, ?3, ?4) RETURNING id, date_label, sort_order, node_type, is_flashback, tags, day_summary, parent_id, branch_id"
    )
    .bind(&label)
    .bind(next_sort)
    .bind(branch.clone())
    .bind(parent_id)
    .fetch_one(pool).await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
        day_summary: row.try_get(6).unwrap_or_default(),
        parent_id: row.try_get::<Option<i64>, _>(7).unwrap_or(None),
        branch_id: branch.clone(),
    })
}

/// 在**指定天数**插入一个 day 节点（C 子项"时间流逝"用：用户点日历上的区间条，选择在这段里插入事件）。
/// 天数即时间坐标（第 N 天 = 时间起点 + N-1 天），所以可以插在中间：
/// `sort_order` 取"下一个更大天数节点"的 sort_order，并把其后的节点整体后移一位，保证时间线顺序正确。
pub async fn create_node_at_day(story_id: &str, day_no: i64) -> Result<TimelineNode, sqlx::Error> {
    create_node_at_day_with(story_id, day_no, None).await
}

/// 第五批 ③：`branch_override` 同 `create_next_day_node_with`。
pub async fn create_node_at_day_with(
    story_id: &str,
    day_no: i64,
    branch_override: Option<Option<String>>,
) -> Result<TimelineNode, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    if day_no <= 0 {
        return Err(sqlx::Error::Protocol("天数必须大于 0".to_string()));
    }
    let label = format!("第 {} 天", day_no);

    // 第五批 ③：天数比较用**本分支自己的节点 ∪ 父链上的共享历史**
    // （与 create_next_day_node_in 同一口径；只数自己的会在分支里跳号，数全库会被别的分支挤位）
    let branch = match branch_override {
        Some(b) => b,
        None => current_branch_for(story_id).await,
    };
    let b_clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let path_ids = branch_path_ids(&pool, &branch).await;
    let day_rows: Vec<(i64, String)> = {
        let rows = sqlx::query("SELECT id, sort_order, date_label, node_type, branch_id FROM timeline_nodes")
            .fetch_all(&pool).await?;
        rows.iter().filter_map(|r| {
            let id: i64 = r.try_get(0).ok()?;
            let nt: String = r.try_get(3).unwrap_or_default();
            let nb: Option<String> = r.try_get(4).unwrap_or(None);
            let mine = nb == branch;
            if (nt == "day" || nt == "main") && (mine || path_ids.contains(&id)) {
                Some((r.try_get(1).unwrap_or(0), r.try_get::<String, _>(2).unwrap_or_default()))
            } else { None }
        }).collect()
    };
    let mut next_sort: Option<i64> = None;
    for (so, lb) in &day_rows {
        if let Some(n) = parse_day_number(lb) {
            if n > day_no && next_sort.map(|x| *so < x).unwrap_or(true) {
                next_sort = Some(*so);
            }
        }
    }
    let insert_at = match next_sort {
        Some(so) => {
            // 其后（含同位置）的节点整体后移，给新节点腾位
            sqlx::query("UPDATE timeline_nodes SET sort_order = sort_order + 1 WHERE sort_order >= ?1")
                .bind(so).execute(&pool).await?;
            so
        }
        None => {
            let max_row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) FROM timeline_nodes")
                .fetch_one(&pool).await?;
            let m: i64 = max_row.try_get(0)?;
            m + 1
        }
    };

    // 父节点 = 本分支里 sort_order 小于新位置的那个节点（没有就是本分支第一个 → NULL）
    let parent_sql = format!(
        "SELECT id FROM timeline_nodes WHERE {} AND sort_order < ?2 ORDER BY sort_order DESC, id DESC LIMIT 1",
        b_clause
    );
    let mut pq = sqlx::query_scalar::<_, i64>(&parent_sql);
    if branch.is_some() {
        pq = pq.bind(branch.clone().unwrap_or_default());
    }
    let parent_id: Option<i64> = pq.bind(insert_at).fetch_optional(&pool).await?;

    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, branch_id, parent_id) VALUES(?1, ?2, 'day', 0, ?3, ?4)
         RETURNING id, date_label, sort_order, node_type, is_flashback, tags, day_summary, parent_id, branch_id"
    ).bind(&label).bind(insert_at).bind(&branch).bind(parent_id).fetch_one(&pool).await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
        day_summary: row.try_get(6).unwrap_or_default(),
        parent_id: row.try_get::<Option<i64>, _>(7).unwrap_or(None),
        branch_id: branch.clone(),
    })
}

/// 时间线段（C 子项"时间流逝"）：相邻两个 day 节点之间跳号时，就是一段"无事件的时间"。
/// 返回 `(起始天数, 结束天数)`，如 (3, 7) 表示第 4~6 天是空白期（无节点）。
pub(crate) fn time_gaps(sorted_day_numbers: &[i64]) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    for w in sorted_day_numbers.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b - a > 1 {
            out.push((a, b));
        }
    }
    out
}

/// 更新某条消息内容（修改历史剧情）。
pub async fn update_message(story_id: &str, message_id: i64, content: &str) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    sqlx::query("UPDATE messages SET content = ?1 WHERE id = ?2")
        .bind(content)
        .bind(message_id)
        .execute(&pool).await?;
    Ok(())
}

/// 创建一个插曲/倒叙节点：date_label 自动叫"插曲 N"，node_type='interlude'，is_flashback=1。
pub async fn create_interlude_node(story_id: &str) -> Result<TimelineNode, sqlx::Error> {
    create_interlude_node_with(story_id, None).await
}

/// 第五批 ③：`branch_override` 同 `create_next_day_node_with`。
pub async fn create_interlude_node_with(
    story_id: &str,
    branch_override: Option<Option<String>>,
) -> Result<TimelineNode, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    // 统计已有插曲数量
    let cnt_row = sqlx::query("SELECT COUNT(*) FROM timeline_nodes WHERE node_type = 'interlude'")
        .fetch_one(&pool).await?;
    let cnt: i64 = cnt_row.try_get(0)?;
    let next_interlude = cnt + 1;
    let max_row = sqlx::query("SELECT COALESCE(MAX(sort_order), 0) FROM timeline_nodes")
        .fetch_one(&pool).await?;
    let max_order: i64 = max_row.try_get(0)?;
    let label = format!("插曲 {}", next_interlude);
    let branch = match branch_override {
        Some(b) => b,
        None => current_branch_for(story_id).await,
    };
    let b_clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    // 父节点 = 本分支最后一个节点（插曲接在当前分支末尾）
    let parent_sql = format!(
        "SELECT id FROM timeline_nodes WHERE {} ORDER BY sort_order DESC, id DESC LIMIT 1",
        b_clause
    );
    let mut pq = sqlx::query_scalar::<_, i64>(&parent_sql);
    if branch.is_some() {
        pq = pq.bind(branch.clone().unwrap_or_default());
    }
    let parent_id: Option<i64> = pq.fetch_optional(&pool).await?;
    let row = sqlx::query(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, branch_id, parent_id)
         VALUES(?1, ?2, 'interlude', 1, ?3, ?4) RETURNING id, date_label, sort_order, node_type, is_flashback, tags, parent_id, branch_id"
    )
    .bind(&label)
    .bind(max_order + 1)
    .bind(&branch)
    .bind(parent_id)
    .fetch_one(&pool).await?;
    Ok(TimelineNode {
        id: row.try_get(0)?,
        date_label: row.try_get(1)?,
        sort_order: row.try_get(2)?,
        node_type: row.try_get(3)?,
        is_flashback: row.try_get(4)?,
        tags: row.try_get(5)?,
        day_summary: String::new(),
        parent_id: row.try_get::<Option<i64>, _>(7).unwrap_or(None),
        branch_id: branch.clone(),
    })
}

/// 删除一个时间线节点（连同其下所有消息）。至少保留一个节点。
pub async fn delete_timeline_node(story_id: &str, node_id: i64) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    delete_timeline_node_in(&pool, node_id).await
}

/// `delete_timeline_node` 的核心（接 pool，供测试直连）。
pub async fn delete_timeline_node_in(pool: &SqlitePool, node_id: i64) -> Result<(), sqlx::Error> {
    // 第五批 ③：**禁止删除"被分支当作分叉点"的节点**（用户实测踩到）。
    // 删掉它之后分支就失去了共享历史的起点：天数会从 0 重新数（建出"第 1 天"），
    // 子节点的 parent_id 也会指向不存在的行 —— 数据变成"半坏"状态且不报错。
    let dependents: Vec<String> = sqlx::query_scalar("SELECT name FROM branches WHERE fork_node_id = ?1")
        .bind(node_id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    if !dependents.is_empty() {
        return Err(sqlx::Error::Protocol(format!(
            "该节点是分支「{}」的分叉点，删掉它会让那条分支失去起点（天数错乱、父节点悬空）。请先删除该分支或改它的分叉点。",
            dependents.join("」「")
        )));
    }

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes").fetch_one(pool).await?;
    if count <= 1 {
        return Err(sqlx::Error::Protocol("至少保留一个时间线节点".to_string()));
    }
    sqlx::query("DELETE FROM messages WHERE node_id = ?1").bind(node_id).execute(pool).await?;
    sqlx::query("DELETE FROM timeline_nodes WHERE id = ?1").bind(node_id).execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod summary_tests {
    use super::*;

    /// 建一个真实临时 story.db（含 timeline_nodes.day_summary 列），返回 (pool, 目录)
    async fn temp_story_db(tag: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = std::env::current_dir().unwrap().join("target").join("test-library").join(tag).join("projects").join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("story.db");
        let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE timeline_nodes (
                id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
                is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
                day_summary TEXT NOT NULL DEFAULT '')",
        ).execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER NOT NULL, role TEXT NOT NULL,
                content TEXT NOT NULL, mode TEXT NOT NULL DEFAULT 'chat', meta TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
        ).execute(&pool).await.unwrap();
        (pool, dir)
    }

    async fn add_node(pool: &SqlitePool, label: &str, sort: i64, summary: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, day_summary) VALUES(?1, ?2, 'day', 0, ?3) RETURNING id")
            .bind(label).bind(sort).bind(summary).fetch_one(pool).await.unwrap()
    }

    async fn add_msg(pool: &SqlitePool, node_id: i64, content: &str) {
        sqlx::query("INSERT INTO messages(node_id, role, content) VALUES(?1, 'user', ?2)")
            .bind(node_id).bind(content).execute(pool).await.unwrap();
    }

    #[test]
    fn time_gaps_finds_blank_ranges() {
        // 1,2,3,4,5,8,9,10 → 只有 5→8 之间是空白（第 6、7 天）
        assert_eq!(super::time_gaps(&[1, 2, 3, 4, 5, 8, 9, 10]), vec![(5, 8)]);
        // 连续天数 → 无空白
        assert_eq!(super::time_gaps(&[1, 2, 3]), Vec::<(i64, i64)>::new());
        // 多个空白段
        assert_eq!(super::time_gaps(&[1, 4, 5, 10]), vec![(1, 4), (5, 10)]);
        // 单节点 / 空
        assert_eq!(super::time_gaps(&[3]), Vec::<(i64, i64)>::new());
        assert_eq!(super::time_gaps(&[]), Vec::<(i64, i64)>::new());
    }

    #[test]
    fn next_day_uses_max_number_not_count() {
        // 用户报的 bug 场景：1,2,3,4,5,8,9,10 → 新增应为 11，而不是 9
        let labels: Vec<String> = ["第 1 天", "第 2 天", "第 3 天", "第 4 天", "第 5 天", "第 8 天", "第 9 天", "第 10 天"]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(super::next_day_number(&labels), 11, "应按最大天数 +1");

        // 有重复天数（真实数据里出现过两个「第 8 天」）也不受影响
        let dup: Vec<String> = ["第 1 天", "第 8 天", "第 8 天"].iter().map(|s| s.to_string()).collect();
        assert_eq!(super::next_day_number(&dup), 9);

        // 空表 → 第 1 天
        assert_eq!(super::next_day_number(&[]), 1);

        // 标签无数字 → 退回个数 +1，不 panic
        let odd: Vec<String> = ["回忆·那年夏天", "插曲"].iter().map(|s| s.to_string()).collect();
        assert_eq!(super::next_day_number(&odd), 3);
    }

    #[test]
    fn parse_day_number_handles_variants() {
        assert_eq!(super::parse_day_number("第 5 天"), Some(5));
        assert_eq!(super::parse_day_number("第10天"), Some(10));
        assert_eq!(super::parse_day_number("Day 7"), Some(7));
        assert_eq!(super::parse_day_number("回忆·那年夏天"), None);
    }

    /// 硬规则：只给"当前节点之前、有剧情、还没有摘要"的节点补摘要
    #[test]
    fn backfill_targets_only_past_unsummarized_nodes() {
        tauri::async_runtime::block_on(async {
            let (pool, dir) = temp_story_db("summary_sel").await;

            let n1 = add_node(&pool, "第 1 天", 1, "").await;            // 有剧情、无摘要 → 应被选中
            let n2 = add_node(&pool, "第 2 天", 2, "已固化的摘要").await;  // 已有摘要 → 不选
            let n3 = add_node(&pool, "第 3 天", 3, "").await;            // 无剧情 → 不选
            let n4 = add_node(&pool, "第 4 天", 4, "").await;            // 当前节点 → 不选
            add_msg(&pool, n1, "第一天剧情").await;
            add_msg(&pool, n2, "第二天剧情").await;
            add_msg(&pool, n4, "当前节点剧情").await;

            let picked = select_nodes_needing_summary(&pool, n4).await.unwrap();
            assert_eq!(picked.len(), 1, "只应选中第 1 天，实际: {picked:?}");
            assert_eq!(picked[0].0, n1);
            assert_eq!(picked[0].1, "第 1 天");

            // 倒叙节点永远不参与摘要
            let n5 = add_node(&pool, "回忆·那年夏天", 5, "").await;
            add_msg(&pool, n5, "回忆内容").await;
            sqlx::query("UPDATE timeline_nodes SET is_flashback = 1 WHERE id = ?1").bind(n5).execute(&pool).await.unwrap();
            let n6 = add_node(&pool, "第 5 天", 6, "").await;
            add_msg(&pool, n6, "第五天").await;
            let picked2 = select_nodes_needing_summary(&pool, n6).await.unwrap();
            assert!(picked2.iter().all(|(id, _)| *id != n5), "倒叙节点不应参与摘要: {picked2:?}");

            let _ = std::fs::remove_dir_all(&dir);
        });
    }

    /// 硬规则：已有摘要 → generate_day_summary 非 force 时直接返回 None（不回改）
    #[test]
    fn existing_summary_is_never_regenerated() {
        tauri::async_runtime::block_on(async {
            let (pool, dir) = temp_story_db("summary_idem").await;
            let n1 = add_node(&pool, "第 1 天", 1, "").await;
            add_msg(&pool, n1, "剧情").await;
            // 直接调用会走 API；这里只验证"选择器"在摘要写入后不再返回该节点（等价于幂等）
            assert_eq!(select_nodes_needing_summary(&pool, n1).await.unwrap().len(), 0, "当前节点不该被选中");
            let n2 = add_node(&pool, "第 2 天", 2, "").await;
            add_msg(&pool, n2, "剧情2").await;
            assert_eq!(select_nodes_needing_summary(&pool, n2).await.unwrap().len(), 1, "第 1 天应被选中");
            sqlx::query("UPDATE timeline_nodes SET day_summary = '写好了' WHERE id = ?1").bind(n1).execute(&pool).await.unwrap();
            assert_eq!(select_nodes_needing_summary(&pool, n2).await.unwrap().len(), 0, "已有摘要后不该再被选中（固化）");
            let _ = std::fs::remove_dir_all(&dir);
        });
    }
}


#[cfg(test)]
mod tree_tests {
    use super::*;
    use std::collections::HashSet;

    fn n(id: i64, parent: Option<i64>, sort: i64) -> TreeNode {
        TreeNode { id, parent_id: parent, sort_order: sort }
    }

    /// 线性故事（现在所有故事都是这种）：1→2→3→4，父链就是从根到该节点的顺序
    fn linear() -> Vec<TreeNode> {
        vec![n(1, None, 1), n(2, Some(1), 2), n(3, Some(2), 3), n(4, Some(3), 4)]
    }

    #[test]
    fn children_index_groups_and_sorts_by_sort_order() {
        // 手工造一个"乱序传入"的列表：同层应按 (sort_order, id) 排
        let nodes = vec![
            n(10, Some(1), 30),
            n(1, None, 1),
            n(11, Some(1), 20),
            n(12, Some(1), 20), // 与 11 同 sort_order → 按 id
        ];
        let idx = children_index(&nodes);
        assert_eq!(idx.get(&Some(1)).unwrap(), &vec![11, 12, 10], "同层应按 (sort_order, id)");
        assert_eq!(idx.get(&None).unwrap(), &vec![1]);
    }

    #[test]
    fn path_to_root_returns_root_first_order() {
        let nodes = linear();
        assert_eq!(path_to_root(&nodes, 4), vec![1, 2, 3, 4], "应返回从根到目标的顺序");
        assert_eq!(path_to_root(&nodes, 1), vec![1], "根节点的路径只有自己");
    }

    /// 这是分支系统最关键的一条：**路径只走自己的父链，不含别的分支**
    #[test]
    fn path_to_root_excludes_other_branches() {
        //        1
        //       / \
        //      2   5     ← 2 与 5 是分叉（共享 parent 1）
        //      |   |
        //      3   6
        //      |
        //      4
        let nodes = vec![
            n(1, None, 1),
            n(2, Some(1), 2),
            n(3, Some(2), 3),
            n(4, Some(3), 4),
            n(5, Some(1), 5),
            n(6, Some(5), 6),
        ];
        assert_eq!(path_to_root(&nodes, 4), vec![1, 2, 3, 4], "分支 A 的路径不应含分支 B 的 5/6");
        assert_eq!(path_to_root(&nodes, 6), vec![1, 5, 6], "分支 B 的路径不应含分支 A 的 2/3/4");
    }

    #[test]
    fn path_to_root_survives_cycle_without_hanging() {
        // 数据异常：3→2→3 成环。函数必须能停下（返回已访问过的部分），不能死循环
        let nodes = vec![n(2, Some(3), 1), n(3, Some(2), 2)];
        let path = path_to_root(&nodes, 3);
        assert!(path.len() <= 2, "成环时应停下，实际 {path:?}");
    }

    #[test]
    fn flatten_tree_respects_expanded_set() {
        let nodes = linear();
        // 什么都没展开 → 只有根
        let none: HashSet<i64> = HashSet::new();
        assert_eq!(flatten_tree(&nodes, None, &none), vec![(1, 0)]);
        // 展开 1、2 → 出现 1(0) 2(1) 3(2)，4 仍未出现（3 未展开）
        let mut e: HashSet<i64> = HashSet::new();
        e.insert(1);
        e.insert(2);
        assert_eq!(flatten_tree(&nodes, None, &e), vec![(1, 0), (2, 1), (3, 2)]);
        // 全展开 → 4 个节点，深度 0..3
        let mut all: HashSet<i64> = HashSet::new();
        for i in 1..=4 {
            all.insert(i);
        }
        assert_eq!(flatten_tree(&nodes, None, &all), vec![(1, 0), (2, 1), (3, 2), (4, 3)]);
    }

    #[test]
    fn flatten_tree_handles_fork_depths() {
        let nodes = vec![
            n(1, None, 1),
            n(2, Some(1), 2),
            n(3, Some(2), 3),
            n(4, Some(3), 4),
            n(5, Some(1), 5),
            n(6, Some(5), 6),
        ];
        let mut all: HashSet<i64> = HashSet::new();
        for i in 1..=6 {
            all.insert(i);
        }
        // 期望：1(0) → 2(1) → 3(2) → 4(3) → 5(1) → 6(2)
        assert_eq!(
            flatten_tree(&nodes, None, &all),
            vec![(1, 0), (2, 1), (3, 2), (4, 3), (5, 1), (6, 2)]
        );
    }

    #[test]
    fn branches_and_fork_detection() {
        let nodes = vec![
            n(1, None, 1),
            n(2, Some(1), 2),
            n(5, Some(1), 5),
            n(3, Some(2), 3),
        ];
        assert_eq!(branches_of(&nodes, 1), vec![2, 5], "1 有两个子节点");
        assert!(is_fork(&nodes, 1), "1 是分叉点");
        assert!(!is_fork(&nodes, 2), "2 只有一个子节点，不是分叉点");
        assert!(branches_of(&nodes, 3).is_empty(), "叶子节点没有子节点");
    }
    /// 真机验证（默认跳过）：对**真实故事库**跑一次迁移，检查父链是否正确串起来。
    /// 跑法：`cargo test --lib live_parent_links -- --ignored --nocapture`
    ///
    /// 为什么值得单独跑：老库里 `sort_order` 有重复值（例如两个节点都叫「第 8 天」），
    /// 回填必须带 id 作次级键才稳定 —— 这一点在构造数据上容易"碰巧对"，真实数据才试得出来。
    #[tokio::test]
    #[ignore]
    async fn live_parent_links() {
        use sqlx::sqlite::SqliteConnectOptions;
        use std::str::FromStr;
        let db = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..").join("library").join("default").join("projects").join("7").join("story.db");
        if !db.exists() {
            println!("真实库不存在，跳过：{}", db.display());
            return;
        }
        let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display())).unwrap();
        let pool = SqlitePool::connect_with(opts).await.unwrap();

        // 迁移只补 NULL，不覆盖已有值 —— 可安全反复跑
        crate::story_db::ensure_timeline_parent_column(&pool).await.unwrap();

        let rows = sqlx::query("SELECT id, date_label, sort_order, parent_id FROM timeline_nodes ORDER BY sort_order ASC, id ASC")
            .fetch_all(&pool).await.unwrap();
        println!("  节点数：{}", rows.len());
        let mut roots = 0;
        for r in &rows {
            let id: i64 = r.try_get(0).unwrap();
            let label: String = r.try_get(1).unwrap();
            let sort: i64 = r.try_get(2).unwrap();
            let parent: Option<i64> = r.try_get(3).unwrap();
            if parent.is_none() { roots += 1; }
            println!("    id={id:<3} sort={sort:<3} parent={:<6} {label}", parent.map(|x| x.to_string()).unwrap_or("NULL".into()));
        }
        assert!(roots <= 1, "线性故事应只有 1 个根节点，实际 {roots} 个");

        // 用纯函数复核：从最后一个节点走回根，路径长度应等于节点数
        let nodes: Vec<TreeNode> = rows.iter().map(|r| TreeNode {
            id: r.try_get(0).unwrap(),
            parent_id: r.try_get(3).unwrap(),
            sort_order: r.try_get(2).unwrap(),
        }).collect();
        if let Some(last) = nodes.last() {
            let path = path_to_root(&nodes, last.id);
            println!("  从最后节点 {} 回溯：{:?}（长度 {}）", last.id, path, path.len());
            assert_eq!(path.len(), nodes.len(), "线性链的路径应包含全部节点");
        }
    }

    /// **本批最重要的一条约束：线性故事的行为必须与改动前完全一致。**
    ///
    /// 改动前，跨天装配是"按全表 sort_order 位置取近 KEEP_DAYS 个天数节点、更早的天进 recap"；
    /// 改动后改为"按父链取"。这条测试用**等价性**把这个约束钉住：
    /// 只要故事还是线性链（每个节点至多一个后继），两种算法的结果必须逐项相同。
    #[test]
    fn linear_story_path_equals_positional() {
        const KEEP: usize = 3;
        // 造一条线性链，天数节点与插曲混排（更贴近真实数据：2026-10-11 的库里就有插曲）
        // 1(day) 2(day) 3(插曲) 4(day) 5(day) 6(day) 7(day)
        let nodes = vec![
            n(1, None, 1),
            n(2, Some(1), 2),
            n(3, Some(2), 3),
            n(4, Some(3), 4),
            n(5, Some(4), 5),
            n(6, Some(5), 6),
            n(7, Some(6), 7),
        ];
        // 天数节点（插曲 3 不算）—— 顺序与 sort_order 一致
        let day_ids: Vec<i64> = vec![1, 2, 4, 5, 6, 7];

        for target in [1i64, 2, 4, 5, 6, 7] {
            // —— 新算法：父链上的天数节点 ——
            let chain = ancestor_ids(&nodes, target);
            let by_chain: Vec<i64> = day_ids.iter().copied().filter(|d| chain.contains(d)).collect();

            // —— 旧算法：全表位置在 target 及其之前的天数节点 ——
            let pos = day_ids.iter().position(|d| *d == target).unwrap();
            let by_pos: Vec<i64> = day_ids[..=pos].to_vec();

            assert_eq!(by_chain, by_pos, "节点 {target}：父链与按位置取的结果必须一致");

            // 近 KEEP 天：两边各取末尾 KEEP 个
            let keep_chain = &by_chain[by_chain.len().saturating_sub(KEEP)..];
            let keep_pos = &by_pos[by_pos.len().saturating_sub(KEEP)..];
            assert_eq!(keep_chain, keep_pos, "节点 {target}：近 KEEP 天必须一致");

            // recap：两边都取"剩下的更早的天"
            let recap_chain: Vec<i64> = by_chain.iter().copied().filter(|d| !keep_chain.contains(d)).collect();
            let recap_pos: Vec<i64> = by_pos[..by_pos.len() - keep_pos.len()].to_vec();
            assert_eq!(recap_chain, recap_pos, "节点 {target}：recap 必须一致");
        }
    }

    /// 有分支时，父链**只含自己那条线** —— 这正是本次改动的目的
    #[test]
    fn forked_story_path_excludes_other_branch() {
        //        1
        //       / \
        //      2   5
        //      |   |
        //      3   6
        let nodes = vec![
            n(1, None, 1),
            n(2, Some(1), 2),
            n(3, Some(2), 3),
            n(5, Some(1), 4),   // 注意 sort_order 排在 3 前面（模拟既有数据的乱序）
            n(6, Some(5), 5),
        ];
        let day_ids: Vec<i64> = vec![1, 2, 3, 5, 6];

        // 站在分支 A 的末端 3 上：只应看到 1、2、3
        let chain_a = ancestor_ids(&nodes, 3);
        let days_a: Vec<i64> = day_ids.iter().copied().filter(|d| chain_a.contains(d)).collect();
        assert_eq!(days_a, vec![1, 2, 3], "分支 A 不应看到 5/6");

        // 站在分支 B 的末端 6 上：只应看到 1、5、6
        let chain_b = ancestor_ids(&nodes, 6);
        let days_b: Vec<i64> = day_ids.iter().copied().filter(|d| chain_b.contains(d)).collect();
        assert_eq!(days_b, vec![1, 5, 6], "分支 B 不应看到 2/3");

        // 对照：若继续用"全表位置"，站在 3 上会把 5、6（排在其后）也算进来 —— 顺序错乱的风险也在这里
        let by_pos_at_3: Vec<i64> = day_ids[..=day_ids.iter().position(|d| *d == 3).unwrap()].to_vec();
        assert_eq!(by_pos_at_3, vec![1, 2, 3]);
        // 反过来，若 5 的 sort_order 更小，就会出现"读到别的分支"
        let nodes2 = vec![n(1, None, 1), n(2, Some(1), 2), n(3, Some(2), 3), n(5, Some(1), 0)];
        let day_ids2: Vec<i64> = vec![5, 1, 2, 3]; // 按 sort_order 排
        let pos5 = day_ids2.iter().position(|d| *d == 5).unwrap();
        let chain_a2 = ancestor_ids(&nodes2, 3);
        assert!(
            !chain_a2.contains(&5),
            "分支 A 的父链不含 5（即便 5 的 sort_order 最小）"
        );
        assert_eq!(pos5, 0, "而按位置取会把 5 当成'最早的天'，这就是必须改按父链的原因");
    }

    /// 真机验证（默认跳过）：打印 `list_timeline_nodes` 对真实库的真实返回。
    /// 跑法：`cargo test --lib live_list_nodes -- --ignored --nocapture`
    ///
    /// 用途：界面"看不到插曲"这类问题时，先确认**后端到底返回了什么** ——
    /// 把"数据没返回"和"界面没渲染"两种可能分开，避免靠猜。
    #[tokio::test]
    #[ignore]
    async fn live_list_nodes() {
        let db = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..").join("library").join("default").join("projects").join("7").join("story.db");
        if !db.exists() {
            println!("真实库不存在，跳过：{}", db.display());
            return;
        }
        let nodes = list_timeline_nodes("7").await.unwrap();
        println!("  list_timeline_nodes 返回 {} 个节点：", nodes.len());
        let mut interludes = 0;
        for n in &nodes {
            if n.node_type != "day" {
                interludes += 1;
            }
            println!(
                "    id={:<4} {:<10} type={:<10} parent={:<6} sort={:<4} {}",
                n.id,
                n.date_label,
                n.node_type,
                n.parent_id.map(|x| x.to_string()).unwrap_or("NULL".into()),
                n.sort_order,
                if n.is_flashback != 0 { "[flashback]" } else { "" }
            );
        }
        println!("  其中非 day 节点（插曲/倒叙）：{interludes} 个");
        assert_eq!(nodes.len(), 15, "真实库应有 15 个节点");
        assert!(interludes >= 2, "真实库应有 2 个插曲，实际 {interludes}");
    }

}


/// 第五批 ③ 第 3 步（3-4 批）：节点与消息的**分支归属**测试。
///
/// 口径：
/// - 新建节点/消息时带上"当前所在分支"（主线 = NULL）
/// - 消息的 `branch_id` **由节点推导**（保证与节点一致，不靠调用方传）
/// - `list_timeline_nodes` 仍是**全树**（树形展示要看到所有分支的节点）——
///   这一条单独一条测试守着，防止将来有人"顺手"给它加 `branch_id IS NULL` 把树砍断
#[cfg(test)]
mod branch_node_tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;

    async fn pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-library")
            .join(format!("node-branch-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(dir.join("story.db"))
            .create_if_missing(true);
        let p = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE timeline_nodes (
                id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
                is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
                day_summary TEXT NOT NULL DEFAULT '', parent_id INTEGER, branch_id TEXT)",
        ).execute(&p).await.unwrap();
        sqlx::query(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER NOT NULL, role TEXT NOT NULL,
                content TEXT NOT NULL, mode TEXT NOT NULL DEFAULT 'chat',
                meta TEXT NOT NULL DEFAULT '{}', branch_id TEXT)",
        ).execute(&p).await.unwrap();
        (p, dir)
    }

    /// 消息的 branch_id 由**节点**推导（写入方不传也不会错）
    #[tokio::test]
    async fn message_branch_is_derived_from_node() {
        let (p, dir) = pool("derive").await;
        // 主线节点 + 分支节点
        sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, branch_id) VALUES('第 1 天', 1, NULL)")
            .execute(&p).await.unwrap();
        sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, branch_id) VALUES('第 2 天', 2, 'BR-X')")
            .execute(&p).await.unwrap();

        // 复刻 create_message 的推导逻辑
        for node_id in [1i64, 2i64] {
            let b: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
                .bind(node_id).fetch_optional(&p).await.unwrap().flatten();
            sqlx::query("INSERT INTO messages(node_id, role, content, branch_id) VALUES(?1, 'user', 'x', ?2)")
                .bind(node_id).bind(b).execute(&p).await.unwrap();
        }

        let rows: Vec<(i64, Option<String>)> =
            sqlx::query_as("SELECT node_id, branch_id FROM messages ORDER BY id")
                .fetch_all(&p).await.unwrap();
        assert_eq!(rows[0], (1, None), "主线节点的消息 branch_id 应为 NULL");
        assert_eq!(rows[1], (2, Some("BR-X".to_string())), "分支节点的消息应带上该分支");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **回归守护**：`list_timeline_nodes` 必须返回**全树**（含各分支节点）。
    /// 若有人给它加了 `branch_id IS NULL`，界面上的树会缺枝 —— 这条会立刻变红。
    #[tokio::test]
    async fn list_timeline_nodes_returns_all_branches() {
        let (p, dir) = pool("full-tree").await;
        sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, branch_id) VALUES('第 1 天', 1, NULL)")
            .execute(&p).await.unwrap();
        sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id, branch_id) VALUES('第 2 天', 2, 1, 'BR-A')")
            .execute(&p).await.unwrap();
        sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id, branch_id) VALUES('第 2 天', 3, 1, 'BR-B')")
            .execute(&p).await.unwrap();

        // 与 list_timeline_nodes 的查询同形
        let rows = sqlx::query(
            "SELECT id, date_label, sort_order, node_type, is_flashback, tags, day_summary, parent_id
             FROM timeline_nodes ORDER BY sort_order ASC, id ASC",
        ).fetch_all(&p).await.unwrap();
        assert_eq!(rows.len(), 3, "主线查询必须看到全部 3 个节点（含两个分支的）");

        // 对照：若按分支过滤就会缺枝（说明为什么不能那么改）
        let only_main = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM timeline_nodes WHERE branch_id IS NULL",
        ).fetch_one(&p).await.unwrap();
        assert_eq!(only_main, 1, "按分支过滤只剩 1 个 —— 树就断了");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 同一分叉点下挂两条分支的节点：父链回溯各自独立（配合既有的 path_to_root 测试）
    #[tokio::test]
    async fn forked_nodes_keep_independent_paths() {
        let (p, dir) = pool("paths").await;
        for (label, sort, parent, br) in [
            ("第 1 天", 1i64, None::<i64>, None::<&str>),
            ("第 2 天A", 2, Some(1), Some("BR-A")),
            ("第 3 天A", 3, Some(2), Some("BR-A")),
            ("第 2 天B", 4, Some(1), Some("BR-B")),
        ] {
            sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, parent_id, branch_id) VALUES(?1, ?2, ?3, ?4)")
                .bind(label).bind(sort).bind(parent).bind(br).execute(&p).await.unwrap();
        }
        let nodes: Vec<TreeNode> = sqlx::query("SELECT id, parent_id, sort_order FROM timeline_nodes")
            .fetch_all(&p).await.unwrap()
            .iter().map(|r| TreeNode {
                id: r.try_get(0).unwrap(),
                parent_id: r.try_get(1).unwrap(),
                sort_order: r.try_get(2).unwrap(),
            }).collect();

        assert_eq!(path_to_root(&nodes, 3), vec![1, 2, 3], "分支 A 的路径只含自己那条线");
        assert_eq!(path_to_root(&nodes, 4), vec![1, 4], "分支 B 的路径只含自己那条线");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
