//! 第二批 · ② 检查：创建剧情前对设定做审核
//!
//! 分工（2026-10-11 与用户确认）：
//! - **本地规则**（本文件 `struct_*`，零 API 成本）：只查**纯结构问题** —— 字段为空、重名、术语重复、
//!   势力关系指向已删势力、地图坐标重复、关系数值非法（非数字 / 超出 0~100）
//! - **AI 检查**（`check_board_ai`）：查**语法、歧义、以及关系数值与类型是否矛盾**；
//!   提示词里注入「数据字典」（强度档位 + 状态含义）作为统一标尺；
//!   约束：不得找茬、不得提扩充建议、不得评价文笔，只报读不通 / 有歧义 / 与标尺矛盾，并给最小修改
//!
//! 一键替换：AI 输出**约定的 JSON 补丁** → 软件**白名单校验** → 用户二次确认 → 软件执行 → 打快照。
//! 不使用 API 层 tool calling（那需要额外往返，且等于让模型直接写库）。

use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::SqlitePool;


#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CheckItem {
    /// 出问题的条目（角色名 / 设定名 / 术语 …）
    pub where_: String,
    /// 字段或位置（如「人物小传」「关系数值」）
    pub field: String,
    /// 类型：空值 / 重复 / 悬空 / 坐标冲突 / 数值非法 / 语法 / 歧义 / 矛盾
    pub kind: String,
    /// 为什么是问题（给用户看的人话）
    pub why: String,
    /// 建议怎么改（最小修改）
    pub suggestion: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PatchItem {
    /// 板块：characters / world / relations / terms / factions
    pub board: String,
    /// 条目类型：character / world_entry / term / faction / faction_relation / relation
    pub target_kind: String,
    /// 条目定位名
    pub target_name: String,
    /// 字段（英文名，须在白名单内）
    pub field: String,
    /// AI 认为的"当前值"（用于核对，防止 AI 基于错误理解给补丁）
    #[serde(default)]
    pub before: String,
    /// 改好的完整值（必须是完整值，不是半个） 
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct CheckReport {
    /// 板块显示名
    pub board: String,
    /// 是否有问题
    pub has_issue: bool,
    /// 逐条问题
    pub items: Vec<CheckItem>,
    /// 可选的结构化补丁（AI 检查才可能有）
    pub patch: Vec<PatchItem>,
    /// 说明 / 错误（本地检查为空；AI 失败时写原因）
    pub note: String,
}

// ==================== 工具 ====================


fn push(items: &mut Vec<CheckItem>, where_: &str, field: &str, kind: &str, why: &str, suggestion: &str) {
    items.push(CheckItem {
        where_: where_.to_string(),
        field: field.to_string(),
        kind: kind.to_string(),
        why: why.to_string(),
        suggestion: suggestion.to_string(),
    });
}

/// 关系表数值字段的合法性（0~100；非数字视为非法）
pub(crate) fn strength_problem(strength: &str) -> Option<String> {
    let s = strength.trim();
    if s.is_empty() {
        return Some("强度为空".to_string());
    }
    match s.parse::<i64>() {
        Ok(n) if (0..=100).contains(&n) => None,
        Ok(n) => Some(format!("强度 {n} 超出 0~100")),
        Err(_) => Some(format!("强度“{s}”不是数字")),
    }
}

// ==================== 本地规则：角色板块 ====================

/// 第五批 ③：检查功能使用的"当前分支"。
///
/// 检查（结构规则与 AI 检查）**只看当前分支的设定** —— 在分支里点检查，
/// 不该把主线的角色/关系/世界观混进来报问题。
async fn check_branch(story_id: &str) -> Option<String> {
    crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None)
}

/// 拼"按分支过滤"的语句片段 + 是否需要绑定（主线时不需要）
fn clause_for(col: &str, branch: &Option<String>, n: usize) -> (String, bool) {
    (crate::story_db::branch_clause(col, branch, n), branch.is_some())
}

