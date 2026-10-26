// 从 `check.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `check.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use local_rules::*;` 取回，
// 外部 `crate::check::xxx` 的调用路径**全部不变**。

use super::*;

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

pub(super) fn push(items: &mut Vec<CheckItem>, where_: &str, field: &str, kind: &str, why: &str, suggestion: &str) {
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

/// 第五批 ③：检查功能使用的"当前分支"。
///
/// 检查（结构规则与 AI 检查）**只看当前分支的设定** —— 在分支里点检查，
/// 不该把主线的角色/关系/世界观混进来报问题。
pub(super) async fn check_branch(story_id: &str) -> Option<String> {
    crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None)
}

/// 拼"按分支过滤"的语句片段 + 是否需要绑定（主线时不需要）
pub(super) fn clause_for(col: &str, branch: &Option<String>, n: usize) -> (String, bool) {
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
            // 界面「地图与地点」写的是 meta.x / meta.y（WorldPage.tsx 的 X%/Y% 输入框）——
            // 这里以前读 map_x / map_y，键名对不上，这条规则从来没触发过（2026-10-26 修）。
            let x = meta.get("x").and_then(|v| v.as_str()).unwrap_or("");
            let y = meta.get("y").and_then(|v| v.as_str()).unwrap_or("");
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

pub(super) fn seen_insert(map: &mut std::collections::HashMap<String, i64>, k: &str) -> bool {
    if map.contains_key(k) {
        true
    } else {
        map.insert(k.to_string(), 1);
        false
    }
}

/// 检查提示词的公共部分（数据字典 + 通用约束）
pub(super) fn prompt_common() -> String {
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
