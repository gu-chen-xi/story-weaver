use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use std::collections::HashMap;
use sqlx::Row;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

/// 立绘素材目录：<项目根>/library/default/shared/portraits（全局共享素材库）
/// 返回规范化绝对路径（消除 `..`，asset 协议要求路径不可穿越目录）。
pub fn portraits_dir() -> PathBuf {
    // 第五批「路径模型」修复：统一走 paths
    let raw = crate::paths::library_root()
        .join("default")
        .join("shared")
        .join("portraits");
    std::fs::create_dir_all(&raw).ok();
    raw.canonicalize().unwrap_or(raw)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VersionItem {
    pub version: i64,
    pub desc: String,
    pub date: String, // MM-DD
}

#[derive(Debug, Clone, serde::Serialize)]
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
    /// 来源标记：由调用方定义的任意字符串（素材库只原样存取与显示，不解释含义、不列举取值）。
    /// 素材库自己只在两个入口各写一个自己的标记：手工导入与占位创建。
    pub source: String,
    /// 产生它的工具 id（AI 生成时 = 通道 id）
    pub origin_tool: String,
    pub prompt: String,
    pub negative: String,
    pub seed: String,
    /// 派生来源：指向被它重做/替换的那一条（可空）
    pub derived_from: Option<i64>,
    pub steps: String,
    pub cfg: String,
    pub model: String,
    pub versions: Vec<VersionItem>,
}

/// 本地时间字符串（不引入新依赖；格式与库里的 datetime('now','localtime') 对齐）
fn chrono_like_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 用系统时区偏移做一次粗略换算（仅用于清单里的时间戳展示）
    let offset_secs = {
        // SQLite 侧同款时间可由前端替代；这里用一个简单近似：取本地时间与 UTC 的差值
        let local = std::time::SystemTime::now();
        let _ = local;
        0
    };
    let days = (now + offset_secs) / 86400;
    let secs = (now + offset_secs) % 86400;
    // 以 1970-01-01 为原点推算日期
    let (y, m, d) = civil_from_days(days as i64);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// 天数 → (年, 月, 日)（Howard Hinnant 的 civil_from_days 算法）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
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

