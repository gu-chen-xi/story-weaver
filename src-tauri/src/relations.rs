use sqlx::Row;
use sqlx::SqlitePool;



/// 第五批 ③：对外函数使用的"当前分支"（读 `stories.current_branch_id`，`None` = 主线）。
///
/// 规则：**对外函数按"当前分支"工作，`_in` 核心版只管"给定分支"**。
/// 这样界面切分支只需调一次 `set_current_branch`，其余命令自动沿用（不会漏传参数）。
async fn current_branch(story_id: &str) -> Option<String> {
    crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None)
}


#[derive(serde::Serialize, Clone)]
pub struct RelationSplit {
    pub text: String,
    pub role: String,
}

#[derive(serde::Serialize, Clone)]
pub struct Relation {
    pub id: i64,
    pub from_character_id: i64,
    pub to_character_id: i64,
    pub relation_type: String,
    pub direction: String,
    pub strength: i64,
    pub state: String,
    pub description: String,
    pub evidence: String,
    pub version: i64,
    pub split_records: Vec<RelationSplit>,
    pub split_matched: bool,
}

/// 可识别的两字亲属词（姐妹 / 兄弟 按需求排除）：词 → 长辈单字 → 晚辈单字
const KIN_PAIRS: &[(&str, &str, &str)] = &[
    ("父女", "父", "女"),
    ("父子", "父", "子"),
    ("母女", "母", "女"),
    ("母子", "母", "子"),
    ("兄妹", "兄", "妹"),
    ("姐弟", "姐", "弟"),
];

fn role_gender(role: &str) -> Option<&'static str> {
    match role {
        "父" | "兄" | "弟" | "子" => Some("男"),
        "母" | "姐" | "妹" | "女" => Some("女"),
        _ => None,
    }
}

/// 关系类型是否命中可识别的两字亲属词（前端只给命中的双向条目开「展开拆分」开关）。
pub fn is_kinship(relation_type: &str) -> bool {
    KIN_PAIRS.iter().any(|(w, _, _)| relation_type.contains(w))
}

fn parse_age(age: &str) -> Option<i64> {
    age.trim().parse::<i64>().ok()
}

/// 双向关系拆成两条单向记录：
/// 1) 先按性别判断长辈（同性的父子/母女进入年龄判断）；
/// 2) 年龄大的是长辈；
/// 3) 都判断不了时，把 from(a) 兜底当长辈。
pub fn split_for_relation(
    direction: &str,
    from_name: &str,
    to_name: &str,
    from_gender: &str,
    to_gender: &str,
    from_age: &str,
    to_age: &str,
    relation_type: &str,
) -> Vec<RelationSplit> {
    if direction != "双向" {
        return Vec::new();
    }
    let rel = relation_type.trim();
    if rel.is_empty() || rel == "陌生人" {
        return vec![RelationSplit {
            text: format!("{} 和 {} 是陌生人", from_name, to_name),
            role: "陌生人".to_string(),
        }];
    }

    // 关键词识别（只认两字词）
    if let Some((_, elder_char, younger_char)) = KIN_PAIRS.iter().find(|(w, _, _)| rel.contains(w)) {
        let elder_gender = role_gender(elder_char).unwrap_or("");
        let from_matches = !from_gender.is_empty() && from_gender == elder_gender;
        let to_matches = !to_gender.is_empty() && to_gender == elder_gender;
        let from_is_elder = if from_matches && !to_matches {
            true
        } else if to_matches && !from_matches {
            false
        } else {
            match (parse_age(from_age), parse_age(to_age)) {
                (Some(a), Some(b)) if a != b => a > b,
                _ => true, // 兜底：from(a) 当长辈
            }
        };
        let (elder_name, younger_name) = if from_is_elder { (from_name, to_name) } else { (to_name, from_name) };
        return vec![
            RelationSplit { text: format!("{} 是 {} 的 {}", elder_name, younger_name, elder_char), role: elder_char.to_string() },
            RelationSplit { text: format!("{} 是 {} 的 {}", younger_name, elder_name, younger_char), role: younger_char.to_string() },
        ];
    }

    // 非亲属词的双向：同一关系类型拆成两条单向
    vec![
        RelationSplit { text: format!("{} 是 {} 的「{}」", from_name, to_name, rel), role: rel.to_string() },
        RelationSplit { text: format!("{} 是 {} 的「{}」", to_name, from_name, rel), role: rel.to_string() },
    ]
}

