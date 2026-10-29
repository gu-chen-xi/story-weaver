mod message;
pub use message::*;

mod node;
pub use node::*;

mod summary;
pub use summary::*;

mod day_close;
pub use day_close::*;

use sqlx::Row;
use sqlx::SqlitePool;


/// 第五批 ③：取该故事"当前所在分支"（`None` = 主线）。
///
/// 新建节点/消息时用它决定 `branch_id` —— 在分支里写作就写进分支，主线就写 NULL。
/// 读不到（library.db 异常）时按主线处理，不让写作失败。
async fn current_branch_for(story_id: &str) -> Option<String> {
    crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None)
}


// ===== 第五批 ①：时间线树 —— 纯函数（不碰数据库，便于单测与复用） =====

// ===================== D 子项：日摘要 / 阶段摘要 =====================

/// 调一次"摘要模型"：关思考、1500 额度、失败重试一次、空正文带诊断。
///
/// 日摘要与阶段摘要共用（原来这段 HTTP 逻辑写在 `generate_day_summary` 里，
/// 接阶段摘要时要复制一遍 —— 抽出来避免两份逐渐跑偏）。
pub(crate) async fn call_summary_model(
    lib_pool: &SqlitePool,
    system_prompt: &str,
    user_content: &str,
) -> Result<String, String> {
    let base_url = crate::db::get_config(lib_pool, "model.base_url")
        .await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let model = crate::db::get_config(lib_pool, "model.name")
        .await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "deepseek-v4-flash".to_string());
    let api_key = crate::keychain::get_api_key("openai_api_key".to_string())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未设置 API Key".to_string())?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    // 用 1500 额度重试一次：思考模式下 max_tokens 太小会被思考内容吃光，导致正文为空
    let attempts: [(usize, bool); 2] = [(1500, true), (1500, false)];
    let mut summary = String::new();
    let mut last_note = String::new();
    for (max_tokens, disable_thinking) in attempts {
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": system_prompt},
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
    Ok(summary)
}

// 说明（2026-10-17 改）：**不需要在压缩时排除"最近几天"**。
// 最近 3 天是在**原文区**（keep_nodes）里整段给出的，它们的日摘要本来就不会进【前情提要】；
// 所以阶段摘要覆盖到它们也不会造成重复 —— 早先那版"排除最近 3 天"反而把 x 的语义搞乱了。

