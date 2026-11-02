use sqlx::sqlite::SqlitePool;
use sqlx::Row;



/// 第五批 ③：对外函数使用的"当前分支"（读 `stories.current_branch_id`，`None` = 主线）。
///
/// 规则：**对外函数按"当前分支"工作，`_in` 核心版只管"给定分支"**。
/// 这样界面切分支只需调一次 `set_current_branch`，其余命令自动沿用（不会漏传参数）。
async fn current_branch(story_id: &str) -> Option<String> {
    crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None)
}


#[derive(serde::Serialize, Clone)]
pub struct Character {
    pub id: i64,
    pub name: String,
    pub alias: String,
    pub role: String,
    pub faction: String,
    pub age: String,
    pub gender: String,
    pub first_appearance: String,
    pub one_liner: String,
    pub profile: String,
    pub speech_style: String,
    pub traits: String,
    pub current_status: String,
    pub stat_stamina: i64,
    pub stat_sanity: i64,
    pub stat_emotion: i64,
    pub stat_screen: i64,
    /// v3.4：过去（他的往事）
    pub background_past: String,
    /// 外貌（v3.4）：作者设定，AI 白名单里**故意没有**这一列
    pub appearance: String,
    /// v3.4：动机（他为什么这么做）
    pub motivation: String,
    /// v3.4：目的（他想达成什么）
    pub goal: String,
    /// v3.4：对话示例（few-shot；只有焦点角色会进提示词）
    pub dialogue_examples: String,
    /// v3.4：行为准则（他会怎么做 / 绝不做什​么）
    pub behavior_rules: String,
    pub version: i64,
}

fn default_stat() -> i64 { 50 }

#[derive(serde::Deserialize, Clone, Default)]
pub struct CharacterInput {
    #[serde(default)] pub name: String,
    #[serde(default)] pub alias: String,
    #[serde(default)] pub role: String,
    #[serde(default)] pub faction: String,
    #[serde(default)] pub age: String,
    #[serde(default)] pub gender: String,
    #[serde(default)] pub first_appearance: String,
    #[serde(default)] pub one_liner: String,
    #[serde(default)] pub profile: String,
    #[serde(default)] pub speech_style: String,
    #[serde(default)] pub traits: String,
    #[serde(default = "default_stat")] pub stat_stamina: i64,
    #[serde(default = "default_stat")] pub stat_sanity: i64,
    #[serde(default = "default_stat")] pub stat_emotion: i64,
    #[serde(default = "default_stat")] pub stat_screen: i64,
    // v3.4「角色卡 v2」：全部 serde(default)，老前端不传也不会报错
    #[serde(default)] pub background_past: String,
    #[serde(default)] pub appearance: String,
    #[serde(default)] pub motivation: String,
    #[serde(default)] pub goal: String,
    #[serde(default)] pub dialogue_examples: String,
    #[serde(default)] pub behavior_rules: String,
}

const SELECT_COLS: &str = "id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion, stat_screen, version, background_past, motivation, goal, dialogue_examples, behavior_rules, appearance";

/// 列出某分支的角色（`None` = 主线）。
pub async fn list_characters(story_id: &str) -> Result<Vec<Character>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    list_characters_in(&pool, &b).await
}

/// 第五批 ③：分支感知的角色读取（核心版，接 pool）。
/// `branch` 为 `None` 时查主线（`branch_id IS NULL`）—— 现有数据全是主线，行为与之前一致。
pub async fn list_characters_in(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<Character>, sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let sql = format!("SELECT {} FROM characters WHERE {} ORDER BY id", SELECT_COLS, clause);
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(Character {
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
            background_past: r.try_get(18)?,
            appearance: r.try_get(23)?,
            motivation: r.try_get(19)?,
            goal: r.try_get(20)?,
            dialogue_examples: r.try_get(21)?,
            behavior_rules: r.try_get(22)?,
            version: r.try_get(17)?,
        });
    }
    Ok(out)
}

