// 从 `evolution.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `evolution.rs` 里的其余条目（子模块本来就能访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `use analyze::*;` 取回，
// 外部 `crate::evolution::xxx` 的调用路径**全部不变**。

use super::*;

// ---------- 事件信号词典 ----------
pub(super) const CONFESSION: &[&str] = &["我喜欢你", "我爱你", "喜欢你很久", "做我女朋友", "做我男朋友", "跟我交往", "我们在一起吧", "向你表白"];

pub(super) const ACCEPTANCE: &[&str] = &["我也喜欢你", "我也喜欢", "我早就喜欢你", "我也是", "我愿意", "当然愿意", "我答应你", "我答应", "我们在一起吧", "我们在一起", "早就是了", "我也"];

pub(super) const REJECTION: &[&str] = &["我不喜欢你", "我们还是朋友吧", "我们还是朋友", "我拒绝你"];

pub(super) const CARE: &[&str] = &["关心", "递伞", "挡雨", "照顾", "扶住", "担心你", "心疼"];

pub(super) const CONFLICT: &[&str] = &["争吵", "吵架", "冷战", "推开", "不理她", "不理他", "生气地说", "摔门"];

// ===== 第三批新增信号（2026-10-11）=====
// 设计原则：**只加不会误判的强信号** —— 都是明确的剧情动作，避免"关心/照顾"这类泛词导致提案泛滥。
// 各信号用途见 analyze_evolution：
//   promise   → 一次明确的承诺/约定：只记事件，不自动改动数值
//   sacrifice → 为对方挡下/站出来：关系强度较大上升
//   betrayal  → 恶语、冷暴力、失信：关系强度较大下降 + 状态趋向紧张
pub(super) const PROMISE_WORDS: &[&str] = &["我答应你", "我保证", "我发誓", "我会一直", "我永远", "等你回来", "说到做到", "放心交给我"];

pub(super) const SACRIFICE_WORDS: &[&str] = &["挡在她身前", "挡在他身前", "挡在身前", "护住她", "护住他", "替她挨", "替他挨", "挺身而出", "护在身后"];

pub(super) const BETRAYAL_WORDS: &[&str] = &["恶语", "冷暴力", "失信", "骗了她", "骗了他", "背叛", "说出很难听", "翻脸"];

pub(super) const COOLDOWN_DAYS: i64 = 3;

/// 确保 change_proposals 有 group_id / note 列（老库迁移；新库建表时已带）。
pub(super) async fn ensure_group_column(pool: &SqlitePool) {
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

pub(super) fn contains_any(text: &str, words: &[&str]) -> Option<String> {
    for w in words {
        if text.contains(w) {
            return Some(w.to_string());
        }
    }
    None
}

/// 规则引擎：从一段 AI 剧情回复中提取候选事件（每个信号类型最多一条）。
pub(super) fn extract_events(text: &str) -> Vec<Event> {
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
// 保留未用：事件标签词典（提案展示用，保留）
#[allow(dead_code)]
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
pub(super) async fn log_events(pool: &SqlitePool, node_id: i64, events: &[Event]) -> Result<(), sqlx::Error> {
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
pub(super) async fn log_note(pool: &SqlitePool, node_id: i64, event_type: &str, detail: &str) -> Result<(), sqlx::Error> {
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
