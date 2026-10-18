// M5: 状态提取与状态卡
// 规则引擎：从一段 AI 剧情回复文本中，按角色提取 情绪 / 动作 / 神态，
// 组合成 current_status 写入 characters 表。规则优先，置信度 ≥ 0.8 生效。

use sqlx::sqlite::SqlitePool;
use sqlx::Row;


// ---------- 词典 ----------

// 情绪词典：词 -> 标准情绪名
const EMOTIONS: &[(&str, &str)] = &[
    // 生气
    ("火冒三丈", "生气"), ("气炸", "生气"), ("愤怒", "生气"), ("恼火", "生气"),
    ("生气", "生气"), ("不耐烦", "生气"), ("冷漠", "生气"), ("阴沉", "生气"),
    // 开心
    ("愉悦", "开心"), ("喜悦", "开心"), ("欢喜", "开心"), ("高兴", "开心"),
    ("开心", "开心"), ("愉快", "开心"), ("偷笑", "开心"), ("笑出声", "开心"),
    // 难过
    ("绝望", "难过"), ("沮丧", "难过"), ("失落", "难过"), ("哽咽", "难过"),
    ("落泪", "难过"), ("委屈", "难过"), ("悲伤", "难过"), ("伤心", "难过"),
    ("难过", "难过"), ("哭", "难过"),
    // 害羞
    ("慌乱", "害羞"), ("窘迫", "害羞"), ("羞涩", "害羞"), ("耳根红", "害羞"),
    ("脸红", "害羞"), ("害羞", "害羞"), ("尴尬", "害羞"), ("心动", "害羞"),
    // 惊讶
    ("愣住", "惊讶"), ("呆住", "惊讶"), ("诧异", "惊讶"), ("震惊", "惊讶"),
    ("惊讶", "惊讶"), ("怔住", "惊讶"),
    // 害怕
    ("惊慌", "害怕"), ("发抖", "害怕"), ("颤抖", "害怕"), ("恐惧", "害怕"),
    ("害怕", "害怕"), ("瑟缩", "害怕"),
    // 平静
    ("镇定", "平静"), ("冷静", "平静"), ("淡然", "平静"), ("平静", "平静"),
    // 温柔
    ("怜惜", "温柔"), ("心疼", "温柔"), ("柔软", "温柔"), ("温柔", "温柔"),
];

// 动作词典
const ACTIONS: &[&str] = &[
    "抬起手", "举起手", "放下手", "攥紧拳头", "握紧拳头", "攥住衣角", "捂住脸",
    "低下头", "抬起头", "转过身", "转身", "抱住", "扑进", "流泪", "擦泪",
    "叹气", "皱眉", "后退一步", "后退", "靠近", "咬唇", "摇头", "点头",
    "拉住", "抓住", "推开", "站起", "坐下", "蹲下", "蹲下身", "弯腰", "捡起",
    "攥着", "捏紧", "握了握", "缩了缩", "别过脸", "移开视线", "垂下眼", "抿了抿嘴",
];

// 神态词典
const DEMEANORS: &[&str] = &[
    "耳根发红", "眼眶发红", "眼圈泛红", "眼神闪烁", "眼神躲闪", "目光躲闪",
    "脸色苍白", "脸色发白", "面无表情", "眼神温柔", "眉心紧锁", "嘴角上扬",
    "嘴角下沉", "眼神坚定", "眼神黯淡", "脸色变了", "脸色一沉", "嘴唇微动",
    "眼圈通红", "眼眶微红", "神色恍惚", "神情落寞", "面带笑意", "欲言又止",
];

// 角色名匹配时，取前后窗口大小（字符）
// 保留未用：常量：状态抽取窗口（调参用，保留）
#[allow(dead_code)]
const WINDOW: usize = 40;

// ---------- 结构 ----------

#[derive(serde::Serialize, Clone)]
pub struct ExtractedState {
    pub character: String,
    pub emotion: String,
    pub action: String,
    pub demeanor: String,
    pub confidence: f64,
    pub status_text: String,
}

// ---------- 规则提取 ----------

