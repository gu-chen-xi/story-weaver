// 从 `timeline.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `timeline.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use message::*;` 取回，
// 外部 `crate::timeline::xxx` 的调用路径**全部不变**。

use super::*;

#[derive(serde::Serialize, Clone)]
pub struct Message {
    pub id: i64,
    pub node_id: i64,
    pub role: String,
    pub content: String,
    pub mode: String,
    pub created_at: String,
    /// 时段标记（2026-10-29）：早 / 午 / 晚，空串 = 没标。存在 messages.meta 的 JSON 里。
    #[serde(default)]
    pub tod: String,
}


/// 从 messages.meta 的 JSON 里取时段（早/午/晚）；坏数据当没标。
pub fn parse_tod(meta: &str) -> String {
    serde_json::from_str::<serde_json::Value>(meta)
        .ok()
        .and_then(|v| v.get("tod").and_then(|x| x.as_str()).map(|s| s.to_string()))
        .filter(|s| s == "早" || s == "午" || s == "晚")
        .unwrap_or_default()
}

pub async fn list_messages(story_id: &str, node_id: i64) -> Result<Vec<Message>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let rows = sqlx::query(
        "SELECT id, node_id, role, content, mode, created_at, COALESCE(meta,'{}')
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
            tod: parse_tod(r.try_get::<String, _>(6).unwrap_or_default().as_str()),
        });
    }
    Ok(out)
}

/// 消息打标（2026-11-10）：把「这一轮谁在演谁」记进 `meta`，供**角色视角**装配器取料。
///
/// 为什么记在 meta 而不是加列：
/// * meta 已经是 JSON 列（时段就用它），**零迁移**；
/// * 这两个值只是"这一轮的上下文"，不会用来做 WHERE 主键（真要用时 `json_extract` 够使）。
///
/// 规则（都是"少写胜过多写"）：
/// * `tod` 只认早/午/晚（沿用 2026-10-29 的口径），别的值一律不写；
/// * `ai_role` / `my_role` 为空、或等于「自由」时不写 —— 「自由」是界面的"没指定"，不是角色名；
/// * 一个键都没有 → `"{}"`（不写空对象以外的任何东西）。
pub fn message_meta(tod: Option<&str>, ai_role: &str, my_role: &str) -> String {
    let mut m = serde_json::Map::new();
    if let Some(t) = tod {
        if matches!(t, "早" | "午" | "晚") {
            m.insert("tod".into(), serde_json::Value::String(t.to_string()));
        }
    }
    for (key, v) in [("ai_role", ai_role), ("my_role", my_role)] {
        let v = v.trim();
        if !v.is_empty() && v != "自由" {
            m.insert(key.into(), serde_json::Value::String(v.to_string()));
        }
    }
    serde_json::Value::Object(m).to_string()
}

/// 角色视角下，这条消息该不该让「当前这一对」看见？（2026-11-10 建立，2026-11-11 改按**配对**）
///
/// 视角键从"角色"改成"**配对**"（`ai_role` × `my_role`）：a 和 b 聊的、a 和 c 聊的，是两段不同的关系，
/// 不该混进同一个视角（ROADMAP §七 7.8 第 1 条，2026-11-11 定）。
///
/// 规则按优先级：
/// 1. **旁白消息**（`mode = narr`）→ 一律可见（旁白是全局的）；
/// 2. **没有扮演者标记**（`meta` 里既没 `ai_role` 也没 `my_role`，即 2026-11-10 之前的老消息，
///    或 `meta` 坏掉）→ **可见**（"全局可见的兜底"：宁可见也不丢，老数据不猜不补）；
/// 3. 标了 → **配对命中**才可见：`(ai_role, my_role)` 与当前配对相同，或**互换**
///    （同两个人，谁演谁换过也还是同一段关系）；
/// 4. 当前 `my_role` 为空（界面选了「自由」）→ 退回**按角色**判：她出现在任一侧即可见。
pub fn visible_in_role_view(mode: &str, meta_json: &str, ai_role: &str, my_role: &str) -> bool {
    if mode == "narr" {
        return true;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(meta_json) else {
        return true; // 坏 meta 也按"没标记"处理
    };
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
    let (m_ai, m_my) = (get("ai_role"), get("my_role"));
    if m_ai.is_empty() && m_my.is_empty() {
        return true; // 老消息 / 没打标 → 全局兜底
    }
    let (ai, my) = (ai_role.trim(), my_role.trim());
    if ai.is_empty() {
        return false; // 当前没指定她在演谁 → 不猜
    }
    if my.is_empty() {
        // 「自由」模式：没有配对可对，退回按角色判
        return m_ai == ai || m_my == ai;
    }
    (m_ai == ai && m_my == my) || (m_ai == my && m_my == ai)
}

pub async fn create_message(
    story_id: &str,
    node_id: i64,
    role: &str,
    content: &str,
    mode: &str,
    tod: Option<&str>,
    // 2026-11-10：这一轮的扮演者（角色视角装配器的输入；空/「自由」= 不记）
    ai_role: &str,
    my_role: &str,
) -> Result<i64, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    // 第五批 ③：消息带上分支（由节点推导，保证与节点一致）
    let branch: Option<String> = sqlx::query_scalar("SELECT branch_id FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .flatten();
    // 时段 + 扮演者：都走同一个 meta 构造器（见 message_meta）
    let meta = message_meta(tod, ai_role, my_role);
    let row = sqlx::query(
        "INSERT INTO messages(node_id, role, content, mode, branch_id, meta) VALUES(?1, ?2, ?3, ?4, ?5, ?6) RETURNING id"
    )
    .bind(node_id)
    .bind(role)
    .bind(content)
    .bind(mode)
    .bind(branch)
    .bind(meta)
    .fetch_one(&pool)
    .await?;
    Ok(row.try_get(0)?)
}

/// 更新某条消息内容（修改历史剧情）。
/// 第五批：**删除单条消息**（用户实测反馈要的）。
///
/// 用途：请求失败时把**已经入库的那条用户消息**撤掉 ——
/// 否则重发会在历史里留下两份一样的输入（"孤儿用户消息"）。
/// 另外，取消生成留下的半截回复也可以用它清掉。
pub async fn delete_message(story_id: &str, message_id: i64) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    sqlx::query("DELETE FROM messages WHERE id = ?1")
        .bind(message_id)
        .execute(&pool)
        .await?;
    Ok(())
}

