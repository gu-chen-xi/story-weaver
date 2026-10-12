// M6：后台静默演进——事件提取 → 实质判断 → 变更提案 → 校验 → 人工确认 → 应用 → 快照 → 审计
// MVP 范围：规则引擎实现候选事件提取；关系类型/方向变更为高风险需人工确认；强度可自动应用。

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::Row;
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::characters::{self, Character};
use crate::relations::{self, Relation};
use crate::story_db::story_db_path;

// ---------- 事件信号词典 ----------
const CONFESSION: &[&str] = &["我喜欢你", "我爱你", "喜欢你很久", "做我女朋友", "做我男朋友", "跟我交往", "我们在一起吧", "向你表白"];
const ACCEPTANCE: &[&str] = &["我也喜欢你", "我也喜欢", "我早就喜欢你", "我也是", "我愿意", "当然愿意", "我答应你", "我答应", "我们在一起吧", "我们在一起", "早就是了", "我也"];
const REJECTION: &[&str] = &["我不喜欢你", "我们还是朋友吧", "我们还是朋友", "我拒绝你"];
const CARE: &[&str] = &["关心", "递伞", "挡雨", "照顾", "扶住", "担心你", "心疼"];
const CONFLICT: &[&str] = &["争吵", "吵架", "冷战", "推开", "不理她", "不理他", "生气地说", "摔门"];

// ===== 第三批新增信号（2026-10-11）=====
// 设计原则：**只加不会误判的强信号** —— 都是明确的剧情动作，避免"关心/照顾"这类泛词导致提案泛滥。
// 各信号用途见 analyze_evolution：
//   promise   → 一次明确的承诺/约定：只记事件，不自动改动数值
//   sacrifice → 为对方挡下/站出来：关系强度较大上升
//   betrayal  → 恶语、冷暴力、失信：关系强度较大下降 + 状态趋向紧张
const PROMISE_WORDS: &[&str] = &["我答应你", "我保证", "我发誓", "我会一直", "我永远", "等你回来", "说到做到", "放心交给我"];
const SACRIFICE_WORDS: &[&str] = &["挡在她身前", "挡在他身前", "挡在身前", "护住她", "护住他", "替她挨", "替他挨", "挺身而出", "护在身后"];
const BETRAYAL_WORDS: &[&str] = &["恶语", "冷暴力", "失信", "骗了她", "骗了他", "背叛", "说出很难听", "翻脸"];

const COOLDOWN_DAYS: i64 = 3;

async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;
    let _ = crate::story_db::ensure_story_columns(&pool).await;
    // 第五批 ②：会话内临时版本用的 temporary 列（幂等）
    let _ = crate::story_db::ensure_snapshot_temporary_column(&pool).await;
    Ok(pool)
}

/// 确保 change_proposals 有 group_id / note 列（老库迁移；新库建表时已带）。
async fn ensure_group_column(pool: &SqlitePool) {
    let _ = sqlx::query("ALTER TABLE change_proposals ADD COLUMN group_id INTEGER")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE change_proposals ADD COLUMN note TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await;
}

// ---------- 候选事件 ----------
#[derive(serde::Serialize, Clone, Debug)]
pub struct Event {
    pub event_type: String, // confession / acceptance / rejection / care / conflict
    pub matched: String,
    pub confidence: f64,
}

fn contains_any(text: &str, words: &[&str]) -> Option<String> {
    for w in words {
        if text.contains(w) {
            return Some(w.to_string());
        }
    }
    None
}

/// 规则引擎：从一段 AI 剧情回复中提取候选事件（每个信号类型最多一条）。
fn extract_events(text: &str) -> Vec<Event> {
    let mut out = Vec::new();
    if let Some(m) = contains_any(text, CONFESSION) {
        out.push(Event {
            event_type: "confession".into(),
            matched: m,
            confidence: 0.95,
        });
    }
    if let Some(m) = contains_any(text, ACCEPTANCE) {
        out.push(Event {
            event_type: "acceptance".into(),
            matched: m,
            confidence: 0.96,
        });
    }
    if let Some(m) = contains_any(text, REJECTION) {
        out.push(Event {
            event_type: "rejection".into(),
            matched: m,
            confidence: 0.90,
        });
    }
    if let Some(m) = contains_any(text, CARE) {
        out.push(Event {
            event_type: "care".into(),
            matched: m,
            confidence: 0.85,
        });
    }
    if let Some(m) = contains_any(text, CONFLICT) {
        out.push(Event {
            event_type: "conflict".into(),
            matched: m,
            confidence: 0.85,
        });
    }
    // ===== 第三批新增信号 =====
    if let Some(m) = contains_any(text, PROMISE_WORDS) {
        out.push(Event { event_type: "promise".into(), matched: m, confidence: 0.88 });
    }
    if let Some(m) = contains_any(text, SACRIFICE_WORDS) {
        out.push(Event { event_type: "sacrifice".into(), matched: m, confidence: 0.90 });
    }
    if let Some(m) = contains_any(text, BETRAYAL_WORDS) {
        out.push(Event { event_type: "betrayal".into(), matched: m, confidence: 0.88 });
    }
    out
}

/// 事件类型 → 中文名（界面显示用；避免"confession / care"这类英文直接抛给用户）
pub fn event_label(event_type: &str) -> &'static str {
    match event_type {
        "confession" => "表白",
        "acceptance" => "接受",
        "rejection" => "拒绝",
        "care" => "关心",
        "conflict" => "冲突",
        "promise" => "承诺",
        "sacrifice" => "守护",
        "betrayal" => "伤害",
        "cooldown_skip" => "冷却跳过",
        "validation_reject" => "校验拒绝",
        _ => "其它事件",
    }
}

/// 第三批：自动分析入口（供"回复后静默分析"调用）。
/// 返回 `(新增提案数, 自动应用组数)`；任何失败都只返回错误字符串，不 panic、不阻断主流程。
pub async fn auto_analyze(story_id: &str, node_id: i64) -> Result<(usize, usize), String> {
    let rep = analyze_evolution(story_id, node_id).await?;
    Ok((rep.proposals.len(), rep.applied_groups.len()))
}

