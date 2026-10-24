// 「表演标签」—— 动作 + 情绪（v3.5.1，用户 2026-10-24 定）。
//
// ## 为什么需要
//
// AI 写的是 `（非常感动）`、`（转过身）`，但系统**一个都不认识**：
//   * 情绪词表只有 8 个（还只在文本里出现「情绪：X」时才生效），几乎不会触发；
//   * 动作词表**完全没有**。
// 于是导演监视器的角色卡只能把 `current_status` 原样打印出来，
// 后台也没有任何"这一场她是感动还是麻木"的信号。
//
// ## 词表放后端一份（不是前后端各一份）
//
// 刚踩过"两份词表漂移"的坑（回顾词典被复制两份，其中一份漏了「物品」）。
// 所以：词表只在这里，前端通过 `perform_lexicon` 命令取（连配色一起拿）。
//
// ## 两个入口
//
// 1. **显式**：`current_status` 里的 `情绪：X` / `动作：X`（导演手写或角色卡里填的）
// 2. **静默**：AI 回复正文里的 `（动作/神态）` + 情绪词 —— 用户看不见，但会记进 `event_logs`
//
// ## 分组为什么重要
//
// 光有词不够："后退"和"走近"都是动作，但意思相反。
// 分组让上层能问"这一场身体是在靠近还是在退开"，而不只是"有没有动作"。

/// 情绪标签：`(词, 分组, 前景色, 背景色)`。
///
/// 分组是"往哪个方向"：暖 / 冷 / 紧 / 散 / 炸 / 弱。
pub const EMOTIONS: &[(&str, &str, &str, &str)] = &[
    // 暖：向对方打开
    ("感动", "暖", "#E8734A", "#fdf0ea"),
    ("心疼", "暖", "#D96A8A", "#fdeef3"),
    ("温柔", "暖", "#94D8C3", "#eaf8f3"),
    ("雀跃", "暖", "#F5A623", "#fef6e6"),
    ("害羞", "暖", "#EAA7B2", "#fdf0f2"),
    ("依恋", "暖", "#C58BD9", "#f7eefc"),
    ("开心", "暖", "#52C41A", "#eaf7e6"),
    ("庆幸", "暖", "#3FA796", "#e9f6f3"),
    // 冷：向后退
    ("疏离", "冷", "#6B7280", "#f1f2f4"),
    ("防备", "冷", "#7089A8", "#eef2f7"),
    ("不以为然", "冷", "#8A8F98", "#f3f4f6"),
    ("漠然", "冷", "#7B8794", "#f2f4f5"),
    ("冷淡", "冷", "#7B8794", "#f2f4f5"),
    // 紧：绷着
    ("绷紧", "紧", "#C2410C", "#fdeee7"),
    ("焦虑", "紧", "#CA8A04", "#fef7e0"),
    ("委屈", "紧", "#8B5CF6", "#f3eefc"),
    ("赌气", "紧", "#DB2777", "#fceef5"),
    ("不甘", "紧", "#B45309", "#fdf3e6"),
    ("心慌", "紧", "#DC2626", "#fdeded"),
    ("害怕", "紧", "#C9A7E8", "#f5eefc"),
    ("紧张", "紧", "#EAB308", "#fef9e0"),
    // 散：散掉
    ("麻木", "散", "#6B7280", "#f2f2f2"),
    ("抽离", "散", "#64748B", "#f1f5f9"),
    ("空落", "散", "#78716C", "#f5f4f3"),
    ("疲倦", "散", "#78716C", "#f4f4f2"),
    ("平静", "散", "#9E9E9E", "#f2f2f2"),
    ("难过", "散", "#4A7FD9", "#eaf1fc"),
    // 炸：炸开
    ("崩溃", "炸", "#B91C1C", "#fdeaea"),
    ("失控", "炸", "#DC2626", "#fdeded"),
    ("决绝", "炸", "#7F1D1D", "#fbe9e9"),
    ("歇斯底里", "炸", "#991B1B", "#fce8e8"),
    ("暴怒", "炸", "#DC2626", "#fbeaea"),
    ("生气", "炸", "#EA6668", "#fdecec"),
    ("恼怒", "炸", "#D9534F", "#fdecea"),
    // 弱：撑不住
    ("哽咽", "弱", "#4A7FD9", "#eaf1fc"),
    ("哽住", "弱", "#4A7FD9", "#eaf1fc"),
    ("怔住", "弱", "#6B7A99", "#f0f3f8"),
    ("发怵", "弱", "#C9A7E8", "#f5eefc"),
    ("破防", "弱", "#EAA7B2", "#fdf0f2"),
    ("惊讶", "弱", "#FAAD14", "#fef6e6"),
    ("心虚", "弱", "#8FA0B8", "#eff3f8"),
];