pub async fn update_message(story_id: &str, message_id: i64, content: &str) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    sqlx::query("UPDATE messages SET content = ?1 WHERE id = ?2")
        .bind(content)
        .bind(message_id)
        .execute(&pool).await?;
    Ok(())
}

#[cfg(test)]
mod meta_tests {
    use super::message_meta;
    use serde_json::Value;

    fn parsed(tod: Option<&str>, ai: &str, my: &str) -> Value {
        serde_json::from_str(&message_meta(tod, ai, my)).expect("message_meta 必须吐出合法 JSON")
    }

    #[test]
    fn writes_tod_and_both_roles() {
        let v = parsed(Some("早"), "夏婉琴", "顾宸熙");
        assert_eq!(v["tod"], "早");
        assert_eq!(v["ai_role"], "夏婉琴");
        assert_eq!(v["my_role"], "顾宸熙");
        // 不多写：只有这三个键
        assert_eq!(v.as_object().unwrap().len(), 3);
    }

    #[test]
    fn skips_unknown_tod_and_free_roles() {
        // 时段只认早/午/晚（沿用 2026-10-29 口径）
        let v = parsed(Some("深夜"), "夏婉琴", "顾宸熙");
        assert!(v.get("tod").is_none(), "非法时段不该写进去");
        assert_eq!(v["ai_role"], "夏婉琴");

        // 「自由」是界面的"没指定"，不是角色名；空白同理
        let v = parsed(None, "自由", "   ");
        assert!(v.get("ai_role").is_none());
        assert!(v.get("my_role").is_none());
    }

    #[test]
    fn nothing_to_record_is_empty_object() {
        assert_eq!(message_meta(None, "", ""), "{}");
        assert_eq!(message_meta(Some("晚"), "自由", "自由"), r#"{"tod":"晚"}"#);
    }

    #[test]
    fn roles_are_trimmed() {
        let v = parsed(None, "  夏婉琴 ", " 顾宸熙");
        assert_eq!(v["ai_role"], "夏婉琴");
        assert_eq!(v["my_role"], "顾宸熙");
    }
}

#[cfg(test)]
mod role_view_tests {
    use super::visible_in_role_view;

    const MARKED: &str = r#"{"ai_role":"夏婉琴","my_role":"顾宸熙"}"#;

    #[test]
    fn narration_is_always_visible() {
        // 旁白消息在角色视角里也要读得到（ROADMAP §七 7.1）
        assert!(visible_in_role_view("narr", MARKED, "老张头", "王老师"));
        assert!(visible_in_role_view("narr", "{}", "老张头", "王老师"));
    }

    #[test]
    fn unmarked_or_broken_meta_falls_back_to_visible() {
        // 老消息（2026-11-10 之前）没有标记 → 全局兜底，看得见
        assert!(visible_in_role_view("chat", "{}", "夏婉琴", "顾宸熙"));
        assert!(visible_in_role_view("chat", "", "夏婉琴", "顾宸熙"));
        assert!(visible_in_role_view("chat", "not-json", "夏婉琴", "顾宸熙"));
    }

    #[test]
    fn pair_matches_its_own_round() {
        // 这一轮是「夏婉琴 × 顾宸熙」：同一个配对看得见
        assert!(visible_in_role_view("chat", MARKED, "夏婉琴", "顾宸熙"));
        // 互换（她俩谁演谁换过）也算同一段关系
        assert!(visible_in_role_view("chat", MARKED, "顾宸熙", "夏婉琴"));
    }

    #[test]
    fn other_pairs_and_other_people_are_hidden() {
        // 同一对里的另一个人换了 → 不是同一段关系（视角按配对）
        assert!(!visible_in_role_view("chat", MARKED, "夏婉琴", "王老师"));
        // 完全没参与的人
        assert!(!visible_in_role_view("chat", MARKED, "老张头", "王老师"));
        // 当前没指定她在演谁 → 不猜
        assert!(!visible_in_role_view("chat", MARKED, "", "顾宸熙"));
    }

    #[test]
    fn free_mode_falls_back_to_role_matching() {
        // 界面选「自由」时没有配对可对 → 按角色判（她出现在任一侧即可见）
        assert!(visible_in_role_view("chat", MARKED, "夏婉琴", ""));
        assert!(visible_in_role_view("chat", MARKED, "顾宸熙", ""));
        assert!(!visible_in_role_view("chat", MARKED, "老张头", ""));
    }

    #[test]
    fn message_meta_and_role_view_agree() {
        // 用真构造器产出的 meta 反过来验一遍，防止两边格式漂移
        let meta = super::message_meta(None, "夏婉琴", "顾宸熙");
        assert!(visible_in_role_view("chat", &meta, "夏婉琴", "顾宸熙"));
        let only_ai = super::message_meta(None, "夏婉琴", "");
        assert!(visible_in_role_view("chat", &only_ai, "夏婉琴", ""), "自由模式下按角色兜底");
        assert!(!visible_in_role_view("chat", &only_ai, "顾宸熙", "夏婉琴"), "一对一半 = 配对不成立");
    }
}
