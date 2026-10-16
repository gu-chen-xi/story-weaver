use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;
use std::str::FromStr;

use crate::story_db::story_db_path;

async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;
    let _ = crate::story_db::ensure_story_columns(&pool).await;
    let _ = crate::story_db::ensure_branch_columns(&pool).await;
    Ok(pool)
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
}

const SELECT_COLS: &str = "id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion, stat_screen, version";

/// 列出某分支的角色（`None` = 主线）。
pub async fn list_characters(story_id: &str) -> Result<Vec<Character>, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    list_characters_in(&pool, &None).await
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
            version: r.try_get(17)?,
        });
    }
    Ok(out)
}

pub async fn create_character(story_id: &str, input: &CharacterInput) -> Result<i64, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    create_character_in(&pool, &None, input).await
}

/// 第五批 ③：新建角色时写入 `branch_id`（`None` = 主线）。
pub async fn create_character_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    input: &CharacterInput,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO characters(name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, stat_stamina, stat_sanity, stat_emotion, stat_screen, branch_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16) RETURNING id",
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
    .bind(branch.clone())
    .fetch_one(pool)
    .await?;
    Ok(row.try_get(0)?)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_character(story_id: &str, id: i64, input: &CharacterInput) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    update_character_in(&pool, &None, id, input).await
}

/// 第五批 ③：更新角色（带分支条件 —— 防止拿着别的分支的 id 误改本分支数据）。
pub async fn update_character_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    id: i64,
    input: &CharacterInput,
) -> Result<(), sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 17);
    let sql = format!(
        "UPDATE characters SET name=?1, alias=?2, role=?3, faction=?4, age=?5, gender=?6,
         first_appearance=?7, one_liner=?8, profile=?9, speech_style=?10, traits=?11,
         stat_stamina=?12, stat_sanity=?13, stat_emotion=?14, stat_screen=?15, version=version+1
         WHERE id=?16 AND {}",
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
    .bind(id);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.execute(pool).await?;
    Ok(())
}

pub async fn delete_character(story_id: &str, id: i64) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    delete_character_in(&pool, &None, id).await
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
    let pool = open_story_pool(story_id).await?;
    character_usage_in(&pool, &None).await
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
