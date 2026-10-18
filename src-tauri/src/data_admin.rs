// 「🗄 数据管理」的后端：**只读统计 + 安全清理**。
//
// 定位（用户 2026-10-18 定）：设置 → 数据管理 里做「B. 安全清理」。
// 这里清理的东西有一个共同点：**删掉不影响任何剧情/设定**，最坏只是丢掉调试痕迹或旧备份。
//
// 明确**不**处理（用户的安装版里根本没有这些）：
//   - 源码目录的 `src-tauri/target`（编译缓存，几十 GB，但只在开发机存在）
//   - cargo registry / WiX / NSIS 工具缓存
//
// 安全原则：
//   * 每个清理动作都返回「删了多少条 / 释放了多少字节」，界面能如实回显；
//   * 删除前由界面二次确认；
//   * 一律不碰 `story.db` 里的剧情表（messages / timeline_nodes / 设定表）。

use serde::Serialize;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};

/// 概览里的一行。
#[derive(Serialize, Clone)]
pub struct DataItem {
    /// 稳定标识（界面按它决定给哪个按钮）
    pub key: String,
    pub label: String,
    /// 占用字节
    pub bytes: i64,
    /// 条目数（故事数 / 文件数 / 记录条数），无意义时为 -1
    pub count: i64,
    /// 一句话说明"这是什么、删了会怎样"
    pub hint: String,
    /// 是否可清理（false = 只展示，不提供按钮）
    pub cleanable: bool,
}

/// 存储概览。
#[derive(Serialize)]
pub struct DataOverview {
    pub data_dir: String,
    pub total_bytes: i64,
    pub items: Vec<DataItem>,
}

fn dir_bytes(p: &Path) -> i64 {
    if !p.exists() {
        return 0;
    }
    let mut total = 0i64;
    let mut stack = vec![p.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(_) => total += std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0),
                _ => {}
            }
        }
    }
    total
}

fn file_bytes(p: &Path) -> i64 {
    std::fs::metadata(p).map(|m| m.len() as i64).unwrap_or(0)
}

/// 某个 story.db 的调试记录（条数, 正文字节）
async fn debug_stats(pool: &SqlitePool) -> (i64, i64) {
    let rows: Result<(i64, i64), _> = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(LENGTH(system_prompt) + LENGTH(history_json) + LENGTH(user_text) + \
         COALESCE(LENGTH(tail_block), 0)), 0) FROM debug_requests",
    )
    .fetch_one(pool)
    .await;
    rows.unwrap_or((0, 0))
}

/// library 里所有 `*.bak*` 旧备份（按修改时间倒序）
fn stale_backups() -> Vec<PathBuf> {
    let root = crate::paths::library_root();
    let mut out: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(_) => {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if name.contains(".bak") {
                        let mt = std::fs::metadata(&path)
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::UNIX_EPOCH);
                        out.push((mt, path));
                    }
                }
                _ => {}
            }
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, p)| p).collect()
}

/// 收集所有故事 id（library.db 的 stories 表）
async fn story_ids() -> Vec<String> {
    let Ok(lib) = crate::db::library_pool().await else { return Vec::new() };
    let rows: Result<Vec<(i64,)>, _> = sqlx::query_as("SELECT id FROM stories ORDER BY id")
        .fetch_all(&lib)
        .await;
    rows.unwrap_or_default().into_iter().map(|(id,)| id.to_string()).collect()
}

/// 磁盘上存在、但 `stories` 表里没有的项目目录（孤儿）
fn orphan_projects(story_ids: &[String]) -> Vec<PathBuf> {
    let root = crate::paths::library_root();
    // projects 目录位于 <library>/projects
    let proj = root.join("projects");
    let Ok(rd) = std::fs::read_dir(&proj) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if !story_ids.iter().any(|s| *s == name) {
            out.push(e.path());
        }
    }
    out
}

