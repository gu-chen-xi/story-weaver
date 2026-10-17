//! 第三批 · ④ 评估模型：给 AI 回复打分（用户 2026-10-11 选定方案 A = 评估回复质量）
//!
//! 设计要点：
//! - **评分维度直接对应"软件已有的要求"**，不做主观"文笔好不好"评判：
//!   format（输出排版是否守规矩）/ character（是否出戏、OOC）/ consistency（是否接得住前文）/ focus（是否推进剧情）
//! - **每一维必须给"证据引文"**（quote），避免模型拍脑袋打分；无证据的分数会被软件标注
//! - 复用日摘要/检查那套踩坑经验：**强制 JSON 输出 + 显式关思考**
//! - 一次只评一条回复（可控成本）；评完写 `reply_evaluations` 表，可查询趋势

use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::SqlitePool;


/// 固定四个维度（顺序即界面展示顺序）
pub const DIMENSIONS: &[(&str, &str)] = &[
    ("format", "输出排版"),
    ("character", "角色贴合"),
    ("consistency", "接续前文"),
    ("focus", "剧情推进"),
];

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DimScore {
    /// 维度键：format / character / consistency / focus
    pub key: String,
    /// 1~5 分
    pub score: i64,
    /// 一句话理由
    pub reason: String,
    /// 证据引文（AI 从回复里抄的原句；没有则空）
    #[serde(default)]
    pub quote: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Evaluation {
    pub message_id: i64,
    pub node_id: i64,
    pub model: String,
    pub dims: Vec<DimScore>,
    /// 总分（各维度平均，保留一位小数；**由软件计算，不采信模型自报**）
    pub total: f64,
    pub summary: String,
    pub advice: String,
    pub created_at: String,
}


/// 建表（幂等；每次打开库时调用）
pub async fn ensure_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS reply_evaluations (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            message_id  INTEGER NOT NULL,
            node_id     INTEGER NOT NULL,
            model       TEXT NOT NULL DEFAULT '',
            dims_json   TEXT NOT NULL DEFAULT '[]',
            total       REAL NOT NULL DEFAULT 0,
            summary     TEXT NOT NULL DEFAULT '',
            advice      TEXT NOT NULL DEFAULT '',
            created_at  TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 评分提示词：维度 = 软件已有的要求；每维必须给证据引文。
/// 用原始字符串避免转义问题；注意**不得含全角方括号**（否则会把 Prompt 预览的段落切碎）。
pub fn eval_system_prompt() -> String {
    let body = r#"你是小说写作助手的质检员。下面给你一条 AI 刚生成的剧情回复，请按 4 个维度打分。

打分维度（每维 1~5 分，5 最好）：

1. format 输出排版：是否守规矩 —— 台词必须写成 [角色名]“台词”、动作/神态用（圆括号）独占一行、心声整行用 *星号* 包裹。违反任何一条都不能给 4 分以上。
2. character 角色贴合：说话与行为是否符合该角色的性格、说话方式、当前身份与关系；是否出现出戏（角色知道他不该知道的事、性格突变、替用户角色做决定）。
3. consistency 接续前文：是否接得住上文的场景、时间、人物位置与情绪；有没有与已发生的事矛盾。
4. focus 剧情推进：这一轮是否让剧情有实质前进（新的信息、动作、情绪变化），还是原地打转、只是重复环境描写。

硬性要求：
- 每一维必须给出一条 quote（从回复里原样抄一句作为证据，不超过 40 字）；抄不出证据的维度请给 3 分并在 reason 里说明"未找到明确证据"。
- 不要评价文笔好不好、有没有创意这类主观口味；只按上面 4 条的可核对标准打分。
- 不要因为字数多少打分。
- 只输出 JSON，不要解释、不要用代码块包裹。格式：
{
  "dims": [
    {"key":"format","score":5,"reason":"台词与动作格式均正确","quote":"[夏婉琴]“你回来了。”"},
    {"key":"character","score":4,"reason":"...","quote":"..."},
    {"key":"consistency","score":5,"reason":"...","quote":"..."},
    {"key":"focus","score":3,"reason":"...","quote":"..."}
  ],
  "summary":"一句话总评",
  "advice":"给下一轮的一句话改进建议（可以留空字符串）"
}

顺带说明本项目对关系的标尺（判断 character 时用得到）：
__STRENGTH__
__STATE__"#;
    body.replace("__STRENGTH__", &crate::relations::strength_legend())
        .replace("__STATE__", &crate::relations::state_legend())
}

/// 评估一条 AI 回复并落库。
pub async fn evaluate_message(
    lib_pool: &SqlitePool,
    story_id: &str,
    message_id: i64,
) -> Result<Evaluation, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_table(&pool).await.map_err(|e| e.to_string())?;

    let row = sqlx::query("SELECT node_id, role, content FROM messages WHERE id = ?1")
        .bind(message_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("找不到消息 #{message_id}"))?;
    let node_id: i64 = row.try_get(0).unwrap_or(0);
    let role: String = row.try_get(1).unwrap_or_default();
    let content: String = row.try_get(2).unwrap_or_default();
    if role != "assistant" {
        return Err("只能评估 AI 回复（assistant）".to_string());
    }

    // 上文 = 同节点里这条之前的两条（用户输入 + 再前一条）
    let prev = sqlx::query("SELECT role, content FROM messages WHERE node_id = ?1 AND id < ?2 ORDER BY id DESC LIMIT 2")
        .bind(node_id).bind(message_id)
        .fetch_all(&pool).await.map_err(|e| e.to_string())?;
    let mut ctx_lines: Vec<String> = Vec::new();
    for r in prev.iter().rev() {
        let rr: String = r.try_get(0).unwrap_or_default();
        let cc: String = r.try_get(1).unwrap_or_default();
        if !cc.trim().is_empty() {
            ctx_lines.push(format!("[{}] {}", if rr == "user" { "用户" } else { "AI" }, crate::relations::strip_director(&cc)));
        }
    }
    let day_summary: String = sqlx::query_scalar("SELECT COALESCE(day_summary,'') FROM timeline_nodes WHERE id = ?1")
        .bind(node_id).fetch_optional(&pool).await.map_err(|e| e.to_string())?.unwrap_or_default();

    let base_url = crate::db::get_config(lib_pool, "model.base_url").await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let model = crate::db::get_config(lib_pool, "model.name").await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "deepseek-flash".to_string());
    let api_key = crate::keychain::get_api_key("openai_api_key".to_string()).map_err(|e| e.to_string())?
        .ok_or_else(|| "未设置 API Key".to_string())?;

    let mut user_msg = String::new();
    if !day_summary.trim().is_empty() {
        user_msg.push_str(&format!("本日摘要（供判断接续）：\n{}\n\n", day_summary.trim()));
    }
    if !ctx_lines.is_empty() {
        user_msg.push_str(&format!("上文：\n{}\n\n", ctx_lines.join("\n")));
    }
    user_msg.push_str(&format!("要评估的这条 AI 回复：\n{}", content.chars().take(8000).collect::<String>()));

    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": eval_system_prompt()},
            {"role": "user", "content": user_msg}
        ],
        "max_tokens": 1500,
        "temperature": 0.1,
        "stream": false,
        "response_format": {"type": "json_object"}
    });
    body["thinking"] = serde_json::json!({"type": "disabled"});

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let resp = reqwest::Client::new().post(&url).bearer_auth(&api_key).json(&body).send().await
        .map_err(|e| format!("评估请求失败: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("评估失败 HTTP {}: {}", status.as_u16(), text.chars().take(200).collect::<String>()));
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("评估响应解析失败: {e}"))?;
    let raw = v["choices"][0]["message"]["content"].as_str().unwrap_or_default().trim().to_string();
    let cleaned = raw.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let parsed: serde_json::Value = serde_json::from_str(cleaned)
        .map_err(|e| format!("评估返回的不是合法 JSON: {e}"))?;

    // 归一化：只认固定四维、分数夹到 1~5、缺证据的标注出来
    let mut dims: Vec<DimScore> = Vec::new();
    for (key, _label) in DIMENSIONS {
        let found = parsed["dims"].as_array().and_then(|arr| {
            arr.iter().find(|d| d["key"].as_str().unwrap_or("") == *key)
        });
        let (mut score, reason, quote) = match found {
            Some(d) => (
                d["score"].as_i64().unwrap_or(3),
                d["reason"].as_str().unwrap_or("").trim().to_string(),
                d["quote"].as_str().unwrap_or("").trim().to_string(),
            ),
            None => (3, "模型未给出该维度评分".to_string(), String::new()),
        };
        score = score.clamp(1, 5);
        let reason = if quote.is_empty() {
            format!("{reason}（未提供证据引文，分数仅供参考）")
        } else {
            reason
        };
        dims.push(DimScore { key: (*key).to_string(), score, reason, quote });
    }
    let total = (dims.iter().map(|d| d.score as f64).sum::<f64>() / dims.len() as f64 * 10.0).round() / 10.0;
    let summary = parsed["summary"].as_str().unwrap_or("").trim().to_string();
    let advice = parsed["advice"].as_str().unwrap_or("").trim().to_string();

    let dims_json = serde_json::to_string(&dims).unwrap_or_else(|_| "[]".to_string());
    let new_id: i64 = sqlx::query(
        "INSERT INTO reply_evaluations(message_id, node_id, model, dims_json, total, summary, advice)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )
    .bind(message_id).bind(node_id).bind(&model).bind(&dims_json).bind(total).bind(&summary).bind(&advice)
    .execute(&pool).await.map_err(|e| e.to_string())?.last_insert_rowid();

    let created_at: String = sqlx::query_scalar("SELECT created_at FROM reply_evaluations WHERE id = ?1")
        .bind(new_id)
        .fetch_optional(&pool).await.map_err(|e| e.to_string())?.unwrap_or_default();

    Ok(Evaluation { message_id, node_id, model, dims, total, summary, advice, created_at })
}

/// 列出某故事最近的评估记录（倒序，便于看趋势）
pub async fn list_evaluations(story_id: &str, limit: i64) -> Result<Vec<Evaluation>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    ensure_table(&pool).await.map_err(|e| e.to_string())?;
    let rows = sqlx::query(
        "SELECT message_id, node_id, model, dims_json, total, summary, advice, created_at
         FROM reply_evaluations ORDER BY id DESC LIMIT ?1"
    ).bind(limit.max(1)).fetch_all(&pool).await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        let dims_json: String = r.try_get(3).unwrap_or_else(|_| "[]".to_string());
        out.push(Evaluation {
            message_id: r.try_get(0).unwrap_or(0),
            node_id: r.try_get(1).unwrap_or(0),
            model: r.try_get(2).unwrap_or_default(),
            dims: serde_json::from_str(&dims_json).unwrap_or_default(),
            total: r.try_get(4).unwrap_or(0.0),
            summary: r.try_get(5).unwrap_or_default(),
            advice: r.try_get(6).unwrap_or_default(),
            created_at: r.try_get(7).unwrap_or_default(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_are_fixed_four() {
        assert_eq!(DIMENSIONS.len(), 4);
        let keys: Vec<&str> = DIMENSIONS.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, vec!["format", "character", "consistency", "focus"]);
    }

    #[test]
    fn prompt_demands_evidence_and_forbids_taste_judgement() {
        let p = eval_system_prompt();
        assert!(p.contains("quote"), "必须要求证据引文");
        assert!(p.contains("原样抄"), "证据必须原样抄写");
        assert!(p.contains("不要评价"), "必须禁止口味评判");
        assert!(p.contains("输出排版") && p.contains("角色贴合"), "四个维度都要在提示词里");
    }

    #[test]
    fn total_is_average_of_dimensions() {
        let dims = vec![5i64, 4, 3, 4];
        let total = (dims.iter().map(|d| *d as f64).sum::<f64>() / dims.len() as f64 * 10.0).round() / 10.0;
        assert_eq!(total, 4.0);
    }
}
