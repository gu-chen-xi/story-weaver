// 从 `timeline.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `timeline.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use summary::*;` 取回，
// 外部 `crate::timeline::xxx` 的调用路径**全部不变**。

use super::*;

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
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;

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

    let node_label: String = sqlx::query_scalar("SELECT date_label FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| format!("节点 {node_id}"));

    let user_content = format!("【{node_label}】的剧情记录：\n{}", transcript.chars().take(24000).collect::<String>());
    let summary = call_summary_model(lib_pool, DAY_SUMMARY_PROMPT, &user_content).await?;
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
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // RAG-Q7：只补**当前分支可见范围**内的节点（原来会挑到兄弟分支的节点）
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let rows = select_nodes_needing_summary(&pool, current_node_id, &branch)
        .await
        .map_err(|e| e.to_string())?;

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
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
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
        Ok(Some(_s)) => Ok(Some(format!("{label}（{replies} 条回复）"))),
        Ok(None) => Ok(None),
        Err(e) => Err(e),
    }
}

/// 阶段摘要（C 层）：**攒够多少天"已有日摘要"的天**才滚成一段。默认 10，可在界面调整。
///
/// 为什么默认 10：日摘要按"天"积累，天数一多（几十天）光日摘要就吃满预算；
/// 10 天 ≈ 2000 字压成 300 字，压缩比足够，又不会太频繁地调模型。
pub const PHASE_MIN_DAYS: usize = 10;

/// 配置键：攒够多少天才压缩（界面里的 x）
pub const CFG_PHASE_MIN_DAYS: &str = "summary.phase_min_days";

/// 读配置里的 x（读不到/非法 → 默认 10）
pub async fn phase_min_days_cfg(lib_pool: &SqlitePool) -> usize {
    crate::db::get_config(lib_pool, CFG_PHASE_MIN_DAYS)
        .await.ok().flatten()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| (2..=200).contains(n))
        .unwrap_or(PHASE_MIN_DAYS)
}

/// 阶段摘要提示词（比日摘要更粗：只保留"到这一段结束时仍然成立的"事实）
pub const PHASE_SUMMARY_PROMPT: &str = "你在为一部连续剧整理\"总纲\"。下面是连续的若干天，每天一段摘要。\n\
请把它们合并成**一段**阶段摘要，供很久以后再回读时使用。硬性要求：\n\
- 只写**到这一段结束时仍然成立**的事实：人物关系与立场、目标与处境、重要约定与未回收伏笔、关键转折；\n\
- 丢掉过程性细节（当天去了哪、说了什么、临时情绪），不要逐天复述；\n\
- 不写感想、不写评价、不写台词原文；\n\
- 第三人称、过去时，人名用正式名；\n\
- 300 字以内，一段话，不要分段、不要小标题、不要 JSON、不要任何解释。";

/// 计划一次阶段摘要：找**最老的一段**「已有日摘要、且还没被阶段摘要覆盖」的连续天数。
///
/// 返回 `(day_from, day_to, [(label, summary)])`；不足 `PHASE_MIN_DAYS` 时返回 `None`
/// （说明还没积累够，等日摘要再多几天）。
///
/// 只统计**当前分支**这条线：阶段摘要是"整条线"的汇总，跨分支混在一起没有意义。
pub async fn plan_phase_summary_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    x: usize,
    ignore_covered: bool,
) -> Result<Option<(i64, i64, Vec<(String, String)>)>, String> {
    let path = branch_path_ids(pool, branch).await;
    let rows = sqlx::query(
        "SELECT id, date_label, COALESCE(day_summary, '') FROM timeline_nodes
         WHERE (node_type = 'day' OR node_type = 'main') AND is_flashback = 0
         ORDER BY sort_order ASC, id ASC",
    ).fetch_all(pool).await.map_err(|e| e.to_string())?;

    let covered = if ignore_covered {
        Vec::new()
    } else {
        list_phase_summaries_in(pool, branch).await?
    };

    // 候选 = **有日摘要、且没被已有阶段覆盖**的天，按时间线从老到新
    let mut candidates: Vec<(i64, String, String)> = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get(0).unwrap_or_default();
        if !path.is_empty() && !path.contains(&id) {
            continue;
        }
        let label: String = r.try_get(1).unwrap_or_default();
        let summary: String = r.try_get(2).unwrap_or_default();
        if summary.trim().is_empty() {
            continue; // 没摘要的天不参与（先补齐日摘要）
        }
        let Some(day_no) = parse_day_number(&label) else { continue };
        if covered.iter().any(|(f, to, _, _)| day_no >= *f && day_no <= *to) {
            continue; // 已被某段阶段摘要覆盖
        }
        candidates.push((day_no, label, summary));
    }

    // **x 的语义（用户 2026-10-17 定，示例见 ROADMAP §10.78）**：
    // `x` = **一段阶段摘要覆盖多少天**（按"有日摘要的天"计数，跨过"时间流逝"的空档）。
    // 例：日摘要在 1~5 与 8~12（共 10 天）时 ——
    //   x=5  → 压 1~5（第 1~5 天）
    //   x=7  → 压 1~5 + 8~9（覆盖 1~9）
    //   x=10 → 压 1~5 + 8~12（覆盖 1~12）
    // 压完之后这些天就算"已覆盖"，下次自然从**断点之后**接着算（x=5 之后从第 8 天起）。
    if candidates.len() < x.max(2) {
        return Ok(None);
    }
    let take = &candidates[..x.max(2)];
    let day_from = take[0].0;
    let day_to = take[take.len() - 1].0;
    let material: Vec<(String, String)> =
        take.iter().map(|(_, l, s)| (l.clone(), s.clone())).collect();
    Ok(Some((day_from, day_to, material)))
}