pub async fn create_character(story_id: &str, input: &CharacterInput) -> Result<i64, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    create_character_in(&pool, &b, input).await
}

/// 第五批 ③：新建角色时写入 `branch_id`（`None` = 主线）。
pub async fn create_character_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    input: &CharacterInput,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO characters(name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, stat_stamina, stat_sanity, stat_emotion, stat_screen, background_past, motivation, goal, dialogue_examples, behavior_rules, branch_id, appearance)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22) RETURNING id",
    )
    .bind(&input.name)
    .bind(&input.alias)
    .bind(&input.role)
    .bind(&input.faction)
    .bind(&input.age)
    .bind(&input.gender)
    .bind(&input.first_appearance)
    .bind(&input.one_liner)
    .bind(&input.profile)
    .bind(&input.speech_style)
    .bind(&input.traits)
    .bind(input.stat_stamina)
    .bind(input.stat_sanity)
    .bind(input.stat_emotion)
    .bind(input.stat_screen)
    .bind(&input.background_past)
    .bind(&input.motivation)
    .bind(&input.goal)
    .bind(&input.dialogue_examples)
    .bind(&input.behavior_rules)
    .bind(branch.clone())
    .bind(&input.appearance)
    .fetch_one(pool)
    .await?;
    Ok(row.try_get(0)?)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_character(story_id: &str, id: i64, input: &CharacterInput) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    update_character_in(&pool, &b, id, input).await
}

/// 第五批 ③：更新角色（带分支条件 —— 防止拿着别的分支的 id 误改本分支数据）。
pub async fn update_character_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    id: i64,
    input: &CharacterInput,
) -> Result<(), sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 23);
    let sql = format!(
        "UPDATE characters SET name=?1, alias=?2, role=?3, faction=?4, age=?5, gender=?6,
         first_appearance=?7, one_liner=?8, profile=?9, speech_style=?10, traits=?11,
         stat_stamina=?12, stat_sanity=?13, stat_emotion=?14, stat_screen=?15,
         background_past=?16, motivation=?17, goal=?18, dialogue_examples=?19, behavior_rules=?20,
         appearance=?21,
         version=version+1
         WHERE id=?22 AND {}",
        clause
    );
    let mut q = sqlx::query(&sql)
        .bind(&input.name)
    .bind(&input.alias)
    .bind(&input.role)
    .bind(&input.faction)
    .bind(&input.age)
    .bind(&input.gender)
    .bind(&input.first_appearance)
    .bind(&input.one_liner)
    .bind(&input.profile)
    .bind(&input.speech_style)
    .bind(&input.traits)
    .bind(input.stat_stamina)
    .bind(input.stat_sanity)
    .bind(input.stat_emotion)
    .bind(input.stat_screen)
    .bind(&input.background_past)
    .bind(&input.motivation)
    .bind(&input.goal)
    .bind(&input.dialogue_examples)
    .bind(&input.behavior_rules)
    .bind(&input.appearance)
    .bind(id);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.execute(pool).await?;
    Ok(())
}

pub async fn delete_character(story_id: &str, id: i64) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    delete_character_in(&pool, &b, id).await
}

/// 第五批 ③：删除角色（带分支条件）。
pub async fn delete_character_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    id: i64,
) -> Result<(), sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 2);
    let sql = format!("DELETE FROM characters WHERE id = ?1 AND {}", clause);
    let mut q = sqlx::query(&sql).bind(id);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.execute(pool).await?;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct CharacterUsage {
    pub id: i64,
    pub dialogue_count: i64,
    pub node_count: i64,
}

/// 按 AI 台词里的 [角色名] 标记统计台词条数与出场节点数（用于 A 卡墙底部元信息）。
pub async fn character_usage(story_id: &str) -> Result<Vec<CharacterUsage>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    character_usage_in(&pool, &b).await
}

