//! 日结时的「新实体抽取」（2026-11-05，3.15）。
//!
//! 为什么放在日结：它本来就要读当天全文，而且**每天只跑一次** —— 和「她的告白」同一个理由。
//! 让主对话每一轮都抽，等于每轮多一句提示词 + 多一次判断，而"今天有没有出现新人物/新地点"
//! 一天最多变一次。
//!
//! 边界（三条）：
//!   ① 模型只输出协议行，**不进摘要正文**（解析后立刻从摘要里剥掉）；
//!   ② 抽到的东西**只入队、不落库**（SECURITY.md 红线 2：AI 内容改库必须人工拍板）；
//!   ③ 已有同名角色 / 同 key 设定 / 同名的待确认提案 → 直接跳过，不重复排队。

use sqlx::SqlitePool;

/// 协议行前缀 → (target_type, world_settings 的 category)
const UPDATE_TAGS: &[(&str, &str)] = &[("〔改角色〕", "character"), ("〔改设定〕", "world_setting")];

const NEW_TAGS: &[(&str, &str, &str)] = &[
    ("〔新角色〕", "character", ""),
    ("〔新地点〕", "world_setting", "地理"),
    ("〔新术语〕", "world_setting", "术语"),
    ("〔世界观〕", "world_setting", "世界书"),
];

/// 一天最多收几条（防模型报菜名，也防一次塞爆审批队列）
pub const MAX_PER_DAY: usize = 6;

/// 「改已有条目」（2026-11-07）：旧名字 → 新内容。
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateEntity {
    pub target_type: &'static str,
    pub name: String,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewEntity {
    pub target_type: &'static str,
    pub name: String,
    pub detail: String,
    pub category: &'static str,
}


/// 解析日结回复里的新增实体行。格式：`〔新角色〕名字：一句话`（冒号全角半角都认）。
pub fn parse_new_entities(text: &str) -> Vec<NewEntity> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        for (tag, ty, cat) in NEW_TAGS {
            let Some(rest) = t.strip_prefix(tag) else { continue };
            let Some((name, detail)) = rest.split_once(['：', ':']) else { continue };
            let name = name.trim().trim_matches(['「', '」', '“', '”', '"']).trim();
            let detail: String = detail.trim().chars().take(80).collect();
            if name.is_empty() || detail.is_empty() {
                continue;
            }
            out.push(NewEntity {
                target_type: ty,
                name: name.to_string(),
                detail,
                category: cat,
            });
            break;
        }
        if out.len() >= MAX_PER_DAY {
            break;
        }
    }
    out
}

/// 把协议行从摘要正文里剥掉（只剥以标记开头的整行）。
/// 解析「改已有条目」行：`〔改角色〕阿婆：她其实是他亲奶奶` / `〔改设定〕西厢房：后来改成了书房`。
pub fn parse_update_entities(text: &str) -> Vec<UpdateEntity> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        for (tag, ty) in UPDATE_TAGS {
            let Some(rest) = t.strip_prefix(tag) else { continue };
            let Some((name, new_text)) = rest.split_once(['：', ':']) else { continue };
            let name = name.trim().trim_matches(['「', '」', '“', '”', '"']).trim();
            let new_text: String = new_text.trim().chars().take(80).collect();
            if name.is_empty() || new_text.is_empty() {
                continue;
            }
            out.push(UpdateEntity { target_type: ty, name: name.to_string(), new_text });
            break;
        }
        if out.len() >= MAX_PER_DAY {
            break;
        }
    }
    out
}
pub fn strip_new_entities(text: &str) -> String {
    text.lines()
        .filter(|l| {
            let t = l.trim();
            !NEW_TAGS.iter().any(|(tag, _, _)| t.starts_with(tag))
                && !UPDATE_TAGS.iter().any(|(tag, _)| t.starts_with(tag))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// 载荷（`change_proposals.new_value`）：角色用 name/role/one_liner，设定用 key/value/category。
pub fn payload_json(e: &NewEntity) -> String {
    if e.target_type == "character" {
        serde_json::json!({
            "name": e.name,
            "one_liner": e.detail,
            "role": "",
        })
    } else {
        serde_json::json!({
            "key": e.name,
            "value": e.detail,
            "category": e.category,
            "is_hard_rule": false,
        })
    }
    .to_string()
}

/// 置信度（2026-11-07）：按**在这一天原文里出现的次数**给分 —— 提一次 0.55，提得多更可信（最高 0.85）。
/// 模型给不出概率，就用"它被提了几次"这个可数的证据。
pub fn confidence_for(name: &str, transcript: &str) -> f64 {
    let hits = transcript.matches(name).count().min(3);
    0.55 + 0.1 * hits as f64
}

/// 把抽到的实体**入队**（不落库）。返回实际入队的条数。
///
/// 去重三路：已有同名角色 / 已有同 key 设定 / 已有同名待确认提案。
pub async fn queue_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    entities: &[NewEntity],
    transcript: &str,
) -> Result<usize, String> {
    if entities.is_empty() {
        return Ok(0);
    }
    let group: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(group_id), 0) + 1 FROM change_proposals")
        .fetch_one(pool)
        .await
        .unwrap_or(1);
    let mut queued = 0usize;
    for e in entities {
        let (exists, pending_mark) = if e.target_type == "character" {
            let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE name = ?1")
                .bind(&e.name)
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            (n > 0, format!("%{}%", e.name))
        } else {
            let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM world_settings WHERE key = ?1")
                .bind(&e.name)
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            (n > 0, format!("%{}%", e.name))
        };
        if exists {
            continue;
        }
        let dup: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM change_proposals WHERE status = 'pending' AND new_value LIKE ?1",
        )
        .bind(&pending_mark)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        if dup > 0 {
            continue;
        }
        crate::evolution::insert_proposal(
            pool,
            branch,
            node_id,
            group,
            e.target_type,
            0,
            "new",
            "",
            &payload_json(e),
            &format!("{day_label}出现的{}{}", if e.target_type == "character" { "新人物：" } else { "新设定：" }, e.name),
            &format!("{day_label}的剧情里提到「{}」", e.name),
            confidence_for(&e.name, transcript),
            "pending",
        )
        .await
        .map_err(|e| e.to_string())?;
        queued += 1;
    }
    Ok(queued)
}