/// 动作标签：`(词, 分组)`。
///
/// 分组是"身体往哪去"：靠近 / 退开 / 定格 / 手部 / 视线 / 声音。
/// 有了分组，上层才能判断"这一场是在靠近还是在退开"，而不只是"有没有动作"。
pub const ACTIONS: &[(&str, &str)] = &[
    // 靠近
    ("走近", "靠近"),
    ("凑近", "靠近"),
    ("靠近", "靠近"),
    ("伸手", "靠近"),
    ("扶住", "靠近"),
    ("抱住", "靠近"),
    ("拥抱", "靠近"),
    ("抓住", "靠近"),
    ("拉过", "靠近"),
    ("贴上去", "靠近"),
    ("肩靠肩", "靠近"),
    ("靠肩", "靠近"),
    ("握住", "靠近"),
    ("牵手", "靠近"),
    // 退开
    ("转身", "退开"),
    // "转过身/转过脸" 与 "转身" 不是连续子串，必须单独登记
    ("转过身", "退开"),
    ("转过脸", "退开"),
    ("回头", "退开"),
    ("后退", "退开"),
    ("别开脸", "退开"),
    ("躲开", "退开"),
    ("抽手", "退开"),
    ("退到", "退开"),
    ("退开", "退开"),
    ("挪开", "退开"),
    ("让开半步", "退开"),
    ("偏头", "退开"),
    ("背过身", "退开"),
    ("捂住脸", "退开"),
    // 定格
    ("僵住", "定格"),
    ("愣住", "定格"),
    ("停住", "定格"),
    ("屏住呼吸", "定格"),
    ("没有动", "定格"),
    ("站着不动", "定格"),
    ("定住", "定格"),
    ("一动不动", "定格"),
    // 手部
    ("攥拳", "手部"),
    ("握紧", "手部"),
    ("攥住", "手部"),
    ("攥紧", "手部"),
    ("手指收紧", "手部"),
    ("手抖", "手部"),
    ("捏着", "手部"),
    ("放下杯子", "手部"),
    ("扣住", "手部"),
    ("按住", "手部"),
    ("拍桌", "手部"),
    ("推开", "手部"),
    ("摔门", "手部"),
    ("擦桌子", "手部"),
    ("摸袖口", "手部"),
    // 视线
    ("垂眼", "视线"),
    ("垂下眼", "视线"),
    ("别开眼", "视线"),
    ("低头", "视线"),
    ("抬眼", "视线"),
    ("看一眼", "视线"),
    ("避开目光", "视线"),
    ("看着别处", "视线"),
    ("移开视线", "视线"),
    ("盯着", "视线"),
    ("望过去", "视线"),
    // 声音
    ("压低声音", "声音"),
    ("沉默", "声音"),
    ("没接话", "声音"),
    ("叹了口气", "声音"),
    ("嗓子发紧", "声音"),
    ("提高音量", "声音"),
    ("冷笑", "声音"),
    ("轻声", "声音"),
    ("闷声", "声音"),
];

/// 查某个情绪词（精确匹配）；未登记返回 None
pub fn emotion_of(word: &str) -> Option<(&'static str, &'static str, &'static str)> {
    EMOTIONS
        .iter()
        .find(|(w, _, _, _)| *w == word)
        .map(|(_, g, c, bg)| (*g, *c, *bg))
}

/// 从一段文本里找出所有命中的情绪标签（去重、按词长降序避免"心慌"被"慌"之类干扰）
pub fn emotions_in(text: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (w, _, _, _) in EMOTIONS.iter() {
        if text.contains(w) && !out.contains(w) {
            out.push(w);
        }
    }
    out
}

/// 从一段文本里找出所有命中的动作标签
pub fn actions_in(text: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (w, _) in ACTIONS.iter() {
        if text.contains(w) && !out.contains(w) {
            out.push(w);
        }
    }
    out
}

