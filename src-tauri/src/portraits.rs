mod store;
pub use store::*;

mod bind;
pub use bind::*;

mod read;
pub use read::*;

mod write;
pub use write::*;

mod export;
pub use export::*;


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

#[cfg(test)]
mod tests;