/// 把「改已有条目」入队（2026-11-07）。返回实际入队的条数。
///
/// 只改**一句话设定**（角色的 one_liner / 设定的 value）—— 这是抽取能可靠拿到的那一档；
/// 更细的字段（身份、关系数值）留给人工。找不到同名条目就跳过（说明这是新东西，不该走「改」）。
pub async fn queue_updates_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    node_id: i64,
    day_label: &str,
    updates: &[UpdateEntity],
    transcript: &str,
) -> Result<usize, String> {
    if updates.is_empty() {
        return Ok(0);
    }
    let group: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(group_id), 0) + 1 FROM change_proposals")
        .fetch_one(pool)
        .await
        .unwrap_or(1);
    let mut queued = 0usize;
    for u in updates {
        let (id, old, field) = if u.target_type == "character" {
            let row: Option<(i64, String)> =
                sqlx::query_as("SELECT id, COALESCE(one_liner,'') FROM characters WHERE name = ?1")
                    .bind(&u.name)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| e.to_string())?;
            match row {
                Some((id, old)) => (id, old, "one_liner"),
                None => continue,
            }
        } else {
            let row: Option<(i64, String)> =
                sqlx::query_as("SELECT id, COALESCE(value,'') FROM world_settings WHERE key = ?1")
                    .bind(&u.name)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| e.to_string())?;
            match row {
                Some((id, old)) => (id, old, "value"),
                None => continue,
            }
        };
        if old.trim() == u.new_text.trim() {
            continue; // 没变，不用提案
        }
        let dup: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM change_proposals WHERE status='pending' AND target_type=?1 AND target_id=?2 AND new_value=?3",
        )
        .bind(u.target_type)
        .bind(id)
        .bind(&u.new_text)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        if dup > 0 {
            continue;
        }
        crate::evolution::insert_proposal(
            pool,
            branch,
            node_id,
            group,
            u.target_type,
            id,
            field,
            &old,
            &u.new_text,
            &format!("{day_label}里「{}」的设定变了", u.name),
            &format!("{day_label}的剧情里提到了新的说法"),
            confidence_for(&u.name, transcript),
            "pending",
        )
        .await
        .map_err(|e| e.to_string())?;
        queued += 1;
    }
    Ok(queued)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn parses_four_kinds_and_caps() {
        let r = "她推门进去。\n〔新角色〕阿婆：院子里晒药的邻居\n〔新地点〕西厢房：常年锁着\n〔新术语〕守夜：每月十五不熄灯";
        let got = parse_new_entities(r);
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(got[0].target_type, "character");
        assert_eq!(got[0].name, "阿婆");
        assert_eq!(got[1].category, "地理");
        assert_eq!(got[2].category, "术语");
    }

    #[test]
    fn strips_protocol_lines_from_summary() {
        let r = "正文一句。\n〔新角色〕阿婆：晒药的\n〔作息〕甲：睡好";
        let s = strip_new_entities(r);
        assert!(s.contains("正文一句"));
        assert!(!s.contains("阿婆"), "协议行不能留在摘要里：{s}");
        assert!(s.contains("〔作息〕"), "别把别的协议行也剥了：{s}");
    }

    #[test]
    fn payload_shape_per_type() {
        let c = NewEntity { target_type: "character", name: "阿婆".into(), detail: "晒药".into(), category: "" };
        let v: serde_json::Value = serde_json::from_str(&payload_json(&c)).unwrap();
        assert_eq!(v["name"], "阿婆");
        assert_eq!(v["one_liner"], "晒药");
        let w = NewEntity { target_type: "world_setting", name: "西厢房".into(), detail: "锁着".into(), category: "地理" };
        let v2: serde_json::Value = serde_json::from_str(&payload_json(&w)).unwrap();
        assert_eq!(v2["key"], "西厢房");
        assert_eq!(v2["category"], "地理");
    }
    #[tokio::test]
    async fn queues_entities_and_skips_duplicates() {
        let dir = crate::paths::test_root()
            .join("new-entity")
            .join(format!("q-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("story.db");
        let opts = sqlx::sqlite::SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display()))
            .unwrap()
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE characters (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, alias TEXT NOT NULL DEFAULT '', role TEXT NOT NULL DEFAULT '', faction TEXT NOT NULL DEFAULT '', age TEXT NOT NULL DEFAULT '', gender TEXT NOT NULL DEFAULT '', first_appearance TEXT NOT NULL DEFAULT '', one_liner TEXT NOT NULL DEFAULT '', profile TEXT NOT NULL DEFAULT '', speech_style TEXT NOT NULL DEFAULT '', traits TEXT NOT NULL DEFAULT '', current_status TEXT NOT NULL DEFAULT '', stat_stamina INTEGER NOT NULL DEFAULT 50, stat_sanity INTEGER NOT NULL DEFAULT 50, stat_emotion INTEGER NOT NULL DEFAULT 50, stat_screen INTEGER NOT NULL DEFAULT 50, background_past TEXT NOT NULL DEFAULT '', motivation TEXT NOT NULL DEFAULT '', goal TEXT NOT NULL DEFAULT '', dialogue_examples TEXT NOT NULL DEFAULT '', behavior_rules TEXT NOT NULL DEFAULT '', appearance TEXT NOT NULL DEFAULT '', version INTEGER NOT NULL DEFAULT 1, branch_id TEXT)",
            "CREATE TABLE world_settings (id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL DEFAULT '', value TEXT NOT NULL DEFAULT '', is_hard_rule INTEGER NOT NULL DEFAULT 0, version INTEGER NOT NULL DEFAULT 1, category TEXT NOT NULL DEFAULT '设定', meta_json TEXT NOT NULL DEFAULT '{}', branch_id TEXT)",
            "CREATE TABLE change_proposals (id INTEGER PRIMARY KEY AUTOINCREMENT, target_type TEXT NOT NULL, target_id INTEGER, field TEXT NOT NULL, old_value TEXT, new_value TEXT, reason TEXT, evidence TEXT, confidence REAL NOT NULL DEFAULT 0.0, status TEXT NOT NULL DEFAULT 'pending', note TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')), group_id INTEGER, branch_id TEXT, node_id INTEGER)",
        ] {
            sqlx::query(sqlx::AssertSqlSafe(ddl)).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO characters(name) VALUES('甲')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO world_settings(key, value) VALUES('西厢房', '锁着')").execute(&pool).await.unwrap();

        let ents = vec![
            NewEntity { target_type: "character", name: "阿婆".into(), detail: "晒药".into(), category: "" },
            NewEntity { target_type: "character", name: "甲".into(), detail: "已存在".into(), category: "" },
            NewEntity { target_type: "world_setting", name: "西厢房".into(), detail: "已存在".into(), category: "地理" },
            NewEntity { target_type: "world_setting", name: "地窖".into(), detail: "藏酒".into(), category: "地理" },
        ];
        assert_eq!(queue_in(&pool, &None, 5, "第 5 天", &ents, "阿婆在门口晒药，阿婆说…").await.unwrap(), 2, "只有阿婆和地窖该入队");
        // 再跑一次：同名的待确认提案已经存在 → 不重复
        assert_eq!(queue_in(&pool, &None, 6, "第 6 天", &ents, "同上").await.unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn parses_update_lines() {
        let r = "〔改角色〕阿婆：她其实是他亲奶奶\n〔改设定〕西厢房：后来改成书房了";
        let got = parse_update_entities(r);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[0].target_type, "character");
        assert_eq!(got[0].name, "阿婆");
        assert!(got[0].new_text.contains("亲奶奶"));
        assert_eq!(got[1].target_type, "world_setting");
    }

    #[test]
    fn strips_update_lines_too() {
        let r = "正文。\n〔改角色〕阿婆：新说法";
        assert_eq!(strip_new_entities(r), "正文。");
    }
}