pub async fn struct_characters(story_id: &str) -> Result<CheckReport, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let mut items = Vec::new();

    // 角色（第五批 ③：只查当前分支）
    let branch = check_branch(story_id).await;
    let (c_clause, c_bind) = clause_for("branch_id", &branch, 1);
    let c_sql = format!(
        "SELECT id, name, alias, role, gender, age, one_liner, profile, speech_style, traits FROM characters WHERE {}",
        c_clause
    );
    let mut cq = sqlx::query(&c_sql);
    if c_bind {
        cq = cq.bind(branch.clone().unwrap_or_default());
    }
    let rows = cq.fetch_all(&pool).await.map_err(|e| e.to_string())?;
    let mut seen: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for r in &rows {
        let name: String = r.try_get(1).unwrap_or_default();
        let _alias: String = r.try_get(2).unwrap_or_default();
        let role: String = r.try_get(3).unwrap_or_default();
        let gender: String = r.try_get(4).unwrap_or_default();
        let age: String = r.try_get(5).unwrap_or_default();
        let one_liner: String = r.try_get(6).unwrap_or_default();
        let profile: String = r.try_get(7).unwrap_or_default();
        let speech: String = r.try_get(8).unwrap_or_default();
        let traits: String = r.try_get(9).unwrap_or_default();
        let who = if name.trim().is_empty() { "（未命名角色）".to_string() } else { name.clone() };

        // 重名
        if !name.trim().is_empty() {
            if let Some(_) = seen.get(name.trim()) {
                push(&mut items, &who, "角色名", "重复", "存在同名角色，提示词里无法区分", "改成不同的名字，或把其中一个设为别名");
            } else {
                seen.insert(name.trim().to_string(), 1);
            }
        }
        for (field, v) in [("角色名", &name), ("定位", &role), ("性别", &gender), ("年龄", &age)] {
            if v.trim().is_empty() {
                push(&mut items, &who, field, "空值", &format!("{field}还没填，AI 只能自己猜"), &format!("补上{field}"));
            }
        }
        // 只看不判的内容：仅在完全空白时提示（不做文笔/合理性评判）
        for (field, v) in [("一句话身份", &one_liner), ("人物小传", &profile), ("说话方式", &speech), ("性格", &traits)] {
            if v.trim().is_empty() {
                push(&mut items, &who, field, "空值", &format!("{field}为空，角色扮演时缺少依据"), &format!("补上{field}（一句话即可）"));
            }
        }
        // 年龄非空但不像年龄
        let a = age.trim();
        if !a.is_empty() && !a.chars().any(|c| c.is_ascii_digit()) {
            push(&mut items, &who, "年龄", "数值非法", &format!("年龄“{a}”里没有数字"), "填数字（如 18），或写“约 30”");
        }
    }

    // 关系：只查结构（数值与类型的矛盾交给 AI）。第五批 ③：关系与两端角色都限当前分支
    let (rf, _) = clause_for("f.branch_id", &branch, 1);
    let (rt, _) = clause_for("t.branch_id", &branch, 2);
    let (rr, _) = clause_for("r.branch_id", &branch, 3);
    let rel_sql = format!(
        "SELECT f.name, t.name, r.relation_type, r.direction, r.strength FROM relations r
         JOIN characters f ON f.id = r.from_character_id AND {}
         JOIN characters t ON t.id = r.to_character_id AND {}
         WHERE {}",
        rf, rt, rr
    );
    let mut rel_q = sqlx::query(&rel_sql);
    if branch.is_some() {
        rel_q = rel_q.bind(&branch).bind(&branch).bind(&branch);
    }
    let rel = rel_q
        .fetch_all(&pool).await.map_err(|e| e.to_string())?;
    for r in &rel {
        let from: String = r.try_get(0).unwrap_or_default();
        let to: String = r.try_get(1).unwrap_or_default();
        let rel_type: String = r.try_get(2).unwrap_or_default();
        let dir: String = r.try_get(3).unwrap_or_default();
        let strength: i64 = r.try_get(4).unwrap_or(0);
        let who = format!("{from} → {to}");
        if rel_type.trim().is_empty() {
            push(&mut items, &who, "关系类型", "空值", "关系类型为空（提示词里会当成“陌生人”）", "填写关系类型，或确认就是陌生人");
        }
        if dir.trim().is_empty() {
            push(&mut items, &who, "关系方向", "空值", "关系方向未设置（单向 / 双向）", "设为单向或双向");
        }
        if let Some(msg) = strength_problem(&strength.to_string()) {
            push(&mut items, &who, "关系数值", "数值非法", &msg, "改为 0~100 的整数");
        }
    }

    Ok(CheckReport {
        board: "角色管理".to_string(),
        has_issue: !items.is_empty(),
        items,
        patch: Vec::new(),
        note: String::new(),
    })
}

// ==================== 本地规则：世界设定板块 ====================

pub async fn struct_world(story_id: &str) -> Result<CheckReport, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let mut items = Vec::new();

    // 第五批 ③：只查当前分支的世界观
    let branch = check_branch(story_id).await;
    let (w_clause, _) = clause_for("branch_id", &branch, 1);
    let w_sql = format!("SELECT id, key, value, category, meta_json FROM world_settings WHERE {}", w_clause);
    let mut wq = sqlx::query(&w_sql);
    if branch.is_some() {
        wq = wq.bind(&branch);
    }
    let rows = wq.fetch_all(&pool).await.map_err(|e| e.to_string())?;
    let mut keys: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut coords: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let factions: std::collections::HashSet<String> = rows.iter()
        .filter(|r| r.try_get::<String, _>(3).unwrap_or_default() == "势力")
        .map(|r| r.try_get::<String, _>(1).unwrap_or_default())
        .collect();

    for r in &rows {
        let key: String = r.try_get(1).unwrap_or_default();
        let value: String = r.try_get(2).unwrap_or_default();
        let category: String = r.try_get(3).unwrap_or_default();
        let meta_json: String = r.try_get(4).unwrap_or_default();
        let meta: serde_json::Value = serde_json::from_str(&meta_json).unwrap_or(serde_json::json!({}));
        let who = if key.trim().is_empty() { format!("（未命名{category}）") } else { key.clone() };

        if key.trim().is_empty() {
            push(&mut items, &who, "名称", "空值", "条目没有名称", "填写名称");
        } else if seen_insert(&mut keys, key.trim()) {
            push(&mut items, &who, "名称", "重复", "存在同名条目", "改名，或合并到已有条目");
        }
        if value.trim().is_empty() {
            push(&mut items, &who, "内容", "空值", "条目内容为空", "填写内容（一句话即可）");
        }

        // 地图坐标重复
        if category == "地理" {
            let x = meta.get("map_x").and_then(|v| v.as_str()).unwrap_or("");
            let y = meta.get("map_y").and_then(|v| v.as_str()).unwrap_or("");
            if !x.trim().is_empty() && !y.trim().is_empty() {
                let pos = format!("{},{}", x.trim(), y.trim());
                if let Some(other) = coords.get(&pos) {
                    push(&mut items, &who, "地图坐标", "坐标冲突", &format!("与「{other}」的坐标完全相同（{pos}）"), "把其中一个挪开，或合并这两个地点");
                } else {
                    coords.insert(pos, who.clone());
                }
            }
        }

        // 势力关系指向已删势力
        if category == "势力关系" {
            let from = meta.get("from").and_then(|v| v.as_str()).unwrap_or("");
            let to = meta.get("to").and_then(|v| v.as_str()).unwrap_or("");
            for (side, n) in [("from", from), ("to", to)] {
                if !n.trim().is_empty() && !factions.contains(n.trim()) {
                    push(&mut items, &who, "势力关系", "悬空", &format!("{side} 指向的势力「{n}」已不存在"), "删除这条关系，或重建该势力");
                }
            }
        }
    }

    Ok(CheckReport {
        board: "世界设定".to_string(),
        has_issue: !items.is_empty(),
        items,
        patch: Vec::new(),
        note: String::new(),
    })
}