// ===== 关系强度区间（左闭右开，100 单独归顶档）：档位名 + 含义 + 表演要求 =====
/// (档位名, 区间左值, 区间右值含, 含义, 表演要求)；区间左闭右开，100 单独归"羁绊"
pub const STRENGTH_BANDS: [(&str, i64, i64, &str, &str); 5] = [
    ("疏离", 0, 20, "只有身份的牵连，没有情感投入。", "对话简短、公事化；不主动关心对方，不记得对方的细节；冲突时优先保护自己。"),
    ("相识", 20, 40, "有过交集，有基本好感或轻微戒备。", "会打招呼、会闲聊，但点到为止；对方遇事只给一句场面话，不会出手。"),
    ("亲近", 40, 60, "愿意分享一部分生活，有分寸的往来。", "主动搭话、开玩笑、告知无关紧要的私事；真出事时会犹豫，但仍以自身安危优先。"),
    ("交心", 60, 80, "信任对方，会依赖，会为对方让步。", "重要的事会找对方商量；冲突时愿意先低头；对方的情绪能直接影响自己的行动。"),
    ("羁绊", 80, 100, "视对方为不可或缺的人，愿意牺牲或背负。", "对方受伤害会激烈反应；为对方破例、破规矩、担风险都在所不惜；台词和动作里带明显的占有或守护。"),
];

// ===== 导演指令（沉浸对话专用）：内联标记 + 注入说明 =====
/// 内联标记前缀（前端写入消息内容、后端剥离；改这里必须同步改 App.tsx 的 DIRECTOR_TAG_RE）
pub const DIRECTOR_TAG: &str = "〔导演指令〕";

/// 取出消息里的导演指令内容（指令本身可多行，直到空行结束）；没有则 None
pub fn parse_director(content: &str) -> Option<String> {
    let t = content.trim_start();
    if !t.starts_with(DIRECTOR_TAG) {
        return None;
    }
    let body = &t[DIRECTOR_TAG.len()..];
    let body = body.split("\n\n").next().unwrap_or(body).trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

/// 去掉导演指令行，只留用户正文（用于发给模型 / 调试快照 / 状态提取）
pub fn strip_director(content: &str) -> String {
    let t = content.trim_start();
    if !t.starts_with(DIRECTOR_TAG) {
        return content.to_string();
    }
    let body = &t[DIRECTOR_TAG.len()..];
    match body.split_once("\n\n") {
        Some((_, rest)) => rest.trim_start().to_string(),
        None => String::new(),
    }
}

/// 导演指令的注入说明（只在沉浸对话模式注入；同样不能出现【】）
pub fn director_legend() -> String {
    let mut s = String::from("用户作为导演下达了本次剧情要求（不是用户角色的台词）：");
    s.push_str("\n- 必须执行：优先满足该要求来推进剧情，不必先征得角色同意。");
    s.push_str("\n- 但要写出转变过程：让相关角色经历内心挣扎、被触动、被迫妥协等合理过程，不要突兀硬拗，也不要写成角色突然换了个人。");
    s.push_str("\n- 用户扮演的角色仍由用户掌控（不得替其写台词或心声）；只作用于本次回复，不改变已设定的角色性格、关系与世界观。");
    s
}

/// 强度值 → 档位名。区间左闭右开（含下限不含上限），100 单独归顶档；越界值夹到 0~100
pub fn strength_band(strength: i64) -> &'static str {
    let v = strength.clamp(0, 100);
    for (name, lo, hi, _, _) in STRENGTH_BANDS.iter() {
        if v >= *lo && v < *hi {
            return name;
        }
    }
    "羁绊"
}

/// 关系状态档位（三选一，界面为固定下拉）
pub const RELATION_STATES: [(&str, &str); 3] = [
    ("稳定", "冲突止于拌嘴"),
    ("紧张", "容易升级为争执"),
    ("破裂", "出现回避、断绝、伤害性言行"),
];