/// 解析 `current_status` 里**显式写死**的标签：`情绪：感动` / `动作：转过身`。
///
/// 只认这两个前缀（而不是满文本扫），因为 `current_status` 是自由文本，
/// 里面可能恰好含"心疼"这类词但并不是在标情绪。
pub fn parse_status(status: &str) -> (Option<String>, Option<String>) {
    let pick = |key: &str| -> Option<String> {
        let idx = status.find(key)?;
        let rest = &status[idx + key.len()..];
        let rest = rest.trim_start_matches(['：', ':', ' ']);
        // 截到分隔符为止（｜ 。 ； 换行）
        let end = rest
            .find(|c: char| matches!(c, '｜' | '。' | '；' | '\n' | '|'))
            .unwrap_or(rest.len());
        let v = rest[..end].trim();
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    };
    (pick("情绪"), pick("动作"))
}

/// 提取 AI 回复里 `（…）` 括号中的片段（动作/神态描写）
pub fn parenthesized(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '（' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '）' {
                j += 1;
            }
            if j < chars.len() {
                let seg: String = chars[i + 1..j].iter().collect();
                let seg = seg.trim().to_string();
                if !seg.is_empty() && seg.chars().count() <= 60 {
                    out.push(seg);
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// 一轮回复的静默标签：动作 / 情绪（去重），供 `event_logs` 记录。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Performance {
    pub actions: Vec<String>,
    pub emotions: Vec<String>,
}

impl Performance {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty() && self.emotions.is_empty()
    }
    /// 一行摘要（给界面/日志看）：`动作：转身、低头 · 情绪：感动`
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !self.actions.is_empty() {
            parts.push(format!("动作：{}", self.actions.join("、")));
        }
        if !self.emotions.is_empty() {
            parts.push(format!("情绪：{}", self.emotions.join("、")));
        }
        parts.join(" · ")
    }
}

/// 从 AI 回复里抽静默标签。
///
/// 只看**括号段落**（`（…）`）里的动作/情绪 —— 理由：正文里的"感动"可能只是转述，
/// 括号里才是表演。这样误报低得多。
pub fn from_reply(text: &str) -> Performance {
    let mut p = Performance::default();
    for seg in parenthesized(text) {
        for a in actions_in(&seg) {
            if !p.actions.iter().any(|x| x == a) {
                p.actions.push(a.to_string());
            }
        }
        for e in emotions_in(&seg) {
            if !p.emotions.iter().any(|x| x == e) {
                p.emotions.push(e.to_string());
            }
        }
    }
    p
}

/// 记录到 `event_logs`（复用既有表，零迁移）。
///
/// `event_type = 'performance'`，`payload_json = {"actions":[…],"emotions":[…]}`。
/// 失败**不阻断**主流程（表演标签是附加物）。
pub async fn log_performance(
    pool: &sqlx::SqlitePool,
    node_id: i64,
    p: &Performance,
) -> Result<(), sqlx::Error> {
    if p.is_empty() {
        return Ok(());
    }
    let payload = serde_json::json!({
        "actions": p.actions,
        "emotions": p.emotions,
    });
    sqlx::query(
        "INSERT INTO event_logs(node_id, event_type, payload_json, confidence, source)
         VALUES(?1, 'performance', ?2, 0.9, 'rule_engine')",
    )
    .bind(node_id)
    .bind(payload.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// 读某个节点**最近一条**表演标签（来自 `event_logs`）。
///
/// 用途：导演监视器要显示"本场刚才那轮她做了什么动作、是什么情绪"——
/// 这信息本来就在 AI 回复里（`（非常感动）`），只是之前没人接住。
pub async fn latest(
    pool: &sqlx::SqlitePool,
    node_id: i64,
) -> Result<Option<(Performance, String)>, sqlx::Error> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT payload_json, COALESCE(created_at,'') FROM event_logs
         WHERE node_id = ?1 AND event_type = 'performance'
         ORDER BY id DESC LIMIT 1",
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await?;
    let Some((payload, created_at)) = row else {
        return Ok(None);
    };
    let v: serde_json::Value = serde_json::from_str(&payload).unwrap_or(serde_json::Value::Null);
    let get = |k: &str| -> Vec<String> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default()
    };
    let perf = Performance { actions: get("actions"), emotions: get("emotions") };
    if perf.is_empty() {
        Ok(None)
    } else {
        Ok(Some((perf, created_at)))
    }
}

#[cfg(test)]
mod perform_tests;
