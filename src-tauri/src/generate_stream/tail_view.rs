//! 尾部块里**依赖视角**的那几块的**唯一**实现（2026-11-14 引入，2026-11-19 去重）。
//!
//! ## 为什么有这个文件
//!
//! 双池对比要预览"关键记忆按角色"发生在尾部的差异。2026-11-14 第一版是**另抄一份**
//! 选择逻辑（预览一份、真实装配一份）—— 抄错就是"预览骗人"，ROADMAP §2.1 #23 记的 3 处差异
//! （角色属性不看 `focus_role`、示例对话只传 `ai_role` 漏了三级回落、台词口径取人方式略偏）
//! 都出在这。
//!
//! 2026-11-19 合并：**这里就是唯一实现**，`generate_reply_streaming` 与 `preview_prompt_pair`
//! 都调下面这三个函数，参数用 [`TailCtx`] 一次给全
//! （`branch` / `scope` / `ai_role` / `focus_role` / `panel_focus`）——
//! 少传一个就会静默改变真实提示词，所以宁可用结构体强制列出来。
//!
//! 预览侧拿不到 `@焦点`（那是用户这一句里的临时标记）与面板焦点时传空串，
//! 规则同源，剩下的只是"输入不同"。
//!
//! 只读、不落库、不调模型。

use sqlx::{Row, SqlitePool};

use super::PromptScope;

/// 三块共用的上下文：少传一个就会静默改变真实提示词（这是 #23 的教训）。
pub(crate) struct TailCtx<'a> {
    /// 当前分支（None = 主线）
    pub branch: &'a Option<String>,
    /// 视角（`scope.is_role()` = 角色视角）
    pub scope: PromptScope<'a>,
    /// 本轮 AI 扮演的角色（台词口径 / 示例对话第②优先级）
    pub ai_role: &'a str,
    /// 用户这一句 `@` 到的角色（示例对话第①优先级）
    pub focus_role: &'a str,
    /// 导演监视器「当前角色卡」上的焦点（示例对话第③优先级）
    pub panel_focus: &'a str,
}

/// ① 关系强度：一行一个方向（角色视角只留 `from=她`）。
pub(crate) async fn rel_strength_lines(
    pool: &SqlitePool,
    ctx: &TailCtx<'_>,
) -> Result<Vec<String>, String> {
    let (r_clause, f_clause, t_clause) = (
        crate::story_db::branch_clause("r.branch_id", ctx.branch, 1),
        crate::story_db::branch_clause("f.branch_id", ctx.branch, 2),
        crate::story_db::branch_clause("t.branch_id", ctx.branch, 3),
    );
    let rel_sql = format!(
        "SELECT f.name, t.name, r.strength, r.state FROM relations r
         JOIN characters f ON f.id = r.from_character_id AND {}
         JOIN characters t ON t.id = r.to_character_id AND {}
         WHERE {} ORDER BY r.id",
        f_clause, t_clause, r_clause
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(rel_sql.as_str()));
    if ctx.branch.is_some() {
        q = q.bind(ctx.branch).bind(ctx.branch).bind(ctx.branch);
    }
    let rows = q.fetch_all(pool).await.map_err(|e| e.to_string())?;
    let mut lines: Vec<String> = Vec::new();
    for r in &rows {
        let from: String = r.try_get(0).unwrap_or_default();
        let to: String = r.try_get(1).unwrap_or_default();
        // 2026-11-10 角色视角：只列"她对别人"的强度 —— 别人对她的那条是别人的隐私（漏风点 ①）
        if ctx.scope.is_role() && from != ctx.scope.role.trim() {
            continue;
        }
        let strength: i64 = r.try_get(2).unwrap_or(50);
        let state: String = r.try_get(3).unwrap_or_default();
        lines.push(crate::relations::rel_value_line(&from, &to, strength, &state));
    }
    Ok(lines)
}

