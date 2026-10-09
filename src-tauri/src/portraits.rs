use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// 立绘素材目录：<项目根>/library/default/shared/portraits（全局共享素材库）
/// 返回规范化绝对路径（消除 `..`，asset 协议要求路径不可穿越目录）。
pub fn portraits_dir() -> PathBuf {
    let raw = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("..")
        .join("library")
        .join("default")
        .join("shared")
        .join("portraits");
    std::fs::create_dir_all(&raw).ok();
    raw.canonicalize().unwrap_or(raw)
}

#[derive(Clone, serde::Serialize)]
pub struct VersionItem {
    pub version: i64,
    pub desc: String,
    pub date: String, // MM-DD
}

#[derive(serde::Serialize)]
pub struct PortraitItem {
    pub id: i64,
    pub name: String,
    pub role: String,
    pub ver: String,
    pub ratio: String,
    pub width: i64,
    pub height: i64,
    pub size: String,       // 人类可读（KB/MB）
    pub size_bytes: i64,
    pub date: String,       // MM-DD
    pub pinned: bool,
    pub tags: Vec<String>,
    pub g: String,
    pub path: Option<String>, // 绝对路径（有图时）
    pub story_id: Option<i64>,     // 绑定故事（library.db stories.id）
    pub character_id: Option<i64>, // 绑定角色（story.db characters.id）
    pub bound_story: Option<String>,
    pub bound_character: Option<String>,
    pub group: String,
    pub prompt: String,
    pub negative: String,
    pub seed: String,
    pub steps: String,
    pub cfg: String,
    pub model: String,
    pub versions: Vec<VersionItem>,
}

fn fmt_size(b: i64) -> String {
    if b >= 1_048_576 {
        format!("{:.1} MB", b as f64 / 1_048_576.0)
    } else if b >= 1024 {
        format!("{:.0} KB", b as f64 / 1024.0)
    } else {
        format!("{} B", b)
    }
}

fn split_tags(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c == '，' || c == '、' || c == ' ')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

async fn pool() -> Result<SqlitePool, String> {
    let p = crate::db::library_db_path();
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", p.display()))
        .map_err(|e| e.to_string())?;
    SqlitePool::connect_with(opts).await.map_err(|e| e.to_string())
}