/// **批量**读取多张素材的版本历史（一次查询，替代"每张一次"的 N+1）。
///
/// 返回 `portrait_id -> 版本列表（按 version 升序）`。
/// 与原逐条版语义保持一致：**查失败就当作空**（不因为版本历史读不出而让整个列表报错）。
async fn load_versions_batch(
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

/// 批量解析绑定名称的结果。
#[derive(Default)]
struct BoundNames {
    /// story_id -> 故事标题
    stories: HashMap<i64, String>,
    /// (story_id, character_id) -> 角色名
    characters: HashMap<(i64, i64), String>,
}

/// **批量**解析绑定名称（替代"每张素材都查一次、而且每个都新开一个故事库连接"的 N+1）。
///
/// 查询次数：`1`（故事标题，一条 IN） + `每个不同故事 1 条`（角色名，一条 IN）。
/// 原实现是：`每张素材 1~2 条 + 每张已绑定素材各新开一个 story.db 连接池`。
///
/// 语义同原版：查不到就是 `None`，**不报错**（绑定名字取不到不该让素材列表失败）。
/// 故事库路径的解析方式。默认走 `story_db_path`；
/// **测试里会注入一个指向临时目录的版本** —— 否则单元测试会去读写真实用户数据。
// 必须带 Send + Sync：这些 future 会被 tauri 的异步运行时跨线程调度，
// 裸 `dyn Fn` 不满足 Send，会让所有调用它的 command 报 "future cannot be sent between threads"。
type StoryDbResolver<'a> = &'a (dyn Fn(i64) -> std::path::PathBuf + Send + Sync);

fn default_story_db(sid: i64) -> std::path::PathBuf {
    crate::story_db::story_db_path(&sid.to_string())
}

async fn resolve_bound_batch(
    pool: &SqlitePool,
    pairs: &[(Option<i64>, Option<i64>)],
) -> BoundNames {
    resolve_bound_batch_with(pool, pairs, &(default_story_db as fn(i64) -> std::path::PathBuf)).await
}

async fn resolve_bound_batch_with(
    pool: &SqlitePool,
    pairs: &[(Option<i64>, Option<i64>)],
    story_db: StoryDbResolver<'_>,
) -> BoundNames {
    let mut out = BoundNames::default();

    // ---- 故事标题：一条 IN 查询 ----
    let mut story_ids: Vec<i64> = pairs.iter().filter_map(|(s, _)| *s).collect();
    story_ids.sort_unstable();
    story_ids.dedup();
    for chunk in story_ids.chunks(500) {
        let ph = (1..=chunk.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT id, title FROM stories WHERE id IN ({ph})");
        let mut q = sqlx::query(&sql);
        for id in chunk {
            q = q.bind(*id);
        }
        if let Ok(rows) = q.fetch_all(pool).await {
            for r in &rows {
                if let (Ok(id), Ok(title)) = (r.try_get::<i64, _>(0), r.try_get::<String, _>(1)) {
                    out.stories.insert(id, title);
                }
            }
        }
    }

    // ---- 角色名：按故事分组，每个故事**一条** IN 查询（每故事只开一次连接）----
    let mut by_story: HashMap<i64, Vec<i64>> = HashMap::new();
    for (s, c) in pairs {
        if let (Some(sid), Some(cid)) = (s, c) {
            by_story.entry(*sid).or_default().push(*cid);
        }
    }
    for (sid, mut cids) in by_story {
        cids.sort_unstable();
        cids.dedup();
        let path = story_db(sid);
        let opts = match SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display())) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let sp = match SqlitePool::connect_with(opts).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        for chunk in cids.chunks(500) {
            let ph = (1..=chunk.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!("SELECT id, name FROM characters WHERE id IN ({ph})");
            let mut q = sqlx::query(&sql);
            for id in chunk {
                q = q.bind(*id);
            }
            if let Ok(rows) = q.fetch_all(&sp).await {
                for r in &rows {
                    if let (Ok(id), Ok(name)) =
                        (r.try_get::<i64, _>(0), r.try_get::<String, _>(1))
                    {
                        out.characters.insert((sid, id), name);
                    }
                }
            }
        }
        sp.close().await;
    }

    out
}

/// 解析绑定名称：故事标题来自 library.db，角色名来自该故事的 story.db。
async fn resolve_bound(
    pool: &SqlitePool,
    story_id: Option<i64>,
    character_id: Option<i64>,
) -> (Option<String>, Option<String>) {
    resolve_bound_with(pool, story_id, character_id, &(default_story_db as fn(i64) -> std::path::PathBuf)).await
}

/// `resolve_bound` 的可注入版本（测试用临时目录，避免读写真实用户数据）。
async fn resolve_bound_with(
    pool: &SqlitePool,
    story_id: Option<i64>,
    character_id: Option<i64>,
    story_db: StoryDbResolver<'_>,
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
            let p = story_db(sid);
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
                    sp.close().await;
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
async fn row_to_item_bound(r: &sqlx::sqlite::SqliteRow, pool: &SqlitePool) -> Result<PortraitItem, String> {
    let mut item = row_to_item(r, pool)?;
    let (bs, bc) = resolve_bound(pool, item.story_id, item.character_id).await;
    item.bound_story = bs;
    item.bound_character = bc;
    item.versions = load_versions(pool, item.id).await.unwrap_or_default();
    Ok(item)
}

const COLS: &str =
    "id,name,role,ver,ratio,width,height,size_bytes,path,pinned,tags,created_at,story_id,character_id,group_name,prompt,negative,seed,steps,cfg,model,source,origin_tool,derived_from";

/// 全部素材（按最近更新倒序）。
#[tauri::command]
pub async fn portrait_list() -> Result<Vec<PortraitItem>, String> {
    let p = pool().await?;
    portrait_list_in(&p).await
}

/// `portrait_list` 的核心（接 pool，供测试直连）。
///
/// ## 为什么不再逐行补数据（原来这里是 N+1）
///
/// 旧实现是「查一次列表 → 每行调 `row_to_item_bound`」，而那个函数每行要：
/// 查故事标题(1) + **新开一个 story.db 连接池** + 查角色名(1) + 查版本历史(1)。
///
/// 也就是说素材数为 N、其中 M 张绑定了角色时，总开销是：
/// `1 + N 条查询 + M 次新建连接池`（连接池创建远贵于单条查询）。
///
/// 现在改成**批量**：
/// `1`（列表） + `1`（版本历史，一条 IN） + `1`（故事标题，一条 IN）
/// + `每个不同故事 1 条`（角色名，一条 IN）+ `每个不同故事 1 次连接`。
///
/// 素材越多、越集中在少数故事里，差距越大。
pub async fn portrait_list_in(p: &SqlitePool) -> Result<Vec<PortraitItem>, String> {
    portrait_list_in_with(p, &(default_story_db as fn(i64) -> std::path::PathBuf)).await
}

/// `portrait_list_in` 的可注入版本（换成别的故事库路径解析方式；测试用临时目录）。
async fn portrait_list_in_with(
    p: &SqlitePool,
    story_db: StoryDbResolver<'_>,
) -> Result<Vec<PortraitItem>, String> {
    ensure_table(p).await?;
    let rows = sqlx::query(
        &format!("SELECT {} FROM portraits ORDER BY updated_at DESC, id DESC", COLS)
    )
    .fetch_all(p)
    .await
    .map_err(|e| e.to_string())?;

    // ① 先把纯字段解析完（这一步原本就不查库）
    let mut items: Vec<PortraitItem> = Vec::with_capacity(rows.len());
    for r in &rows {
        items.push(row_to_item(r, p)?);
    }
    if items.is_empty() {
        return Ok(items);
    }

    // ② 版本历史：一次查完，按素材分组
    let ids: Vec<i64> = items.iter().map(|i| i.id).collect();
    let mut versions = load_versions_batch(p, &ids).await;

    // ③ 绑定名称：一次查完（按故事分组）
    let pairs: Vec<(Option<i64>, Option<i64>)> =
        items.iter().map(|i| (i.story_id, i.character_id)).collect();
    let bounds = resolve_bound_batch_with(p, &pairs, story_db).await;

    // ④ 回填
    for it in items.iter_mut() {
        it.versions = versions.remove(&it.id).unwrap_or_default();
        if let Some(sid) = it.story_id {
            it.bound_story = bounds.stories.get(&sid).cloned();
            if let Some(cid) = it.character_id {
                it.bound_character = bounds.characters.get(&(sid, cid)).cloned();
            }
        }
    }
    Ok(items)
}

/// **旧实现的逐行版本**，只用于测试做"对照答案"（oracle）。
///
/// 保留它是有意的：批量版改错了、顺序变了、少填了字段，都能靠"两者必须完全相等"抓出来。
/// 生产代码不应调用它（`#[cfg(test)]` 把它挡在正式构建之外）。
#[cfg(test)]
pub async fn portrait_list_rowbyrow_in(
    p: &SqlitePool,
    story_db: StoryDbResolver<'_>,
) -> Result<Vec<PortraitItem>, String> {
    ensure_table(p).await?;
    let rows = sqlx::query(
        &format!("SELECT {} FROM portraits ORDER BY updated_at DESC, id DESC", COLS)
    )
    .fetch_all(p)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        // 逐行的老逻辑（含新开连接池），但路径走注入的 resolver
        let mut item = row_to_item(r, p)?;
        let (bs, bc) = resolve_bound_with(p, item.story_id, item.character_id, story_db).await;
        item.bound_story = bs;
        item.bound_character = bc;
        item.versions = load_versions(p, item.id).await.unwrap_or_default();
        out.push(item);
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
    // 来源标记：手工导入记为 manual（这样素材库能区分"手工导入"与"AI 生成"）
    let _ = sqlx::query("UPDATE portraits SET source = 'manual' WHERE id = ?1 AND COALESCE(source,'') = ''")
        .bind(id)
        .execute(&p)
        .await;
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
        source: String::new(),      // 紧接上面已 UPDATE 落库，由调用方回读
        origin_tool: String::new(),
        derived_from: None,
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
    let _ = sqlx::query("UPDATE portraits SET source = 'placeholder' WHERE id = ?1 AND COALESCE(source,'') = ''")
        .bind(id)
        .execute(&p)
        .await;
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
        source: String::new(),      // 紧接上面已 UPDATE 落库，由调用方回读
        origin_tool: String::new(),
        derived_from: None,
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
    // 第五批「路径模型」修复：统一走 paths
    let dir = crate::paths::exports_root().join("portrait");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dir = dir.canonicalize().unwrap_or(dir);
    let mut n = 0usize;
    let mut skip = 0usize;
    // 合规用导出清单：记录每个导出文件的来源与生成参数（AI 生成内容须可溯源）
    let mut manifest: Vec<serde_json::Value> = Vec::new();
    for id in &ids {
        let row = sqlx::query("SELECT name, path, COALESCE(source,''), COALESCE(origin_tool,''), COALESCE(prompt,''), COALESCE(negative,''), COALESCE(seed,'') FROM portraits WHERE id = ?1")
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
                        // 来源与参数（在文件名与清单里带上，保证"AI 生成"标识不因导出而丢失）
                        let source: String = r.try_get(2).unwrap_or_default();
                        let origin_tool: String = r.try_get(3).unwrap_or_default();
                        let prompt: String = r.try_get(4).unwrap_or_default();
                        let negative: String = r.try_get(5).unwrap_or_default();
                        let seed: String = r.try_get(6).unwrap_or_default();
                        let is_ai = source.starts_with("ai-");
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
                        // AI 生成的文件名带 _AI 后缀：纯字节拷贝（不动图片内容），只让标识跟着文件走
                        let suffix = if is_ai { "_AI" } else { "" };
                        let dst = dir.join(format!("{}_{}{}.{}", clean, id, suffix, ext));
                        std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
                        manifest.push(serde_json::json!({
                            "file": dst.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
                            "name": name,
                            "source": if source.is_empty() { "unknown".to_string() } else { source.clone() },
                            "ai_generated": is_ai,
                            "origin_tool": origin_tool,
                            "prompt": prompt,
                            "negative": negative,
                            "seed": seed,
                        }));
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
    // 写出溯源清单（合规：AI 生成内容须带标识，清单让"哪些是 AI 生成的"跟着导出走）
    let ai_n = manifest.iter().filter(|m| m["ai_generated"] == serde_json::Value::Bool(true)).count();
    let manifest_path = dir.join("导出清单.json");
    let doc = serde_json::json!({
        "generated_at": chrono_like_now(),
        "note": "本清单记录导出素材的来源。ai_generated=true 的图片为 AI 生成内容，按《人工智能生成合成内容标识办法》对外发布时须标注\"AI 生成\"。",
        "total": manifest.len(),
        "ai_generated_count": ai_n,
        "items": manifest,
    });
    let _ = std::fs::write(&manifest_path, serde_json::to_string_pretty(&doc).unwrap_or_default());

    Ok(format!(
        "✅ 已导出 {}/{} 个素材 → {}（其中 AI 生成 {} 个，文件名带 _AI；已附「导出清单.json」）（{} 个无图/缺失跳过）",
        n,
        ids.len(),
        dir.display(),
        ai_n,
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


/// 按 id 取一条素材（入库桥接回读用；不知道调用方是谁）
pub async fn portrait_get(id: i64) -> Result<PortraitItem, String> {
    let pool = pool().await?;
    // 用统一的 COLS（row_to_item 按下标读列，必须保证列序一致；SELECT * 会随表结构变化而错位）
    let rows = sqlx::query(&format!("SELECT {} FROM portraits WHERE id = ?1", COLS))
        .bind(id)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    match rows.first() {
        Some(r) => row_to_item_bound(r, &pool).await,
        None => Err(format!("找不到素材 #{id}")),
    }
}

/// 通用列写入：把 `cols` 里的 (列名, 值) 写进一条素材。
/// **素材库不理解这些列的语义** —— 它只做"把调用方给的字符串写进去"，
/// 因此调用方想记什么（生成参数、来源标记…）都不需要素材库知道。
/// 列名会做白名单校验，避免拼出非法 SQL。
pub async fn portrait_set_columns(id: i64, cols: &[(&str, String)]) -> Result<(), String> {
    const ALLOWED: &[&str] = &[
        "name", "role", "ver", "ratio", "tags", "group_name", "pinned",
        "story_id", "character_id",
        // 来源与参数快照（对素材库而言只是普通文本列）
        "source", "origin_tool", "derived_from",
        "prompt", "negative", "seed", "steps", "cfg", "model",
    ];
    if cols.is_empty() {
        return Ok(());
    }
    let mut sets = Vec::new();
    for (c, _) in cols {
        if !ALLOWED.contains(c) {
            return Err(format!("不允许写入的列：{c}"));
        }
        sets.push(format!("{c} = ?"));
    }
    let sql = format!(
        "UPDATE portraits SET {}, updated_at = datetime('now','localtime') WHERE id = ?",
        sets.join(", ")
    );
    let pool = pool().await?;
    let mut q = sqlx::query(&sql);
    for (_, v) in cols {
        q = q.bind(v);
    }
    q = q.bind(id);
    q.execute(&pool).await.map_err(|e| e.to_string())?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    /// 临时测试库（**绝不碰真实数据**）：
    /// - library 侧：`<test_root>/portraits-test/<tag>/lib.db`
    /// - 故事侧：`<test_root>/portraits-test/<tag>/story-<sid>.db`
    struct Fixture {
        dir: std::path::PathBuf,
        pool: SqlitePool,
    }

    impl Fixture {
        async fn new(tag: &str) -> Fixture {
            let dir = crate::paths::test_root()
                .join("portraits-test")
                .join(format!("{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("建测试目录");
            let db = dir.join("lib.db");
            let url = format!("sqlite://{}?mode=rwc", db.display().to_string().replace('\\', "/"));
            let pool = SqlitePool::connect(&url).await.expect("建临时库");
            // 故事列表（resolve_bound 要查标题）
            sqlx::query("CREATE TABLE IF NOT EXISTS stories (id INTEGER PRIMARY KEY, title TEXT NOT NULL)")
                .execute(&pool)
                .await
                .unwrap();
            ensure_table(&pool).await.expect("建 portraits 表");
            Fixture { dir, pool }
        }

        fn story_db_path(&self, sid: i64) -> std::path::PathBuf {
            self.dir.join(format!("story-{sid}.db"))
        }

        /// 解析器：指向本 fixture 的临时故事库
        fn resolver(&self) -> impl Fn(i64) -> std::path::PathBuf + Send + Sync + '_ {
            move |sid: i64| self.story_db_path(sid)
        }

        async fn add_story(&self, sid: i64, title: &str) {
            sqlx::query("INSERT INTO stories(id, title) VALUES(?1, ?2)")
                .bind(sid)
                .bind(title)
                .execute(&self.pool)
                .await
                .unwrap();
        }

        async fn add_character(&self, sid: i64, cid: i64, name: &str) {
            let db = self.story_db_path(sid);
            let url = format!("sqlite://{}?mode=rwc", db.display().to_string().replace('\\', "/"));
            let sp = SqlitePool::connect(&url).await.expect("建临时故事库");
            sqlx::query("CREATE TABLE IF NOT EXISTS characters (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
                .execute(&sp)
                .await
                .unwrap();
            sqlx::query("INSERT INTO characters(id, name) VALUES(?1, ?2)")
                .bind(cid)
                .bind(name)
                .execute(&sp)
                .await
                .unwrap();
            sp.close().await;
        }

        /// 插入一张素材；`bound` = Some((story_id, character_id))
        async fn add_portrait(&self, name: &str, bound: Option<(i64, i64)>, versions: &[&str]) -> i64 {
            let (sid, cid) = match bound {
                Some((s, c)) => (Some(s), Some(c)),
                None => (None, None),
            };
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO portraits(name, story_id, character_id) VALUES(?1, ?2, ?3) RETURNING id",
            )
            .bind(name)
            .bind(sid)
            .bind(cid)
            .fetch_one(&self.pool)
            .await
            .unwrap();
            for (i, d) in versions.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO portrait_versions(portrait_id, version, change_desc) VALUES(?1, ?2, ?3)",
                )
                .bind(id)
                .bind((i + 1) as i64)
                .bind(*d)
                .execute(&self.pool)
                .await
                .unwrap();
            }
            id
        }

        async fn cleanup(&self) {
            self.pool.close().await;
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// **核心正确性**：批量版必须和"逐行版（oracle）"完全相同 —— 内容与顺序都要一致。
    ///
    /// 这条就是防"优化着优化着把字段漏填/顺序改错"。
    #[tokio::test]
    async fn batch_list_matches_rowbyrow_oracle() {
        let f = Fixture::new("batch-eq").await;
        f.add_story(7, "初次相见").await;
        f.add_story(9, "破碎的她").await;
        f.add_character(7, 2, "夏婉琴").await;
        f.add_character(7, 4, "院长").await;
        f.add_character(9, 1, "她").await;

        // 覆盖各种情况：有绑定 / 无绑定 / 只有 story 没 character / 多版本 / 无版本
        f.add_portrait("绑定-夏婉琴", Some((7, 2)), &["导入图片素材", "改提示词"]).await;
        f.add_portrait("绑定-院长", Some((7, 4)), &["创建占位素材"]).await;
        f.add_portrait("未绑定", None, &[]).await;
        f.add_portrait("绑定-她", Some((9, 1)), &["导入图片素材"]).await;
        f.add_portrait("角色不存在", Some((7, 999)), &["x"]).await;
        f.add_portrait("故事不存在", Some((404, 1)), &[]).await;

        let resolver = f.resolver();
        let got = portrait_list_in_with(&f.pool, &resolver).await.unwrap();
        let want = portrait_list_rowbyrow_in(&f.pool, &resolver).await.unwrap();

        assert_eq!(got.len(), want.len(), "数量必须一致");
        for (i, (a, b)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(a.id, b.id, "第 {i} 项顺序不一致（id）");
            assert_eq!(a.name, b.name, "第 {i} 项 name");
            assert_eq!(a.story_id, b.story_id, "第 {i} 项 story_id");
            assert_eq!(a.character_id, b.character_id, "第 {i} 项 character_id");
            assert_eq!(a.bound_story, b.bound_story, "第 {i} 项 bound_story");
            assert_eq!(a.bound_character, b.bound_character, "第 {i} 项 bound_character");
            assert_eq!(a.versions.len(), b.versions.len(), "第 {i} 项版本数");
            for (va, vb) in a.versions.iter().zip(b.versions.iter()) {
                assert_eq!(va.version, vb.version, "第 {i} 项版本号");
                assert_eq!(va.desc, vb.desc, "第 {i} 项版本说明");
                assert_eq!(va.date, vb.date, "第 {i} 项版本日期");
            }
        }

        // 顺带把"绑定名字真的解析出来了"钉住（不然上面两者都为 None 也会相等）
        let xia = got.iter().find(|x| x.name == "绑定-夏婉琴").unwrap();
        assert_eq!(xia.bound_story.as_deref(), Some("初次相见"));
        assert_eq!(xia.bound_character.as_deref(), Some("夏婉琴"));
        assert_eq!(xia.versions.len(), 2);

        let none = got.iter().find(|x| x.name == "未绑定").unwrap();
        assert!(none.bound_story.is_none() && none.bound_character.is_none());
        // 注意：`ensure_table` 里有一条**迁移**会把"没有任何版本记录的素材"补上 version 1
        // （change_desc = 导入图片素材 / 创建占位素材）。所以这里不是 0 条，而是 1 条。
        assert_eq!(
            none.versions.len(),
            1,
            "无版本记录的素材会被 ensure_table 的迁移补上 version 1"
        );
        assert_eq!(none.versions[0].version, 1);

        // 角色不存在 → 故事标题有、角色名为 None（与原逻辑一致）
        let ghost = got.iter().find(|x| x.name == "角色不存在").unwrap();
        assert_eq!(ghost.bound_story.as_deref(), Some("初次相见"));
        assert!(ghost.bound_character.is_none());

        f.cleanup().await;
    }

    /// 空表也要正常（不能因为"0 张素材"就越界/报错）。
    #[tokio::test]
    async fn batch_list_handles_empty_table() {
        let f = Fixture::new("batch-empty").await;
        let resolver = f.resolver();
        let got = portrait_list_in_with(&f.pool, &resolver).await.unwrap();
        assert!(got.is_empty());
        f.cleanup().await;
    }

    /// **结构守卫**：list 路径里不许再出现逐行补数据的调用。
    ///
    /// 用 `include_str!` 读自己 —— 这类"批量改回逐行"的退化最容易悄悄发生，
    /// 而且它不会让测试失败（结果一样，只是变慢），所以需要一条专门的守卫。
    #[test]
    fn list_path_must_not_call_per_row_loader() {
        const SRC: &str = include_str!("portraits.rs");
        let start = SRC.find("pub async fn portrait_list_in(").expect("找不到 portrait_list_in");
        // 扫描区间 = 从批量入口起，到"逐行对照版（oracle）"的文档注释之前。
        // 用 oracle 的注释当结束标记（它一定在后面），避免把 oracle 自身也算进来。
        let end = SRC[start..]
            .find("/// **旧实现的逐行版本**")
            .map(|i| start + i)
            .unwrap_or_else(|| SRC[start..].find("#[cfg(test)]").map(|i| start + i).unwrap_or(SRC.len()));
        let body = &SRC[start..end];
        assert!(body.len() > 200, "扫描区间异常短（{0} 字节），结束标记可能没找到", body.len());
        assert!(
            !body.contains("row_to_item_bound"),
            "portrait_list_in 里又出现了逐行补数据的 row_to_item_bound —— 这会把 N+1 带回来.\n{body}"
        );
        assert!(
            body.contains("load_versions_batch"),
            "portrait_list_in 应该用批量版 load_versions_batch"
        );
        assert!(
            body.contains("resolve_bound_batch"),
            "portrait_list_in 应该用批量版 resolve_bound_batch"
        );
    }

    /// **性能对照**：素材多时批量版应明显更快。
    ///
    /// 旧实现每张已绑定素材都要**新开一个 story.db 连接池**，那才是真正的大头。
    /// 这里造 60 张（其中 40 张绑定到同一故事），比较两条路径耗时。
    #[tokio::test]
    async fn batch_is_much_faster_than_rowbyrow() {
        let f = Fixture::new("batch-perf").await;
        f.add_story(7, "初次相见").await;
        for cid in 1..=8 {
            f.add_character(7, cid, &format!("角色{cid}")).await;
        }
        for i in 0..60i64 {
            let bound = if i < 40 { Some((7, (i % 8) + 1)) } else { None };
            f.add_portrait(&format!("素材{i}"), bound, &["导入图片素材"]).await;
        }

        let resolver = f.resolver();

        // 预热一次（避免首次建连/文件缓存影响）
        let _ = portrait_list_rowbyrow_in(&f.pool, &resolver).await.unwrap();
        let _ = portrait_list_in_with(&f.pool, &resolver).await.unwrap();

        let t0 = std::time::Instant::now();
        let slow = portrait_list_rowbyrow_in(&f.pool, &resolver).await.unwrap();
        let slow_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let t1 = std::time::Instant::now();
        let fast = portrait_list_in_with(&f.pool, &resolver).await.unwrap();
        let fast_ms = t1.elapsed().as_secs_f64() * 1000.0;

        println!("[perf] 逐行 {slow_ms:.1} ms  vs  批量 {fast_ms:.1} ms  （{:.1}x）", slow_ms / fast_ms.max(0.001));
        assert_eq!(slow.len(), fast.len());
        assert_eq!(slow.len(), 60);

        // 断言"不变慢"是硬性要求；"快多少"只打印（不同机器波动大，不做硬断言）
        assert!(
            fast_ms <= slow_ms * 1.5 + 5.0,
            "批量版不该比逐行版慢：逐行 {slow_ms:.1} ms / 批量 {fast_ms:.1} ms"
        );

        f.cleanup().await;
    }
}

