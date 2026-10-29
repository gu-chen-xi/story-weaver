// M6：后台静默演进——事件提取 → 实质判断 → 变更提案 → 校验 → 人工确认 → 应用 → 快照 → 审计
// MVP 范围：规则引擎实现候选事件提取；关系类型/方向变更为高风险需人工确认；强度可自动应用。

mod snapshot;
pub use snapshot::*;

mod analyze;
pub use analyze::*;

mod proposal;
pub use proposal::*;

use sqlx::Row;
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::characters::{self, Character};
use crate::relations::{self, Relation};

// ============================================================
//  ⑤-B：设定变更"全部入队、人工拍板"
// ============================================================
//
// 用户要求：**软件自动维护"设定"，会依次累计，但最终仍需人工拍板才能落地。**
// 原来低风险变更（关心/守护/恶语/表白被拒/冲突 → 关系强度）会**插入后立即应用**，
// 等于没经过人。现在：
//   · 开关 `evolution.require_approval`（**默认 true**）→ 只入队
//   · 关掉它（= false / 0）→ 恢复旧行为（低风险自动应用）
//
// 为什么默认 true：用户的原话是"最终仍需人工拍板"，所以默认就该拦住。

pub(crate) async fn insert_proposal(
    pool: &SqlitePool,
    branch: &Option<String>,
    node_id: i64,
    group_id: i64,
    target_type: &str,
    target_id: i64,
    field: &str,
    old_value: &str,
    new_value: &str,
    reason: &str,
    evidence: &str,
    confidence: f64,
    status: &str,
) -> Result<i64, sqlx::Error> {
    // 第五批 ③：提案归属当前分支（由调用方传入 —— 分支里分析剧情 → 提案只属于该分支）
    let row = sqlx::query(
        "INSERT INTO change_proposals(target_type, target_id, field, old_value, new_value, reason, evidence, confidence, status, group_id, branch_id, node_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) RETURNING id",
    )
    .bind(target_type)
    .bind(target_id)
    .bind(field)
    .bind(old_value)
    .bind(new_value)
    .bind(reason)
    .bind(evidence)
    .bind(confidence)
    .bind(status)
    .bind(group_id)
    .bind(branch.clone())
    .bind(node_id)
    .fetch_one(pool)
    .await?;
    Ok(row.try_get(0)?)
}

// ---------- 核心分析：事件提取 → 实质判断 → 提案生成 → 校验 → 策略 ----------
#[derive(serde::Serialize)]
pub struct EvolutionReport {
    pub events: Vec<Event>,
    pub proposals: Vec<Proposal>,
    pub applied_groups: Vec<i64>,
    pub notes: Vec<String>,
}

pub async fn analyze_evolution(story_id: &str, node_id: i64) -> Result<EvolutionReport, String> {
    eprintln!("[M6] analyze_evolution called: story={} node={}", story_id, node_id);
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_group_column(&pool).await;

    // 第五批 ③：分析只针对**当前分支** ——
    // ① 读设定（角色/关系）时按分支；② 产出的提案写进该分支；③ 冷却判断也只看该分支
    let proposal_branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let need_approval = require_approval().await;
    analyze_evolution_in(&pool, story_id, node_id, proposal_branch, need_approval).await
}



/// 这一轮用户输入是否涉及"需要模型表态"的事（表白 / 承诺）。
///
/// 用户定的范围（2026-10-26）：表白现在就要判定；承诺先留口子 —— 同一条协议，
/// 以后再往上加"承诺有没有兑现"的标记，不用再改提示词结构。
pub fn stance_marker_kind(user_text: &str) -> Option<&'static str> {
    if analyze::contains_any(user_text, analyze::CONFESSION).is_some() {
        return Some("confession");
    }
    if analyze::contains_any(user_text, analyze::PROMISE_WORDS).is_some() {
        return Some("promise");
    }
    None
}


/// 这一轮：**最后一条用户消息 + 它之后的 AI 回复**（同 `memory_round` 的口径）。
///
/// 为什么分析也要按"轮"而不是"最近 6 条"：用户实测（2026-10-27，测试0）里，
/// 上一轮的「我爱你」与本轮 AI 正文里的「可我也撑不住再吵一次」被拼在一起，
/// 规则当成"表白 + 接受" → 提议关系 6 → 85。跨轮拼接会把两件事缝成一件。
fn last_round<'a>(msgs: &'a [(String, String)]) -> &'a [(String, String)] {
    match msgs.iter().rposition(|(role, _)| role == "user") {
        Some(i) => &msgs[i..],
        None => msgs,
    }
}

