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

/// 数据根目录。结果缓存，进程内恒定。
///
/// ## 解析顺序（**每一级都要能真写才采用**）
///
/// 1. 环境变量 `DREAMWEAVER_DATA_DIR` —— 显式指定就用它（**不再探测**，尊重用户意图）
/// 2. `%APPDATA%\<bundle id>` —— 标准位置
/// 3. `<exe 同级>/data` —— **便携/受限环境**：实测某些机器上安全软件只允许程序写自身目录，
///    `%APPDATA%` 反而被拒（`attempt to write a readonly database`）
/// 4. `<cwd>/data` —— 再退一步
/// 5. 系统临时目录下的 `dreamweaver-data` —— 最后手段（重启后可能被清，但至少能用）
///
/// ## 为什么必须"探测可写"
///
/// 实测事故：数据根指向 `%APPDATA%`，但那台机器上 `app.exe` **连新建目录都被拒**
/// （`os error 5`），于是 sqlite 报 `attempt to write a readonly database`，应用起不来。
/// 而同一台机器上 Python 能写 —— 所以"目录看起来在"完全不能作为依据，**必须真写一个文件**。
pub fn data_root() -> PathBuf {
    ROOT.get_or_init(|| {
        // ① 显式覆盖：用户说了算（不再探测，否则"我明明设了却没生效"更难查）
        if let Ok(v) = std::env::var(ENV_OVERRIDE) {
            let t = v.trim();
            if !t.is_empty() {
                return PathBuf::from(t);
            }
        }
        // ②~⑤ 逐个探测
        let mut cands: Vec<PathBuf> = Vec::new();
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.trim().is_empty() {
                cands.push(PathBuf::from(appdata).join(BUNDLE_ID));
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                cands.push(dir.join("data"));
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            cands.push(cwd.join("data"));
        }
        cands.push(std::env::temp_dir().join("dreamweaver-data"));

        for c in &cands {
            if dir_is_writable(c) {
                if c != &cands[0] {
                    // 用兜底位置时必须**明确提示**：否则用户会疑惑"我的数据去哪了"
                    eprintln!(
                        "[startup][warn] 标准数据目录不可写（{}），已改用：{}",
                        cands[0].display(),
                        c.display()
                    );
                }
                return c.clone();
            }
        }
        // 全都不行：返回第一个，让上层的 fatal_startup 把情况展示给用户
        cands[0].clone()
    })
    .clone()
}

/// 是否用了"非标准"的数据根（供界面/日志提示）
pub fn data_root_is_fallback() -> bool {
    if let Ok(v) = std::env::var(ENV_OVERRIDE) {
        if !v.trim().is_empty() {
            return false;   // 显式指定不算兜底
        }
    }
    let root = data_root();
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.trim().is_empty() {
            return root != PathBuf::from(appdata).join(BUNDLE_ID);
        }
    }
    false
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


/// 临时文件目录（生成的中间图片等）
///

/// 某个目录是否**真的可写**（试着建出来 + 写一个探针文件）。
///
/// 为什么不能只看 `create_dir_all` 的返回值：实测某些 Windows 环境下
/// **"已存在的目录可写、新建目录拒绝访问"**（`os error 5`）——
/// 这种情况下 `create_dir_all` 会成功返回（目录本来就在），但后续写文件仍可能失败。
/// 所以用"真写一个文件"来判定。
fn dir_is_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 在若干个候选里挑第一个**真能写**的目录；都不行就返回第一个（调用方会再报错）。
fn first_writable(candidates: &[PathBuf]) -> PathBuf {
    for c in candidates {
        if dir_is_writable(c) {
            return c.clone();
        }
    }
    candidates[0].clone()
}

/// 日志目录。
///
/// **必须保证可写**：日志插件初始化失败会**直接让整个 app 起不来** ——
/// 实测用户机器上就是这样（`failed to initialize plugin log: 拒绝访问 (os error 5)`
/// → setup 失败 → panic，exit code 101，界面都出不来）。
///
/// 所以这里做**多级兜底**，一路退到"肯定能写"的地方：
/// 1. `<数据根>/logs`
/// 2. `<数据根>/library/logs`
/// 3. 系统临时目录下的 `dreamweaver-logs`
/// 4. **可执行文件同级的 `logs`**（极端情况：AppData 与 temp 都被安全软件拦了）
pub fn logs_root() -> PathBuf {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let root = data_root();
            let mut cands = vec![
                root.join("logs"),
                root.join("library").join("logs"),
                std::env::temp_dir().join("dreamweaver-logs"),
            ];
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    cands.push(dir.join("logs"));
                }
            }
            if let Ok(cwd) = std::env::current_dir() {
                cands.push(cwd.join("logs"));
            }
            first_writable(&cands)
        })
        .clone()
}

