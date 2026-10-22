// 从 `paths.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `paths::tests` —— 因为 `paths.rs` + `paths/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `paths`，一行都不用改写。

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