/// 第五批 ③：角色的使用统计（角色列表按分支；台词统计走 messages，随节点天然隔离）。
pub async fn character_usage_in(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<CharacterUsage>, sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let sql = format!("SELECT id, name FROM characters WHERE {} ORDER BY id", clause);
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get(0)?;
        let name: String = r.try_get(1)?;
        let marker = format!("[{}]", name);
        let dialogue_count: i64 = sqlx::query(
            "SELECT COUNT(*) FROM messages WHERE role='assistant' AND instr(content, ?1) > 0",
        )
        .bind(&marker)
        .fetch_one(pool)
        .await?
        .try_get(0)?;
        let node_count: i64 = sqlx::query(
            "SELECT COUNT(DISTINCT node_id) FROM messages WHERE role='assistant' AND instr(content, ?1) > 0",
        )
        .bind(&marker)
        .fetch_one(pool)
        .await?
        .try_get(0)?;
        out.push(CharacterUsage { id, dialogue_count, node_count });
    }
    Ok(out)
}

// ===== 角色四维属性 → 提示词（v3.2 ①）=====
//
// 为什么需要这一层：属性值（体力/理智/存在感，0-100）一直只存在库里和界面上，
// **模型完全看不到** —— 所以"角色状态"演不出变化。但直接把数字塞给模型有两个坑：
//   ① 模型会把它当**游戏血条**来演（"我只剩 30% 体力了！"），破坏叙事语感；
//   ② 它不知道"存在感"是什么（数据字典里原先只有关系强度/状态的解释）。
// 所以这里做两件事：把数值翻译成**档位语义 + 括号里的数字**；并在数据字典里解释四维含义。

/// 数值 → 档位语义（单一真相见 STAT_TIERS / stat_tier_index）。
pub fn stat_tier(v: i64) -> &'static str {
    STAT_TIERS[stat_tier_index(v)]
}

// ===== 2026-11-02「数值影响台词」：三维 → 台词量 =====
//
// 用户的三个决定：
//   ① 阈值**不分角色**（全角色统一 20/40/60/80，暂不做角色级配置）；
//   ② 台词量给**硬上限**（软提示会被模型放水，这个项目所有翻车都是这么来的）；
//   ③ 规则进 system（静态、吃前缀缓存），只有每轮会变的「数值/结论」进尾部块。
//
// 分工（别改）：**数值只决定「表现得出多少」，不决定「愿不愿意」**。
// 愿不愿意是关系强度的事（见 relations::coop_legend），这里只压「说得出来多少字」。

/// 三维档位名（下标 = 五档）
pub const STAT_TIERS: [&str; 5] = ["很低", "偏低", "正常", "偏高", "很高"];

/// 数值 → 档位下标（0..4）。边界 20/40/60/80，与关系强度同量级。
pub fn stat_tier_index(v: i64) -> usize {
    match v.clamp(0, 100) {
        0..=19 => 0,
        20..=39 => 1,
        40..=59 => 2,
        60..=79 => 3,
        _ => 4,
    }
}

/// 每档的「台词表现」。正常档（下标 2）是空串 —— 正常就不注入，省 token。
const SANITY_PLAY: [&str; 5] = [
    "只剩短句，句子可能断在半截；会把平时不肯说的那句说漏",
    "短句为主，缓冲词少，容易抢话",
    "",
    "能绕、能控场，话说一半留一半",
    "措辞精确，能说服人，也能压人一头",
];
const STAMINA_PLAY: [&str; 5] = [
    "说话带气口、句子短，动作少而重，容易被一件事打断",
    "避长句、少动作，躲开要花力气的场面",
    "",
    "",
    "",
];
const SCREEN_PLAY: [&str; 5] = [
    "话常没人接：说了要重复、被抢话、说到一半被岔开",
    "话要重复一遍才有人应",
    "",
    "说话有人等",
    "出场先被看见：可以先不说话，别人会停下来等她",
];