/// 状态名 → 含义；非三档取值统一按「稳定」处理
pub fn relation_state_meaning(state: &str) -> &'static str {
    let s = state.trim();
    for (name, meaning) in RELATION_STATES.iter() {
        if s == *name {
            return meaning;
        }
    }
    RELATION_STATES[0].1
}

/// 关系方向读法（跟强度/状态图例一起注入；同样不能出现【】）
/// 单向＝数值是主动方的投入强度；双向＝数值是双方的相互义务；两侧各自记的数值各读各的
pub fn direction_legend() -> String {
    let mut s = String::from("关系方向读法（同一条数值，方向不同读法不同）：");
    s.push_str("\n- 单向：数值代表主动方（箭头起点那一方）的投入强度，只描述主动方；对面那一方视为未明确，按该角色自身性格与其对人的既有态度来演，不主动、不假定同档反应。");
    s.push_str("\n- 双向：数值代表双方的相互义务与牺牲意愿，两人都按该档表现。");
    s.push_str("\n- 两侧各自记录数值时（A→B 与 B→A 各有一条），各读各的，不要合并成平均关系。");
    s
}

/// 关系状态图例（跟强度档位一起注入；同样不能出现【】）
pub fn state_legend() -> String {
    let mut s = String::from("关系状态（当前相处状态，决定冲突时的表现）：");
    for (name, meaning) in RELATION_STATES.iter() {
        s.push_str(&format!("\n- {}：{}", name, meaning));
    }
    s
}

/// 关系强度档位图例（注入【数据字典】段，让 AI 按档位把握表演分寸）
/// 注意：图例正文不能出现【】（提示词预览按【】切段，会被误拆成独立段落）
pub fn strength_legend() -> String {
    let mut s = String::from("关系强度档位（数值越大关系越近，区间含下限不含上限，100 为最高档）：");
    for (name, lo, hi, desc, act) in STRENGTH_BANDS.iter() {
        s.push_str(&format!("\n- {}-{} {}：{} 表演：{}", lo, hi, name, desc, act));
    }
    s.push_str("\n以上档位决定两人的相处分寸与表演方式，请严格按当前数值对应的档位来演；关系类型与「说明」中的既有事实（亲属、称呼、恩怨等）仍然有效，两者冲突时以档位决定分寸、以说明决定事实。");
    s
}

// ===== 关系条目的两种渲染：「方向和关系」（稳定，进 system）与「数值与状态」（易变，进尾部块）=====

/// 「方向和关系」一条：A 是 B 的「类型」（方向：…，说明：…）—— 不含强度数值
pub fn rel_direction_line(text: &str, dir: &str, desc: &str) -> String {
    let d = if desc.trim().is_empty() { String::new() } else { format!("，说明：{}", desc.trim()) };
    format!("- {}（方向：{}{}）", text, dir, d)
}

/// 「数值与状态」一条：A → B：强度 83（羁绊）｜状态：稳定
pub fn rel_value_line(from: &str, to: &str, strength: i64, state: &str) -> String {
    format!("- {} → {}：强度 {}（{}）｜状态：{}（{}）", from, to, strength, strength_band(strength), state, relation_state_meaning(state))
}

/// 断言档位划分（自测）：边界左闭右开、100 归顶档
pub fn assert_strength_bands() {
    assert_eq!(strength_band(0), "疏离");
    assert_eq!(strength_band(19), "疏离");
    assert_eq!(strength_band(20), "相识");
    assert_eq!(strength_band(39), "相识");
    assert_eq!(strength_band(40), "亲近");
    assert_eq!(strength_band(59), "亲近");
    assert_eq!(strength_band(60), "交心");
    assert_eq!(strength_band(79), "交心");
    assert_eq!(strength_band(80), "羁绊");
    assert_eq!(strength_band(99), "羁绊");
    assert_eq!(strength_band(100), "羁绊");
    assert_eq!(strength_band(-5), "疏离");
    assert_eq!(strength_band(120), "羁绊");
}