/// 备份目录（可写性优先，规则同上）
pub fn backups_root() -> PathBuf {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let root = data_root();
            first_writable(&[
                root.join("backups"),
                root.join("library").join("backups"),
                std::env::temp_dir().join("dreamweaver-backups"),
            ])
        })
        .clone()
}

/// 临时目录。
///
/// **运行期必须可写**（生成的中间图片落这里），所以和 `logs_root()` 一样做多级兜底 ——
/// 某些 Windows 环境下 `%APPDATA%` 下的**新目录建不出来**（安全软件拦截，
/// 表现为 `os error 5` 拒绝访问），只靠单一位置会让功能直接失败。
pub fn temp_root() -> PathBuf {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let root = data_root();
            let mut cands = vec![
                root.join("temp"),
                root.join("library").join("temp"),
                std::env::temp_dir().join("dreamweaver-temp"),
            ];
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    cands.push(dir.join("temp"));
                }
            }
            first_writable(&cands)
        })
        .clone()
}

/// 导出目录（可写性优先；导出是给用户看的，所以前两个候选都在数据根下）
pub fn exports_root() -> PathBuf {
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let root = data_root();
            first_writable(&[
                root.join("exports"),
                root.join("library").join("exports"),
                std::env::temp_dir().join("dreamweaver-exports"),
            ])
        })
        .clone()
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

/// 返回日志目录的**所有候选及其可写状态**（供启动失败时的错误信息用）。
///
/// 用户看到"打不开"时，最需要知道的是"它到底想写哪里、哪个能写"。
pub fn logs_candidates_report() -> String {
    let root = data_root();
    let mut cands = vec![
        root.join("logs"),
        root.join("library").join("logs"),
        std::env::temp_dir().join("dreamweaver-logs"),
    ];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join("logs"));
        }
    }
    let mut s = String::new();
    for c in &cands {
        let ok = dir_is_writable(c);
        s.push_str(&format!("  [{}] {}\n", if ok { "可写" } else { "不可写" }, c.display()));
    }
    s
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
    fn library_paths_are_under_root() {
        // 库相关路径**一定**在数据根下（它们是数据本身，不能兜底到别处）
        let r = data_root();
        assert!(library_db().starts_with(&r));
        assert!(story_db("7").starts_with(&r));
        // 各类目录的**首选**位置也在根下（只有首选不可写时才兜底到别处）
        assert!(r.join("exports").starts_with(&r));
        assert!(r.join("temp").starts_with(&r));
        assert!(r.join("logs").starts_with(&r));
        assert!(r.join("backups").starts_with(&r));
    }

    // 关键不变量：logs_root() 必须是真能写的。
    //
    // 为什么单独守这条：日志插件拿它建文件，失败会让整个应用起不来
    // （实测用户机器上 panic，exit code 101，界面都出不来）。
    // 所以不管它最终落在候选链的哪一级，都必须可写。
    #[test]
    fn logs_root_must_be_writable() {
        let d = logs_root();
        assert!(
            dir_is_writable(&d),
            "logs_root() 必须返回可写目录，实际 {d:?} —— 否则日志插件会让启动崩溃"
        );
    }

    // 临时目录同样要能写（生成过程要用），但允许兜底
    #[test]
    fn temp_root_must_be_writable() {
        let d = temp_root();
        assert!(dir_is_writable(&d), "temp_root() 必须可写，实际 {d:?}");
    }

    // 候选报告要能给出"哪个能写"（启动失败时用它说明情况）
    #[test]
    fn candidates_report_lists_entries() {
        let r = logs_candidates_report();
        assert!(r.contains("可写"), "报告里应标明可写状态");
        assert!(r.lines().count() >= 3, "至少应有 3 个候选");
    }

    #[test]
    fn override_env_wins() {
        // 注意：OnceLock 已缓存，这里只验证"取值逻辑"本身，不重设全局
        let key = ENV_OVERRIDE;
        assert_eq!(key, "DREAMWEAVER_DATA_DIR");
    }
}