/// 从一段文本中，按给定角色名列表提取各角色状态。
/// 规则命中即输出（置信度 ≥ 0.8），无命中则跳过该角色。
pub fn extract_states(text: &str, char_names: &[String]) -> Vec<ExtractedState> {
    let mut out = Vec::new();
    // 段落归属：段落中出现角色名 -> 该段状态归该角色；
    // 段落无角色名 -> 继承上一段焦点角色（代词回指简化版）
    let mut focus: Option<String> = None;
    // 每个角色的累积状态 (character, emotion, action, demeanor)
    let mut states: Vec<(String, String, String, String)> = Vec::new();

    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }

        // 段落级扫描，提取本段状态
        let mut emo = String::new();
        let mut act = String::new();
        let mut dem = String::new();
        for (word, std_name) in EMOTIONS {
            if para.contains(word) {
                emo = std_name.to_string();
                break;
            }
        }
        for w in ACTIONS {
            if para.contains(w) {
                act = w.to_string();
                break;
            }
        }
        for w in DEMEANORS {
            if para.contains(w) {
                dem = w.to_string();
                break;
            }
        }

        // 本段出现的第一个角色（决定归属；无角色名则继承 focus）
        let mut owner: Option<&String> = None;
        for name in char_names {
            if para.contains(name) {
                owner = Some(name);
                break;
            }
        }
        if let Some(name) = owner {
            focus = Some(name.clone());
        }

        let owner_name: String = match owner {
            Some(n) => n.clone(),
            None => match &focus {
                Some(f) => f.clone(),
                None => continue, // 无焦点角色，跳过本段
            },
        };

        // 累积到该角色
        if let Some(entry) = states.iter_mut().find(|e| e.0 == owner_name) {
            if !emo.is_empty() {
                entry.1 = emo;
            }
            if !act.is_empty() {
                entry.2 = act;
            }
            if !dem.is_empty() {
                entry.3 = dem;
            }
        } else {
            states.push((owner_name, emo, act, dem));
        }
    }

    for (character, emotion, action, demeanor) in states {
        let hits = [!emotion.is_empty(), !action.is_empty(), !demeanor.is_empty()]
            .iter()
            .filter(|&&x| x)
            .count();
        if hits == 0 {
            continue;
        }

        // 规则命中即生效：1 项 0.85 / 2 项 0.9 / 3 项 0.95
        let confidence: f64 = match hits {
            1 => 0.85,
            2 => 0.90,
            _ => 0.95,
        };

        let mut parts: Vec<String> = Vec::new();
        if !emotion.is_empty() {
            parts.push(format!("情绪：{}", emotion));
        }
        if !action.is_empty() {
            parts.push(format!("动作：{}", action));
        }
        if !demeanor.is_empty() {
            parts.push(format!("神态：{}", demeanor));
        }

        out.push(ExtractedState {
            character,
            emotion,
            action,
            demeanor,
            confidence,
            status_text: parts.join(" ｜ "),
        });
    }
    out
}

// ---------- 数据库 ----------


/// 读某节点最后一条 AI 回复，提取状态并更新角色表。
/// 返回更新后的角色状态列表（含本次更新的标记）。
// 保留未用：旧的状态抽取入口（保留供迁移）
#[allow(dead_code)]
pub async fn update_states_from_node(story_id: &str, node_id: i64) -> Result<Vec<ExtractedState>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;

    // 1. 取最后一条 assistant 消息（剧情事实），并附加最后一条 user 输入
    //    （用户指令/对话是剧情内容的一部分，作为补充一同参与提取）
    let ai_row = sqlx::query(
        "SELECT content FROM messages WHERE node_id = ?1 AND role = 'assistant' ORDER BY id DESC LIMIT 1"
    )
    .bind(node_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| e.to_string())?;

    let Some(ai_row) = ai_row else {
        return Err(format!("节点 {} 没有 AI 回复可提取", node_id));
    };
    let mut text: String = ai_row.try_get(0).map_err(|e| e.to_string())?;

    let usr_row = sqlx::query(
        "SELECT content FROM messages WHERE node_id = ?1 AND role = 'user' ORDER BY id DESC LIMIT 1"
    )
    .bind(node_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| e.to_string())?;
    if let Some(u) = usr_row {
        if let Ok(ut) = u.try_get::<String, _>(0) {
            text.push_str("\n\n");
            text.push_str(&ut);
        }
    }

    // 2. 读角色名列表
    let rows = sqlx::query("SELECT name FROM characters ORDER BY id")
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let names: Vec<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String, _>(0).ok())
        .collect();

    // 3. 提取
    let states = extract_states(&text, &names);

    // 4. 更新角色状态
    for s in &states {
        let _ = sqlx::query("UPDATE characters SET current_status = ?1 WHERE name = ?2")
            .bind(&s.status_text)
            .bind(&s.character)
            .execute(&pool)
            .await;
    }

    Ok(states)
}

/// 在 AI 回复落库后自动调用（generate_stream.rs 内使用）。
/// 失败不影响主流程（静默）。
pub async fn auto_update_after_reply(pool: &SqlitePool, story_id: &str, node_id: i64) {
    // 从消息表取最后一条 assistant 内容
    let row = sqlx::query(
        "SELECT content FROM messages WHERE node_id = ?1 AND role = 'assistant' ORDER BY id DESC LIMIT 1"
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else { return };
    let Ok(text) = row.try_get::<String, _>(0) else { return };

    // 读角色名
    let rows = match sqlx::query("SELECT name FROM characters ORDER BY id")
        .fetch_all(pool)
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };
    let names: Vec<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String, _>(0).ok())
        .collect();

    let states = extract_states(&text, &names);
    for s in &states {
        let _ = sqlx::query("UPDATE characters SET current_status = ?1 WHERE name = ?2")
            .bind(&s.status_text)
            .bind(&s.character)
            .execute(pool)
            .await;
    }
    let _ = story_id; // story_id 保留备用
}
