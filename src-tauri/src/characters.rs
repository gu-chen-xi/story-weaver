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
    #[serde(default)] pub motivation: String,
    #[serde(default)] pub goal: String,
    #[serde(default)] pub dialogue_examples: String,
    #[serde(default)] pub behavior_rules: String,
}

const SELECT_COLS: &str = "id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion, stat_screen, version, background_past, motivation, goal, dialogue_examples, behavior_rules";

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
        "INSERT INTO characters(name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, stat_stamina, stat_sanity, stat_emotion, stat_screen, background_past, motivation, goal, dialogue_examples, behavior_rules, branch_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21) RETURNING id",
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
    let clause = crate::story_db::branch_clause("branch_id", branch, 22);
    let sql = format!(
        "UPDATE characters SET name=?1, alias=?2, role=?3, faction=?4, age=?5, gender=?6,
         first_appearance=?7, one_liner=?8, profile=?9, speech_style=?10, traits=?11,
         stat_stamina=?12, stat_sanity=?13, stat_emotion=?14, stat_screen=?15,
         background_past=?16, motivation=?17, goal=?18, dialogue_examples=?19, behavior_rules=?20,
         version=version+1
         WHERE id=?21 AND {}",
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
// 为什么需要这一层：四个属性值（体力/理智/情绪/存在感，0-100）一直只存在库里和界面上，
// **模型完全看不到** —— 所以"角色状态"演不出变化。但直接把数字塞给模型有两个坑：
//   ① 模型会把它当**游戏血条**来演（"我只剩 30% 体力了！"），破坏叙事语感；
//   ② 它不知道"存在感"是什么（数据字典里原先只有关系强度/状态的解释）。
// 所以这里做两件事：把数值翻译成**档位语义 + 括号里的数字**；并在数据字典里解释四维含义。

/// 数值 → 档位语义。边界用 20/40/60/80，与关系强度的 0-100 分区同量级。
pub fn stat_tier(v: i64) -> &'static str {
    match v.clamp(0, 100) {
        0..=19 => "很低",
        20..=39 => "偏低",
        40..=59 => "正常",
        60..=79 => "偏高",
        _ => "很高",
    }
}

/// 一个角色的属性行（进尾部「数值与状态」块）：
/// `- 夏婉琴：体力 偏低(30) · 理智 正常(55) · 情绪 很高(90) · 存在感 偏低(25)`
pub fn stat_value_line(name: &str, stamina: i64, sanity: i64, emotion: i64, screen: i64) -> String {
    format!(
        "- {}：体力 {}({}) · 理智 {}({}) · 情绪 {}({}) · 存在感 {}({})",
        name,
        stat_tier(stamina),
        stamina,
        stat_tier(sanity),
        sanity,
        stat_tier(emotion),
        emotion,
        stat_tier(screen),
        screen
    )
}

/// 数据字典里对四维的解释（只说"怎么用"，具体数值在「数值与状态」块里）。
pub fn stat_legend() -> String {
    [
        "角色四维属性（0-100，仅作倾向参考，不要写成游戏血条式的台词或数值播报）：",
        "- 体力：越低越疲惫、动作越迟钝、越容易露出破绽；",
        "- 理智：越低越冲动、越听不进劝、越容易说漏真话；",
        "- 情绪：越高越外放（激动、兴奋、压不住），越低越内敛（冷淡、麻木）；",
        "- 存在感：越低越容易被忽略（插不上话、被人忘了），越高越受注目。",
        "表现方式：靠动作、语气、身体反应间接体现（如「手在抖」「没接话」「难得提高音量」），不直接报数字。",
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
        let s = stat_value_line("夏婉琴", 30, 55, 90, 25);
        assert!(s.starts_with("- 夏婉琴："), "{s}");
        assert!(s.contains("体力 偏低(30)"), "{s}");
        assert!(s.contains("理智 正常(55)"), "{s}");
        assert!(s.contains("情绪 很高(90)"), "{s}");
        assert!(s.contains("存在感 偏低(25)"), "{s}");
        assert_eq!(s.matches(" · ").count(), 3, "四段之间应有 3 个分隔符：{s}");
    }

    /// 数据字典必须解释「存在感」，且明确禁止血条式播报
    #[test]
    fn stat_legend_explains_screen_and_forbids_hp_talk() {
        let s = stat_legend();
        for k in ["体力", "理智", "情绪", "存在感"] {
            assert!(s.contains(k), "数据字典漏了 {k}");
        }
        assert!(s.contains("游戏血条"), "缺少'别演血条'的约束");
        assert!(s.contains("0-100"), "缺少取值范围说明");
    }
}