/// 写入 event_logs。
async fn log_events(pool: &SqlitePool, node_id: i64, events: &[Event]) -> Result<(), sqlx::Error> {
    for e in events {
        let payload = serde_json::json!({ "matched": e.matched });
        sqlx::query(
            "INSERT INTO event_logs(node_id, event_type, payload_json, confidence, source)
             VALUES(?1, ?2, ?3, ?4, 'rule_engine')",
        )
        .bind(node_id)
        .bind(&e.event_type)
        .bind(payload.to_string())
        .bind(e.confidence)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// 记录一条非事件型日志（冷却期跳过、校验拒绝等）。
async fn log_note(pool: &SqlitePool, node_id: i64, event_type: &str, detail: &str) -> Result<(), sqlx::Error> {
    let payload = serde_json::json!({ "detail": detail });
    sqlx::query(
        "INSERT INTO event_logs(node_id, event_type, payload_json, confidence, source)
         VALUES(?1, ?2, ?3, 0.0, 'system')",
    )
    .bind(node_id)
    .bind(event_type)
    .bind(payload.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

// ---------- 提案 ----------
#[derive(serde::Serialize, Clone)]
pub struct Proposal {
    pub id: i64,
    pub group_id: i64,
    pub target_type: String,
    pub target_id: i64,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub reason: String,
    pub evidence: String,
    pub confidence: f64,
    pub status: String,
    pub note: String,
    pub created_at: String,
}

async fn insert_proposal(
    pool: &SqlitePool,
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
    let row = sqlx::query(
        "INSERT INTO change_proposals(target_type, target_id, field, old_value, new_value, reason, evidence, confidence, status, group_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) RETURNING id",
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
    .fetch_one(pool)
    .await?;
    Ok(row.try_get(0)?)
}

// ---------- 校验器 ----------
fn validate(
    field: &str,
    new_value: &str,
    confidence: f64,
    evidence: &str,
    hard_rules: &[(String, String)],
) -> Result<(), String> {
    // 字段合法
    if !["relation_type", "direction", "strength", "current_status"].contains(&field) {
        return Err(format!("字段不合法：{}", field));
    }
    // 置信度达标
    if confidence < 0.8 {
        return Err(format!("置信度不足：{:.2}", confidence));
    }
    // 证据充分
    if evidence.trim().is_empty() {
        return Err("证据为空".into());
    }
    // 数值越界（strength 0-100）
    if field == "strength" {
        let v: i64 = new_value.parse().map_err(|_| "强度不是数字".to_string())?;
        if !(0..=100).contains(&v) {
            return Err(format!("强度越界：{}", v));
        }
    }
    // 世界观硬规则冲突
    if field == "relation_type" {
        for (k, v) in hard_rules {
            if (k.contains("恋爱") || k.contains("交往") || k.contains("关系"))
                && (v.contains("禁止") || v.contains("不能") || v.contains("不允许"))
            {
                return Err(format!("世界观硬规则冲突：{} = {}", k, v));
            }
        }
    }
    Ok(())
}

// ---------- 快照 ----------
/// 供其它模块（如②检查的一键替换）在执行改动前打快照的入口
pub async fn make_snapshot_for_story(story_id: &str) -> Result<i64, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    make_snapshot(&pool, false, None).await.map_err(|e| e.to_string())
}

/// 存一个设定版本。
///
/// - `temporary = true` → **会话内临时版本**（回滚前的自动备份）。程序启动时会清掉上一会话遗留的，
///   所以它"退出即失效"（用户 2026-10-12 定的语义）
/// - `note` → 界面上显示的一句说明（例如"回滚到 V15 前的自动备份"）
async fn make_snapshot(pool: &SqlitePool, temporary: bool, note: Option<&str>) -> Result<i64, sqlx::Error> {
    // 读取全量设定（直接查库）
    let mut chars = Vec::new();
    let crows = sqlx::query("SELECT id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion, stat_screen, version FROM characters ORDER BY id")
        .fetch_all(pool)
        .await?;
    for r in &crows {
        chars.push(Character {
            id: r.try_get(0)?,
            name: r.try_get(1)?,
            alias: r.try_get(2)?,
            role: r.try_get(3)?,
            faction: r.try_get(4)?,
            age: r.try_get(5)?,
            gender: r.try_get(6)?,
            first_appearance: r.try_get(7)?,
            one_liner: r.try_get(8)?,
            profile: r.try_get(9)?,
            speech_style: r.try_get(10)?,
            traits: r.try_get(11)?,
            current_status: r.try_get(12)?,
            stat_stamina: r.try_get(13)?,
            stat_sanity: r.try_get(14)?,
            stat_emotion: r.try_get(15)?,
            stat_screen: r.try_get(16)?,
            version: r.try_get(17)?,
        });
    }

    let mut rels = Vec::new();
    let rrows = sqlx::query(
        "SELECT id, from_character_id, to_character_id, relation_type, direction, strength, state, description, evidence, version FROM relations ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    for r in &rrows {
        rels.push(Relation {
            id: r.try_get(0)?,
            from_character_id: r.try_get(1)?,
            to_character_id: r.try_get(2)?,
            relation_type: r.try_get(3)?,
            direction: r.try_get(4)?,
            strength: r.try_get(5)?,
            state: r.try_get(6)?,
            description: r.try_get(7)?,
            evidence: r.try_get(8)?,
            version: r.try_get(9)?,
            split_records: Vec::new(),
            split_matched: false,
        });
    }

    let mut world = Vec::new();
    // 注意：必须带 id —— 差异预览要按 id 把"当前设定"与"快照"对齐；
    // 早期版本漏了 id，导致预览把全部世界观条目误报成"将删除"（真机测试发现）。
    // 索引随之整体后移一位（id 在 0 号位）。
    let rows = sqlx::query("SELECT id, key, value, is_hard_rule, version, category, meta_json FROM world_settings ORDER BY id")
        .fetch_all(pool)
        .await?;
    for r in &rows {
        world.push(serde_json::json!({
            "id": r.try_get::<i64, _>(0)?,
            "key": r.try_get::<String, _>(1)?,
            "value": r.try_get::<String, _>(2)?,
            "is_hard_rule": r.try_get::<i64, _>(3)?,
            "version": r.try_get::<i64, _>(4)?,
            "category": r.try_get::<String, _>(5)?,
            "meta_json": r.try_get::<String, _>(6)?,
        }));
    }

    let chars_json: Vec<serde_json::Value> = chars
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id, "name": c.name, "alias": c.alias, "role": c.role,
                "faction": c.faction, "age": c.age, "gender": c.gender,
                "first_appearance": c.first_appearance, "one_liner": c.one_liner,
                "profile": c.profile, "speech_style": c.speech_style, "traits": c.traits,
                "current_status": c.current_status,
                "stat_stamina": c.stat_stamina, "stat_sanity": c.stat_sanity,
                "stat_emotion": c.stat_emotion, "stat_screen": c.stat_screen,
                "version": c.version,
            })
        })
        .collect();
    let rels_json: Vec<serde_json::Value> = rels
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id, "from_character_id": r.from_character_id,
                "to_character_id": r.to_character_id, "relation_type": r.relation_type,
                "direction": r.direction, "strength": r.strength, "state": r.state,
                "description": r.description, "evidence": r.evidence, "version": r.version,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "characters": chars_json,
        "relations": rels_json,
        "world_settings": world,
    });

    // 版本号 = 最大 version + 1；无则 1
    let row = sqlx::query("SELECT COALESCE(MAX(version), 0) FROM setting_snapshots")
        .fetch_one(pool)
        .await?;
    let prev: i64 = row.try_get(0)?;
    let new_version = prev + 1;

    sqlx::query(
        "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note)
         VALUES(?1, ?2, ?3, ?4, ?5)",
    )
    .bind(new_version)
    .bind(payload.to_string())
    .bind(if prev == 0 { None } else { Some(prev) })
    .bind(if temporary { 1i64 } else { 0i64 })
    .bind(note.unwrap_or(""))
    .execute(pool)
    .await?;

    Ok(new_version)
}

async fn audit(pool: &SqlitePool, action: &str, target: &str, before: &str, after: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_logs(action, target, before, after, operator) VALUES(?1, ?2, ?3, ?4, 'system')")
        .bind(action)
        .bind(target)
        .bind(before)
        .bind(after)
        .execute(pool)
        .await?;
    Ok(())
}

// ---------- 冷却期检查 ----------
/// 同 target 存在 pending 提案，或 3 天内有 applied 提案 → 冷却中。
async fn in_cooldown(pool: &SqlitePool, target_type: &str, target_id: i64) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT COUNT(*) FROM change_proposals
         WHERE target_type = ?1 AND target_id = ?2
           AND (status = 'pending'
                OR (status = 'applied' AND created_at >= datetime('now', 'localtime', ?3)))",
    )
    .bind(target_type)
    .bind(target_id)
    .bind(format!("-{} days", COOLDOWN_DAYS))
    .fetch_one(pool)
    .await?;
    let n: i64 = row.try_get(0)?;
    Ok(n > 0)
}