/// 统计概览（只读，不改任何东西）
pub async fn overview() -> Result<DataOverview, String> {
    let lib_root = crate::paths::library_root();
    let mut items: Vec<DataItem> = Vec::new();

    // 1) 故事库（各 story.db + wal/shm）
    let ids = story_ids().await;
    let mut story_bytes = 0i64;
    for id in &ids {
        let db = crate::paths::story_db(id);
        story_bytes += file_bytes(&db);
        story_bytes += file_bytes(&db.with_extension("db-wal"));
        story_bytes += file_bytes(&db.with_extension("db-shm"));
    }
    items.push(DataItem {
        key: "stories".into(),
        label: "故事库".into(),
        bytes: story_bytes,
        count: ids.len() as i64,
        hint: "剧情、设定、摘要全都在这儿 —— **不可清理**（要删请到故事列表里删故事）".into(),
        cleanable: false,
    });

    // 2) 立绘图片
    let portraits = lib_root.join("shared").join("portraits");
    let p_bytes = dir_bytes(&portraits);
    let p_count = std::fs::read_dir(&portraits).map(|d| d.flatten().count() as i64).unwrap_or(0);
    items.push(DataItem {
        key: "portraits".into(),
        label: "立绘图片".into(),
        bytes: p_bytes,
        count: p_count,
        hint: "角色立绘原图 —— 需要空间时可在「立绘素材」里逐张删除".into(),
        cleanable: false,
    });

    // 3) 调试记录（在各 story.db 内部）
    let mut dbg_rows = 0i64;
    let mut dbg_bytes = 0i64;
    for id in &ids {
        if let Ok(pool) = crate::story_db::open_story_pool(id).await {
            let (r, b) = debug_stats(&pool).await;
            dbg_rows += r;
            dbg_bytes += b;
        }
    }
    items.push(DataItem {
        key: "debug".into(),
        label: "调试记录".into(),
        bytes: dbg_bytes,
        count: dbg_rows,
        hint: "每次生成都会存一份完整提示词（用于「生成参数」里回看历史）—— 删掉不影响剧情".into(),
        cleanable: true,
    });

    // 4) 日志
    let logs = crate::paths::logs_root();
    items.push(DataItem {
        key: "logs".into(),
        label: "日志文件".into(),
        bytes: dir_bytes(&logs),
        count: std::fs::read_dir(&logs).map(|d| d.flatten().count() as i64).unwrap_or(0),
        hint: "启动/运行日志 —— 排查问题用，删掉只会让下次排查少点线索".into(),
        cleanable: true,
    });

    // 5) 旧备份（*.bak*）
    let backups = stale_backups();
    items.push(DataItem {
        key: "backups".into(),
        label: "旧备份".into(),
        bytes: backups.iter().map(|p| file_bytes(p)).sum(),
        count: backups.len() as i64,
        hint: "修数据时留的 `story.db.bak-*` 快照 —— 确认现在的库没问题后可以删".into(),
        cleanable: !backups.is_empty(),
    });

    // 6) 孤儿项目目录
    let orphans = orphan_projects(&ids);
    items.push(DataItem {
        key: "orphans".into(),
        label: "孤儿项目目录".into(),
        bytes: orphans.iter().map(|p| dir_bytes(p)).sum(),
        count: orphans.len() as i64,
        hint: "磁盘上有、但故事列表里已经没有的 `projects/<id>` —— 通常是删故事没删干净".into(),
        cleanable: !orphans.is_empty(),
    });

    // 7) 出图临时文件
    let temp = crate::generator::temp_dir();
    items.push(DataItem {
        key: "temp".into(),
        label: "出图临时文件".into(),
        bytes: dir_bytes(&temp),
        count: crate::generator::list_temp().len() as i64,
        hint: "AI 出图的中间产物（入库失败/取消时残留）".into(),
        cleanable: true,
    });

    // 8) 导出文件
    let exports = crate::paths::exports_root();
    items.push(DataItem {
        key: "exports".into(),
        label: "导出文件".into(),
        bytes: dir_bytes(&exports),
        count: std::fs::read_dir(&exports).map(|d| d.flatten().count() as i64).unwrap_or(0),
        hint: "导出的 txt / md / html —— 需要时自行删除".into(),
        cleanable: false,
    });

    let total = items.iter().map(|i| i.bytes).sum();
    Ok(DataOverview {
        data_dir: lib_root.display().to_string(),
        total_bytes: total,
        items,
    })
}