/// 选择"需要补日摘要"的节点（幂等）：**时间线上排在当前节点之前**、有剧情、还没有摘要的非倒叙节点。
///
/// 用"位置"而不是"sort_order 比大小"来判断先后 —— 因为历史数据里 sort_order 可能重复
/// （例如两个节点都叫「第 8 天」、sort_order 分别为 9 和 13），比大小会在极端情况下全表落空。
/// 抽成独立函数以便单测直接验证（不触发任何 API 调用）。
/// 选出"应该补摘要"的节点：**当前分支可见范围内、早于当前节点、有剧情、无摘要、非倒叙**。
///
/// ⚠ **RAG-Q7 修复（2026-10-17）**：原实现读**全表**再按 `sort_order` 判断"是否在当前节点之前"，
/// 完全不管分支。在分支故事里后果是：
///   · 挑到**兄弟分支**的节点 → 花用户的 API 钱去给不在的那条线生成摘要（并写进该分支）；
///   · 漏掉当前分支上全表位置靠后的节点。
///
/// 现在先用 `branch_path_ids`（= 本分支自己的节点 ∪ 沿父链的共享历史）限定范围，
/// 再在这个范围内按 `sort_order, id` 排序判断"之前"。主线（`branch_id IS NULL`）行为不变。
pub(crate) async fn select_nodes_needing_summary(
    pool: &SqlitePool,
    current_node_id: i64,
    branch: &Option<String>,
) -> Result<Vec<(i64, String)>, sqlx::Error> {
    // 可见范围：本分支 + 父链共享历史
    let visible = branch_path_ids(pool, branch).await;
    if visible.is_empty() {
        return Ok(Vec::new());
    }

    // 全表按时间线顺序读出来，但**只用可见范围里的行**参与位置判断
    let all = sqlx::query(
        "SELECT id, date_label, sort_order, is_flashback, COALESCE(day_summary, '')
         FROM timeline_nodes ORDER BY sort_order ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;
    // (id, 标签, sort_order, 是否倒叙, 摘要)
    let rows: Vec<(i64, String, i64, i64, String)> = all
        .iter()
        .filter_map(|r| {
            let id = r.try_get::<i64, _>(0).ok()?;
            if !visible.contains(&id) {
                return None; // ← 别的分支的节点，彻底不参与
            }
            Some((
                id,
                r.try_get::<String, _>(1).unwrap_or_default(),
                r.try_get::<i64, _>(2).unwrap_or(0),
                r.try_get::<i64, _>(3).unwrap_or(0),
                r.try_get::<String, _>(4).unwrap_or_default(),
            ))
        })
        .collect();

    let pos = rows.iter().position(|(id, _, _, _, _)| *id == current_node_id);

    let mut out = Vec::new();
    for (idx, (id, label, _sort, flashback, summary)) in rows.iter().enumerate() {
        // 当前节点本身 / 倒叙节点 / 已有摘要（固化）都不参与
        if *id == current_node_id || *flashback != 0 || !summary.trim().is_empty() {
            continue;
        }
        // 当前节点在可见范围内时必须排在它之前；找不到（数据异常）时退回"全部可见节点"
        if let Some(p) = pos {
            if idx >= p {
                continue;
            }
        }
        let has_msgs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages WHERE node_id = ?1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        if has_msgs > 0 {
            out.push((*id, label.clone()));
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

/// 决定"新节点该落在哪个分支"。
///
/// * `derived` —— 由"上一个节点"推导出来的分支（`None`=节点不存在/库打不开；
///   `Some(None)`=节点在**共享历史/主线**；`Some(Some(b))`=节点在某条分支 b 上）
/// * `from_ui` —— 界面传来的当前分支（`Some(b)`=在某条分支；`None`=主线）
///
/// **规则：界面优先。** 节点推导只作为兜底（界面没给值时）。
///
/// ## 为什么反转（原来是"节点推导优先"）
///
/// 两次实测都栽在这个优先级上：
///
/// 1. **共享历史**（2026-10-17）：新建分支后它的"最后一个节点"就是分叉点，而分叉点
///    `branch_id IS NULL` —— 按节点推导 = 主线 → 新分支里点「下一天」，主线多一天。
/// 2. **嵌套子分支**（2026-10-24）：从"从6分支的第8天"再分一支，这条子分支的**分叉点本身
///    属于父分支**（`branch_id = 从6分支`）—— 按节点推导 = 父分支 → 新建的第9/10天
///    全跑到父分支里去了。
///
/// 两种情况根因相同：**分叉点属于"上一级"的线**，而用户要的是"我现在站的这条线"。
/// 所以只有界面知道答案 —— 前端已经在"点节点时跟随切换分支"（`loadTimelineAndMessages`）。
pub(crate) fn resolve_branch_for_new_node(
    derived: Option<Option<String>>,
    from_ui: Option<String>,
) -> Option<Option<String>> {
    match from_ui {
        Some(b) => Some(Some(b)),
        // 界面说"主线"时：若节点确实挂在某条分支上（界面状态可能过期），仍按节点走，
        // 避免"人在分支、节点却建到主线"的老 bug 复发。
        None => match derived {
            Some(Some(b)) => Some(Some(b)),
            _ => Some(None),
        },
    }
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

/// 时间线段（C 子项"时间流逝"）：相邻两个 day 节点之间跳号时，就是一段"无事件的时间"。
/// 返回 `(起始天数, 结束天数)`，如 (3, 7) 表示第 4~6 天是空白期（无节点）。
// 保留未用：时间断档计算（时间线页提示用，保留）
#[allow(dead_code)]
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

#[cfg(test)]
mod summary_tests;


#[cfg(test)]
mod tree_tests;


/// 第五批 ③ 第 3 步（3-4 批）：节点与消息的**分支归属**测试。
///
/// 口径：
/// - 新建节点/消息时带上"当前所在分支"（主线 = NULL）
/// - 消息的 `branch_id` **由节点推导**（保证与节点一致，不靠调用方传）
/// - `list_timeline_nodes` 仍是**全树**（树形展示要看到所有分支的节点）——
///   这一条单独一条测试守着，防止将来有人"顺手"给它加 `branch_id IS NULL` 把树砍断
#[cfg(test)]
mod branch_node_tests;


/// RAG-Q7 回归测试：**补摘要的选择器不能跨分支**。
///
/// 原实现读全表按 `sort_order` 判断"是否在当前节点之前"，在分支故事里会挑到
/// **兄弟分支**的节点 —— 花用户的 API 钱去给不在的那条线生成摘要。
#[cfg(test)]
mod summary_selector_branch_tests;

#[cfg(test)]
mod branch_resolve_tests;

#[cfg(test)]
mod phase_summary_tests;