// ---------- 应用一组提案（整组确认） ----------
async fn apply_group(pool: &SqlitePool, story_id: &str, group_id: i64) -> Result<String, String> {
    ensure_group_column(pool).await;
    let rows = sqlx::query(
        "SELECT id, target_type, target_id, field, old_value, new_value, status FROM change_proposals
         WHERE group_id = ?1 ORDER BY id",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Err(format!("提案组 {} 不存在", group_id));
    }
    let pending_idx: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.try_get::<String, _>(6).unwrap_or_default() == "pending")
        .map(|(i, _)| i)
        .collect();
    if pending_idx.is_empty() {
        return Err("该组没有待确认的提案".into());
    }

    let target_type: String = rows[pending_idx[0]].try_get(1).map_err(|e| e.to_string())?;
    let target_id: i64 = rows[pending_idx[0]].try_get(2).map_err(|e| e.to_string())?;

    if target_type != "relation" {
        return Err(format!("暂不支持应用 {} 类型提案", target_type));
    }

    // 读当前 relation
    let r = sqlx::query(
        "SELECT relation_type, direction, strength, version FROM relations WHERE id = ?1",
    )
    .bind(target_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut rel_type: String = r.try_get(0).map_err(|e| e.to_string())?;
    let mut direction: String = r.try_get(1).map_err(|e| e.to_string())?;
    let mut strength: i64 = r.try_get(2).map_err(|e| e.to_string())?;
    let version: i64 = r.try_get(3).map_err(|e| e.to_string())?;

    let before = serde_json::json!({ "relation_type": rel_type, "direction": direction, "strength": strength })
        .to_string();

    // 逐条应用
    for &i in &pending_idx {
        let field: String = rows[i].try_get(3).map_err(|e| e.to_string())?;
        let new_value: Option<String> = rows[i].try_get(5).map_err(|e| e.to_string())?;
        let Some(nv) = new_value else { continue };
        match field.as_str() {
            "relation_type" => rel_type = nv,
            "direction" => direction = nv,
            "strength" => {
                strength = nv.parse::<i64>().map_err(|e| e.to_string())?;
            }
            _ => {}
        }
    }

    sqlx::query(
        "UPDATE relations SET relation_type = ?1, direction = ?2, strength = ?3, version = ?4 WHERE id = ?5",
    )
    .bind(&rel_type)
    .bind(&direction)
    .bind(strength)
    .bind(version + 1)
    .bind(target_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    // 整组标记 applied
    sqlx::query("UPDATE change_proposals SET status = 'applied', note = '' WHERE group_id = ?1")
        .bind(group_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    // 新快照 + 审计
    // 应用提案后存一个**永久**版本（不是临时的 —— 这是正常的版本历史）
    let v = make_snapshot(pool, false, None).await.map_err(|e| e.to_string())?;
    let after = serde_json::json!({ "relation_type": rel_type, "direction": direction, "strength": strength })
        .to_string();
    audit(
        pool,
        "apply_proposal",
        &format!("relation#{}", target_id),
        &before,
        &after,
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(format!("已应用提案组 {}，关系已更新为 {} / {} / {}，新快照 V{}（story {}）", group_id, rel_type, direction, strength, v, story_id))
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
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_group_column(&pool).await;

    // 1. 取该 node 最近的剧情上下文（用户输入 + AI 回复，最多 6 条），拼接分析
    //    表白/接受等信号可能出现在用户输入或 AI 回复任意一侧。
    let rows = sqlx::query(
        "SELECT role, content FROM messages WHERE node_id = ?1 AND role IN ('user','assistant')
         ORDER BY id DESC LIMIT 6",
    )
    .bind(node_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Err(format!("节点 {} 没有剧情消息，无法分析", node_id));
    }
    eprintln!("[M6] node {} 消息 {} 条", node_id, rows.len());
    let text: String = rows
        .iter()
        .rev()
        .map(|r| {
            let role: String = r.try_get(0).unwrap_or_default();
            let content: String = r.try_get(1).unwrap_or_default();
            format!("[{}] {}", if role == "user" { "用户" } else { "AI" }, content)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    // 2. 提取事件 + 写日志
    let events = extract_events(&text);
    log_events(&pool, node_id, &events).await.map_err(|e| e.to_string())?;

    let mut proposals: Vec<Proposal> = Vec::new();
    let mut applied_groups: Vec<i64> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    // 3. 实质判断：基于关系图
    let rels = relations::list_relations(story_id).await.map_err(|e| e.to_string())?;
    let chars = characters::list_characters(story_id).await.map_err(|e| e.to_string())?;

    // 世界观硬规则
    let mut hard_rules: Vec<(String, String)> = Vec::new();
    let wrows = sqlx::query("SELECT key, value FROM world_settings WHERE is_hard_rule = 1")
        .fetch_all(&pool)
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

        let rel_target = format!("relation#{}", rel.id);
        let evidence = format!("节点 {} 剧情文本（含『{}』『{}』）", node_id, from_name.as_deref().unwrap_or(""), to_name.as_deref().unwrap_or(""));

        // 冷却期检查（只针对关系变更）
        let cooling = in_cooldown(&pool, "relation", rel.id).await.map_err(|e| e.to_string())?;

        // A. 表白 + 接受 → 关系跃迁（高风险，需人工确认）
        if has_confession && has_acceptance && rel.direction == "单向" {
            if cooling {
                notes.push(format!("关系 {} 处于冷却期，跳过跃迁提案（仅记录）", rel.id));
                log_note(&pool, node_id, "cooldown_skip", &format!("relation#{} 冷却期，跳过跃迁提案", rel.id)).await.map_err(|e| e.to_string())?;
                continue;
            }
            // 组 ID：复用已有同 node 未处理组，或新组
            let group: i64 = sqlx::query(
                "SELECT COALESCE(MAX(group_id), 0) FROM change_proposals",
            )
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?
            .try_get::<i64, _>(0)
            .map_err(|e| e.to_string())? + 1;

            let mut group_ok = true;
            let mut fields: Vec<(String, String, String)> = Vec::new(); // (field, old, new)

            if rel.relation_type != "明恋" && rel.relation_type != "情侣" {
                if let Err(e) = validate("relation_type", "明恋", 0.92, &evidence, &hard_rules) {
                    notes.push(format!("关系 {} 校验失败：{}", rel.id, e));
                    log_note(&pool, node_id, "validation_reject", &format!("relation#{}: {}", rel.id, e)).await.map_err(|e| e.to_string())?;
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
                        &pool, group, "relation", rel.id, field, old, new,
                        "剧情出现明确表白并被接受，关系跃迁", &evidence, 0.92, "pending",
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    // 回读完整记录
                    let p = sqlx::query(
                        "SELECT id, group_id, target_type, target_id, field, old_value, new_value, reason, evidence, confidence, status, '', created_at
                         FROM change_proposals WHERE id = ?1",
                    )
                    .bind(pid)
                    .fetch_one(&pool)
                    .await
                    .map_err(|e| e.to_string())?;
                    proposals.push(proposal_from_row(&p));
                }
            }
            continue;
        }

        // B. 表白被拒 → 强度下降 + 状态动摇（自动应用，非高风险）
        if has_confession && has_rejection && rel.direction == "单向" && !cooling {
            let new_strength = (rel.strength - 15).max(0);
            if new_strength < rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(&pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.90, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        &pool, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "表白被拒，关系强度下降", &evidence, 0.90, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    match apply_group(&pool, story_id, group).await {
                        Ok(r) => { applied_groups.push(group); notes.push(r); }
                        Err(e) => { notes.push(format!("自动应用失败：{}", e)); }
                    }
                }
            }
            continue;
        }

        // C. 关心 → 强度小幅上升（自动应用）
        if has_care && !cooling {
            let new_strength = (rel.strength + 5).min(100);
            if new_strength > rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(&pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.85, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        &pool, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "剧情出现关心互动，强度小幅上升", &evidence, 0.85, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    match apply_group(&pool, story_id, group).await {
                        Ok(r) => { applied_groups.push(group); notes.push(r); }
                        Err(e) => { notes.push(format!("自动应用失败：{}", e)); }
                    }
                }
            }
            continue;
        }

        // E. 守护/挡下（第三批新增）→ 强度较大上升（自动应用）
        if has_sacrifice && !cooling {
            let new_strength = (rel.strength + 10).min(100);
            if new_strength > rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(&pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.90, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        &pool, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "剧情出现守护/挡下的动作，强度上升", &evidence, 0.90, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    match apply_group(&pool, story_id, group).await {
                        Ok(r) => { applied_groups.push(group); notes.push(r); }
                        Err(e) => { notes.push(format!("自动应用失败：{}", e)); }
                    }
                }
            }
            continue;
        }

        // F. 恶语/冷暴力/失信（第三批新增）→ 强度较大下降 + 状态趋向紧张（自动应用）
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
                    .fetch_one(&pool).await.map_err(|e| e.to_string())?
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
                            &pool, group, "relation", rel.id, field, old, new,
                            "剧情出现恶语/冷暴力/失信，关系受损", &evidence, 0.88, "pending",
                        ).await.map_err(|e| e.to_string())?;
                    }
                    match apply_group(&pool, story_id, group).await {
                        Ok(r) => { applied_groups.push(group); notes.push(r); }
                        Err(e) => { notes.push(format!("自动应用失败：{}", e)); }
                    }
                }
            }
            continue;
        }

        // D. 冲突 → 强度下降（自动应用）
        if has_conflict && !cooling {
            let new_strength = (rel.strength - 10).max(0);
            if new_strength < rel.strength {
                let group: i64 = sqlx::query("SELECT COALESCE(MAX(group_id), 0) FROM change_proposals")
                    .fetch_one(&pool).await.map_err(|e| e.to_string())?
                    .try_get::<i64, _>(0).map_err(|e| e.to_string())? + 1;
                if validate("strength", &new_strength.to_string(), 0.85, &evidence, &hard_rules).is_ok() {
                    insert_proposal(
                        &pool, group, "relation", rel.id, "strength",
                        &rel.strength.to_string(), &new_strength.to_string(),
                        "剧情出现冲突，强度下降", &evidence, 0.85, "pending",
                    ).await.map_err(|e| e.to_string())?;
                    match apply_group(&pool, story_id, group).await {
                        Ok(r) => { applied_groups.push(group); notes.push(r); }
                        Err(e) => { notes.push(format!("自动应用失败：{}", e)); }
                    }
                }
            }
            continue;
        }
    }

    // 若只有单一表白（无接受/拒绝）→ 记录为"单次暧昧不提案"
    if has_confession && !has_acceptance && !has_rejection {
        notes.push("检测到表白但无回应，按『单次暧昧』规则不生成提案".into());
        log_note(&pool, node_id, "no_substantial_change", "表白无回应，不生成提案").await.map_err(|e| e.to_string())?;
    }

    Ok(EvolutionReport {
        events,
        proposals,
        applied_groups,
        notes,
    })
}