/// 这一轮是不是**导演指令**轮次（用户那条消息里带 `〔导演指令〕` 标记）。
///
/// 开挂推出来的结果不是"她的真实态度"，所以这类轮次**不产生数值提案**
/// （账本另记，见 director.rs）；这里返回 true 时只记事件、不提议。
fn round_has_director(round: &[(String, String)]) -> bool {
    round
        .iter()
        .any(|(role, content)| role == "user" && crate::relations::parse_director(content).is_some())
}

/// 这一轮 AI 的回复原文（**只给它打态度分**：表白是被拒还是被接受，看 AI 说了什么）。
fn last_ai_reply(msgs: &[(String, String)]) -> String {
    msgs.iter()
        .rev()
        .find(|(role, _)| role == "assistant")
        .map(|(_, content)| content.clone())
        .unwrap_or_default()
}

/// 关键记忆要摘的"这一轮"：**最后一条用户消息 + 它之后的 AI 回复**。
///
/// 为什么不是整段：`excerpt` 取的是命中词 ±80 字的窗口，喂整段（最多 6 条）时，
/// 摘录会从上一轮 AI 的半句话开始 —— 用户实测截图里就是「…（杯子搁回桌上…）」
/// 打头，看不出是谁对谁说了什么。
///
/// 每条截到 400 字（记忆是"提示"，不是全文；全文在磁盘和消息里都有）。
fn memory_round(msgs: &[(String, String)]) -> String {
    if msgs.is_empty() {
        return String::new();
    }
    let start = msgs.iter().rposition(|(role, _)| role == "user").unwrap_or(msgs.len() - 1);
    let clip = |s: &str| -> String {
        let n = s.chars().count();
        if n <= 400 {
            s.trim().to_string()
        } else {
            format!("{}…", s.chars().take(400).collect::<String>().trim())
        }
    };
    msgs[start..]
        .iter()
        .map(|(role, content)| {
            // 协议与元指令都不进记忆正文（2026-10-27）：
            // 用户那条可能带着 `〔导演指令〕…` 整块，AI 那条可能带着 `〔导演代价〕…` 收据 ——
            // 抄进卡片会让人以为"她说过台词"，也让卡片看起来像调试日志。
            let clean = if role == "user" {
                crate::relations::strip_director(content)
            } else {
                crate::stat_change::strip_line(&crate::director::strip_receipt(content))
            };
            format!("[{}] {}", if role == "user" { "用户" } else { "AI" }, clip(&clean))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod memory_round_tests;

/// `analyze_evolution` 的实现（**pool / 分支 / 开关都可注入**）。
///
/// 拆出来是为了能用**真实库的副本**做诊断：AI 剧情信号不稳定，
/// 靠界面点点点很难复现，测试需要能直接喂数据。
pub async fn analyze_evolution_in(
    pool: &SqlitePool,
    story_id: &str,
    node_id: i64,
    proposal_branch: Option<String>,
    need_approval: bool,
) -> Result<EvolutionReport, String> {

    // 1. 取该 node 最近的剧情上下文（用户输入 + AI 回复，最多 6 条），拼接分析
    //    表白/接受等信号可能出现在用户输入或 AI 回复任意一侧。
    let rows = sqlx::query(
        "SELECT role, content FROM messages WHERE node_id = ?1 AND role IN ('user','assistant')
         ORDER BY id DESC LIMIT 6",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Err(format!("节点 {} 没有剧情消息，无法分析", node_id));
    }
    eprintln!("[M6] node {} 消息 {} 条", node_id, rows.len());
    // 按时间正序摊平：`memory_round` 要靠"最后一条用户消息 + 它之后的 AI 回复"取摘录，
    // 顺序反了就会取到上一轮的对话。
    let msgs: Vec<(String, String)> = rows
        .iter()
        .rev()
        .map(|r| {
            let role: String = r.try_get(0).unwrap_or_default();
            let content: String = r.try_get(1).unwrap_or_default();
            (role, content)
        })
        .collect();
    // 只分析**这一轮**（最后一条用户消息 + 它之后的回复）。
    // 原来拼最近 6 条：上一轮的「我爱你」能和本轮的「我也…」配成"表白+接受"（E-AI-15）。
    let round = last_round(&msgs);
    let text: String = round
        .iter()
        .map(|(role, content)| format!("[{}] {}", if role == "user" { "用户" } else { "AI" }, content))
        .collect::<Vec<_>>()
        .join("\n\n");
    let director_round = round_has_director(round);
    // v3.4 修（2026-10-26）：关键记忆只摘"这一轮"—— 用户那条消息 + AI 的回复。
    // 原来把整段（最多 6 条）都喂给 `excerpt`，取 ±80 字窗口，摘录会从上一轮 AI 的
    // 半句话开始，看不出是谁对谁说了什么（用户实测截图）。
    let round_text = memory_round(&msgs);

    // 2. 提取事件 + 写日志
    let events = extract_events(&text);
    log_events(pool, node_id, &events).await.map_err(|e| e.to_string())?;

    // 2.5 v3.4：关键记忆 —— 表白/承诺/牺牲/背叛**每种只记第一次**，一个事件一个 Markdown 文档。
    //     失败不阻断分析（记忆是附加物，不该让提案流程跟着挂）。
    let mut memory_notes: Vec<String> = Vec::new();
    {
        // 按**节点自己的分支**取（不是"当前分支"）：分析历史节点时也对得上
        let branch = crate::branch::branch_of_node(pool, node_id).await.ok().flatten();
        let day_label: String =
            sqlx::query_scalar("SELECT date_label FROM timeline_nodes WHERE id = ?1")
                .bind(node_id)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| format!("节点 {node_id}"));
        let pairs: Vec<(String, String)> =
            events.iter().map(|e| (e.event_type.clone(), e.matched.clone())).collect();
        match crate::memory::capture_first_events(story_id, &branch, node_id, &day_label, &round_text, &last_ai_reply(&msgs), &pairs) {
            Ok(made) => memory_notes = made,
            Err(e) => eprintln!("[memory] 关键记忆写入失败：{e}"),
        }
    }

    // 2.6 v3.5.1：表演标签（动作/情绪）—— 从**括号段落**里静默抽取，记进 event_logs。
    //     用户看不见，但它让"这一场她是感动还是麻木"变成可查的信号（复用既有表，零迁移）。
    {
        let perf = crate::perform::from_reply(&text);
        if !perf.is_empty() {
            if let Err(e) = crate::perform::log_performance(pool, node_id, &perf).await {
                eprintln!("[perform] 表演标签记录失败：{e}");
            }
        }
    }

    let mut proposals: Vec<Proposal> = Vec::new();
    let mut applied_groups: Vec<i64> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    // v3.4：把"本次新记下的关键记忆"一并回传，界面能提示用户
    for label in memory_notes {
        notes.push(format!("已记下关键记忆：{label}"));
    }
    // ⑤-B：need_approval 由调用方传入（便于测试两种行为）


    // 2.7 导演指令轮次（2026-10-27）：**只记事件，不生成数值提案**。
    //
    // 用户定的边界：导演指令是"开挂推进剧情"，代价单独记账（director.rs），
    // 但它不该把"她答应了"写成关系数值 —— 否则一条 10 分的指令能换 79 点关系跃迁（实测 6 → 85）。
    if director_round {
        notes.push("本轮用了导演指令：结果属于开挂推进，不生成数值提案（事件已记录）".to_string());
        log_note(pool, node_id, "director_skip", "导演指令轮次：跳过数值提案，仅记录事件")
            .await
            .map_err(|e| e.to_string())?;
        return Ok(EvolutionReport { events, proposals, applied_groups, notes });
    }

    // 3. 实质判断：基于关系图
    let rels = relations::list_relations(story_id).await.map_err(|e| e.to_string())?;
    let chars = characters::list_characters(story_id).await.map_err(|e| e.to_string())?;

    // 世界观硬规则
    let mut hard_rules: Vec<(String, String)> = Vec::new();
    let wrows = sqlx::query("SELECT key, value FROM world_settings WHERE is_hard_rule = 1")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    for r in &wrows {
        hard_rules.push((r.try_get(0).map_err(|e| e.to_string())?, r.try_get(1).map_err(|e| e.to_string())?));
    }

    let has_confession = events.iter().any(|e| e.event_type == "confession");
    let has_acceptance = events.iter().any(|e| e.event_type == "acceptance");
    let has_rejection = events.iter().any(|e| e.event_type == "rejection");
    let has_care = events.iter().any(|e| e.event_type == "care");
    let has_conflict = events.iter().any(|e| e.event_type == "conflict");
    // 第三批新增信号
    let has_sacrifice = events.iter().any(|e| e.event_type == "sacrifice");
    let has_betrayal = events.iter().any(|e| e.event_type == "betrayal");

    for rel in &rels {
        // 找双方角色名
        let from_name = chars.iter().find(|c| c.id == rel.from_character_id).map(|c| c.name.clone());
        let to_name = chars.iter().find(|c| c.id == rel.to_character_id).map(|c| c.name.clone());
        let both_present = match (&from_name, &to_name) {
            (Some(f), Some(t)) => text.contains(f) && text.contains(t),
            _ => false,
        };
        if !both_present {
            continue;
        }

        let _rel_target = format!("relation#{}", rel.id);
        let evidence = format!("节点 {} 剧情文本（含『{}』『{}』）", node_id, from_name.as_deref().unwrap_or(""), to_name.as_deref().unwrap_or(""));

        // 冷却期检查（只针对关系变更）
        let cooling = in_cooldown(pool, "relation", rel.id, node_id).await.map_err(|e| e.to_string())?;

        // A. 表白 + 接受 → 关系跃迁（高风险，需人工确认）
        if has_confession && has_acceptance && rel.direction == "单向" {
            if cooling {
                notes.push(format!("关系 {} 处于冷却期，跳过跃迁提案（仅记录）", rel.id));
                log_note(pool, node_id, "cooldown_skip", &format!("relation#{} 冷却期，跳过跃迁提案", rel.id)).await.map_err(|e| e.to_string())?;
                continue;
            }
            // 组 ID：复用已有同 node 未处理组，或新组
            let group: i64 = sqlx::query(
                "SELECT COALESCE(MAX(group_id), 0) FROM change_proposals",
            )
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?
            .try_get::<i64, _>(0)
            .map_err(|e| e.to_string())? + 1;

            let mut group_ok = true;
            let mut fields: Vec<(String, String, String)> = Vec::new(); // (field, old, new)

            if rel.relation_type != "明恋" && rel.relation_type != "情侣" {
                if let Err(e) = validate("relation_type", "明恋", 0.92, &evidence, &hard_rules) {
                    notes.push(format!("关系 {} 校验失败：{}", rel.id, e));
                    log_note(pool, node_id, "validation_reject", &format!("relation#{}: {}", rel.id, e)).await.map_err(|e| e.to_string())?;
                    group_ok = false;
                } else {
                    fields.push(("relation_type".to_string(), rel.relation_type.clone(), "明恋".to_string()));
                }
            }
            if rel.direction != "双向" {
                fields.push(("direction".to_string(), rel.direction.clone(), "双向".to_string()));
            }
            let new_strength = rel.strength.max(85);
            if new_strength > rel.strength {
                fields.push(("strength".to_string(), rel.strength.to_string(), new_strength.to_string()));
            }

            if group_ok && !fields.is_empty() {
                for (field, old, new) in &fields {
                    let pid = insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, field, old, new,
                        "剧情出现明确表白并被接受，关系跃迁", &evidence, 0.92, "pending",
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    // 回读完整记录
                    let p = sqlx::query(
                        "SELECT id, group_id, target_type, target_id, field, old_value, new_value, reason, evidence, confidence, status, '', created_at, node_id
                         FROM change_proposals WHERE id = ?1",
                    )
                    .bind(pid)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                    proposals.push(proposal_from_row(&p));
                }
            }
            continue;
        }

        // B. 表白被拒 → 强度下降 + 状态动摇（关系推进：入队待拍板）
        if has_confession && has_rejection && rel.direction == "单向" && !cooling {
            let new_strength = (rel.strength - 15).max(0);
            if new_strength < rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.90, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "表白被拒，关系强度下降", &evidence, 0.90, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }

        // C. 关心 → 强度小幅上升（关系推进：入队待拍板）
        if has_care && !cooling {
            let new_strength = (rel.strength + 5).min(100);
            if new_strength > rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.85, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "剧情出现关心互动，强度小幅上升", &evidence, 0.85, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }

        // E. 守护/挡下（第三批新增）→ 强度较大上升（关系推进：入队待拍板）
        if has_sacrifice && !cooling {
            let new_strength = (rel.strength + 10).min(100);
            if new_strength > rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.90, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "剧情出现守护/挡下的动作，强度上升", &evidence, 0.90, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }

        // F. 恶语/冷暴力/失信（第三批新增）→ 强度较大下降 + 状态趋向紧张（关系推进：入队待拍板）
        if has_betrayal && !cooling {
            let new_strength = (rel.strength - 20).max(0);
            let mut fields: Vec<(String, String, String)> = Vec::new();
            if new_strength < rel.strength {
                fields.push(("strength".to_string(), rel.strength.to_string(), new_strength.to_string()));
            }
            // 稳定 → 紧张（已有更强状态时不覆盖）
            if rel.state == "稳定" {
                fields.push(("state".to_string(), "稳定".to_string(), "紧张".to_string()));
            }
            if !fields.is_empty() {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                let mut ok = true;
                for (field, _old, new) in &fields {
                    if validate(field, new, 0.88, &evidence, &hard_rules).is_err() {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    for (field, old, new) in &fields {
                        insert_proposal(
                            pool, &proposal_branch, node_id, group, "relation", rel.id, field, old, new,
                            "剧情出现恶语/冷暴力/失信，关系受损", &evidence, 0.88, "pending",
                        ).await.map_err(|e| e.to_string())?;
                    }
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }

        // D. 冲突 → 强度下降（关系推进：入队待拍板）
        if has_conflict && !cooling {
            let new_strength = (rel.strength - 10).max(0);
            if new_strength < rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.85, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        pool, &proposal_branch, node_id, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "剧情出现冲突，强度下降", &evidence, 0.85, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    queue_or_apply(pool, story_id, group, need_approval,
                                   &mut proposals, &mut applied_groups, &mut notes).await?;
                }
            }
            continue;
        }
    }

    // 若只有单一表白（无接受/拒绝）→ 记录为"单次暧昧不提案"
    if has_confession && !has_acceptance && !has_rejection {
        notes.push("检测到表白但无回应，按『单次暧昧』规则不生成提案".into());
        log_note(pool, node_id, "no_substantial_change", "表白无回应，不生成提案").await.map_err(|e| e.to_string())?;
    }

    Ok(EvolutionReport {
        events,
        proposals,
        applied_groups,
        notes,
    })
}

// ---------- 快照 / 审计 / 事件日志（M7） ----------

// ---------- 第五批 ②：快照差异预览（回滚前的"体检报告"） ----------

#[derive(serde::Serialize)]
pub struct AuditLog {
    pub id: i64,
    pub action: String,
    pub target: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub operator: String,
    pub created_at: String,
}

pub async fn list_audit_logs(story_id: &str) -> Result<Vec<AuditLog>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let rows = sqlx::query("SELECT id, action, target, before, after, operator, created_at FROM audit_logs ORDER BY id DESC LIMIT 100")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(AuditLog {
            id: r.try_get(0).map_err(|e| e.to_string())?,
            action: r.try_get(1).map_err(|e| e.to_string())?,
            target: r.try_get(2).map_err(|e| e.to_string())?,
            before: r.try_get(3).map_err(|e| e.to_string())?,
            after: r.try_get(4).map_err(|e| e.to_string())?,
            operator: r.try_get(5).map_err(|e| e.to_string())?,
            created_at: r.try_get(6).map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

#[derive(serde::Serialize)]
pub struct EventLogItem {
    pub id: i64,
    pub node_id: Option<i64>,
    pub event_type: String,
    pub payload_json: String,
    pub confidence: f64,
    pub source: String,
}

pub async fn list_event_logs(story_id: &str) -> Result<Vec<EventLogItem>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let rows = sqlx::query("SELECT id, node_id, event_type, payload_json, confidence, source FROM event_logs ORDER BY id DESC LIMIT 50")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(EventLogItem {
            id: r.try_get(0).map_err(|e| e.to_string())?,
            node_id: r.try_get(1).map_err(|e| e.to_string())?,
            event_type: r.try_get(2).map_err(|e| e.to_string())?,
            payload_json: r.try_get(3).map_err(|e| e.to_string())?,
            confidence: r.try_get(4).map_err(|e| e.to_string())?,
            source: r.try_get(5).map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod third_batch_tests;


/// 第五批 ②：快照差异预览的纯函数测试（`diff_items` 不碰数据库，可直接造 JSON 比对）
#[cfg(test)]
mod snapshot_diff_tests;


/// 第五批 ②：回滚路径的 **SQL 层**测试。
///
/// 背景：用户实测点「回滚」报 `UNIQUE constraint failed: world_settings.id`（1555）。
/// 根因是回滚时 `.bind(w.get("id")...unwrap_or(0))` —— **老快照的 payload 里没有 id**，
/// 于是每条都插 id=0，第二条起主键冲突。
///
/// 为什么不写成"调用 apply 函数"的集成测试：那需要把恢复逻辑抽成独立函数（改动较大、
/// 易引入回归）。这里**在临时库上直接跑同一套 SQL**，一样能钉住"缺失 id 必须传 NULL"这条规则，
/// 而且不碰生产代码结构。
#[cfg(test)]
mod rollback_sql_tests;


/// 第五批 ②：会话内临时版本的 SQL 层测试。
///
/// 语义（用户 2026-10-12 定）：回滚前自动存一份"当前状态"，**只在本次运行期间有效**，
/// 程序启动时清掉上一次运行遗留的 —— 也就是"退出即失效"。
#[cfg(test)]
mod temporary_snapshot_tests;


/// 第五批 ③ 第 3 步（3-3 批）：快照与回滚的**分支隔离**测试。
///
/// 这里测的是 `rollback_snapshot` 的"应用 payload"那一段的核心语义：
/// **回滚只清当前分支的行**，不能把别的分支的数据一起清掉。
/// （原实现是整表 `DELETE FROM characters` —— 分支下会把主线与其它分支一起清空。）
///
/// 用临时故事库直接调 `apply` 路径太重（它内部会 resolve 分支、开库），
/// 所以这里用**同一套 SQL** 在临时库上验证语义 —— 与 `branch.rs` 里 SQL 层测试同一思路。
#[cfg(test)]
mod branch_rollback_tests;


/// ⑤-B：设定变更"全部入队、人工拍板"的行为测试。
///
/// 核心命题只有三条：
///   ① 默认（开关 on）→ **只入队，不改库**
///   ② 关掉开关 → 恢复旧行为（低风险自动应用）
///   ③ 批量落地 → 真的落地 + **整批只留一个还原点**
#[cfg(test)]
mod approval_queue_tests;

/// **诊断用（`#[ignore]`，不参与常规测试）**：拿一份真实故事的库副本，
/// 逐个节点跑分析，把后端返回什么摊开打出来。
///
/// 为什么需要它：AI 剧情信号不稳定，「点了没反应」很难靠界面复现。
/// 这个测试能回答"到底是没信号、还是报错了、还是根本没进到引擎"。
///
/// 用法（**单独跑**，因为它会临时改数据根）：
/// ```text
/// $env:DW_DIAG_DB="D:\project\ai生成\data\library\default\projects\7\story.db"
/// cargo test --lib diag_analyze_real_db -- --ignored --nocapture
/// ```
#[cfg(test)]
mod diag_tests;


/// 回归测试：**回滚不能和别的分支的行撞主键**。
///
/// 用户实机报错：`error returned from database: (code: 1555)
/// UNIQUE constraint failed: characters.id`。
/// 根因是 `make_snapshot` 原来读全表（不分分支），主线快照里装着分支的角色
/// （真实数据：主线 id 2/3/4 + 分支 id 27/28/29 都在同一个快照里）；
/// 回滚时只删主线行，再把 6 条按原 id 插回去 → 撞上仍在的分支行。
///
/// 现在的行为：快照按分支隔离；老快照里混进来的"别的分支的行"在回滚时**跳过**。
#[cfg(test)]
mod rollback_branch_scope_tests;


/// 回滚必须**要么全成、要么全不成**（直接测产品路径 `rollback_snapshot_in`）。
///
/// 实测事故：回滚在"插角色"时撞主键（1555）报错，但前面已经把三张设定表删了 ——
/// 主线的关系与世界观被删掉、再也没插回来，用户看到"主线没有关系了"。
#[cfg(test)]
mod rollback_atomic_tests;



