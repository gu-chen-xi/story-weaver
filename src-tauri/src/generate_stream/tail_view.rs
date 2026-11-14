//! 尾部块里**依赖视角**的那几块的只读预览（2026-11-14）。
//!
//! 双池对比原来只比 system 段，而"关键记忆按角色"恰恰发生在尾部 —— 工具里看不见，等于没验。
//! 这里把四块拼成一段可读文本，供 `preview_prompt_pair` 输出：
//!   ① 【数值与当下·关系强度】②【数值与当下·角色属性】③【示例对话】④【关键记忆】
//!
//! **口径来源**：这四块的选择规则与 `generate_reply_streaming` 里那几段一致
//! （数值段只给 `from=她` 的关系与她自己；示例只认她；记忆只留"她的 + 没有归属的"）。
//! 改那边的规则时这里要跟着改 —— 有 `tail_view_tests` 兜底。
//!
//! 只读、不落库、不调模型；不参与真正发出去的提示词。

use sqlx::{Row, SqlitePool};

use super::PromptScope;

fn pack(text: String) -> String {
    text
}

/// 拼出"依赖视角的那几块"（全局 / 角色各拼一次）
pub(crate) async fn build_tail_scope_view(
    pool: &SqlitePool,
    story_id: &str,
    branch: &Option<String>,
    scope: PromptScope<'_>,
    ai_role: &str,
) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();

    // ① 关系强度：一行一个方向（角色视角只留 from=她）
    let (r_clause, f_clause, t_clause) = (
        crate::story_db::branch_clause("r.branch_id", branch, 1),
        crate::story_db::branch_clause("f.branch_id", branch, 2),
        crate::story_db::branch_clause("t.branch_id", branch, 3),
    );
    let rel_sql = format!(
        "SELECT f.name, t.name, r.strength, r.state FROM relations r
         JOIN characters f ON f.id = r.from_character_id AND {}
         JOIN characters t ON t.id = r.to_character_id AND {}
         WHERE {} ORDER BY r.id",
        f_clause, t_clause, r_clause
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(rel_sql.as_str()));
    if branch.is_some() {
        q = q.bind(branch).bind(branch).bind(branch);
    }
    let rows = q.fetch_all(pool).await.map_err(|e| e.to_string())?;
    let mut rel_lines: Vec<String> = Vec::new();
    for r in &rows {
        let from: String = r.try_get(0).unwrap_or_default();
        let to: String = r.try_get(1).unwrap_or_default();
        if scope.is_role() && from != scope.role.trim() {
            continue;
        }
        let strength: i64 = r.try_get(2).unwrap_or(50);
        let state: String = r.try_get(3).unwrap_or_default();
        rel_lines.push(crate::relations::rel_value_line(&from, &to, strength, &state));
    }
    if !rel_lines.is_empty() {
        parts.push(format!("【数值与当下·关系强度】\n{}", rel_lines.join("\n")));
    }

    // ② 角色属性：全局最多 8 个（带"其余略"），角色视角只给她自己
    let c_clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let c_sql = format!(
        "SELECT name, stat_stamina, stat_sanity, stat_screen, current_status FROM characters WHERE {} ORDER BY id",
        c_clause
    );
    let mut cq = sqlx::query(sqlx::AssertSqlSafe(c_sql.as_str()));
    if branch.is_some() {
        cq = cq.bind(branch);
    }
    let crows = cq.fetch_all(pool).await.map_err(|e| e.to_string())?;
    let all: Vec<(String, i64, i64, i64, String)> = crows
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
    let picked: Vec<(String, i64, i64, i64, String)> = if scope.is_role() {
        all.iter().filter(|c| c.0 == scope.role.trim()).cloned().collect()
    } else {
        all.iter().take(8).cloned().collect()
    };
    let mut stat_lines: Vec<String> = picked
        .iter()
        .map(|(n, a, b, c, d)| crate::characters::stat_value_line(n, *a, *b, *c, d))
        .collect();
    if !scope.is_role() && all.len() > 8 {
        stat_lines.push(format!("（其余 {} 个角色略）", all.len() - 8));
    }
    // 台词口径（只有她自己的三维才给）
    let target = if !ai_role.trim().is_empty() && ai_role != "自由" {
        picked.iter().find(|c| c.0 == ai_role)
    } else if picked.len() == 1 {
        picked.first()
    } else {
        None
    };
    if let Some((n, st, sa, sc, _)) = target {
        if let Some(line) = crate::characters::perf_directive(n, *st, *sa, *sc) {
            stat_lines.push(line);
        }
    }
    if !stat_lines.is_empty() {
        parts.push(format!("【数值与当下·角色属性】\n{}", stat_lines.join("\n")));
    }

    // ③ 示例对话：只认她（角色视角）/ 三级回落（全局）
    let ex_clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let ex_sql = format!(
        "SELECT name, dialogue_examples FROM characters WHERE {} AND TRIM(dialogue_examples) <> '' ORDER BY id",
        ex_clause
    );
    let mut eq = sqlx::query(sqlx::AssertSqlSafe(ex_sql.as_str()));
    if branch.is_some() {
        eq = eq.bind(branch);
    }
    let erows = eq.fetch_all(pool).await.map_err(|e| e.to_string())?;
    let with_examples: Vec<(String, String)> = erows
        .iter()
        .filter_map(|r| Some((r.try_get::<String, _>(0).ok()?, r.try_get::<String, _>(1).ok()?)))
        .collect();
    let names: Vec<String> = with_examples.iter().map(|(n, _)| n.clone()).collect();
    let owner = if scope.is_role() {
        let r = scope.role.trim();
        if names.iter().any(|n| n == r) { Some(r.to_string()) } else { None }
    } else {
        super::prompt::pick_example_owner("", ai_role, "", &names)
    };
    let ex = match owner {
        Some(o) => with_examples
            .into_iter()
            .find(|(n, _)| *n == o)
            .map(|(_, e)| e)
            .unwrap_or_default(),
        None => String::new(),
    };
    if !ex.trim().is_empty() {
        parts.push(format!("【示例对话】\n{}", ex.trim()));
    }

    // ④ 关键记忆：她的 + 没有归属的（角色视角）；全局全给
    let mems = crate::memory::list_for_ancestors_in(
        &crate::memory::memories_dir(story_id),
        &crate::memory::ancestors_of(story_id, branch).await,
    );
    let mems = crate::memory::filter_for_role(&mems, scope.role);
    if let Some(block) = crate::memory::render_block_for(&mems, 1200, true) {
        parts.push(block.trim_start_matches('\n').to_string());
    }

    Ok(pack(parts.join("\n\n")))
}