const SELECT_COLS: &str = "r.id, r.from_character_id, r.to_character_id, r.relation_type, r.direction, r.strength, r.state, r.description, r.evidence, r.version, COALESCE(f.name,''), COALESCE(f.gender,''), COALESCE(f.age,''), COALESCE(t.name,''), COALESCE(t.gender,''), COALESCE(t.age,'')";

pub async fn list_relations(story_id: &str) -> Result<Vec<Relation>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    list_relations_in(&pool, &b).await
}

/// 第五批 ③：分支感知的关系列表（`None` = 主线）。
///
/// 同时给**两端的角色表**加分支条件：分支里的关系指向的是该分支自己的角色行，
/// 主线的角色行不该被别的分支的关系 JOIN 出来。
pub async fn list_relations_in(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<Relation>, sqlx::Error> {
    let rel_clause = crate::story_db::branch_clause("r.branch_id", branch, 1);
    let f_clause = crate::story_db::branch_clause("f.branch_id", branch, 2);
    let t_clause = crate::story_db::branch_clause("t.branch_id", branch, 3);
    let sql = format!(
        "SELECT {} FROM relations r
         LEFT JOIN characters f ON f.id = r.from_character_id AND {}
         LEFT JOIN characters t ON t.id = r.to_character_id AND {}
         WHERE {} ORDER BY r.id",
        SELECT_COLS, f_clause, t_clause, rel_clause
    );
    let mut q = sqlx::query(&sql);
    // 占位符顺序：1=关系分支，2=from 角色分支，3=to 角色分支
    for _ in 0..3 {
        if let Some(b) = branch {
            q = q.bind(b);
        }
    }
    let rows = q.fetch_all(pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        let direction: String = r.try_get(4)?;
        let relation_type: String = r.try_get(3)?;
        let from_name_raw: String = r.try_get(10)?;
        let from_name = if from_name_raw.trim().is_empty() { format!("#{}", r.try_get::<i64, _>(1)?) } else { from_name_raw };
        let from_gender: String = r.try_get(11)?;
        let from_age: String = r.try_get(12)?;
        let to_name_raw: String = r.try_get(13)?;
        let to_name = if to_name_raw.trim().is_empty() { format!("#{}", r.try_get::<i64, _>(2)?) } else { to_name_raw };
        let to_gender: String = r.try_get(14)?;
        let to_age: String = r.try_get(15)?;
        let split_records = split_for_relation(&direction, &from_name, &to_name, &from_gender, &to_gender, &from_age, &to_age, &relation_type);
        let split_matched = is_kinship(&relation_type);
        out.push(Relation {
            id: r.try_get(0)?,
            from_character_id: r.try_get(1)?,
            to_character_id: r.try_get(2)?,
            relation_type,
            direction,
            strength: r.try_get(5)?,
            state: r.try_get(6)?,
            description: r.try_get(7)?,
            evidence: r.try_get(8)?,
            version: r.try_get(9)?,
            split_records,
            split_matched,
        });
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_relation(
    story_id: &str,
    from_id: i64,
    to_id: i64,
    relation_type: &str,
    direction: &str,
    strength: i64,
    state: &str,
    description: &str,
    evidence: &str,
) -> Result<i64, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    create_relation_in(&pool, &b, from_id, to_id, relation_type, direction, strength, state, description, evidence).await
}

/// 第五批 ③：新建关系（写入带上分支）。
#[allow(clippy::too_many_arguments)]
pub async fn create_relation_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    from_id: i64,
    to_id: i64,
    relation_type: &str,
    direction: &str,
    strength: i64,
    state: &str,
    description: &str,
    evidence: &str,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO relations(from_character_id, to_character_id, relation_type, direction, strength, state, description, evidence, branch_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) RETURNING id",
    )
    .bind(from_id)
    .bind(to_id)
    .bind(relation_type)
    .bind(direction)
    .bind(strength)
    .bind(state)
    .bind(description)
    .bind(evidence)
    .bind(branch.clone())
    .fetch_one(pool)
    .await?;
    Ok(row.try_get(0)?)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_relation(
    story_id: &str,
    id: i64,
    relation_type: &str,
    direction: &str,
    strength: i64,
    state: &str,
    description: &str,
    evidence: &str,
) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    update_relation_in(&pool, &b, id, relation_type, direction, strength, state, description, evidence).await
}