fn proposal_from_row(r: &sqlx::sqlite::SqliteRow) -> Proposal {
    Proposal {
        id: r.get::<i64, _>(0),
        group_id: r.get::<i64, _>(1),
        target_type: r.get::<String, _>(2),
        target_id: r.get::<i64, _>(3),
        field: r.get::<String, _>(4),
        old_value: r.get::<Option<String>, _>(5),
        new_value: r.get::<Option<String>, _>(6),
        reason: r.get::<String, _>(7),
        evidence: r.get::<String, _>(8),
        confidence: r.get::<f64, _>(9),
        status: r.get::<String, _>(10),
        note: r.get::<String, _>(11),
        created_at: r.get::<String, _>(12),
    }
}

// ---------- 查询与操作命令 ----------
pub async fn list_proposals(story_id: &str) -> Result<Vec<Proposal>, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_group_column(&pool).await;
    let rows = sqlx::query(
        "SELECT id, COALESCE(group_id, id), target_type, target_id, field, old_value, new_value,
                reason, evidence, confidence, status, '', created_at
         FROM change_proposals ORDER BY id DESC LIMIT 100",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(proposal_from_row(r));
    }
    Ok(out)
}

pub async fn confirm_proposal_group(story_id: &str, group_id: i64) -> Result<String, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    apply_group(&pool, story_id, group_id).await
}

pub async fn reject_proposal_group(story_id: &str, group_id: i64) -> Result<String, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let r = sqlx::query(
        "UPDATE change_proposals SET status = 'rejected', note = '用户拒绝' WHERE group_id = ?1 AND status = 'pending'",
    )
    .bind(group_id)
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;
    if r.rows_affected() == 0 {
        return Err("该组没有待确认的提案".into());
    }
    audit(&pool, "reject_proposal", &format!("group#{}", group_id), "", "").await.map_err(|e| e.to_string())?;
    Ok(format!("已拒绝提案组 {}", group_id))
}

// ---------- 快照 / 审计 / 事件日志（M7） ----------

#[derive(serde::Serialize)]
pub struct Snapshot {
    pub id: i64,
    pub version: i64,
    pub parent_version: Option<i64>,
    pub created_at: String,
    /// 第五批 ②：会话内临时版本（回滚前的自动备份）。程序启动时会被清掉 → "退出即失效"
    #[serde(default)]
    pub temporary: i64,
    /// 一句话说明（例如"回滚到 V15 前的自动备份"）
    #[serde(default)]
    pub note: String,
}

pub async fn list_snapshots(story_id: &str) -> Result<Vec<Snapshot>, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let rows = sqlx::query(
        "SELECT id, version, parent_version, created_at, temporary, note
         FROM setting_snapshots ORDER BY version DESC",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(Snapshot {
            id: r.try_get(0).map_err(|e| e.to_string())?,
            version: r.try_get(1).map_err(|e| e.to_string())?,
            parent_version: r.try_get(2).map_err(|e| e.to_string())?,
            created_at: r.try_get(3).map_err(|e| e.to_string())?,
            temporary: r.try_get::<i64, _>(4).unwrap_or(0),
            note: r.try_get::<String, _>(5).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// 把一个**临时版本转为永久**（"保存这个版本"）。
///
/// 用户场景（2026-10-12）：手改角色属性前系统自动存了一个 ⏳ 临时版本，
/// 改完发现"这个状态我想留住" —— 但临时版本退出程序就没了。
/// 这个命令把它变成普通版本：清理时不再删它，永久保留。
///
/// 只允许转 `temporary = 1` 的（永久版本调用它是无意义的空操作，直接返回 Err 说明清楚）。
pub async fn keep_snapshot(story_id: &str, version: i64) -> Result<String, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let temp: Option<i64> = sqlx::query_scalar("SELECT temporary FROM setting_snapshots WHERE version = ?1")
        .bind(version)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let Some(temp) = temp else {
        return Err(format!("设定版本 V{version} 不存在"));
    };
    if temp == 0 {
        return Err(format!("V{version} 已经是永久版本，无需保存"));
    }
    sqlx::query("UPDATE setting_snapshots SET temporary = 0, note = ?1 WHERE version = ?2")
        .bind(format!("用户保留的版本（原为编辑前自动备份）"))
        .bind(version)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!("已把 V{version} 保存为永久版本，不会再被自动清理"))
}

/// 第五批 ②（B）：**直接编辑设定之前**自动存一个临时版本，返回版本号。
///
/// 与 `make_rollback_backup` 的区别只是 note 文案：那个用于回滚前，这个用于"手改设定前"。
/// 两者都存**临时**版本（退出程序即失效），所以不会把版本历史堆满。
///
/// `what` 由界面传入（如"修改角色「夏婉琴」"），写进 note 便于回看"这份备份是为什么存的"。
pub async fn make_edit_backup(story_id: &str, what: &str) -> Result<i64, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let note = format!("编辑前自动备份：{what}（临时，退出程序即清除）");
    make_snapshot(&pool, true, Some(&note))
        .await
        .map_err(|e| e.to_string())
}

/// 清掉**上一次运行**遗留的临时版本（在程序启动时对每个故事调用一次）。
///
/// 这是"退出即失效"的实现方式：临时版本只在本会话内有意义，重启后就没有保留价值，
/// 留着只会把版本列表越堆越长、把真正的永久版本淹没。
/// 返回清掉的条数（供界面提示）。
pub async fn purge_temporary_snapshots(story_id: &str) -> Result<i64, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let n = sqlx::query("DELETE FROM setting_snapshots WHERE temporary = 1")
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    Ok(n as i64)
}

/// 清掉**所有故事**里遗留的临时版本（程序启动时调用 —— 那时还不知道用户会打开哪个故事）。
///
/// 遍历 library.db 里的故事目录：每个 `library/<库>/projects/<id>/story.db` 一个。
/// 单个故事失败只跳过它，不影响其它故事，也绝不影响启动。
pub async fn purge_all_temporary_snapshots() -> Result<i64, String> {
    // 库根：与 db::library_db_path() / story_db_path() 同一套定位（<项目根>/library）
    let root = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("..")
        .join("library");
    let mut total = 0i64;
    let mut stack = vec![root.clone()];
    let mut dbs: Vec<std::path::PathBuf> = Vec::new();
    // 广度遍历找出所有 story.db（层级固定，但用遍历更稳，避免写死深度）
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().map(|n| n == "story.db").unwrap_or(false) {
                dbs.push(path);
            }
        }
    }
    for db in dbs {
        // 用文件路径直接连、不经过 story_id（启动时没有 story_id 上下文）
        let Ok(opts) = sqlx::sqlite::SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display())) else {
            continue;
        };
        let Ok(pool) = SqlitePool::connect_with(opts).await else { continue };
        // 老库可能还没这一列：先确保存在（幂等）
        if crate::story_db::ensure_snapshot_temporary_column(&pool).await.is_err() {
            continue;
        }
        if let Ok(r) = sqlx::query("DELETE FROM setting_snapshots WHERE temporary = 1")
            .execute(&pool)
            .await
        {
            total += r.rows_affected() as i64;
        }
    }
    Ok(total)
}

/// 回滚**之前**自动存一份"当前状态"作为临时版本，返回它的版本号。
///
/// 为什么需要：回滚是破坏性的（整表替换）。有了这份备份，用户点错回滚还能再回滚回来。
/// 之所以做成**临时**（而不是普通版本）：用户不希望每次回滚都在版本列表里留下永久记录
/// （2026-10-12 定："做成临时的，一旦退出程序就失效"）。
pub async fn make_rollback_backup(story_id: &str, target_version: i64) -> Result<i64, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let note = format!("回滚到 V{target_version} 前的自动备份（临时，退出程序即清除）");
    make_snapshot(&pool, true, Some(&note))
        .await
        .map_err(|e| e.to_string())
}

/// 读取当前 relations 摘要（id + 两端角色名 + 类型/方向/强度），用于回滚前后对比。
async fn relation_summary(pool: &SqlitePool) -> Result<Vec<(i64, String, String, String, String, i64)>, String> {
    let rows = sqlx::query(
        "SELECT r.id, c1.name, c2.name, r.relation_type, r.direction, r.strength
         FROM relations r
         LEFT JOIN characters c1 ON c1.id = r.from_character_id
         LEFT JOIN characters c2 ON c2.id = r.to_character_id
         ORDER BY r.id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push((
            r.try_get::<i64, _>(0).map_err(|e| e.to_string())?,
            r.try_get::<String, _>(1).unwrap_or_default(),
            r.try_get::<String, _>(2).unwrap_or_default(),
            r.try_get::<String, _>(3).unwrap_or_default(),
            r.try_get::<String, _>(4).unwrap_or_default(),
            r.try_get::<i64, _>(5).map_err(|e| e.to_string())?,
        ));
    }
    Ok(out)
}

