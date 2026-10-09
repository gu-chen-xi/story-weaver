use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;
use std::str::FromStr;

use crate::story_db::story_db_path;

async fn open_story_pool(story_id: &str) -> Result<SqlitePool, sqlx::Error> {
    let path = story_db_path(story_id);
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))?;
    let pool = SqlitePool::connect_with(opts).await?;
    let _ = crate::story_db::ensure_story_columns(&pool).await;
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

#[derive(serde::Deserialize, Clone)]
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

pub async fn list_characters(story_id: &str) -> Result<Vec<Character>, sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    let rows = sqlx::query(&format!("SELECT {} FROM characters ORDER BY id", SELECT_COLS))
        .fetch_all(&pool)
        .await?;
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
    let row = sqlx::query(
        "INSERT INTO characters(name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, stat_stamina, stat_sanity, stat_emotion, stat_screen)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15) RETURNING id",
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
    .fetch_one(&pool)
    .await?;
    Ok(row.try_get(0)?)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_character(story_id: &str, id: i64, input: &CharacterInput) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    sqlx::query(
        "UPDATE characters SET name=?1, alias=?2, role=?3, faction=?4, age=?5, gender=?6,
         first_appearance=?7, one_liner=?8, profile=?9, speech_style=?10, traits=?11,
         stat_stamina=?12, stat_sanity=?13, stat_emotion=?14, stat_screen=?15, version=version+1
         WHERE id=?16",
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
    .bind(id)
    .execute(&pool)
    .await?;
    Ok(())
}

pub async fn delete_character(story_id: &str, id: i64) -> Result<(), sqlx::Error> {
    let pool = open_story_pool(story_id).await?;
    sqlx::query("DELETE FROM characters WHERE id = ?1")
        .bind(id)
        .execute(&pool)
        .await?;
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
    let rows = sqlx::query("SELECT id, name FROM characters ORDER BY id")
        .fetch_all(&pool)
        .await?;
    let mut out = Vec::new();
    for r in &rows {
        let id: i64 = r.try_get(0)?;
        let name: String = r.try_get(1)?;
        let marker = format!("[{}]", name);
        let dialogue_count: i64 = sqlx::query(
            "SELECT COUNT(*) FROM messages WHERE role='assistant' AND instr(content, ?1) > 0",
        )
        .bind(&marker)
        .fetch_one(&pool)
        .await?
        .try_get(0)?;
        let node_count: i64 = sqlx::query(
            "SELECT COUNT(DISTINCT node_id) FROM messages WHERE role='assistant' AND instr(content, ?1) > 0",
        )
        .bind(&marker)
        .fetch_one(&pool)
        .await?
        .try_get(0)?;
        out.push(CharacterUsage { id, dialogue_count, node_count });
    }
    Ok(out)
}