/// 台词硬上限：`(每句最多字数, 本段最多句数)`。
///
/// 理智决定底子（还剩多少语言），体力只**收紧**不放松（撑不住的人一次说不长），取更严的一组。
/// 用户 2026-11-02 定「给硬上限」—— 所以这里是能验数的规则，不是「尽量简短」这种方向话。
pub fn dialogue_cap(sanity: i64, stamina: i64) -> (usize, usize) {
    let base = [(15usize, 3usize), (25, 5), (40, 8), (50, 10), (60, 12)][stat_tier_index(sanity)];
    let tight = [
        (20usize, 3usize),
        (30, 6),
        (usize::MAX, usize::MAX),
        (usize::MAX, usize::MAX),
        (usize::MAX, usize::MAX),
    ][stat_tier_index(stamina)];
    (base.0.min(tight.0), base.1.min(tight.1))
}

/// 本轮的「台词口径」一行（进尾部【数值与当下】）。全正常 → None（零 token）。
pub fn perf_directive(name: &str, stamina: i64, sanity: i64, screen: i64) -> Option<String> {
    let (st, sa, sc) = (stat_tier_index(stamina), stat_tier_index(sanity), stat_tier_index(screen));
    let mut notes: Vec<String> = Vec::new();
    if sa != 2 {
        notes.push(format!("理智 {}({}) → {}", STAT_TIERS[sa], sanity, SANITY_PLAY[sa]));
    }
    if st != 2 {
        notes.push(format!("体力 {}({}) → {}", STAT_TIERS[st], stamina, STAMINA_PLAY[st]));
    }
    if sc != 2 {
        notes.push(format!("存在感 {}({}) → {}", STAT_TIERS[sc], screen, SCREEN_PLAY[sc]));
    }
    if notes.is_empty() {
        return None;
    }
    let (cap_chars, cap_lines) = dialogue_cap(sanity, stamina);
    Some(format!(
        "本段台词口径：{} {}；台词硬上限：每句 ≤{} 字、本段 ≤{} 句",
        name,
        notes.join("；"),
        cap_chars,
        cap_lines
    ))
}

/// 【台词口径】段（进 system，静态）。**规则进这里、数字进尾部**（用户 2026-11-02 定）。
///
/// 为什么要有优先级链：三条轴会互相打架（她体力 10，但关系强度 85）。
/// 定死「数值只管表现、不管意愿」，模型就不会拿「体力高」去换「答应」。
pub fn performance_rules() -> String {
    [
        "【台词口径】数值只决定「表现得出多少」，**不决定「愿不愿意」**。冲突时按这个顺序裁决：",
        "- 关系态势（破裂 → 拒绝/回避优先）＞ 关系强度（愿不愿意配合）＞ 理智（语言量）＞ 体力（气口）＞ 存在感（是否被打断）。",
        "- 理智：0-19 只剩短句、会说漏；20-39 短句为主、缓冲词少；40-59 照常；60-79 能绕能控场；80-100 措辞精确、能压人。",
        "- 体力：0-19 句子短、气口多、易被打断；20-39 避长句、少动作；40 以上不额外限制。",
        "- 存在感：0-19 话常没人接（被抢话、被岔开）；20-39 要重复一遍才有人应；40-79 照常；80-100 出场先被看见。",
        "- **台词硬上限**（理智定底子、体力只收紧，取更严的一组）：理智 0-19＝每句 ≤15 字、本段 ≤3 句；20-39＝≤25 字、≤5 句；40-59＝≤40 字、≤8 句；60-79＝≤50 字、≤10 句；80-100＝≤60 字、≤12 句。体力 0-19 再压到 ≤20 字、≤3 句；体力 20-39 压到 ≤30 字、≤6 句。",
        "- 超了上限就**删字**，不许改成「多说几句短的」来凑；语气词、停顿、破折号都算在字数里。",
        "- 永远不要把这些数字写进正文：不出现「体力 -15」「理智 20」这类血条式表达，只让它影响说话的样子。",
    ]
    .join("\n")
}