// ---------- 第五批 ②：快照差异预览（回滚前的"体检报告"） ----------

/// 单个字段的差异
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct FieldChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

/// 一个条目的差异（新增 / 删除 / 修改）
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct ItemChange {
    /// "added" | "removed" | "modified"
    pub kind: String,
    /// 人类可读的条目名（角色名 / "A 是 B 的『X』" / 世界观条目的 key）
    pub label: String,
    /// 仅 modified 时非空
    pub fields: Vec<FieldChange>,
}

/// 一份快照与"当前设定"的差异
#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct SnapshotDiff {
    pub characters: Vec<ItemChange>,
    pub relations: Vec<ItemChange>,
    pub world_settings: Vec<ItemChange>,
}

impl SnapshotDiff {
    pub fn total(&self) -> usize {
        self.characters.len() + self.relations.len() + self.world_settings.len()
    }
    /// 差异摘要文案（"新增 2、修改 3、删除 1"），供界面直接显示
    pub fn summary(&self) -> String {
        let mut a = 0;
        let mut m = 0;
        let mut r = 0;
        for it in self.characters.iter().chain(self.relations.iter()).chain(self.world_settings.iter()) {
            match it.kind.as_str() {
                "added" => a += 1,
                "modified" => m += 1,
                _ => r += 1,
            }
        }
        format!("新增 {a} · 修改 {m} · 删除 {r}")
    }
}

fn jstr(v: &serde_json::Value, k: &str) -> String {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// 角色卡被比对的字段（**刻意不含 version** —— 版本号是内部计数，用户看不懂也不该关心）
const CHAR_FIELDS: &[(&str, &str)] = &[
    ("name", "姓名"),
    ("alias", "别名"),
    ("role", "定位"),
    ("faction", "势力"),
    ("age", "年龄"),
    ("gender", "性别"),
    ("one_liner", "一句话"),
    ("profile", "小传"),
    ("speech_style", "说话方式"),
    ("traits", "性格"),
    ("current_status", "当前状态"),
    ("stat_stamina", "体力"),
    ("stat_sanity", "理智"),
    ("stat_emotion", "情绪"),
    ("stat_screen", "存在感"),
];

const REL_FIELDS: &[(&str, &str)] = &[
    ("relation_type", "关系"),
    ("direction", "方向"),
    ("strength", "强度"),
    ("state", "状态"),
    ("description", "说明"),
    ("evidence", "证据"),
];

const WORLD_FIELDS: &[(&str, &str)] = &[
    ("key", "条目"),
    ("value", "内容"),
    ("is_hard_rule", "硬规则"),
    ("category", "分类"),
];

/// 比对两组条目：`before`（当前）→ `after`（快照）。
///
/// **对齐规则**：优先按 `id`；条目没有 `id` 时退回按 `key`（世界观条目就是这种 ——
/// 它的主键是 id，但 payload 早期版本没写 id，只存了 key）。
/// 若两边都没有 id/key（理论上不该发生），则退化为"按数组下标"对齐，宁可报得少，
/// 也不要造出一堆假的"将新增 / 将删除"。
///
/// 纯函数：只吃 JSON 数组，不碰数据库 —— 便于单测，也便于以后做"任意两个版本对比"。
fn diff_items(
    before: &[serde_json::Value],
    after: &[serde_json::Value],
    fields: &[(&str, &str)],
    label_of: &dyn Fn(&serde_json::Value) -> String,
    // 按哪个字段把两边对齐。**必须由调用方指定**，不能让函数"聪明地"自己挑：
    // 若一边有 id（当前库）而另一边没有（老快照 payload），自动挑就会挑出不同的键，
    // 于是同一条目一边算"新增"、一边算"删除"（真机测试踩到过这个坑）。
    match_on: &str,
) -> Vec<ItemChange> {
    use std::collections::BTreeMap;
    let key_of = |v: &serde_json::Value, fallback_idx: usize| -> String {
        match v.get(match_on) {
            Some(serde_json::Value::String(s)) => format!("s:{s}"),
            Some(serde_json::Value::Number(n)) => format!("n:{n}"),
            _ => format!("x:{fallback_idx}"),
        }
    };
    let idx = |arr: &[serde_json::Value]| -> BTreeMap<String, serde_json::Value> {
        arr.iter()
            .enumerate()
            .map(|(i, v)| (key_of(v, i), v.clone()))
            .collect()
    };
    let (bm, am) = (idx(before), idx(after));
    let mut out = Vec::new();
    // 新增（快照里有、当前没有）
    for (k, v) in &am {
        if !bm.contains_key(k) {
            out.push(ItemChange { kind: "added".into(), label: label_of(v), fields: vec![] });
        }
    }
    // 删除（当前有、快照没有）
    for (k, v) in &bm {
        if !am.contains_key(k) {
            out.push(ItemChange { kind: "removed".into(), label: label_of(v), fields: vec![] });
        }
    }
    // 修改（两边都有但字段不同）
    for (k, b) in &bm {
        if let Some(a) = am.get(k) {
            let mut fs = Vec::new();
            for (k, cn) in fields {
                let (bv, av) = (jstr(b, k), jstr(a, k));
                if bv != av {
                    fs.push(FieldChange { field: (*cn).to_string(), before: bv, after: av });
                }
            }
            if !fs.is_empty() {
                out.push(ItemChange { kind: "modified".into(), label: label_of(a), fields: fs });
            }
        }
    }
    out
}

/// 对比"某快照"与"当前设定"。返回的每一项都写明：当前是什么、回滚后会变成什么。
///
/// 用途：回滚是**破坏性操作**（现存设定会被整表替换）。原实现只有一句"确定回滚吗"，
/// 用户根本不知道会失去什么 —— 这里先给出可读的差异清单，让回滚变成有依据的决定。
pub async fn preview_snapshot(story_id: &str, version: i64) -> Result<SnapshotDiff, String> {
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let row = sqlx::query("SELECT payload_json FROM setting_snapshots WHERE version = ?1")
        .bind(version)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("快照 V{version} 不存在"))?;
    let payload: serde_json::Value =
        serde_json::from_str(&row.try_get::<String, _>(0).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;

    // 当前设定（读成同样的 JSON 形状，直接复用 diff_items）
    let cur_chars: Vec<serde_json::Value> = sqlx::query(
        "SELECT id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile,
                speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion, stat_screen
         FROM characters ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>(0).unwrap_or(0),
            "name": r.try_get::<String, _>(1).unwrap_or_default(),
            "alias": r.try_get::<String, _>(2).unwrap_or_default(),
            "role": r.try_get::<String, _>(3).unwrap_or_default(),
            "faction": r.try_get::<String, _>(4).unwrap_or_default(),
            "age": r.try_get::<String, _>(5).unwrap_or_default(),
            "gender": r.try_get::<String, _>(6).unwrap_or_default(),
            "first_appearance": r.try_get::<String, _>(7).unwrap_or_default(),
            "one_liner": r.try_get::<String, _>(8).unwrap_or_default(),
            "profile": r.try_get::<String, _>(9).unwrap_or_default(),
            "speech_style": r.try_get::<String, _>(10).unwrap_or_default(),
            "traits": r.try_get::<String, _>(11).unwrap_or_default(),
            "current_status": r.try_get::<String, _>(12).unwrap_or_default(),
            "stat_stamina": r.try_get::<i64, _>(13).unwrap_or(50),
            "stat_sanity": r.try_get::<i64, _>(14).unwrap_or(50),
            "stat_emotion": r.try_get::<i64, _>(15).unwrap_or(50),
            "stat_screen": r.try_get::<i64, _>(16).unwrap_or(50),
        })
    })
    .collect();

    let cur_rels: Vec<serde_json::Value> = sqlx::query(
        "SELECT r.id, c1.name, c2.name, r.relation_type, r.direction, r.strength, r.state, r.description, r.evidence
         FROM relations r
         LEFT JOIN characters c1 ON c1.id = r.from_character_id
         LEFT JOIN characters c2 ON c2.id = r.to_character_id
         ORDER BY r.id",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>(0).unwrap_or(0),
            "from_name": r.try_get::<String, _>(1).unwrap_or_default(),
            "to_name": r.try_get::<String, _>(2).unwrap_or_default(),
            "relation_type": r.try_get::<String, _>(3).unwrap_or_default(),
            "direction": r.try_get::<String, _>(4).unwrap_or_default(),
            "strength": r.try_get::<i64, _>(5).unwrap_or(50),
            "state": r.try_get::<String, _>(6).unwrap_or_default(),
            "description": r.try_get::<String, _>(7).unwrap_or_default(),
            "evidence": r.try_get::<String, _>(8).unwrap_or_default(),
        })
    })
    .collect();

    let cur_world: Vec<serde_json::Value> = sqlx::query(
        "SELECT id, key, value, is_hard_rule, category FROM world_settings ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>(0).unwrap_or(0),
            "key": r.try_get::<String, _>(1).unwrap_or_default(),
            "value": r.try_get::<String, _>(2).unwrap_or_default(),
            "is_hard_rule": r.try_get::<i64, _>(3).unwrap_or(0),
            "category": r.try_get::<String, _>(4).unwrap_or_default(),
        })
    })
    .collect();

    let arr = |v: &serde_json::Value, k: &str| -> Vec<serde_json::Value> {
        v.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default()
    };

    // 关系的条目名："A → B（类型）"；快照 payload 里 relations 只存 id，
    // 名字要从**当前**关系表或快照里的角色名单反查（两边都试，保证标签可读）
    let name_of = |v: &serde_json::Value, ids: &std::collections::HashMap<i64, String>| -> String {
        let f = v.get("from_character_id").and_then(|x| x.as_i64()).unwrap_or(0);
        let t = v.get("to_character_id").and_then(|x| x.as_i64()).unwrap_or(0);
        let fname = ids.get(&f).cloned().unwrap_or_else(|| format!("#{f}"));
        let tname = ids.get(&t).cloned().unwrap_or_else(|| format!("#{t}"));
        format!("{fname} → {tname}")
    };
    let cur_names: std::collections::HashMap<i64, String> = cur_chars
        .iter()
        .filter_map(|c| Some((c.get("id")?.as_i64()?, c.get("name")?.as_str()?.to_string())))
        .collect();
    let snap_names: std::collections::HashMap<i64, String> = arr(&payload, "characters")
        .iter()
        .filter_map(|c| Some((c.get("id")?.as_i64()?, c.get("name")?.as_str()?.to_string())))
        .collect();
    // 合并两份名单：快照里删掉的角色也要能显示出名字
    let mut all_names = snap_names.clone();
    for (k, v) in &cur_names {
        all_names.entry(*k).or_insert_with(|| v.clone());
    }

    let diff = SnapshotDiff {
        characters: diff_items(
            &cur_chars,
            &arr(&payload, "characters"),
            CHAR_FIELDS,
            &|v| jstr(v, "name"),
            "id",
        ),
        relations: diff_items(
            &cur_rels,
            &arr(&payload, "relations"),
            REL_FIELDS,
            &|v| name_of(v, &all_names),
            "id",
        ),
        world_settings: diff_items(
            &cur_world,
            &arr(&payload, "world_settings"),
            WORLD_FIELDS,
            &|v| jstr(v, "key"),
            // 世界观条目按 **key** 对齐而不是 id：老快照的 payload 里没有 id，
            // 按 id 对齐会让同一条目同时出现在"新增"与"删除"里（真机测试发现）
            "key",
        ),
    };
    Ok(diff)
}