/// 第五批 ③：更新关系（带分支条件，防止跨分支误改）。
#[allow(clippy::too_many_arguments)]
pub async fn update_relation_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    id: i64,
    relation_type: &str,
    direction: &str,
    strength: i64,
    state: &str,
    description: &str,
    evidence: &str,
) -> Result<(), sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 8);
    let sql = format!(
        "UPDATE relations SET relation_type=?1, direction=?2, strength=?3, state=?4, description=?5, evidence=?6, version=version+1 WHERE id=?7 AND {}",
        clause
    );
    let mut q = sqlx::query(&sql)
        .bind(relation_type)
        .bind(direction)
        .bind(strength)
        .bind(state)
        .bind(description)
        .bind(evidence)
        .bind(id);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.execute(pool).await?;
    Ok(())
}

pub async fn delete_relation(story_id: &str, id: i64) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    delete_relation_in(&pool, &b, id).await
}

/// 第五批 ③：删除关系（带分支条件）。
pub async fn delete_relation_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    id: i64,
) -> Result<(), sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 2);
    let sql = format!("DELETE FROM relations WHERE id = ?1 AND {}", clause);
    let mut q = sqlx::query(&sql).bind(id);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{assert_strength_bands, split_for_relation};

    #[test]
    fn director_tag_parse_and_strip() {
        let content = "〔导演指令〕强制让夏婉琴接受告白\n\n（宸熙攥紧了拳头）我不管，今天你必须给我答案。";
        assert_eq!(super::parse_director(content).as_deref(), Some("强制让夏婉琴接受告白"));
        assert_eq!(super::strip_director(content), "（宸熙攥紧了拳头）我不管，今天你必须给我答案。");
        let only = "〔导演指令〕让雨下得更大";
        assert_eq!(super::strip_director(only), "");
        assert_eq!(super::parse_director(only).as_deref(), Some("让雨下得更大"));
        let multi = "〔导演指令〕先让院长暴怒\n再让夏婉琴摔门\n\n她说不出话。";
        assert_eq!(super::parse_director(multi).as_deref(), Some("先让院长暴怒\n再让夏婉琴摔门"));
        assert_eq!(super::strip_director(multi), "她说不出话。");
        assert_eq!(super::parse_director("（他笑了）你来了。"), None);
        assert_eq!(super::strip_director("（他笑了）你来了。"), "（他笑了）你来了。");
        let dl = super::director_legend();
        assert!(!dl.contains('【') && !dl.contains('】'));
        // 不能自带标题：注入处已有【导演指令】前缀，重复会出现"导演指令：导演指令（…）"
        assert!(!dl.starts_with("导演指令"), "说明文案不应自带「导演指令」标题");
    }

    #[test]
    fn strength_band_boundaries() {
        // 左闭右开 + 100 单独归顶档
        assert_strength_bands();
    }

    #[test]
    fn legend_has_no_section_marker() {
        // 图例正文不能含【】：提示词预览按【】切段，含了会被误拆成独立段
        let legend = super::strength_legend();
        assert!(!legend.contains('【'), "图例不应包含【标记");
        assert!(!legend.contains('】'), "图例不应包含】标记");
        assert!(legend.contains("0-20 疏离") && legend.contains("80-100 羁绊"));
        let sl = super::state_legend();
        assert!(!sl.contains('【') && !sl.contains('】'), "状态图例不应包含【】标记");
        assert!(sl.contains("稳定：冲突止于拌嘴") && sl.contains("破裂：出现回避、断绝、伤害性言行"));
        let dl = super::direction_legend();
        assert!(!dl.contains('【') && !dl.contains('】'), "方向读法不应包含【】标记");
        assert!(dl.contains("单向：") && dl.contains("双向：") && dl.contains("各读各的"));
    }

    #[test]
    fn relation_state_meaning_fallback() {
        assert_eq!(super::relation_state_meaning("稳定"), "冲突止于拌嘴");
        assert_eq!(super::relation_state_meaning("紧张"), "容易升级为争执");
        assert_eq!(super::relation_state_meaning("破裂"), "出现回避、断绝、伤害性言行");
        // 老数据的自由文本（动摇 / 待确认 / 空）统一按稳定
        assert_eq!(super::relation_state_meaning("动摇"), "冲突止于拌嘴");
        assert_eq!(super::relation_state_meaning(""), "冲突止于拌嘴");
    }

    #[test]
    fn relation_rows_split_into_two_kinds() {
        // 「方向和关系」行：只有方向 + 说明，不含强度数值与状态
        let d = super::rel_direction_line("宸熙 是 夏婉琴 的「明恋」", "单向", "两人高中同学");
        assert!(d.contains("方向：单向"), "缺少方向：{d}");
        assert!(d.contains("说明：两人高中同学"), "缺少说明：{d}");
        assert!(!d.contains("强度"), "方向行不应出现强度：{d}");
        assert!(!d.contains("83"), "方向行不应出现数值：{d}");
        // 「数值与状态」行：强度 + 档位 + 状态 + 含义
        let v = super::rel_value_line("宸熙", "夏婉琴", 83, "稳定");
        assert!(v.contains("强度 83（羁绊）"), "缺少强度档位：{v}");
        assert!(v.contains("状态：稳定（冲突止于拌嘴）"), "缺少状态含义：{v}");
        // 状态含义只随数值行出现，方向行不得带状态
        assert!(!d.contains("状态"), "方向行不应出现状态：{d}");
    }

    #[test]
    fn legends_do_not_repeat_titles() {
        // 各图例都不带【】标题、也不重复「关系强度档位 / 关系状态」之外的标题，
        // 保证注入后每个标题只出现一次（数据字典只放一次）
        let s = super::strength_legend();
        let st = super::state_legend();
        assert!(s.starts_with("关系强度档位"), "强度图例标题异常：{s}");
        assert!(st.starts_with("关系状态"), "状态图例标题异常：{st}");
        for text in [&s, &st] {
            assert!(!text.contains('【') && !text.contains('】'), "图例不得含【】：{text}");
        }
        // 合成数据字典后，标题各自只出现一次
        let dict = format!("{}\n{}", s, st);
        assert_eq!(dict.matches("关系强度档位").count(), 1, "强度标题重复");
        assert_eq!(dict.matches("关系状态").count(), 1, "状态标题重复");
    }

    #[test]
    fn split_kinship_by_gender() {
        let s = split_for_relation("双向", "院长", "夏婉琴", "男", "女", "", "", "养父女");
        assert_eq!(s.len(), 2);
        assert!(s[0].text.contains("院长 是 夏婉琴 的 父"));
        assert!(s[1].text.contains("夏婉琴 是 院长 的 女"));
    }

    #[test]
    fn split_same_gender_by_age() {
        let s = split_for_relation("双向", "甲", "乙", "女", "女", "45", "20", "母女");
        assert_eq!(s.len(), 2);
        assert!(s[0].text.contains("甲 是 乙 的 母"));
        assert!(s[1].text.contains("乙 是 甲 的 女"));
    }

    #[test]
    fn split_fallback_from_when_unknown() {
        let s = split_for_relation("双向", "甲", "乙", "", "", "", "", "姐弟");
        assert_eq!(s.len(), 2);
        assert!(s[0].text.contains("甲 是 乙 的 姐"));
    }

    #[test]
    fn split_generic_bilateral_and_stranger() {
        let g = split_for_relation("双向", "甲", "乙", "", "", "", "", "同学");
        assert_eq!(g.len(), 2);
        assert!(g[0].text.contains("甲 是 乙 的「同学」"));
        let s = split_for_relation("双向", "甲", "乙", "", "", "", "", "陌生人");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].text, "甲 和 乙 是陌生人");
    }

    #[test]
    fn single_direction_is_not_split() {
        assert!(split_for_relation("单向", "甲", "乙", "男", "女", "", "", "父女").is_empty());
    }
}
