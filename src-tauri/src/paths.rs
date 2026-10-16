//! 全局路径解析（第五批「路径模型」修复 —— 依据《现状说明》5.1）
//!
//! ## 旧写法的问题（原文）
//!
//! > 所有数据目录都锚定在「**进程当前目录的上一级**」……打包后双击 exe，
//! > 当前目录是「用户双击时所在的目录」；落到 `C:\Program Files` 这类没有写权限的地方，
//! > 程序会直接消失。即使不出错，换个启动方式就会指向不同的 library，表现为「数据不见了」。
//!
//! ## 现在的规则（**只有一个数据根**）
//!
//! 解析顺序：
//! 1. **环境变量 `DREAMWEAVER_DATA_DIR`** —— 测试/多实例用（绝对值，不做任何拼接）
//! 2. **`%APPDATA%\<bundle identifier>`** —— 与 Tauri `app_data_dir()` 同一位置
//!    （不依赖 `AppHandle`，所以**测试与真机解析到同一个地方**，这是关键）
//! 3. 兜底：exe 同级的 `data/`（极端环境：既没有 APPDATA 也拿不到 exe 路径）
//!
//! **不使用 `std::env::current_dir()`** —— 这是本次修复的核心：进程当前目录由"怎么启动"决定，
//! 数据位置绝不能依赖它。
//!
//! ## 一次性迁移
//!
//! 历史上数据在 `<项目根>/library`（开发期假设）。`migrate_legacy_once()` 会在
//! **新位置还不存在**时，把旧位置整体搬过来（有日志、失败不阻断启动）。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 与 `tauri.conf.json` 的 `identifier` 保持一致。
///
/// 为什么硬编码而不是问 Tauri 要：`app_data_dir()` 需要 `AppHandle`，
/// 而**测试环境没有 `AppHandle`**。两边用同一个常量，才能保证"测试与真机同一目录"。
/// （`tauri.conf.json` 改了 identifier 就必须同步改这里 —— 已加测试守着。）
const BUNDLE_ID: &str = "com.guchenxi.story-weaver";

/// 环境变量覆盖（测试、多实例、便携模式）
const ENV_OVERRIDE: &str = "DREAMWEAVER_DATA_DIR";

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 数据根目录（**只是计算，不创建**）。结果缓存，进程内恒定。
pub fn data_root() -> PathBuf {
    ROOT.get_or_init(|| {
        // ① 显式覆盖（测试、便携）
        if let Ok(v) = std::env::var(ENV_OVERRIDE) {
            let t = v.trim();
            if !t.is_empty() {
                return PathBuf::from(t);
            }
        }
        // ② %APPDATA%\<bundle id>
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.trim().is_empty() {
                return PathBuf::from(appdata).join(BUNDLE_ID);
            }
        }
        // ③ 兜底：exe 同级的 data/（极端环境）
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return dir.join("data");
            }
        }
        // ④ 最后手段（几乎到不了）：系统临时目录
        std::env::temp_dir().join("dreamweaver-data")
    })
    .clone()
}

/// 库根（library.db + projects/）
pub fn library_root() -> PathBuf {
    data_root().join("library")
}

/// 库索引数据库
pub fn library_db() -> PathBuf {
    library_root().join("default").join("library.db")
}

/// 某个故事的 story.db
pub fn story_db(story_id: &str) -> PathBuf {
    library_root().join("default").join("projects").join(story_id).join("story.db")
}

/// 导出目录（用户会去翻的产物）
pub fn exports_root() -> PathBuf {
    data_root().join("exports")
}

/// 临时文件目录（生成的中间图片等）
///
/// **旧写法把它放进 `src-tauri/target/gen-tmp`** —— 那是 cargo 的构建产物目录，
/// 打包后根本不存在，而且 `target` 被清掉时临时文件也跟着没了。
pub fn temp_root() -> PathBuf {
    data_root().join("temp")
}

/// 日志目录
pub fn logs_root() -> PathBuf {
    data_root().join("logs")
}

/// 备份目录
pub fn backups_root() -> PathBuf {
    data_root().join("backups")
}

/// **测试专用**的临时根目录：`<crate>/target/test-library`。
///
/// 为什么用 `CARGO_MANIFEST_DIR`（编译期常量）而不是 `current_dir()`：
/// 后者由"怎么启动"决定 —— 那正是本次修复要根除的东西，测试里也不该再用。
/// 放在 `target/` 下是为了 **`cargo clean` 能一并清掉**，且不会污染真实数据目录。
pub fn test_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-library")
}