/// ② 角色属性：**焦点优先**（`focus_role`），其余最多 8 个并注明略去多少；角色视角只给她自己。
pub(crate) async fn char_stat_lines(
    pool: &SqlitePool,
    ctx: &TailCtx<'_>,
) -> Result<Vec<String>, String> {
    let c_clause = crate::story_db::branch_clause("branch_id", ctx.branch, 1);
    let c_sql = format!(
        // 情绪不再取数值（v3.4）：改由 current_status 的文字表达
        "SELECT name, stat_stamina, stat_sanity, stat_screen, COALESCE(current_status,'')
         FROM characters WHERE {} ORDER BY id",
        c_clause
    );
    let mut cq = sqlx::query(sqlx::AssertSqlSafe(c_sql.as_str()));
    if ctx.branch.is_some() {
        cq = cq.bind(ctx.branch);
    }
    let rows = cq.fetch_all(pool).await.map_err(|e| e.to_string())?;
    let all: Vec<(String, i64, i64, i64, String)> = rows
        .iter()
        .filter_map(|r| {
            Some((
                r.try_get::<String, _>(0).ok()?,
                r.try_get::<i64, _>(1).unwrap_or(50),
                r.try_get::<i64, _>(2).unwrap_or(50),
                r.try_get::<i64, _>(3).unwrap_or(50),
                r.try_get::<String, _>(4).unwrap_or_default(),
            ))
        })
        .collect();

    // 有焦点就只给焦点一人；没指定焦点（自由模式）才列全部，但最多 8 个。
    let focused: Vec<(String, i64, i64, i64, String)> =
        if !ctx.focus_role.is_empty() && ctx.focus_role != "自由" {
            all.iter().filter(|c| c.0 == ctx.focus_role).cloned().collect()
        } else {
            Vec::new()
        };
    let no_focus = focused.is_empty();
    let picked = if no_focus {
        all.iter().take(8).cloned().collect::<Vec<_>>()
    } else {
        focused
    };
    // 2026-11-10 角色视角：属性只给她自己（漏风点 ② —— 不能让她看到别人的体力/理智）
    let picked: Vec<(String, i64, i64, i64, String)> = if ctx.scope.is_role() {
        all.iter()
            .filter(|c| c.0 == ctx.scope.role.trim())
            .cloned()
            .collect()
    } else {
        picked
    };
    let mut v: Vec<String> = picked
        .iter()
        .map(|(n, a, b, c, d)| crate::characters::stat_value_line(n, *a, *b, *c, d))
        .collect();
    if no_focus && !ctx.scope.is_role() && all.len() > 8 {
        v.push(format!("（其余 {} 个角色略）", all.len() - 8));
    }
    // 2026-11-02「数值影响台词」：把三维翻成**本段的台词硬上限**（全正常 → 不加行）。
    // 只给 AI 扮演的那个角色；没指定扮演者时，只有单角色故事才给（多角色不猜是谁）。
    let target = if !ctx.ai_role.is_empty() && ctx.ai_role != "自由" {
        picked.iter().find(|c| c.0 == ctx.ai_role)
    } else if picked.len() == 1 {
        picked.first()
    } else {
        None
    };
    if let Some((n, st, sa, sc, _)) = target {
        if let Some(line) = crate::characters::perf_directive(n, *st, *sa, *sc) {
            v.push(line);
        }
    }
    Ok(v)
}

/// ③ 示例对话：角色视角只认她自己；全局走三级回落
/// （`@焦点` > AI 扮演 > 面板焦点 > 全库唯一有示例的那个）。
pub(crate) async fn example_dialogue(
    pool: &SqlitePool,
    ctx: &TailCtx<'_>,
) -> Result<String, String> {
    let c_clause = crate::story_db::branch_clause("branch_id", ctx.branch, 1);
    let all_sql = format!(
        "SELECT name, dialogue_examples FROM characters
         WHERE {} AND TRIM(dialogue_examples) <> '' ORDER BY id",
        c_clause
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(all_sql.as_str()));
    if ctx.branch.is_some() {
        q = q.bind(ctx.branch);
    }
    let with_examples: Vec<(String, String)> = q
        .fetch_all(pool)
        .await
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    Some((
                        r.try_get::<String, _>(0).ok()?,
                        r.try_get::<String, _>(1).ok()?,
                    ))
                })
                .collect()
        })
        .map_err(|e| e.to_string())?;
    let names: Vec<String> = with_examples.iter().map(|(n, _)| n.clone()).collect();
    // 2026-11-10 角色视角：示例对话只认她自己（漏风点 ③ —— 否则会落到"全库唯一有示例"的别人头上）
    let owner = if ctx.scope.is_role() {
        let r = ctx.scope.role.trim();
        if names.iter().any(|n| n == r) {
            Some(r.to_string())
        } else {
            None
        }
    } else {
        super::prompt::pick_example_owner(ctx.focus_role, ctx.ai_role, ctx.panel_focus, &names)
    };
    Ok(match owner {
        Some(o) => with_examples
            .into_iter()
            .find(|(n, _)| *n == o)
            .map(|(_, e)| e)
            .unwrap_or_default(),
        None => String::new(),
    })
}

/// 拼出"依赖视角的那几块"，供 `preview_prompt_pair` 输出（全局 / 角色各拼一次）。
///
/// 与真实装配**同源**：内部只调上面三个函数，不另写选择逻辑。
pub(crate) async fn build_tail_scope_view(
    pool: &SqlitePool,
    // 记忆目录 / story_id：关键记忆挪去第③段后这里不再用（保留形参免得调用方大改）
    _memories_dir: &std::path::Path,
    _story_id: &str,
    branch: &Option<String>,
    scope: PromptScope<'_>,
    ai_role: &str,
    focus_role: &str,
    panel_focus: &str,
) -> Result<String, String> {
    let ctx = TailCtx {
        branch,
        scope,
        ai_role,
        focus_role,
        panel_focus,
    };
    let mut parts: Vec<String> = Vec::new();

    let rel_lines = rel_strength_lines(pool, &ctx).await?;
    if !rel_lines.is_empty() {
        parts.push(format!("【数值与当下·关系强度】\n{}", rel_lines.join("\n")));
    }

    let stat_lines = char_stat_lines(pool, &ctx).await?;
    if !stat_lines.is_empty() {
        parts.push(format!("【数值与当下·角色属性】\n{}", stat_lines.join("\n")));
    }

    let ex = example_dialogue(pool, &ctx).await?;
    if !ex.trim().is_empty() {
        parts.push(format!("【示例对话】\n{}", ex.trim()));
    }

    // ④ 关键记忆**不在这里**（2026-11-15 用户定）：它按「关系组」（a×b / b×a 同一组）单独成第③段，
    //    见 `memory::group_by_participants` 与预览命令返回的 `memory_groups`。

    Ok(parts.join("\n\n"))
}