/// 一个角色的属性行（进尾部「数值与状态」块）：
/// `- 夏婉琴：体力 偏低(30) · 理智 正常(55) · 存在感 偏低(25)｜当下「刚被拒绝，强撑着」`
///
/// **情绪不再有数值**（用户 2026-10-22 定）：情感是质的东西，压成一个 0-100 反而失真，
/// 也省掉"数值档位 × 状态文字"的联合判断。情绪改由 `status`（角色的当前状态文字）表达。
pub fn stat_value_line(name: &str, stamina: i64, sanity: i64, screen: i64, status: &str) -> String {
    let st = status.trim();
    let tail = if st.is_empty() {
        String::new()
    } else {
        format!("｜当下「{}」", st)
    };
    format!(
        "- {}：体力 {}({}) · 理智 {}({}) · 存在感 {}({}){}",
        name,
        stat_tier(stamina),
        stamina,
        stat_tier(sanity),
        sanity,
        stat_tier(screen),
        screen,
        tail
    )
}

/// 数据字典里对角色属性的解释（只说"怎么用"，具体数值在「数值与状态」块里）。
///
/// 情绪不进这里：它由「状态」文字表达（见 `stat_value_line` 的注释）。
pub fn stat_legend() -> String {
    [
        "Character attributes (0-100, hints for performance only — never narrate them as numbers or as a game HP bar):",
        "- 体力 (stamina): lower = more tired, slower, more likely to slip up;",
        "- 理智 (sanity): lower = more impulsive, less persuadable, more likely to blurt out the truth;",
        "- 存在感 (presence): lower = easier to be overlooked (cut off, forgotten); higher = more attention.",
        "Emotion: read it from the character's 当下 (present moment — what they feel/are doing), NOT from a number.",
        "  e.g. 崩溃 → crying, resolve, hysteria; 强撑 → clipped speech, stiff movement, sudden silence.",
        "  Do not default to sweet/coquettish lines; do not default to refusal either.",
        "How to show it: through action, tone and body reactions (手在抖 / 没接话 / 难得提高音量) — never state the number.",
        "All narrative output must be in Simplified Chinese.",
    ]
    .join("\n")
}

#[cfg(test)]
mod stat_prompt_tests {
    use super::*;

    /// 档位边界：低值必须能明显区分；越界脏数据不许 panic
    #[test]
    fn stat_tier_boundaries() {
        assert_eq!(stat_tier(0), "很低");
        assert_eq!(stat_tier(19), "很低");
        assert_eq!(stat_tier(20), "偏低");
        assert_eq!(stat_tier(39), "偏低");
        assert_eq!(stat_tier(40), "正常");
        assert_eq!(stat_tier(59), "正常");
        assert_eq!(stat_tier(60), "偏高");
        assert_eq!(stat_tier(79), "偏高");
        assert_eq!(stat_tier(80), "很高");
        assert_eq!(stat_tier(100), "很高");
        assert_eq!(stat_tier(-5), "很低", "负数按 0 处理");
        assert_eq!(stat_tier(999), "很高", "超 100 按 100 处理");
    }

    /// 属性行格式：名字 + 四段「档位(数字)」，用 · 分隔
    #[test]
    fn stat_value_line_shape() {
        let s = stat_value_line("夏婉琴", 30, 55, 25, "刚被拒绝，强撑着");
        assert!(s.starts_with("- 夏婉琴："), "{s}");
        assert!(s.contains("体力 偏低(30)"), "{s}");
        assert!(s.contains("理智 正常(55)"), "{s}");
        assert!(s.contains("当下「刚被拒绝，强撑着」"), "当下文字必须出现：{s}");
        assert!(!s.contains("情绪"), "情绪不该再有数值：{s}");
        assert!(s.contains("存在感 偏低(25)"), "{s}");
        // 三维之间 2 个「 · 」；状态用「｜」分隔（不占档位分隔符）\n        assert_eq!(s.matches(" · ").count(), 2, "三维之间应有 2 个分隔符：{s}");
    }