fn seen_insert(map: &mut std::collections::HashMap<String, i64>, k: &str) -> bool {
    if map.contains_key(k) {
        true
    } else {
        map.insert(k.to_string(), 1);
        false
    }
}

// ==================== AI 检查 ====================

/// 检查提示词的公共部分（数据字典 + 通用约束）
fn prompt_common() -> String {
    format!(
        "严格约束（必须遵守）：\n\
- 不要找茬，不要评价文笔、创意、人物设定好不好；\n\
- 不要提扩充建议（不要建议增加新设定、新角色、新剧情）；\n\
- 与标尺无关的“个人口味”问题一律不要报；\n\
- 只报你能明确说出问题在哪一句的条目；没有问题就返回空数组；\n\
- 每条问题都要给出**最小修改**（只改到能读懂、不歧义即可）。\n\
\n\
数据字典（关系数值与状态的统一标尺）：\n\
{}\n\
{}\n\
关系方向读法：单向 = 数值只描述主动方投入；双向 = 双方相互义务与牺牲意愿。\n\
\n\
只输出 JSON，不要输出任何解释、不要用代码块包裹。\n",
        crate::relations::strength_legend(),
        crate::relations::state_legend(),
    )
}

/// 检查提示词（**一板一套**：角色板块只出现角色/关系字段，世界板块只出现世界字段，
/// 避免模型把两个板块的字段名混用 —— 这是实机上出现过的问题）。
pub fn check_system_prompt(board: &str) -> String {
    let common = prompt_common();
    if board == "角色管理" {
        format!(
            "你是设定校对助手，正在检查一部小说的「角色管理」板块（角色卡 + 角色之间的关系）。\n\
你的任务只有三类：\n\
1) 语法：读起来不通顺、缺字、标点或断句明显错误；\n\
2) 歧义：代词指代不清（例如用“他”指代女性角色）、一句话能被理解成两种意思、同一段自相矛盾；\n\
3) 与标尺矛盾：关系数值与关系类型 / 状态 / 方向是否对不上（见数据字典）。\n\
\n\
{common}\
\n\
JSON 格式：\n\
{{\n\
  \"has_issue\": true,\n\
  \"problems\": [\n\
    {{\"where\":\"条目名\",\"field\":\"字段或位置\",\"kind\":\"语法|歧义|矛盾\",\"why\":\"为什么是问题\",\"suggestion\":\"最小修改建议\"}}\n\
  ],\n\
  \"patch\": [\n\
    {{\"board\":\"characters\",\"target_kind\":\"character 或 relation\",\"target_name\":\"角色名 或 A→B\",\"field\":\"只能取下面列出的字段\",\"before\":\"你看到的当前值\",\"value\":\"改好的完整值\"}}\n\
  ]\n\
}}\n\
\n\
本板块**只允许**这些字段（写别的字段名会被软件拒绝）：\n\
- 角色字段（target_kind=\"character\"，target_name=角色名）：name、alias、role、gender、age、one_liner、profile、speech_style、traits\n\
- 关系字段（target_kind=\"relation\"，target_name=「A→B」，用箭头连接双方姓名）：relation_type、strength、state、description\n\
\n\
硬性规则：\n\
- `board` 固定写 \"characters\"；\n\
- `target_kind` 只能是 character 或 relation；\n\
- 关系数值（strength）的 value 必须是 0~100 的整数文本（如 \"70\"），**不要写「单向」这类文字**；\n\
- 关系方向（direction）**不可修改**：若发现方向存疑，只在 problems 里说明，不要放进 patch；\n\
- `before` 必须原样复制你在上面内容里看到的当前值；\n\
- `value` 必须是该字段改好后的**完整值**；\n\
- 只把确实改动过的条目放进 patch。",
            common = common
        )
    } else {
        format!(
            "你是设定校对助手，正在检查一部小说的「世界设定」板块（设定条目 / 地图地点 / 势力 / 术语）。\n\
你的任务只有两类：\n\
1) 语法：读起来不通顺、缺字、标点或断句明显错误；\n\
2) 歧义：指代不清、一句话能被理解成两种意思、同一条目自相矛盾。\n\
\n\
{common}\
\n\
JSON 格式：\n\
{{\n\
  \"has_issue\": true,\n\
  \"problems\": [\n\
    {{\"where\":\"条目名\",\"field\":\"字段或位置\",\"kind\":\"语法|歧义\",\"why\":\"为什么是问题\",\"suggestion\":\"最小修改建议\"}}\n\
  ],\n\
  \"patch\": [\n\
    {{\"board\":\"world\",\"target_kind\":\"world_entry\",\"target_name\":\"条目名称\",\"field\":\"只能取下面列出的字段\",\"before\":\"你看到的当前值\",\"value\":\"改好的完整值\"}}\n\
  ]\n\
}}\n\
\n\
本板块**只允许**这些字段（写别的字段名会被软件拒绝）：\n\
- value（条目内容）、category（分类）、attitude（对主角态度）、scale（规模）、leader（头目）、control_range（控制范围）\n\
\n\
硬性规则：\n\
- `board` 固定写 \"world\"；`target_kind` 固定写 \"world_entry\"；`target_name` 填条目名称；\n\
- **不要改动条目名称本身**（key 字段不存在于可改列表中）；名称有问题只在 problems 里说明；\n\
- `before` 必须原样复制你在上面内容里看到的当前值；\n\
- `value` 必须是该字段改好后的**完整值**；\n\
- 只把确实改动过的条目放进 patch。",
            common = common
        )
    }
}