/// 清理重叠的阶段摘要：同一批天被多条覆盖时，**保留 id 最大的那条**（最新规则压的），
/// 其余删掉。返回删了几条。
///
/// 为什么需要：手动重压只在"生成成功后"删重叠；历史上多次失败/旧版本留下的多条会一直并存，
/// 而【前情提要】只能挑一条来用 —— 多条并存会让结果取决于排序（用户实测踩到）。
pub async fn dedupe_phase_rows(story_id: &str) -> Result<usize, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let rows = list_phase_rows(story_id).await?;
    let mut doomed: Vec<i64> = Vec::new();
    for (i, a) in rows.iter().enumerate() {
        for b in rows.iter().skip(i + 1) {
            let overlap = a.day_from <= b.day_to && b.day_from <= a.day_to;
            if overlap {
                // 旧的那条（id 小）会被删；两条都保留会让结果不确定
                doomed.push(if a.id < b.id { a.id } else { b.id });
            }
        }
    }
    doomed.sort_unstable();
    doomed.dedup();
    let mut deleted = 0usize;
    for id in doomed {
        // 如果这条已经被删过（同一 id 在多对重叠里出现），rowcount=0，不影响计数
        let n = sqlx::query("DELETE FROM story_summaries WHERE kind = 'phase' AND id = ?1")
            .bind(id).execute(&pool).await.map_err(|e| e.to_string())?.rows_affected();
        deleted += n as usize;
    }
    let _ = branch;
    Ok(deleted)
}

/// 阶段摘要的管理视图（导演监视器里的「摘要管理」用）：带 id / 字数 / 创建时间。
#[derive(serde::Serialize)]
pub struct PhaseRow {
    pub id: i64,
    pub day_from: i64,
    pub day_to: i64,
    pub source_days: i64,
    pub chars: i64,
    pub created_at: String,
}

pub async fn list_phase_rows(story_id: &str) -> Result<Vec<PhaseRow>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let sql = format!(
        "SELECT id, day_from, day_to, source_days, LENGTH(content), created_at
         FROM story_summaries WHERE kind = 'phase' AND {} ORDER BY day_from ASC, id ASC",
        clause
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = &branch {
        q = q.bind(b);
    }
    let rows = q.fetch_all(&pool).await.map_err(|e| e.to_string())?;
    Ok(rows.iter().map(|r| PhaseRow {
        id: r.try_get(0).unwrap_or(0),
        day_from: r.try_get(1).unwrap_or(0),
        day_to: r.try_get(2).unwrap_or(0),
        source_days: r.try_get(3).unwrap_or(0),
        chars: r.try_get(4).unwrap_or(0),
        created_at: r.try_get(5).unwrap_or_default(),
    }).collect())
}

/// 删掉一条阶段摘要（管理界面用）。返回是否真的删了。
pub async fn delete_phase_row(story_id: &str, id: i64) -> Result<bool, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let n = sqlx::query("DELETE FROM story_summaries WHERE kind = 'phase' AND id = ?1")
        .bind(id).execute(&pool).await.map_err(|e| e.to_string())?.rows_affected();
    Ok(n > 0)
}