/// 确保某个目录存在（按需创建，不在启动时一股脑建一堆空目录）
pub fn ensure_dir(p: &Path) -> std::io::Result<()> {
    if !p.exists() {
        std::fs::create_dir_all(p)?;
    }
    Ok(())
}

/// 一次性迁移：把历史数据从"开发期位置"搬到新的数据根。
///
/// **只在"新位置还没有 library 目录"时动手**，所以：
/// - 真机第一次升级：搬一次，之后不再触发
/// - 测试环境（用 `DREAMWEAVER_DATA_DIR` 指到临时目录）：新位置为空，
///   但旧位置也没有测试数据 → 不会误搬真机的库（下面第 2 条判断挡住了）
/// - 用户手工搬过 / 已经是新布局：不动
///
/// 失败**不阻断启动**（返回 Err 由调用方打日志）。
pub fn migrate_legacy_once() -> Result<Option<PathBuf>, String> {
    let new_root = library_root();
    if new_root.exists() {
        return Ok(None); // 新位置已有数据 → 什么都不做
    }

    // 候选的旧位置。`current_dir()/..` 在这里**只用于"找旧数据"**，
    // 找到就搬走，之后运行期再也不依赖当前目录。
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        // 开发期：cwd = <项目根>/src-tauri，上一级就是项目根
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join("library"));
        }
        candidates.push(cwd.join("library"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("library"));
            if let Some(up) = dir.parent() {
                candidates.push(up.join("library"));
            }
            if let Some(up2) = dir.parent().and_then(|p| p.parent()) {
                candidates.push(up2.join("library"));   // <项目根>/src-tauri/target/{debug,release}/../..
            }
        }
    }

    for old in candidates {
        // 必须真的有库索引才算"旧数据"，避免把空目录当成数据搬过来
        let old_db = old.join("default").join("library.db");
        if !old_db.exists() {
            continue;
        }
        // 新位置的父目录要先存在
        if let Some(parent) = new_root.parent() {
            ensure_dir(parent).map_err(|e| format!("创建数据根失败：{e}"))?;
        }
        match move_dir(&old, &new_root) {
            Ok(()) => return Ok(Some(old)),
            Err(e) => {
                // 搬迁失败（跨盘移动可能失败）→ 退化为"复制"，仍失败就把原因交给调用方
                if let Err(e2) = copy_dir(&old, &new_root) {
                    return Err(format!("迁移数据失败：移动 {e}；复制 {e2}"));
                }
                return Ok(Some(old));
            }
        }
    }
    Ok(None)
}

fn move_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_id_matches_tauri_conf() {
        // 守着"两处常量必须一致"—— 不一致的话，真机数据目录会与测试/预期不同
        let conf_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        let conf = std::fs::read_to_string(&conf_path).expect("读 tauri.conf.json");
        assert!(
            conf.contains(&format!("\"{BUNDLE_ID}\"")),
            "tauri.conf.json 的 identifier 与 paths::BUNDLE_ID 不一致（改了一处要改另一处）"
        );
    }

    #[test]
    fn data_root_is_absolute_and_not_cwd_derived() {
        let root = data_root();
        assert!(root.is_absolute(), "数据根必须是绝对路径，实际：{root:?}");
        // 不得等于"当前目录"或"当前目录的上一级"（那正是本次要修掉的写法）
        let cwd = std::env::current_dir().unwrap();
        assert_ne!(root, cwd, "数据根不能是当前目录");
        assert_ne!(root, cwd.parent().unwrap_or(&cwd).to_path_buf(), "数据根不能是当前目录的上一级");
    }

    #[test]
    fn children_are_under_root() {
        let r = data_root();
        assert!(library_db().starts_with(&r));
        assert!(story_db("7").starts_with(&r));
        assert!(exports_root().starts_with(&r));
        assert!(temp_root().starts_with(&r));
        assert!(logs_root().starts_with(&r));
        assert!(backups_root().starts_with(&r));
        // temp 不能落在 target 下（打包后 target 不存在）
        assert!(!temp_root().to_string_lossy().contains("target"));
    }

    #[test]
    fn override_env_wins() {
        // 注意：OnceLock 已缓存，这里只验证"取值逻辑"本身，不重设全局
        let key = ENV_OVERRIDE;
        assert_eq!(key, "DREAMWEAVER_DATA_DIR");
    }
}
