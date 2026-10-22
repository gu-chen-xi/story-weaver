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
pub(super) async fn add_version(pool: &SqlitePool, portrait_id: i64, desc: &str) -> Result<(), String> {
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
pub(super) async fn load_versions(pool: &SqlitePool, portrait_id: i64) -> Result<Vec<VersionItem>, String> {
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

/// **批量**读取多张素材的版本历史（一次查询，替代"每张一次"的 N+1）。
///
/// 返回 `portrait_id -> 版本列表（按 version 升序）`。
/// 与原逐条版语义保持一致：**查失败就当作空**（不因为版本历史读不出而让整个列表报错）。
pub(super) async fn load_versions_batch(
    pool: &SqlitePool,
    ids: &[i64],
) -> HashMap<i64, Vec<VersionItem>> {
    let mut out: HashMap<i64, Vec<VersionItem>> = HashMap::new();
    if ids.is_empty() {
        return out;
    }
    // SQLite 的绑定参数上限是 999，分批以防素材数量很大
    for chunk in ids.chunks(500) {
        let ph = (1..=chunk.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT portrait_id, version, change_desc, created_at
             FROM portrait_versions
             WHERE portrait_id IN ({ph})
             ORDER BY portrait_id, version"
        );
        let mut q = sqlx::query(&sql);
        for id in chunk {
            q = q.bind(*id);
        }
        let rows = match q.fetch_all(pool).await {
            Ok(r) => r,
            Err(_) => continue, // 与原实现一致：读失败视为"没有版本历史"
        };
        for r in &rows {
            let pid: i64 = match r.try_get(0) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let v: i64 = r.try_get(1).unwrap_or(0);
            let d: String = r.try_get(2).unwrap_or_default();
            let ts: String = r.try_get(3).unwrap_or_default();
            out.entry(pid).or_default().push(VersionItem {
                version: v,
                desc: d,
                date: ts.chars().skip(5).take(5).collect(),
            });
        }
    }
    out
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
        versions: Vec::new(),
    })
}

/// 行转条目 + 异步解析绑定名称。
pub(super) async fn row_to_item_bound(r: &sqlx::sqlite::SqliteRow, pool: &SqlitePool) -> Result<PortraitItem, String> {
    let mut item = row_to_item(r, pool)?;
    let (bs, bc) = resolve_bound(pool, item.story_id, item.character_id).await;
    item.bound_story = bs;
    item.bound_character = bc;
    item.versions = load_versions(pool, item.id).await.unwrap_or_default();
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