/// 手动压缩的返回：给界面回显"压了哪几天、摘要多长"。
#[derive(serde::Serialize)]
pub struct PhaseMade {
    pub day_from: i64,
    pub day_to: i64,
    pub source_days: usize,
    pub content: String,
}

/// 手动压缩：用指定 x（或配置值）**重压**一段，返回覆盖区间与正文；攒不够时返回 `None`。
///
/// 手动模式**忽略已有阶段摘要**：否则旧的一段（例如覆盖 1~12 的）会把可压的天全吃掉，
/// 界面永远提示"攒不够"（用户 2026-10-17 实测踩到）。生成成功后删掉与之重叠的旧阶段。
pub async fn run_phase_summary_report(
    lib_pool: &SqlitePool,
    story_id: &str,
    min_days: Option<usize>,
) -> Result<Option<PhaseMade>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let x = match min_days {
        Some(n) => n.max(2),
        None => phase_min_days_cfg(lib_pool).await,
    };
    let Some((day_from, day_to, days)) =
        plan_phase_summary_in(&pool, &branch, x, false).await? else {
        return Ok(None);
    };
    let mut material = String::new();
    for (label, summary) in &days {
        material.push_str(&format!("【{label}】{summary}\n"));
    }
    let user_content = format!("以下是连续的 {} 天，每天一段摘要：\n{}", days.len(), material);
    let content = call_summary_model(lib_pool, PHASE_SUMMARY_PROMPT, &user_content).await?;
    let content: String = content.chars().take(900).collect();
    let new_id = save_phase_summary_in(&pool, &branch, day_from, day_to, &content, days.len() as i64).await?;
    // 删掉与新区间重叠的旧阶段（先写后删：生成失败时旧数据不丢）
    let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let del_sql = format!(
        "DELETE FROM story_summaries WHERE kind = 'phase' AND id <> ?1 AND day_from <= ?2 AND day_to >= ?3 AND {}",
        clause
    );
    let mut dq = sqlx::query(&del_sql).bind(new_id).bind(day_to).bind(day_from);
    if let Some(b) = &branch {
        dq = dq.bind(b);
    }
    let _ = dq.execute(&pool).await;

    Ok(Some(PhaseMade { day_from, day_to, source_days: days.len(), content }))
}

/// 自动生成一段阶段摘要（幂等：攒够 `PHASE_MIN_DAYS` 天才生成一段；生成后这些天被覆盖，不再重复）。
/// 返回 `Some(摘要)` = 本次新生成，`None` = 还不够 / 不需要。
pub async fn generate_phase_summary(
    lib_pool: &SqlitePool,
    story_id: &str,
) -> Result<Option<String>, String> {
    generate_phase_summary_with(lib_pool, story_id, None).await
}

/// `min_days`：本次用多少天（`None` = 用配置里的 x）。手动触发时传界面上的值。
pub async fn generate_phase_summary_with(
    lib_pool: &SqlitePool,
    story_id: &str,
    min_days: Option<usize>,
) -> Result<Option<String>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);

    let x = match min_days {
        Some(n) => n.max(2),
        None => phase_min_days_cfg(lib_pool).await,
    };
    let Some((day_from, day_to, days)) =
        plan_phase_summary_in(&pool, &branch, x, true).await? else {
        return Ok(None);
    };

    let mut material = String::new();
    for (label, summary) in &days {
        material.push_str(&format!("【{label}】{summary}\n"));
    }
    let user_content = format!("以下是连续的 {} 天，每天一段摘要：\n{}", days.len(), material);
    let content = call_summary_model(lib_pool, PHASE_SUMMARY_PROMPT, &user_content).await?;
    let content: String = content.chars().take(900).collect();
    save_phase_summary_in(&pool, &branch, day_from, day_to, &content, days.len() as i64).await?;
    Ok(Some(content))
}

/// 阶段摘要（C 层接口预留）：写入一段覆盖 [day_from, day_to] 的摘要。
/// 本期不自动生成 —— 只提供读写，等日摘要积累后再接"若干天滚一段"。
pub async fn save_phase_summary(
    story_id: &str, day_from: i64, day_to: i64, content: &str, source_days: i64,
) -> Result<i64, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
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
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
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