async fn ensure_table(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS portraits (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            name         TEXT NOT NULL,
            role         TEXT NOT NULL DEFAULT '',
            ver          TEXT NOT NULL DEFAULT 'v1',
            ratio        TEXT NOT NULL DEFAULT '',
            width        INTEGER NOT NULL DEFAULT 0,
            height       INTEGER NOT NULL DEFAULT 0,
            size_bytes   INTEGER NOT NULL DEFAULT 0,
            path         TEXT,
            pinned       INTEGER NOT NULL DEFAULT 0,
            tags         TEXT NOT NULL DEFAULT '',
            story_id     INTEGER,
            character_id INTEGER,
            group_name   TEXT NOT NULL DEFAULT '',
            prompt       TEXT NOT NULL DEFAULT '',
            negative     TEXT NOT NULL DEFAULT '',
            seed         TEXT NOT NULL DEFAULT '',
            steps        TEXT NOT NULL DEFAULT '',
            cfg          TEXT NOT NULL DEFAULT '',
            model        TEXT NOT NULL DEFAULT '',
            created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            updated_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )"
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    // 兼容旧表：缺 story_id / character_id 列时补加
    let cols: Vec<String> = sqlx::query("PRAGMA table_info(portraits)")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .filter_map(|r| r.try_get::<String, _>(1).ok())
        .collect();
    if !cols.iter().any(|c| c == "story_id") {
        sqlx::query("ALTER TABLE portraits ADD COLUMN story_id INTEGER")
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    if !cols.iter().any(|c| c == "character_id") {
        sqlx::query("ALTER TABLE portraits ADD COLUMN character_id INTEGER")
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    if !cols.iter().any(|c| c == "group_name") {
        sqlx::query("ALTER TABLE portraits ADD COLUMN group_name TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    for (col, ddl) in [
        ("prompt", "ALTER TABLE portraits ADD COLUMN prompt TEXT NOT NULL DEFAULT ''"),
        ("negative", "ALTER TABLE portraits ADD COLUMN negative TEXT NOT NULL DEFAULT ''"),
        ("seed", "ALTER TABLE portraits ADD COLUMN seed TEXT NOT NULL DEFAULT ''"),
        ("steps", "ALTER TABLE portraits ADD COLUMN steps TEXT NOT NULL DEFAULT ''"),
        ("cfg", "ALTER TABLE portraits ADD COLUMN cfg TEXT NOT NULL DEFAULT ''"),
        ("model", "ALTER TABLE portraits ADD COLUMN model TEXT NOT NULL DEFAULT ''"),
    ] {
        if !cols.iter().any(|c| c == col) {
            sqlx::query(ddl).execute(pool).await.map_err(|e| e.to_string())?;
        }
    }
    // 版本历史表：每次内容性变更自动追加一条，version 连续递增
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS portrait_versions (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            portrait_id  INTEGER NOT NULL,
            version      INTEGER NOT NULL,
            change_desc  TEXT NOT NULL DEFAULT '',
            created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )"
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    // 存量补录 A：已有素材无版本记录时补 v1（有图=导入，无图=占位创建）
    let missing: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT p.id, p.path FROM portraits p
         WHERE NOT EXISTS (SELECT 1 FROM portrait_versions v WHERE v.portrait_id = p.id)"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    for (id, path) in missing {
        let desc = if path.is_some() { "导入图片素材" } else { "创建占位素材" };
        add_version(pool, id, desc).await?;
    }
    // 存量补录 B：已有版本记录但第一条不是「导入/创建」的素材（历史早于版本功能启用），
    // 原记录版本号整体 +1，再插入 version=1 的初始记录，补出 v1=导入/创建
    let misplaced: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT p.id, p.path FROM portraits p
         WHERE EXISTS (SELECT 1 FROM portrait_versions v WHERE v.portrait_id = p.id)
           AND NOT EXISTS (SELECT 1 FROM portrait_versions v2
                           WHERE v2.portrait_id = p.id AND v2.change_desc IN ('导入图片素材','创建占位素材'))"
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    for (id, path) in misplaced {
        let desc = if path.is_some() { "导入图片素材" } else { "创建占位素材" };
        // 先整体 +1（1→2、2→3…），再插入 version=1 的初始记录
        sqlx::query("UPDATE portrait_versions SET version = version + 1 WHERE portrait_id = ?1")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("INSERT INTO portrait_versions(portrait_id, version, change_desc) VALUES(?1, 1, ?2)")
            .bind(id)
            .bind(&desc)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 为素材追加一条版本记录（version = 该素材当前最大版本 + 1）。
async fn add_version(pool: &SqlitePool, portrait_id: i64, desc: &str) -> Result<(), String> {
    let maxv: Option<i64> =
        sqlx::query_scalar("SELECT MAX(version) FROM portrait_versions WHERE portrait_id = ?1")
            .bind(portrait_id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
    let v = maxv.unwrap_or(0) + 1;
    sqlx::query("INSERT INTO portrait_versions(portrait_id, version, change_desc) VALUES(?1,?2,?3)")
        .bind(portrait_id)
        .bind(v)
        .bind(desc)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 读取某素材的完整版本历史（按版本号升序）。
async fn load_versions(pool: &SqlitePool, portrait_id: i64) -> Result<Vec<VersionItem>, String> {
    let rows = sqlx::query(
        "SELECT version, change_desc, created_at FROM portrait_versions
         WHERE portrait_id = ?1 ORDER BY version"
    )
    .bind(portrait_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        let v: i64 = r.try_get(0).map_err(|e: sqlx::Error| e.to_string())?;
        let d: String = r.try_get(1).map_err(|e: sqlx::Error| e.to_string())?;
        let ts: String = r.try_get(2).map_err(|e: sqlx::Error| e.to_string())?;
        out.push(VersionItem {
            version: v,
            desc: d,
            date: ts.chars().skip(5).take(5).collect(),
        });
    }
    Ok(out)
}

/// 解析绑定名称：故事标题来自 library.db，角色名来自该故事的 story.db。
async fn resolve_bound(
    pool: &SqlitePool,
    story_id: Option<i64>,
    character_id: Option<i64>,
) -> (Option<String>, Option<String>) {
    let mut bs: Option<String> = None;
    let mut bc: Option<String> = None;
    if let Some(sid) = story_id {
        if let Ok(r) = sqlx::query("SELECT title FROM stories WHERE id = ?1")
            .bind(sid)
            .fetch_one(pool)
            .await
        {
            if let Ok(t) = r.try_get::<String, _>(0) {
                bs = Some(t);
            }
        }
        if let Some(cid) = character_id {
            let p = crate::story_db::story_db_path(&sid.to_string());
            if let Ok(opts) = SqliteConnectOptions::from_str(&format!("sqlite:{}", p.display())) {
                if let Ok(sp) = SqlitePool::connect_with(opts).await {
                    if let Ok(r) = sqlx::query("SELECT name FROM characters WHERE id = ?1")
                        .bind(cid)
                        .fetch_one(&sp)
                        .await
                    {
                        if let Ok(nm) = r.try_get::<String, _>(0) {
                            bc = Some(nm);
                        }
                    }
                }
            }
        }
    }
    (bs, bc)
}

fn row_to_item(r: &sqlx::sqlite::SqliteRow, pool: &SqlitePool) -> Result<PortraitItem, String> {
    let id: i64 = r.try_get(0).map_err(|e: sqlx::Error| e.to_string())?;
    let name: String = r.try_get(1).map_err(|e: sqlx::Error| e.to_string())?;
    let role: String = r.try_get(2).map_err(|e: sqlx::Error| e.to_string())?;
    let ver: String = r.try_get(3).map_err(|e: sqlx::Error| e.to_string())?;
    let ratio: String = r.try_get(4).map_err(|e: sqlx::Error| e.to_string())?;
    let width: i64 = r.try_get(5).map_err(|e: sqlx::Error| e.to_string())?;
    let height: i64 = r.try_get(6).map_err(|e: sqlx::Error| e.to_string())?;
    let size_bytes: i64 = r.try_get(7).map_err(|e: sqlx::Error| e.to_string())?;
    let path: Option<String> = r.try_get(8).map_err(|e: sqlx::Error| e.to_string())?;
    let pinned: i64 = r.try_get(9).map_err(|e: sqlx::Error| e.to_string())?;
    let tags: String = r.try_get(10).map_err(|e: sqlx::Error| e.to_string())?;
    let created: String = r.try_get(11).map_err(|e: sqlx::Error| e.to_string())?;
    let story_id: Option<i64> = r.try_get(12).map_err(|e: sqlx::Error| e.to_string())?;
    let character_id: Option<i64> = r.try_get(13).map_err(|e: sqlx::Error| e.to_string())?;
    let group: String = r.try_get(14).map_err(|e: sqlx::Error| e.to_string())?;
    let prompt: String = r.try_get(15).map_err(|e: sqlx::Error| e.to_string())?;
    let negative: String = r.try_get(16).map_err(|e: sqlx::Error| e.to_string())?;
    let seed: String = r.try_get(17).map_err(|e: sqlx::Error| e.to_string())?;
    let steps: String = r.try_get(18).map_err(|e: sqlx::Error| e.to_string())?;
    let cfg: String = r.try_get(19).map_err(|e: sqlx::Error| e.to_string())?;
    let model: String = r.try_get(20).map_err(|e: sqlx::Error| e.to_string())?;
    let abs = path.map(|f| portraits_dir().join(&f).display().to_string());
    Ok(PortraitItem {
        id,
        name,
        role,
        ver,
        ratio,
        width,
        height,
        size: fmt_size(size_bytes),
        size_bytes,
        date: created.chars().skip(5).take(5).collect(),
        pinned: pinned != 0,
        tags: split_tags(&tags),
        g: format!("g{}", (id % 6) + 1),
        path: abs,
        story_id,
        character_id,
        bound_story: None,
        bound_character: None,
        group,
        prompt,
        negative,
        seed,
        steps,
        cfg,
        model,
        versions: Vec::new(),
    })
}

/// 行转条目 + 异步解析绑定名称。
async fn row_to_item_bound(r: &sqlx::sqlite::SqliteRow, pool: &SqlitePool) -> Result<PortraitItem, String> {
    let mut item = row_to_item(r, pool)?;
    let (bs, bc) = resolve_bound(pool, item.story_id, item.character_id).await;
    item.bound_story = bs;
    item.bound_character = bc;
    item.versions = load_versions(pool, item.id).await.unwrap_or_default();
    Ok(item)
}

const COLS: &str =
    "id,name,role,ver,ratio,width,height,size_bytes,path,pinned,tags,created_at,story_id,character_id,group_name,prompt,negative,seed,steps,cfg,model";

/// 全部素材（按最近更新倒序）。
#[tauri::command]
pub async fn portrait_list() -> Result<Vec<PortraitItem>, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let rows = sqlx::query(
        &format!("SELECT {} FROM portraits ORDER BY updated_at DESC, id DESC", COLS)
    )
    .fetch_all(&p)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(row_to_item_bound(r, &p).await?);
    }
    Ok(out)
}

fn image_size(path: &Path) -> (i64, i64) {
    match image::ImageReader::open(path) {
        Ok(reader) => match reader.with_guessed_format() {
            Ok(r) => match r.decode() {
                Ok(img) => (img.width() as i64, img.height() as i64),
                Err(_) => (0, 0),
            },
            Err(_) => (0, 0),
        },
        Err(_) => (0, 0),
    }
}

/// 导入本地图片：复制到素材目录 + 登记元数据，返回新素材。
#[tauri::command]
pub async fn portrait_import(
    srcPath: String,
    name: String,
    role: String,
    ver: String,
    tags: String,
    group: String,
    storyId: Option<i64>,
    characterId: Option<i64>,
) -> Result<PortraitItem, String> {
    let src = Path::new(&srcPath);
    if !src.exists() {
        return Err(format!("文件不存在：{}", srcPath));
    }
    let meta = std::fs::metadata(src).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("所选路径不是文件".to_string());
    }
    let size_bytes = meta.len() as i64;
    let (w, h) = image_size(src);
    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let dir = portraits_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let fname = format!("p{}.{}", millis, ext);
    let dst = dir.join(&fname);
    std::fs::copy(src, &dst).map_err(|e| e.to_string())?;

    let ratio = if w > 0 && h > 0 {
        format!("{}×{}", w, h)
    } else {
        "未知".to_string()
    };
    let p = pool().await?;
    ensure_table(&p).await?;
    let row = sqlx::query(
        "INSERT INTO portraits(name,role,ver,ratio,width,height,size_bytes,path,tags,group_name,story_id,character_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         RETURNING id,created_at"
    )
    .bind(&name)
    .bind(&role)
    .bind(&ver)
    .bind(&ratio)
    .bind(w)
    .bind(h)
    .bind(size_bytes)
    .bind(&fname)
    .bind(&tags)
    .bind(&group.trim())
    .bind(storyId)
    .bind(characterId)
    .fetch_one(&p)
    .await
    .map_err(|e| e.to_string())?;
    let id: i64 = row.try_get(0).map_err(|e: sqlx::Error| e.to_string())?;
    let created: String = row.try_get(1).map_err(|e: sqlx::Error| e.to_string())?;
    add_version(&p, id, "导入图片素材").await?;
    let (bound_story, bound_character) = resolve_bound(&p, storyId, characterId).await;
    let dv: String = created.chars().skip(5).take(5).collect();
    Ok(PortraitItem {
        id,
        name,
        role,
        ver,
        ratio,
        width: w,
        height: h,
        size: fmt_size(size_bytes),
        size_bytes,
        date: dv.clone(),
        pinned: false,
        tags: split_tags(&tags),
        g: format!("g{}", (id % 6) + 1),
        path: Some(dst.display().to_string()),
        story_id: storyId,
        character_id: characterId,
        bound_story,
        bound_character,
        group: String::new(),
        prompt: String::new(),
        negative: String::new(),
        seed: String::new(),
        steps: String::new(),
        cfg: String::new(),
        model: String::new(),
        versions: vec![VersionItem { version: 1, desc: "导入图片素材".to_string(), date: dv }],
    })
}

/// 新建素材（无图占位，后续可导入/绑定图片）。
#[tauri::command]
pub async fn portrait_create(
    name: String,
    role: String,
    ver: String,
    ratio: String,
    tags: String,
    storyId: Option<i64>,
    characterId: Option<i64>,
) -> Result<PortraitItem, String> {
    let ratio = if ratio.trim().is_empty() {
        "待补充".to_string()
    } else {
        ratio.trim().to_string()
    };
    let p = pool().await?;
    ensure_table(&p).await?;
    let row = sqlx::query(
        "INSERT INTO portraits(name,role,ver,ratio,tags,story_id,character_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7) RETURNING id,created_at"
    )
    .bind(&name)
    .bind(&role)
    .bind(&ver)
    .bind(&ratio)
    .bind(&tags)
    .bind(storyId)
    .bind(characterId)
    .fetch_one(&p)
    .await
    .map_err(|e| e.to_string())?;
    let id: i64 = row.try_get(0).map_err(|e: sqlx::Error| e.to_string())?;
    let created: String = row.try_get(1).map_err(|e: sqlx::Error| e.to_string())?;
    add_version(&p, id, "创建占位素材").await?;
    let (bound_story, bound_character) = resolve_bound(&p, storyId, characterId).await;
    let dv: String = created.chars().skip(5).take(5).collect();
    Ok(PortraitItem {
        id,
        name,
        role,
        ver,
        ratio,
        width: 0,
        height: 0,
        size: "—".to_string(),
        size_bytes: 0,
        date: dv.clone(),
        pinned: false,
        tags: split_tags(&tags),
        g: format!("g{}", (id % 6) + 1),
        path: None,
        story_id: storyId,
        character_id: characterId,
        bound_story,
        bound_character,
        group: String::new(),
        prompt: String::new(),
        negative: String::new(),
        seed: String::new(),
        steps: String::new(),
        cfg: String::new(),
        model: String::new(),
        versions: vec![VersionItem { version: 1, desc: "创建占位素材".to_string(), date: dv }],
    })
}

/// 绑定素材到某故事的某个角色（传 null 解除绑定）。
#[tauri::command]
pub async fn portrait_bind(
    id: i64,
    storyId: Option<i64>,
    characterId: Option<i64>,
) -> Result<PortraitItem, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    sqlx::query(
        "UPDATE portraits SET story_id = ?1, character_id = ?2, updated_at = datetime('now','localtime')
         WHERE id = ?3"
    )
    .bind(storyId)
    .bind(characterId)
    .bind(id)
    .execute(&p)
    .await
    .map_err(|e| e.to_string())?;
    let row = sqlx::query(&format!("SELECT {} FROM portraits WHERE id = ?1", COLS))
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    row_to_item_bound(&row, &p).await
}


/// 批量设置分组（group_name 字段）。返回更新条数。
#[tauri::command]
pub async fn portrait_group_set(ids: Vec<i64>, group: String) -> Result<usize, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let g = group.trim().to_string();
    let mut n = 0usize;
    for id in &ids {
        sqlx::query(
            "UPDATE portraits SET group_name = ?1, updated_at = datetime('now','localtime') WHERE id = ?2"
        )
        .bind(&g)
        .bind(id)
        .execute(&p)
        .await
        .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// 批量追加标签（去重合并）。返回更新条数。
#[tauri::command]
pub async fn portrait_tag_batch(ids: Vec<i64>, tags: String) -> Result<usize, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let mut n = 0usize;
    for id in &ids {
        let old: String = sqlx::query_scalar("SELECT tags FROM portraits WHERE id = ?1")
            .bind(id)
            .fetch_one(&p)
            .await
            .map_err(|e| e.to_string())?;
        let mut set: Vec<String> = split_tags(&old);
        for t in split_tags(&tags) {
            if !set.contains(&t) {
                set.push(t);
            }
        }
        sqlx::query(
            "UPDATE portraits SET tags = ?1, updated_at = datetime('now','localtime') WHERE id = ?2"
        )
        .bind(set.join(","))
        .bind(id)
        .execute(&p)
        .await
        .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// 批量导出图片到 exports/portrait/。返回摘要文本。
#[tauri::command]
pub async fn portrait_export(ids: Vec<i64>) -> Result<String, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("..")
        .join("exports")
        .join("portrait");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dir = dir.canonicalize().unwrap_or(dir);
    let mut n = 0usize;
    let mut skip = 0usize;
    for id in &ids {
        let row = sqlx::query("SELECT name, path FROM portraits WHERE id = ?1")
            .bind(id)
            .fetch_optional(&p)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(r) = row {
            let name: String = r.try_get(0).map_err(|e: sqlx::Error| e.to_string())?;
            let fname: Option<String> = r.try_get(1).map_err(|e: sqlx::Error| e.to_string())?;
            match fname {
                Some(f) => {
                    let src = portraits_dir().join(&f);
                    if src.exists() {
                        let ext = f.rsplit('.').next().unwrap_or("png");
                        let clean: String = name
                            .chars()
                            .map(|c| {
                                if "\\/:*?\"<>| ".contains(c) {
                                    '_'
                                } else {
                                    c
                                }
                            })
                            .collect();
                        let dst = dir.join(format!("{}_{}.{}", clean, id, ext));
                        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
                        n += 1;
                    } else {
                        skip += 1;
                    }
                }
                None => { skip += 1; }
            }
        } else {
            skip += 1;
        }
    }
    Ok(format!(
        "✅ 已导出 {}/{} 个素材 → {}（{} 个无图/缺失跳过）",
        n,
        ids.len(),
        dir.display(),
        skip
    ))
}

/// 批量删除素材（删记录 + 删图片文件）。返回删除条数。
#[tauri::command]
pub async fn portrait_delete(ids: Vec<i64>) -> Result<usize, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let mut n = 0usize;
    for id in &ids {
        let fname: Option<String> = sqlx::query_scalar("SELECT path FROM portraits WHERE id = ?1")
            .bind(id)
            .fetch_optional(&p)
            .await
            .map_err(|e| e.to_string())?
            .flatten();
        if let Some(f) = fname {
            let f = portraits_dir().join(&f);
            let _ = std::fs::remove_file(&f);
        }
        sqlx::query("DELETE FROM portraits WHERE id = ?1")
            .bind(id)
            .execute(&p)
            .await
            .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}


/// 更新素材生成参数（提示词 / 负向提示词 / seed / steps / cfg / 模型）。内容变化时记录版本。
#[tauri::command]
pub async fn portrait_update_params(
    id: i64,
    prompt: String,
    negative: String,
    seed: String,
    steps: String,
    cfg: String,
    model: String,
) -> Result<PortraitItem, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let old: (String, String, String, String, String, String) = sqlx::query_as(
        "SELECT prompt, negative, seed, steps, cfg, model FROM portraits WHERE id = ?1"
    )
    .bind(id)
    .fetch_one(&p)
    .await
    .map_err(|e| e.to_string())?;
    let n_prompt = prompt.trim().to_string();
    let n_neg = negative.trim().to_string();
    let n_seed = seed.trim().to_string();
    let n_steps = steps.trim().to_string();
    let n_cfg = cfg.trim().to_string();
    let n_model = model.trim().to_string();
    sqlx::query(
        "UPDATE portraits SET prompt = ?1, negative = ?2, seed = ?3, steps = ?4, cfg = ?5, model = ?6, updated_at = datetime('now','localtime') WHERE id = ?7"
    )
    .bind(&n_prompt)
    .bind(&n_neg)
    .bind(&n_seed)
    .bind(&n_steps)
    .bind(&n_cfg)
    .bind(&n_model)
    .bind(id)
    .execute(&p)
    .await
    .map_err(|e| e.to_string())?;
    let joined = |t: (String, String, String, String, String, String)| -> String {
        format!("{}\x01{}\x01{}\x01{}\x01{}\x01{}", t.0, t.1, t.2, t.3, t.4, t.5)
    };
    if joined(old) != joined((
        n_prompt.clone(),
        n_neg.clone(),
        n_seed.clone(),
        n_steps.clone(),
        n_cfg.clone(),
        n_model.clone(),
    )) {
        add_version(&p, id, "修改生成参数").await?;
    }
    let row = sqlx::query(&format!("SELECT {} FROM portraits WHERE id = ?1", COLS))
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    row_to_item_bound(&row, &p).await
}

/// 更新素材字段（当前支持修改角色 role）。返回更新后的素材。
#[tauri::command]
pub async fn portrait_update(id: i64, role: String) -> Result<PortraitItem, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let old: String = sqlx::query_scalar("SELECT role FROM portraits WHERE id = ?1")
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    let new = role.trim().to_string();
    sqlx::query(
        "UPDATE portraits SET role = ?1, updated_at = datetime('now','localtime') WHERE id = ?2"
    )
    .bind(&new)
    .bind(id)
    .execute(&p)
    .await
    .map_err(|e| e.to_string())?;
    if old != new {
        let desc = if old.is_empty() {
            format!("设置角色：{}", new)
        } else if new.is_empty() {
            format!("清空角色：{} → 未设置", old)
        } else {
            format!("修改角色：{} → {}", old, new)
        };
        add_version(&p, id, &desc).await?;
    }
    let row = sqlx::query(&format!("SELECT {} FROM portraits WHERE id = ?1", COLS))
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    row_to_item_bound(&row, &p).await
}