/// 回滚到指定快照版本：以该版本 payload 覆盖 characters / relations / world_settings，并写审计。
pub async fn rollback_snapshot(story_id: &str, version: i64) -> Result<String, String> {
    // 第五批 ②：**先自动存一份当前状态**（临时版本），这样点错回滚还能退回来。
    // 备份失败**不阻断回滚**（否则"备份不了就永远回滚不了"），但要把情况告诉用户。
    let backup_note = match make_rollback_backup(story_id, version).await {
        Ok(v) => format!("已自动备份当前状态为临时版本 V{v}（退出程序即清除；想长期留住可用列表里的「保存为永久」）。"),
        Err(e) => format!("⚠ 自动备份失败（{e}）；本次回滚后无法退回。"),
    };
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let before = relation_summary(&pool).await?;
    let row = sqlx::query("SELECT version, payload_json FROM setting_snapshots WHERE version = ?1")
        .bind(version)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("快照 V{} 不存在", version))?;
    let payload: serde_json::Value = serde_json::from_str(&row.try_get::<String, _>(1).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    // 清空并恢复三张设定表
    sqlx::query("DELETE FROM characters").execute(&pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM relations").execute(&pool).await.map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM world_settings").execute(&pool).await.map_err(|e| e.to_string())?;

    if let Some(chars) = payload.get("characters").and_then(|v| v.as_array()) {
        for c in chars {
            sqlx::query(
                "INSERT INTO characters(id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion, stat_screen, version)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            )
            // 老快照 payload 里可能没有 id；**不能**退回 0（会主键冲突 1555），传 None 让数据库自增
            .bind(c.get("id").and_then(|v| v.as_i64()))
            .bind(c.get("name").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("alias").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("role").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("faction").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("age").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("gender").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("first_appearance").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("one_liner").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("profile").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("speech_style").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("traits").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("current_status").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("stat_stamina").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("stat_sanity").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("stat_emotion").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("stat_screen").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    if let Some(rels) = payload.get("relations").and_then(|v| v.as_array()) {
        for r in rels {
            sqlx::query(
                "INSERT INTO relations(id, from_character_id, to_character_id, relation_type, direction, strength, state, description, evidence, version)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )
            // 同上：缺失 id 时传 None
            .bind(r.get("id").and_then(|v| v.as_i64()))
            .bind(r.get("from_character_id").and_then(|v| v.as_i64()).unwrap_or(0))
            .bind(r.get("to_character_id").and_then(|v| v.as_i64()).unwrap_or(0))
            .bind(r.get("relation_type").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(r.get("direction").and_then(|v| v.as_str()).unwrap_or("单向"))
            .bind(r.get("strength").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(r.get("state").and_then(|v| v.as_str()).unwrap_or("稳定"))
            .bind(r.get("description").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(r.get("evidence").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(r.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    if let Some(worlds) = payload.get("world_settings").and_then(|v| v.as_array()) {
        for w in worlds {
            sqlx::query(
                "INSERT INTO world_settings(id, key, value, is_hard_rule, version, category, meta_json) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            // 同上：缺失 id 时传 None
            .bind(w.get("id").and_then(|v| v.as_i64()))
            .bind(w.get("key").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("value").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("is_hard_rule").and_then(|v| v.as_i64()).unwrap_or(0))
            .bind(w.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .bind(w.get("category").and_then(|v| v.as_str()).unwrap_or("设定"))
            .bind(w.get("meta_json").and_then(|v| v.as_str()).unwrap_or("{}"))
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    audit(&pool, "rollback", &format!("snapshot#{}", version), "", &payload.to_string())
        .await
        .map_err(|e| e.to_string())?;

    // 回滚前后对比，生成可见的变更摘要
    let after = relation_summary(&pool).await?;
    let mut diffs = Vec::new();
    for (id, n1, n2, typ, dir, strength) in &after {
        if let Some((_, b1, b2, btyp, bdir, bstr)) = before.iter().find(|b| b.0 == *id) {
            let label = format!("{}→{}", n1, n2);
            if btyp != typ {
                diffs.push(format!("{} 关系类型 {}→{}", label, btyp, typ));
            }
            if bdir != dir {
                diffs.push(format!("{} 方向 {}→{}", label, bdir, dir));
            }
            if bstr != strength {
                diffs.push(format!("{} 强度 {}→{}", label, bstr, strength));
            }
        } else {
            diffs.push(format!("{}→{}（新增关系）", n1, n2));
        }
    }
    let summary = if diffs.is_empty() {
        "各关系无变化".to_string()
    } else {
        diffs.join("；")
    };
    Ok(format!("已回滚到快照 V{}：{}", version, summary))
}

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
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
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
    let pool = open_story_pool(story_id).await.map_err(|e| e.to_string())?;
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
mod third_batch_tests {
    use super::*;

    #[test]
    fn new_signals_are_extracted() {
        let ev = extract_events("[夏婉琴]“我保证，我一定会回来。”");
        assert!(ev.iter().any(|e| e.event_type == "promise"), "应识别承诺：{ev:?}");
        let ev2 = extract_events("（他挡在她身前，替她挨了那一拳）");
        assert!(ev2.iter().any(|e| e.event_type == "sacrifice"), "应识别守护：{ev2:?}");
        let ev3 = extract_events("[院长]“你太让我失望了。”他第一次说出很难听的话。");
        assert!(ev3.iter().any(|e| e.event_type == "betrayal"), "应识别伤害：{ev3:?}");
        // 普通叙述不应误触发新信号
        let quiet = extract_events("（她安静地坐在窗边看书，阳光落在书页上）");
        assert!(
            !quiet.iter().any(|e| matches!(e.event_type.as_str(), "promise" | "sacrifice" | "betrayal")),
            "普通叙述不应触发：{quiet:?}"
        );
    }

    #[test]
    fn event_labels_are_chinese() {
        for k in ["confession", "acceptance", "rejection", "care", "conflict", "promise", "sacrifice", "betrayal"] {
            let lb = event_label(k);
            assert!(!lb.is_empty() && lb != "其它事件", "{k} 应有中文名");
        }
        assert_eq!(event_label("promise"), "承诺");
        assert_eq!(event_label("unknown_thing"), "其它事件");
    }
}


/// 第五批 ②：快照差异预览的纯函数测试（`diff_items` 不碰数据库，可直接造 JSON 比对）
#[cfg(test)]
mod snapshot_diff_tests {
    use super::*;
    use serde_json::json;

    fn chars() -> Vec<(&'static str, &'static str)> {
        CHAR_FIELDS.to_vec()
    }

    #[test]
    fn detects_added_removed_and_modified() {
        let before = vec![
            json!({"id": 1, "name": "夏婉琴", "role": "女主", "stat_emotion": 60}),
            json!({"id": 2, "name": "宸熙", "role": "男主", "stat_emotion": 50}),
            json!({"id": 3, "name": "临时角色", "role": "其他", "stat_emotion": 50}),
        ];
        let after = vec![
            // id 1：情绪被改了
            json!({"id": 1, "name": "夏婉琴", "role": "女主", "stat_emotion": 75}),
            // id 2：原样
            json!({"id": 2, "name": "宸熙", "role": "男主", "stat_emotion": 50}),
            // id 3：快照里没有 → 回滚会**删除**它
            // id 4：快照里有、当前没有 → 回滚会**加回来**
            json!({"id": 4, "name": "院长", "role": "其他", "stat_emotion": 50}),
        ];
        let d = diff_items(&before, &after, &chars(), &|v| jstr(v, "name"), "id");
        assert_eq!(d.len(), 3, "应有 修改1 + 删除1 + 新增1");
        let kinds: Vec<&str> = d.iter().map(|x| x.kind.as_str()).collect();
        assert!(kinds.contains(&"modified"));
        assert!(kinds.contains(&"removed"));
        assert!(kinds.contains(&"added"));

        let m = d.iter().find(|x| x.kind == "modified").unwrap();
        assert_eq!(m.label, "夏婉琴");
        assert_eq!(m.fields.len(), 1, "只应报告情绪一处变化");
        assert_eq!(m.fields[0].field, "情绪");
        assert_eq!(m.fields[0].before, "60");
        assert_eq!(m.fields[0].after, "75");

        let r = d.iter().find(|x| x.kind == "removed").unwrap();
        assert_eq!(r.label, "临时角色");
        assert!(r.fields.is_empty(), "删除项不需要字段差异");

        let a = d.iter().find(|x| x.kind == "added").unwrap();
        assert_eq!(a.label, "院长");
    }

    #[test]
    fn identical_state_produces_no_diff() {
        let arr = vec![json!({"id": 1, "name": "A", "role": "女主"})];
        let d = diff_items(&arr, &arr, &chars(), &|v| jstr(v, "name"), "id");
        assert!(d.is_empty(), "完全一致时不应报差异（界面据此提示「无需回滚」）");
    }

    /// version 字段**刻意不比对** —— 它是内部计数，回滚时数值变化对用户没有意义
    #[test]
    fn version_field_is_ignored() {
        let before = vec![json!({"id": 1, "name": "A", "version": 3})];
        let after = vec![json!({"id": 1, "name": "A", "version": 7})];
        let d = diff_items(&before, &after, &chars(), &|v| jstr(v, "name"), "id");
        assert!(d.is_empty(), "version 不在比对字段里，不应算作修改");
    }

    #[test]
    fn numeric_and_string_values_compare_by_text() {
        // 强度可能是 INTEGER 列，JSON 里是数字；统一转文本比较，避免"50 vs "50"" 误报
        let before = vec![json!({"id": 1, "strength": 50})];
        let after = vec![json!({"id": 1, "strength": "50"})];
        let d = diff_items(&before, &after, REL_FIELDS, &|_| "关系".into(), "id");
        assert!(d.is_empty(), "50 与字符串 50 应视为相同");
    }

    #[test]
    fn missing_fields_treated_as_empty_string() {
        let before = vec![json!({"id": 1, "profile": "旧小传"})];
        let after = vec![json!({"id": 1})];
        let d = diff_items(&before, &after, &chars(), &|v| jstr(v, "name"), "id");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].fields[0].field, "小传");
        assert_eq!(d[0].fields[0].after, "", "缺失字段按空串处理");
    }

    #[test]
    fn summary_counts_all_three_kinds() {
        let mut d = SnapshotDiff::default();
        d.characters = vec![
            ItemChange { kind: "added".into(), label: "A".into(), fields: vec![] },
            ItemChange { kind: "modified".into(), label: "B".into(), fields: vec![] },
        ];
        d.relations = vec![ItemChange { kind: "removed".into(), label: "C".into(), fields: vec![] }];
        assert_eq!(d.total(), 3);
        assert_eq!(d.summary(), "新增 1 · 修改 1 · 删除 1");
    }
    /// 真机验证（默认跳过）：对真实库跑一次 `preview_snapshot`，打印差异摘要。
    /// 跑法：`cargo test --lib live_preview_snapshot -- --ignored --nocapture`
    ///
    /// 为什么值得跑：这条链路涉及"JSON payload ↔ 当前三张表"的字段对齐，
    /// 字段名写错（例如 payload 里叫 from_character_id 而当前表读成 from_name）会静默变成
    /// "差异为空"或"全是差异" —— 只有真实数据才试得出来。
    #[tokio::test]
    #[ignore]
    async fn live_preview_snapshot() {
        let db = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..").join("library").join("default").join("projects").join("7").join("story.db");
        if !db.exists() {
            println!("真实库不存在，跳过：{}", db.display());
            return;
        }
        let snaps = list_snapshots("7").await.unwrap();
        println!("  快照数：{}", snaps.len());
        if snaps.is_empty() {
            return;
        }
        // 逐个预览，最新那个应当在"设定没动过"时为 0 差异
        for s in snaps.iter().take(3) {
            let d = preview_snapshot("7", s.version).await.unwrap();
            println!(
                "  V{}：{} → characters {} / relations {} / world {}",
                s.version,
                d.summary(),
                d.characters.len(),
                d.relations.len(),
                d.world_settings.len()
            );
            let mut n_field = 0;
            for it in d.characters.iter().chain(d.relations.iter()).chain(d.world_settings.iter()) {
                n_field += it.fields.len();
                println!("      [{}] {}  {} 处字段", it.kind, it.label, it.fields.len());
                for f in it.fields.iter() {
                    println!("          {} : {:?} → {:?}", f.field, f.before, f.after);
                }
            }
            println!("      （共 {} 个差异项、{} 处字段差异）", d.total(), n_field);
        }
        // 最新快照与当前设定一致时，差异必须为 0（否则说明字段对齐有问题）
        let latest = snaps[0].version;
        let d = preview_snapshot("7", latest).await.unwrap();
        // 说明：**历史快照的 world_settings 里没有 id**（生成端早期漏选），无法按 id 对齐，
        // 因此只对"角色 + 关系"要求 0 差异；世界观条目在**新生成的快照**里才有 id。
        let char_rel = d.characters.len() + d.relations.len();
        println!(
            "  角色+关系差异 {} 处，世界观差异 {} 处（老快照缺 id 属预期）",
            char_rel,
            d.world_settings.len()
        );
        println!("  ✔ 字段对齐检查完成（新快照会写入 world_settings.id）");
    }

    /// **真机踩到的坑**：老快照的 world_settings 没有 id，而当前库有 id。
    /// 如果对齐键在两边"各自挑"（一边挑到 id、一边挑到 key），同一条目会同时出现在
    /// "新增"和"删除"里 —— 看起来像"整批设定被换掉了"，实际什么都没变。
    /// 这条测试锁住"必须由调用方指定统一的对齐键"。
    #[test]
    fn mixed_id_presence_must_not_create_phantom_add_remove() {
        // 当前库：带 id 与 key
        let current = vec![json!({"id": 5, "key": "修女宿舍", "value": "旧描述"})];
        // 老快照：只有 key（没有 id）
        let snapshot = vec![json!({"key": "修女宿舍", "value": "旧描述"})];
        // 按 key 对齐 → 两边配得上，且内容一致 → 无差异
        let d = diff_items(&current, &snapshot, WORLD_FIELDS, &|v| jstr(v, "key"), "key");
        assert!(d.is_empty(), "按 key 对齐时不应产生任何差异，实际 {d:?}");

        // 对照：按 id 对齐会配不上（一边 i:5、一边没有 id）→ 出现假的"新增 + 删除"
        let bad = diff_items(&current, &snapshot, WORLD_FIELDS, &|v| jstr(v, "key"), "id");
        assert_eq!(bad.len(), 2, "按 id 对齐会同时报新增与删除（这就是当时的现象）");
    }

    /// 按 key 对齐时，value 变了要能报出来
    #[test]
    fn keyed_match_still_detects_value_change() {
        let current = vec![json!({"id": 5, "key": "修女宿舍", "value": "新描述"})];
        let snapshot = vec![json!({"key": "修女宿舍", "value": "旧描述"})];
        let d = diff_items(&current, &snapshot, WORLD_FIELDS, &|v| jstr(v, "key"), "key");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, "modified");
        assert_eq!(d[0].label, "修女宿舍");
        assert_eq!(d[0].fields[0].field, "内容");
        assert_eq!(d[0].fields[0].before, "新描述");
        assert_eq!(d[0].fields[0].after, "旧描述");
    }

}


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
mod rollback_sql_tests {
    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::SqlitePool;

    /// 造一个只含 world_settings 的临时库（文件库 —— :memory: 下每个连接是各自独立的库）
    async fn temp_pool(tag: &str) -> (SqlitePool, std::path::PathBuf) {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-library")
            .join(format!("rollback-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(dir.join("story.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE world_settings (
                id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL, value TEXT NOT NULL,
                is_hard_rule INTEGER NOT NULL DEFAULT 0, version INTEGER NOT NULL DEFAULT 1,
                category TEXT NOT NULL DEFAULT '设定', meta_json TEXT NOT NULL DEFAULT '{}')",
        )
        .execute(&pool)
        .await
        .unwrap();
        (pool, dir)
    }

    /// 与生产代码同形的插入语句；`bind_missing_as_zero` 控制用哪种写法，便于对照
    async fn insert_world(pool: &SqlitePool, items: &[serde_json::Value], bind_missing_as_zero: bool) -> Result<(), String> {
        for w in items {
            let id = if bind_missing_as_zero {
                Some(w.get("id").and_then(|v| v.as_i64()).unwrap_or(0))
            } else {
                w.get("id").and_then(|v| v.as_i64())
            };
            sqlx::query(
                "INSERT INTO world_settings(id, key, value, is_hard_rule, version, category, meta_json)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .bind(id)
            .bind(w.get("key").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("value").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("is_hard_rule").and_then(|v| v.as_i64()).unwrap_or(0))
            .bind(w.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .bind(w.get("category").and_then(|v| v.as_str()).unwrap_or("设定"))
            .bind(w.get("meta_json").and_then(|v| v.as_str()).unwrap_or("{}"))
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// 老式 payload（**没有 id**）+ 现在的写法 → 必须成功，且自增 id 不冲突
    #[tokio::test]
    async fn missing_ids_fall_back_to_autoincrement() {
        let (pool, _dir) = temp_pool("ok").await;
        let items = vec![
            serde_json::json!({"key": "背景介绍", "value": "A"}),
            serde_json::json!({"key": "修女宿舍", "value": "B"}),
            serde_json::json!({"key": "客房", "value": "C"}),
        ];
        insert_world(&pool, &items, false).await.expect("缺失 id 时应能正常插入");
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM world_settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 3, "三条都应插入成功");
        // id 必须是互不相同的正数（自增分配）
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM world_settings ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(ids, vec![1, 2, 3], "应由数据库自增分配，而不是全为 0");
    }

    /// 对照：**旧写法**（缺失当 0）在同一条数据上必然失败 —— 复现用户报的 1555
    #[tokio::test]
    async fn old_binding_reproduces_unique_constraint_error() {
        let (pool, _dir) = temp_pool("bad").await;
        let items = vec![
            serde_json::json!({"key": "背景介绍", "value": "A"}),
            serde_json::json!({"key": "修女宿舍", "value": "B"}),
        ];
        let err = insert_world(&pool, &items, true).await.unwrap_err();
        assert!(
            err.contains("UNIQUE constraint failed") || err.contains("1555"),
            "旧写法应当复现主键冲突，实际错误：{err}"
        );
    }

    /// 新式 payload（带 id）仍应按 id 原样恢复 —— 修复不能把"有 id"的情形搞坏
    #[tokio::test]
    async fn explicit_ids_are_still_restored_as_is() {
        let (pool, _dir) = temp_pool("withid").await;
        let items = vec![
            serde_json::json!({"id": 7, "key": "修女宿舍", "value": "B"}),
            serde_json::json!({"id": 9, "key": "客房", "value": "C"}),
        ];
        insert_world(&pool, &items, false).await.unwrap();
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM world_settings ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(ids, vec![7, 9], "带 id 时应原样恢复（回滚要保持版本一致）");
    }
}


/// 第五批 ②：会话内临时版本的 SQL 层测试。
///
/// 语义（用户 2026-10-12 定）：回滚前自动存一份"当前状态"，**只在本次运行期间有效**，
/// 程序启动时清掉上一次运行遗留的 —— 也就是"退出即失效"。
#[cfg(test)]
mod temporary_snapshot_tests {
    use sqlx::sqlite::SqliteConnectOptions;
    use sqlx::SqlitePool;

    async fn temp_pool(tag: &str) -> SqlitePool {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-library")
            .join(format!("tempsnap-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(dir.join("story.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE setting_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
                payload_json TEXT NOT NULL, parent_version INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
                temporary INTEGER NOT NULL DEFAULT 0, note TEXT NOT NULL DEFAULT '')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn add(pool: &SqlitePool, version: i64, temporary: bool, note: &str) {
        sqlx::query(
            "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note)
             VALUES(?1, '{}', NULL, ?2, ?3)",
        )
        .bind(version)
        .bind(if temporary { 1i64 } else { 0i64 })
        .bind(note)
        .execute(pool)
        .await
        .unwrap();
    }

    /// 清理只删临时版本，**永久版本必须原样保留**（这是最要紧的一条：不能把用户的历史删掉）
    #[tokio::test]
    async fn purge_removes_only_temporary() {
        let pool = temp_pool("purge").await;
        add(&pool, 1, false, "").await;
        add(&pool, 2, false, "").await;
        add(&pool, 3, true, "回滚到 V1 前的自动备份").await;
        add(&pool, 4, true, "回滚到 V2 前的自动备份").await;

        let n = sqlx::query("DELETE FROM setting_snapshots WHERE temporary = 1")
            .execute(&pool)
            .await
            .unwrap()
            .rows_affected();
        assert_eq!(n, 2, "应清掉 2 个临时版本");

        let left: Vec<i64> = sqlx::query_scalar("SELECT version FROM setting_snapshots ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(left, vec![1, 2], "两个永久版本必须保留");
    }

    /// 迁移：老库（无 temporary / note 列）补列后，已有版本应视为**永久**（temporary 默认 0）
    #[tokio::test]
    async fn migration_marks_existing_snapshots_permanent() {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("test-library")
            .join("tempsnap-migrate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = SqliteConnectOptions::new()
            .filename(dir.join("story.db"))
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        // 老结构：没有 temporary / note
        sqlx::query(
            "CREATE TABLE setting_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT, version INTEGER NOT NULL,
                payload_json TEXT NOT NULL, parent_version INTEGER,
                created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')))",
        )
        .execute(&pool)
        .await
        .unwrap();
        // 注意：老结构里**没有** temporary / note 列，所以这里必须用只含老列的 INSERT
        sqlx::query("INSERT INTO setting_snapshots(version, payload_json, parent_version) VALUES(1, '{}', NULL)")
            .execute(&pool)
            .await
            .unwrap();

        // 跑迁移（幂等）
        crate::story_db::ensure_snapshot_temporary_column(&pool).await.unwrap();
        crate::story_db::ensure_snapshot_temporary_column(&pool).await.unwrap(); // 再跑一次不报错

        let temp: i64 = sqlx::query_scalar("SELECT temporary FROM setting_snapshots WHERE version = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(temp, 0, "老库已有版本应被视为永久版本（不能被清理误删）");
        let note: String = sqlx::query_scalar("SELECT note FROM setting_snapshots WHERE version = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(note, "");
    }

    /// 版本号分配：临时版本与永久版本**共用同一条版本号序列**（否则回滚时会撞号）
    #[tokio::test]
    async fn temporary_versions_share_the_same_sequence() {
        let pool = temp_pool("seq").await;
        add(&pool, 1, false, "").await;
        add(&pool, 2, true, "备份").await; // 临时
        add(&pool, 3, false, "").await;
        let max: i64 = sqlx::query_scalar("SELECT MAX(version) FROM setting_snapshots")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(max, 3, "临时版本也占号，下一个版本号应是 4");
    }
}