/// AI 检查：payload 里带板块内容 / 上一轮建议 / 用户输入；
/// 要求 AI **每轮输出更新后的完整建议集**（因为下一轮只看这一份，不带多轮上下文）。
pub async fn check_board_ai(
    lib_pool: &SqlitePool,
    board: &str,
    content: &str,
    previous: &str,
    user_input: &str,
) -> Result<CheckReport, String> {
    let base_url = crate::db::get_config(lib_pool, "model.base_url")
        .await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let model = crate::db::get_config(lib_pool, "model.name")
        .await.map_err(|e| e.to_string())?
        .unwrap_or_else(|| "deepseek-flash".to_string());
    let api_key = crate::keychain::get_api_key("openai_api_key".to_string())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未设置 API Key".to_string())?;

    let mut user_msg = String::new();
    user_msg.push_str(&format!("【{board} 当前内容】\n{content}\n"));
    if !previous.trim().is_empty() {
        user_msg.push_str(&format!(
            "\n【上一轮你给出的建议（请在此基础上更新，并输出**更新后的完整建议集**，不是增量）】\n{previous}\n"
        ));
    }
    if !user_input.trim().is_empty() {
        user_msg.push_str(&format!("\n【用户的补充说明】\n{user_input}\n"));
    }
    user_msg.push_str("\n请按要求只输出 JSON。");

    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": check_system_prompt(board)},
            {"role": "user", "content": user_msg}
        ],
        "max_tokens": 4000,
        "temperature": 0.2,
        "stream": false,
        "response_format": {"type": "json_object"}
    });
    // 校对不需要思考：显式关掉（思考会吃光额度导致正文为空 —— 日摘要踩过这个坑）
    body["thinking"] = serde_json::json!({"type": "disabled"});

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let resp = reqwest::Client::new().post(&url).bearer_auth(&api_key).json(&body).send().await
        .map_err(|e| format!("检查请求失败: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("检查失败 HTTP {}: {}", status.as_u16(), text.chars().take(200).collect::<String>()));
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("检查响应解析失败: {e}"))?;
    let raw = v["choices"][0]["message"]["content"].as_str().unwrap_or_default().trim().to_string();

    // 容错：模型偶尔会用代码块包裹
    let cleaned = raw.trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let parsed: serde_json::Value = serde_json::from_str(cleaned)
        .map_err(|e| format!("AI 返回的不是合法 JSON（已降级为只读报告）: {e}"))?;

    let mut items: Vec<CheckItem> = Vec::new();
    if let Some(arr) = parsed["problems"].as_array() {
        for p in arr {
            items.push(CheckItem {
                where_: p["where"].as_str().unwrap_or("（未指明）").to_string(),
                field: p["field"].as_str().unwrap_or("").to_string(),
                kind: p["kind"].as_str().unwrap_or("问题").to_string(),
                why: p["why"].as_str().unwrap_or("").to_string(),
                suggestion: p["suggestion"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    let mut patch: Vec<PatchItem> = Vec::new();
    if let Some(arr) = parsed["patch"].as_array() {
        for p in arr {
            patch.push(PatchItem {
                board: p["board"].as_str().unwrap_or("").to_string(),
                target_kind: p["target_kind"].as_str().unwrap_or("").to_string(),
                target_name: p["target_name"].as_str().unwrap_or("").to_string(),
                field: p["field"].as_str().unwrap_or("").to_string(),
                before: p["before"].as_str().unwrap_or("").to_string(),
                value: p["value"].as_str().unwrap_or("").to_string(),
            });
        }
    }

    // 带上"本轮用的提示词关卡"指纹，便于确认运行时跑的是哪一版（排查"改了没生效"类问题）
    let fingerprint = {
        let p = check_system_prompt(board);
        let chars = p.chars().count();
        let sum: u32 = p.bytes().map(|b| b as u32).sum();
        format!("提示词关卡：{board} / {chars} 字 / 校验和 {sum}")
    };
    Ok(CheckReport {
        board: board.to_string(),
        has_issue: !items.is_empty() || !patch.is_empty(),
        items,
        patch,
        note: fingerprint,
    })
}

// ==================== 白名单校验（一键替换前）====================

/// 每个板块允许被 AI 修改的字段白名单（= 数据库真实列名）。
/// 这是**防止 `no such column` 的关键闸门**：错字段在这里就被拒绝并给出人话原因，
/// 绝不放行到 SQL；同时天然挡掉 id / created_at 等内部字段。
pub(crate) fn field_allowed(board: &str, field: &str) -> bool {
    let chars = ["name", "alias", "role", "gender", "age", "one_liner", "profile", "speech_style", "traits"];
    // 注意：**不含 direction** —— 关系方向不交给 AI 改（用户 2026-10-11 决定）
    let rels = ["relation_type", "strength", "state", "description"];
    let world = ["value", "category", "attitude", "scale", "leader", "control_range"];
    match board {
        "characters" => chars.contains(&field) || rels.contains(&field),
        "world" => world.contains(&field),
        _ => false,
    }
}

/// 上下文感知的字段归一化：把模型写的别名 / 中文 / 泛化名，按**目标类型**翻译成数据库真实列名。
///
/// 关键点：模型经常用泛化词 `value` / `内容` / `数值` 表示"我要改的那个值" ——
/// 它到底是 `strength`（关系数值）、`profile`（人物小传）还是 `value`（世界条目内容），
/// 取决于**改的是谁**，所以本函数先判目标（名字带箭头 = 关系），再挑列名。
pub(crate) fn normalize_field(board: &str, target_kind: &str, target_name: &str, field: &str) -> Option<&'static str> {
    let raw = field.trim();
    let f = raw.to_ascii_lowercase();
    let has_arrow = target_name.contains('→') || target_name.contains("->")
        || target_name.contains("—>") || target_name.contains("–>");
    let is_relation = target_kind == "relation" || has_arrow;

    // 显式英文列名（原样，但要在本板块允许）
    let explicit: &[&str] = &[
        "name", "alias", "role", "gender", "age", "one_liner", "profile", "speech_style", "traits",
        "relation_type", "strength", "state", "description",
        "value", "category", "attitude", "scale", "leader", "control_range",
    ];

    // 别名 → 泛化语义（再由上下文决定具体列名）
    let alias = |k: &str| -> Option<&'static str> {
        Some(match k {
            "内容" | "正文" | "值" | "value" => "GENERIC_VALUE",
            "数值" | "强度" | "关系数值" | "strength" => "strength",
            "关系类型" | "relation_type" | "类型" => "relation_type",
            "状态" | "关系状态" | "state" => "state",
            "说明" | "关系说明" | "description" => "description",
            "人物小传" | "小传" | "profile" | "简介" => "profile",
            "说话方式" | "口癖" | "speech_style" => "speech_style",
            "性格" | "traits" => "traits",
            "一句话身份" | "身份" | "one_liner" => "one_liner",
            "角色名" | "名字" | "名称" | "name" => "name",
            "别名" | "alias" => "alias",
            "定位" | "角色定位" | "role" => "role",
            "性别" | "gender" => "gender",
            "年龄" | "age" => "age",
            "分类" | "category" => "category",
            "对主角态度" | "态度" | "attitude" => "attitude",
            "规模" | "scale" => "scale",
            "头目" | "leader" => "leader",
            "控制范围" | "control_range" => "control_range",
            _ => return None,
        })
    };

    // 顺序很关键：先解析"显式列名 / 别名"成中间语义，**再按上下文落地**，最后才做白名单校验。
    // 因为 `value` 在角色板块不是合法列名，但它在关系语境下应被理解成 strength ——
    // 若先校验白名单，就会被提前拒掉。
    // 先查别名表（它会把 value / 内容 这类**泛化词**标成 GENERIC_VALUE），
    // 别名表没命中才当作"显式列名"；若显式列名在本板块不合法，再按上下文落地。
    let mut col: &str = match alias(raw).or_else(|| alias(f.as_str())) {
        Some(c) => c,
        None => {
            if explicit.contains(&f.as_str()) {
                f.as_str()
            } else {
                return None;
            }
        }
    };
    // 只有泛化词（value / 内容 / 正文）才按上下文兜底 —— 那是模型表达"改那个值"的方式；
    // 像 scale 这种明确属于别的板块的字段一律拒绝，不猜（避免把世界字段偷偷改成关系数值）
    if col == "GENERIC_VALUE" {
        col = if is_relation {
            "strength"
        } else if board == "world" {
            "value"
        } else {
            "profile"
        };
    }
    if !field_allowed(board, col) {
        return None;
    }
    Some(match col {
        "name" => "name", "alias" => "alias", "role" => "role", "gender" => "gender", "age" => "age",
        "one_liner" => "one_liner", "profile" => "profile", "speech_style" => "speech_style", "traits" => "traits",
        "relation_type" => "relation_type", "strength" => "strength", "state" => "state", "description" => "description",
        "value" => "value", "category" => "category", "attitude" => "attitude", "scale" => "scale",
        "leader" => "leader", "control_range" => "control_range", _ => "value",
    })
}

/// 判断一条补丁是否其实是"角色关系"（名字带箭头，或字段是关系字段）—— 用于把误标成 world 的补丁改判回 characters
pub(crate) fn looks_like_relation(p: &PatchItem) -> bool {
    p.target_name.contains('→') || p.target_name.contains("->") || p.target_name.contains("—>") || p.target_name.contains("–>")
        || matches!(p.field.trim(), "strength" | "relation_type" | "state" | "数值" | "强度" | "关系数值" | "关系类型" | "状态")
}
/// 给"字段不被允许"的情况生成一句能指导用户的说明（列出该板块可用字段）
pub(crate) fn field_reject_reason(board: &str, field: &str) -> String {
    let list = match board {
        "characters" => "name / alias / role / gender / age / one_liner / profile / speech_style / traits（角色）或 relation_type / strength / state / description（关系）",
        "world" => "value / category / attitude / scale / leader / control_range",
        _ => "（未知板块）",
    };
    format!("字段「{field}」不在「{board}」板块的可改范围内（会造成数据库列不存在）。该板块只允许：{list}")
}

/// 归一化条目类型：模型偶尔会串用别的板块的枚举值（例如在角色板块写 `faction_relation`），
/// 按"板块 + 是否含箭头 + 字段是否属于关系"推断出正确的类型，而不是直接拒绝整批。
pub(crate) fn normalize_kind(board: &str, kind: &str, target_name: &str, field: &str) -> String {
    let has_arrow = target_name.contains("→") || target_name.contains("->") || target_name.contains("—>") || target_name.contains("–>");
    let rel_field = matches!(field, "relation_type" | "strength" | "state" | "description" | "direction");
    match board {
        "characters" => {
            if kind == "relation" || kind == "faction_relation" || rel_field || has_arrow {
                "relation".to_string()
            } else {
                "character".to_string()
            }
        }
        "world" => {
            // 世界板块全是按 key 定位的条目，统一成 world_entry（执行 SQL 一致）
            "world_entry".to_string()
        }
        _ => kind.to_string(),
    }
}

/// 校验补丁并返回 (归一化后的补丁, 人类可读的执行清单)；有问题则返回 Err（不执行任何修改）
pub(crate) async fn validate_patch(pool: &SqlitePool, items: &[PatchItem]) -> Result<(Vec<PatchItem>, Vec<String>), String> {
    let mut lines = Vec::new();
    let mut fixed: Vec<PatchItem> = Vec::new();
    for (i, p) in items.iter().enumerate() {
        let n = i + 1;
        if p.target_name.trim().is_empty() {
            return Err(format!("第 {n} 条补丁缺少目标名称，已中止"));
        }
        if p.value.trim().is_empty() {
            return Err(format!("第 {n} 条补丁的新值为空（{} · {}），已中止", p.target_name, p.field));
        }
        // ① 用**数据库**判定 target_name 到底是谁（角色名 / 世界条目 / A→B 关系），据此改判板块与类型。
        //    这比"猜字段像什么"可靠得多：模型把「宸熙」标成 world_entry 时也能纠正回来。
        let (mut board, mut kind) = resolve_target(pool, p).await;
        // ② 兜底：库里查不到时，仍按"名字带箭头 = 关系"处理
        if board == "world" && looks_like_relation(p) {
            board = "characters".to_string();
            kind = "relation".to_string();
        }
        // ③ 归一化条目类型（确保与板块匹配）
        let kind = normalize_kind(&board, &kind, &p.target_name, &p.field);
        // ③ 归一化字段名（别名 / 中文 / 泛化词 → 数据库列名）；归不了或本板块不允许 → 人话拒绝
        let mut col = match normalize_field(&board, &kind, &p.target_name, &p.field) {
            Some(c) => c.to_string(),
            None => return Err(format!("第 {n} 条补丁：{}", field_reject_reason(&board, &p.field))),
        };
        // ④ 用 AI 声明的"当前值"反查它**真正属于哪个字段**（模型不知道字段名，只有软件知道）。
        //    这是"以角色名定位、字段由软件判断"的落地：命中就改判到真实字段，避免因字段名写错而整批失败。
        if !p.before.trim().is_empty() {
            if let Some(real) = resolve_field(pool, &board, &kind, &p.target_name, &p.before).await {
                col = real;
            }
        }
        if !matches!(kind.as_str(), "character" | "relation" | "world_entry") {
            return Err(format!("第 {n} 条补丁的条目类型「{}」不认识，已中止", p.target_kind));
        }
        let mut q = p.clone();
        q.board = board.clone();
        q.target_kind = kind;
        q.field = col.clone();
        lines.push(format!("{} · {}（{}）→ {}", q.target_name, q.field, q.target_kind, q.value.trim()));
        fixed.push(q);
    }
    Ok((fixed, lines))
}

// ==================== 一键替换（白名单校验后执行）====================

/// 比较"AI 声明的当前值"与"库中现值"：数值列按数字比（容忍 80 / "80" / 80.0 / 空白），文本列按 trim 后比。
pub(crate) fn values_equal(field: &str, a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if is_numeric_field(field) {
        if let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>()) {
            return (x - y).abs() < f64::EPSILON;
        }
    }
    a.trim() == b.trim()
}

/// 按"AI 看到的当前值"反查它**真正属于哪个字段**（用户 2026-10-11 定：以角色名定位，字段由软件判断）。
///
/// 背景：给 AI 的板块内容是各字段拼成的 JSON，模型分不清某段文字来自 `one_liner` 还是 `profile`，
/// 于是随口写一个字段名 → 与库中现值对不上 → 被 before 核对拒绝（实测报错：「夏婉琴」的 profile 当前值是「」，
/// 但 AI 以为它是「女主，身材瘦小纤细，身高154cm」——那段其实在 one_liner 里）。
///
/// 做法：拿 AI 声明的 `before` 去比对该角色**各个可改字段**的真实值 ——
/// 完全相等 / 互为包含都算命中；命中唯一字段就改判；命中多个就取"包含关系更具体"的那个；都没命中则不猜。
pub(crate) async fn resolve_field(
    pool: &SqlitePool,
    board: &str,
    target_kind: &str,
    target_name: &str,
    before: &str,
) -> Option<String> {
    let b = before.trim();
    if b.is_empty() {
        return None;
    }
    if target_kind == "relation" {
        // 关系：按表中现值反查（strength 是数字列，按 i64 读）
        let (from, to) = split_arrow(target_name).ok()?;
        let row = sqlx::query(
            "SELECT COALESCE(relation_type,''), COALESCE(strength,0), COALESCE(state,''), COALESCE(description,'')
             FROM relations WHERE from_character_id = (SELECT id FROM characters WHERE name = ?1)
               AND to_character_id = (SELECT id FROM characters WHERE name = ?2)"
        ).bind(&from).bind(&to).fetch_optional(pool).await.ok().flatten()?;
        let vals: [(&str, String); 4] = [
            ("relation_type", row.try_get(0).unwrap_or_default()),
            ("strength", row.try_get::<i64, _>(1).unwrap_or(0).to_string()),
            ("state", row.try_get(2).unwrap_or_default()),
            ("description", row.try_get(3).unwrap_or_default()),
        ];
        return pick_field(&vals, b);
    }
    if board == "world" {
        let row = sqlx::query("SELECT COALESCE(value,''), COALESCE(category,'') FROM world_settings WHERE key = ?1")
            .bind(target_name).fetch_optional(pool).await.ok().flatten()?;
        let vals: [(&str, String); 2] = [
            ("value", row.try_get(0).unwrap_or_default()),
            ("category", row.try_get(1).unwrap_or_default()),
        ];
        return pick_field(&vals, b);
    }
    // 角色：查全部可改文本字段
    let row = sqlx::query(
        "SELECT COALESCE(name,''), COALESCE(alias,''), COALESCE(role,''), COALESCE(gender,''), COALESCE(age,''),
                COALESCE(one_liner,''), COALESCE(profile,''), COALESCE(speech_style,''), COALESCE(traits,'')
         FROM characters WHERE name = ?1"
    ).bind(target_name).fetch_optional(pool).await.ok().flatten()?;
    let vals: [(&str, String); 9] = [
        ("name", row.try_get(0).unwrap_or_default()),
        ("alias", row.try_get(1).unwrap_or_default()),
        ("role", row.try_get(2).unwrap_or_default()),
        ("gender", row.try_get(3).unwrap_or_default()),
        ("age", row.try_get(4).unwrap_or_default()),
        ("one_liner", row.try_get(5).unwrap_or_default()),
        ("profile", row.try_get(6).unwrap_or_default()),
        ("speech_style", row.try_get(7).unwrap_or_default()),
        ("traits", row.try_get(8).unwrap_or_default()),
    ];
    pick_field(&vals, b)
}

/// 在"字段 → 当前值"里挑出与 `before` 匹配的那个：
/// 相等优先；否则取"库里值包含 before"或"before 包含库里值"的字段；多个命中取**更长的一方**（更具体）。
fn pick_field(vals: &[(&str, String)], before: &str) -> Option<String> {
    for (f, v) in vals {
        if v.trim() == before && !before.is_empty() {
            return Some((*f).to_string());
        }
    }
    let mut best: Option<(&str, usize)> = None;
    for (f, v) in vals {
        let vt = v.trim();
        if vt.is_empty() {
            continue;
        }
        let related = vt.contains(before) || before.contains(vt);
        if related {
            let score = vt.chars().count();
            if best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some((f, score));
            }
        }
    }
    best.map(|(f, _)| f.to_string())
}

/// 用数据库反查"这个 target_name 到底是谁"，据此**改判板块与条目类型**。
/// 模型经常把角色（如「宸熙」）标成 `world_entry`、把角色关系标成 `world` —— 名字是唯一可靠的线索，
/// 所以这里直接查库：先看是不是角色名、再看是不是世界条目的 key、最后看是不是「A→B」关系。
/// 返回 `(board, target_kind)`；查不到就保留原样（后续白名单/定位会给出人话拒绝）。
pub(crate) async fn resolve_target(pool: &SqlitePool, p: &PatchItem) -> (String, String) {
    let name = p.target_name.trim();
    let is_arrow = p.target_name.contains('→') || p.target_name.contains("->")
        || p.target_name.contains("—>") || p.target_name.contains("–>");
    if is_arrow {
        return ("characters".to_string(), "relation".to_string());
    }
    // 角色名？
    let is_char: Option<i64> = sqlx::query_scalar("SELECT id FROM characters WHERE name = ?1")
        .bind(name).fetch_optional(pool).await.ok().flatten();
    if is_char.is_some() {
        return ("characters".to_string(), "character".to_string());
    }
    // 世界条目？
    let is_world: Option<String> = sqlx::query_scalar("SELECT key FROM world_settings WHERE key = ?1")
        .bind(name).fetch_optional(pool).await.ok().flatten();
    if is_world.is_some() {
        return ("world".to_string(), "world_entry".to_string());
    }
    (p.board.clone(), p.target_kind.clone())
}

/// 数值型列（SQLite 里存 INTEGER）：这些列必须按整数读取，否则 sqlx 会报
/// `mismatched types ... TEXT is not compatible with SQL type INTEGER`。
pub(crate) fn is_numeric_field(col: &str) -> bool {
    matches!(col, "strength")
}

/// 读取补丁目标字段的**当前值**（用于核对 `before` 与前端展示 diff）——
/// **按列类型分别读取**：数值列走 i64，其余走 String（并统一转成字符串返回，便于与 AI 的文本比对）。
/// 找不到条目时返回 Err（不静默）。
pub(crate) async fn read_current(pool: &SqlitePool, p: &PatchItem) -> Result<String, String> {
    let name = p.target_name.trim();
    let col = p.field.as_str();
    let numeric = is_numeric_field(col);

    // 每类目标一条 SQL；数值列与文本列分开处理
    let (sql_num, sql_txt): (String, String) = match (p.board.as_str(), p.target_kind.as_str()) {
        ("characters", "character") => (
            format!("SELECT COALESCE({col}, 0) FROM characters WHERE name = ?1"),
            format!("SELECT COALESCE({col}, '') FROM characters WHERE name = ?1"),
        ),
        ("characters", "relation") => {
            let (_from, _to) = split_arrow(name)?;
            (
                format!("SELECT COALESCE({col}, 0) FROM relations WHERE from_character_id = (SELECT id FROM characters WHERE name = ?1) AND to_character_id = (SELECT id FROM characters WHERE name = ?2)"),
                format!("SELECT COALESCE({col}, '') FROM relations WHERE from_character_id = (SELECT id FROM characters WHERE name = ?1) AND to_character_id = (SELECT id FROM characters WHERE name = ?2)"),
            )
        }
        ("world", _) => (
            format!("SELECT COALESCE({col}, 0) FROM world_settings WHERE key = ?1"),
            format!("SELECT COALESCE({col}, '') FROM world_settings WHERE key = ?1"),
        ),
        (b, k) => return Err(format!("不支持的板块/条目类型组合：{b} / {k}")),
    };

    // 关系类目标多一个绑定参数
    let is_relation = p.target_kind == "relation";
    let (found, text): (bool, String) = if numeric {
        let row: Option<i64> = if is_relation {
            let (from, to) = split_arrow(name)?;
            sqlx::query_scalar(&sql_num).bind(&from).bind(&to).fetch_optional(pool).await.map_err(|e| e.to_string())?
        } else {
            sqlx::query_scalar(&sql_num).bind(name).fetch_optional(pool).await.map_err(|e| e.to_string())?
        };
        match row {
            Some(v) => (true, v.to_string()),
            None => (false, String::new()),
        }
    } else {
        let row: Option<String> = if is_relation {
            let (from, to) = split_arrow(name)?;
            sqlx::query_scalar(&sql_txt).bind(&from).bind(&to).fetch_optional(pool).await.map_err(|e| e.to_string())?
        } else {
            sqlx::query_scalar(&sql_txt).bind(name).fetch_optional(pool).await.map_err(|e| e.to_string())?
        };
        match row {
            Some(v) => (true, v),
            None => (false, String::new()),
        }
    };

    if !found {
        return Err(format!("找不到条目「{name}」（{}）", p.target_kind));
    }
    Ok(text)
}

/// 执行一条补丁：按 `target_kind` 定位条目、按 `field` 更新该字段。
/// 只在 `validate_patch` 通过后才会被调用；失败返回人话原因（不 panic）。
async fn apply_one(pool: &SqlitePool, p: &PatchItem) -> Result<String, String> {
    let name = p.target_name.trim();
    let value = p.value.clone();
    // 防伪核对：AI 给的 before 必须与库中现值一致，否则说明它基于错误理解生成补丁 —— 拒绝，不猜
    let current = read_current(pool, p).await?;
    if !p.before.trim().is_empty() && p.before.trim() != current.trim() {
        return Err(format!(
            "「{name}」的 {} 当前值是「{}」，但 AI 以为它是「{}」——为避免误改，本条已跳过",
            p.field, current.trim(), p.before.trim()
        ));
    }
    match (p.board.as_str(), p.target_kind.as_str()) {
        // 角色字段（数值列按整数写，文本列按文本写 —— 否则 SQLite 会类型不匹配）
        ("characters", "character") => {
            let col = p.field.as_str();
            let sql = format!("UPDATE characters SET {col} = ?1 WHERE name = ?2");
            let n = if is_numeric_field(col) {
                let v: i64 = value.trim().parse().map_err(|_| format!("「{value}」不是有效数字，无法写入 {col}"))?;
                sqlx::query(&sql).bind(v).bind(name).execute(pool).await.map_err(|e| e.to_string())?.rows_affected()
            } else {
                sqlx::query(&sql).bind(&value).bind(name).execute(pool).await.map_err(|e| e.to_string())?.rows_affected()
            };
            if n == 0 { return Err(format!("找不到角色「{name}」")); }
            Ok(format!("角色「{name}」的 {} 已更新", p.field))
        }
        // 关系字段：target_name 形如 "宸熙→夏婉琴"（也兼容 "宸熙 → 夏婉琴"）
        ("characters", "relation") => {
            let (from, to) = split_arrow(name)?;
            let col = p.field.as_str();
            let sql = format!(
                "UPDATE relations SET {col} = ?1 WHERE from_character_id = (SELECT id FROM characters WHERE name = ?2)
                   AND to_character_id = (SELECT id FROM characters WHERE name = ?3)"
            );
            let n = if is_numeric_field(col) {
                let v: i64 = value.trim().parse().map_err(|_| format!("「{value}」不是有效数字，无法写入 {col}"))?;
                sqlx::query(&sql).bind(v).bind(&from).bind(&to).execute(pool).await.map_err(|e| e.to_string())?.rows_affected()
            } else {
                sqlx::query(&sql).bind(&value).bind(&from).bind(&to).execute(pool).await.map_err(|e| e.to_string())?.rows_affected()
            };
            if n == 0 { return Err(format!("找不到关系「{name}」")); }
            Ok(format!("关系「{name}」的 {} 已更新", p.field))
        }
        // 世界设定：设定条目 / 势力 / 术语都按 key 更新
        ("world", _) => {
            let col = p.field.as_str();
            let sql = format!("UPDATE world_settings SET {col} = ?1 WHERE key = ?2");
            let n = sqlx::query(&sql).bind(&value).bind(name)
                .execute(pool).await.map_err(|e| e.to_string())?.rows_affected();
            if n == 0 { return Err(format!("找不到条目「{name}」")); }
            Ok(format!("条目「{name}」的 {} 已更新", p.field))
        }
        (b, k) => Err(format!("不支持的板块/条目类型组合：{b} / {k}")),
    }
}

/// 解析关系定位名（"A→B" / "A → B" / "A->B"）
pub(crate) fn split_arrow(s: &str) -> Result<(String, String), String> {
    for sep in ["→", "->", "—>", "–>"] {
        if let Some((a, b)) = s.split_once(sep) {
            let (a, b) = (a.trim().to_string(), b.trim().to_string());
            if a.is_empty() || b.is_empty() {
                return Err(format!("关系定位「{s}」格式不对，应形如「A→B」"));
            }
            return Ok((a, b));
        }
    }
    Err(format!("关系定位「{s}」缺少箭头，应形如「A→B」"))
}

/// 用给定的池执行补丁（供测试使用；生产路径走 `apply_check_patch`，它会按 story_id 打开池并打快照）
#[cfg(test)]
pub(crate) async fn apply_check_patch_in(pool: &SqlitePool, items: &[PatchItem]) -> Result<Vec<String>, String> {
    let (fixed, _) = validate_patch(pool, items).await?;
    for p in &fixed {
        let current = read_current(pool, p).await?;
        if !p.before.trim().is_empty() && !values_equal(p.field.as_str(), p.before.trim(), current.trim()) {
            return Err(format!(
                "「{}」的 {} 当前值是「{}」，但 AI 以为它是「{}」——为避免误改，本次未做任何修改（请重新检查后再试）",
                p.target_name, p.field, current.trim(), p.before.trim()
            ));
        }
    }
    let mut out = Vec::new();
    for p in &fixed {
        out.push(apply_one(pool, p).await?);
    }
    Ok(out)
}

/// 执行一组补丁（先整体校验，校验不过则一条都不执行）；返回每条的执行结果
pub async fn apply_check_patch(story_id: &str, items: &[PatchItem]) -> Result<Vec<String>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // ① 白名单 + 归一化（任一条不过就整体中止）
    let (fixed, _lines) = validate_patch(&pool, items).await?;
    // ② 执行前打快照（失败不阻断）
    let _ = crate::evolution::make_snapshot_for_story(story_id).await;
    let mut out = Vec::new();
    for p in &fixed {
        // ③ 逐条执行；**先把全部条目的 before 核对一遍**再动手，避免改一半失败
        let current = read_current(&pool, p).await?;
        if !p.before.trim().is_empty() && !values_equal(p.field.as_str(), p.before.trim(), current.trim()) {
            return Err(format!(
                "「{}」的 {} 当前值是「{}」，但 AI 以为它是「{}」——为避免误改，本次未做任何修改（请重新检查后再试）",
                p.target_name, p.field, current.trim(), p.before.trim()
            ));
        }
    }
    for p in &fixed {
        let msg = apply_one(&pool, p).await?;
        out.push(msg);
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