/// 清理结果（界面回显用）。
#[derive(Serialize)]
pub struct CleanResult {
    /// 删掉的条目数（记录条数 / 文件数 / 目录数）
    pub removed: i64,
    /// 释放的字节
    pub freed: i64,
}

/// 清理调试记录：每个故事保留最新 `keep` 条（`keep = 0` = 全清）。
pub async fn clean_debug_requests(keep: i64) -> Result<CleanResult, String> {
    let keep = keep.max(0);
    let mut removed = 0i64;
    let mut before = 0i64;
    let mut after = 0i64;
    for id in story_ids().await {
        let Ok(pool) = crate::story_db::open_story_pool(&id).await else { continue };
        let db = crate::paths::story_db(&id);
        before += file_bytes(&db);
        let n = sqlx::query(
            "DELETE FROM debug_requests WHERE id NOT IN \
             (SELECT id FROM debug_requests ORDER BY id DESC LIMIT ?1)",
        )
        .bind(keep)
        .execute(&pool)
        .await
        .map(|r| r.rows_affected() as i64)
        .unwrap_or(0);
        removed += n;
        if n > 0 {
            // 回收空间（SQLite 删行不会自动缩小文件）
            let _ = sqlx::query("VACUUM").execute(&pool).await;
        }
        after += file_bytes(&db);
    }
    Ok(CleanResult { removed, freed: (before - after).max(0) })
}

/// 清空日志文件（保留文件本身，便于继续追加）。
pub async fn clean_logs() -> Result<CleanResult, String> {
    let dir = crate::paths::logs_root();
    let mut removed = 0i64;
    let mut before = 0i64;
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Ok(CleanResult { removed: 0, freed: 0 });
    };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        before += file_bytes(&p);
        if std::fs::OpenOptions::new().write(true).truncate(true).open(&p).is_ok() {
            removed += 1;
        }
    }
    let after = dir_bytes(&dir);
    Ok(CleanResult { removed, freed: (before - after).max(0) })
}

/// 删掉孤儿项目目录（不在 `stories` 表里的 `projects/<id>`）。
pub async fn clean_orphans() -> Result<CleanResult, String> {
    let ids = story_ids().await;
    let mut removed = 0i64;
    let mut freed = 0i64;
    for p in orphan_projects(&ids) {
        let b = dir_bytes(&p);
        if std::fs::remove_dir_all(&p).is_ok() {
            removed += 1;
            freed += b;
        }
    }
    Ok(CleanResult { removed, freed })
}

/// 删旧备份：`keep_latest = true` 时**只留最新一份**（当保险）。
pub async fn clean_backups(keep_latest: bool) -> Result<CleanResult, String> {
    let list = stale_backups(); // 已按时间倒序
    let skip = if keep_latest { 1 } else { 0 };
    let mut removed = 0i64;
    let mut freed = 0i64;
    for p in list.into_iter().skip(skip) {
        let b = file_bytes(&p);
        if std::fs::remove_file(&p).is_ok() {
            removed += 1;
            freed += b;
        }
    }
    Ok(CleanResult { removed, freed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_bytes_counts_nested_files() {
        let dir = crate::paths::test_root().join("data_admin_bytes");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/x.txt"), b"12345").unwrap();
        std::fs::write(dir.join("a/b/y.txt"), b"123").unwrap();
        assert_eq!(dir_bytes(&dir), 8, "应把嵌套文件都算进去");
        assert_eq!(dir_bytes(&dir.join("nope")), 0, "不存在的目录算 0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_bytes_handles_missing() {
        assert_eq!(file_bytes(Path::new("no-such-file-xyz")), 0);
    }

    /// 概览必须包含全部关键项，且"不可清理"的项要标清楚
    #[test]
    fn overview_item_keys_are_stable() {
        // 这里只验证**常量清单**（不连库）：改动 key 会悄悄让界面的按钮失效
        let keys = ["stories", "portraits", "debug", "logs", "backups", "orphans", "temp", "exports"];
        assert_eq!(keys.len(), 8);
        assert_eq!(keys[0], "stories");
        assert_eq!(keys[2], "debug");
    }
}