    /// 数据字典必须解释「存在感」，且明确禁止血条式播报
    #[test]
    fn stat_legend_explains_screen_and_forbids_hp_talk() {
        let s = stat_legend();
        for k in ["体力", "理智", "存在感"] {
            assert!(s.contains(k), "数据字典漏了 {k}");
        }
        // 英文化试点：约束改成英文表述，但要仍在（别演血条 = game HP bar）
        assert!(s.contains("game HP bar"), "缺少'别演血条'的约束：{s}");
        assert!(s.contains("All narrative output must be in Simplified Chinese"), "缺少输出语言约束：{s}");
        assert!(s.contains("0-100"), "缺少取值范围说明");
    }
    // ===== 2026-11-02「数值影响台词」=====

    /// 档位下标与档位名必须一一对应（两处单一真相）
    #[test]
    fn tier_index_matches_names() {
        for (v, name) in [(0, "很低"), (25, "偏低"), (45, "正常"), (65, "偏高"), (95, "很高")] {
            assert_eq!(STAT_TIERS[stat_tier_index(v)], name, "v={v}");
            assert_eq!(stat_tier(v), name, "v={v}");
        }
    }

    /// 硬上限：理智定底子、体力只收紧，取更严的一组
    #[test]
    fn dialogue_cap_takes_the_stricter_side() {
        assert_eq!(dialogue_cap(90, 90), (60, 12), "全高 → 宽松档");
        assert_eq!(dialogue_cap(10, 90), (15, 3), "理智很低 → 每句 15 字、3 句");
        assert_eq!(dialogue_cap(90, 10), (20, 3), "体力很低收紧到 20 字 3 句");
        assert_eq!(dialogue_cap(50, 50), (40, 8), "全正常 → 40 字 8 句");
        assert_eq!(dialogue_cap(50, 30), (30, 6), "体力偏低收紧到 30 字 6 句");
        assert_eq!(dialogue_cap(30, 10), (20, 3), "两边都紧 → 取更严");
    }

    /// 全正常 → 一行都不加（零 token）；有异常 → 带数字、带硬上限
    #[test]
    fn perf_directive_is_silent_when_everything_is_normal() {
        assert!(perf_directive("夏婉琴", 50, 50, 50).is_none(), "正常档不该占 token");
        let low = perf_directive("夏婉琴", 50, 15, 50).unwrap();
        assert!(low.contains("理智 很低(15)"), "{low}");
        assert!(low.contains("每句 ≤15 字"), "{low}");
        assert!(low.contains("本段 ≤3 句"), "{low}");
        let weak = perf_directive("夏婉琴", 12, 50, 50).unwrap();
        assert!(weak.contains("体力 很低(12)"), "{weak}");
        assert!(weak.contains("每句 ≤20 字"), "体力只收紧：{weak}");
        let star = perf_directive("夏婉琴", 50, 50, 85).unwrap();
        assert!(star.contains("出场先被看见"), "{star}");
        assert!(star.contains("每句 ≤40 字"), "存在感不压字数：{star}");
    }

    /// 规则段：优先级链、硬上限数字、不许血条式播报；且不能再出现【】（预览按【】切段）
    #[test]
    fn performance_rules_carry_the_chain_and_the_caps() {
        let s = performance_rules();
        assert!(s.starts_with("【台词口径】"), "{s}");
        for k in ["愿不愿意", "关系态势", "关系强度", "理智", "体力", "存在感"] {
            assert!(s.contains(k), "缺 {k}：{s}");
        }
        assert!(s.contains("每句 ≤15 字"), "硬上限要写死数字：{s}");
        assert!(s.contains("血条"), "要有禁止播报数字的约束：{s}");
        // 预览按【】切段：除标题外不许再出现【
        assert_eq!(s.matches('【').count(), 1, "除标题外不许出现【：{s}");
        assert!(s.chars().count() < 900, "规则段太长，会挤占预算：{}", s.chars().count());
    }
}
