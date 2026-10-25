// 从 `portraits.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `portraits.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use store::*;` 取回，
// 外部 `crate::portraits::xxx` 的调用路径**全部不变**。

use super::*;

pub(super) fn split_tags(s: &str) -> Vec<String> {
    s.split(|c: char| c == ',' || c == '，' || c == '、' || c == ' ')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

pub(super) async fn pool() -> Result<SqlitePool, String> {
    let p = crate::db::library_db_path();
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", p.display()))
        .map_err(|e| e.to_string())?;
    SqlitePool::connect_with(opts).await.map_err(|e| e.to_string())
}

pub(super) async fn ensure_table(pool: &SqlitePool) -> Result<(), String> {
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
        // 来源/派生标注（纯加列，老数据默认 '' = 未知，不影响任何既有逻辑）
        // 这三个列**故意起业务中立的列名**：素材库只知道"有这么三个普通列"，
        // 不知道谁写、写什么、干什么用 —— 任何外部工具都能用它们记录自己的来源信息。
        //   source        来源标记（由调用方定义取值；素材库不解释）
        //   origin_tool   产生它的工具 id（由调用方定义；素材库不解释）
        //   derived_from  派生来源：指向被它重做/替换的那一条（可空）
        ("source", "ALTER TABLE portraits ADD COLUMN source TEXT NOT NULL DEFAULT ''"),
        ("origin_tool", "ALTER TABLE portraits ADD COLUMN origin_tool TEXT NOT NULL DEFAULT ''"),
        ("derived_from", "ALTER TABLE portraits ADD COLUMN derived_from INTEGER"),
    ] {
        if !cols.iter().any(|c| c == col) {
            sqlx::query(ddl).execute(pool).await.map_err(|e| e.to_string())?;
        }
    }
    // 版本历史功能已整体移除（2026-10-25，用户："太吵、多余"）——
    // 老库里那张表连数据一起清掉；新库本来就不会再建它。
    sqlx::query("DROP TABLE IF EXISTS portrait_versions")
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 分组解析：`group_name` 是**逗号分隔的多个组**（2026-10-25 起支持多归属；
/// 老数据只有一个值，照样能解析）。
pub(super) fn split_groups(s: &str) -> Vec<String> {
    s.split(',')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

pub(super) fn row_to_item(r: &sqlx::sqlite::SqliteRow, _pool: &SqlitePool) -> Result<PortraitItem, String> {
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
    // 来源与派生标注（业务中立列；素材库不解释语义，只原样带给界面）
    let source: String = r.try_get(21).unwrap_or_default();
    let origin_tool: String = r.try_get(22).unwrap_or_default();
    let derived_from: Option<i64> = r.try_get(23).unwrap_or(None);
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
        source,
        origin_tool,
        derived_from,
    })
}

/// 行转条目 + 异步解析绑定名称。
pub(super) async fn row_to_item_bound(r: &sqlx::sqlite::SqliteRow, pool: &SqlitePool) -> Result<PortraitItem, String> {
    let mut item = row_to_item(r, pool)?;
    let (bs, bc) = resolve_bound(pool, item.story_id, item.character_id).await;
    item.bound_story = bs;
    item.bound_character = bc;
    Ok(item)
}

pub(super) const COLS: &str =
    "id,name,role,ver,ratio,width,height,size_bytes,path,pinned,tags,created_at,story_id,character_id,group_name,prompt,negative,seed,steps,cfg,model,source,origin_tool,derived_from";

pub(super) fn image_size(path: &Path) -> (i64, i64) {
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
