// 从 `dev.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `dev::tests` —— 因为 `dev.rs` + `dev/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `dev`，一行都不用改写。

/// 诊断（手动跑）：空壳测试故事（只有 stories 行、没有 story.db）会 CANTOPEN，
/// 而 `ensure_dev_story_pool` 必须能补建并成功打开。
///   DW_DIAG_STORY_DB 不需要；用 DREAMWEAVER_DATA_DIR 指定临时数据根
#[tokio::test]
#[ignore]
async fn diag_shell_story_is_repaired() {
    // ① 建 library，插一个故事（只插行，不建库）
    let lib = crate::db::init_library_db().await.unwrap();
    let id = crate::db::create_story(&lib, "测试0", "2026-01-01", "").await.unwrap();
    let path = crate::paths::story_db(&id.to_string());
    println!("故事 id={id} 期望路径={}", path.display());
    println!("插行后 story.db 存在？{}", path.exists());

    // ② 修复前：直接 open → 应当 CANTOPEN
    let raw = crate::story_db::open_story_pool(&id.to_string()).await;
    match &raw {
        Ok(_) => println!("open 直接成功（说明路径被别的东西建好了）"),
        Err(e) => println!("open 直接失败（复现用户报错）：{e}"),
    }

    // ③ 修复后：ensure 能补建并成功
    let repaired = ensure_dev_story_pool(id).await;
    match &repaired {
        Ok(_) => println!("ensure_dev_story_pool 成功补齐 ✅"),
        Err(e) => println!("ensure 仍失败 ❌：{e}"),
    }
    println!("补建后 story.db 存在？{}", path.exists());
    assert!(repaired.is_ok(), "ensure 应该能补齐空壳故事");

    // ④ 补建后能正常读写
    let pool = repaired.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes")
        .fetch_one(&pool).await.unwrap();
    println!("timeline_nodes 行数 = {n}（0 表示表已建好、只是空的）");
    pool.close().await;
}

use super::*;

#[test]
fn next_title_starts_at_zero() {
    assert_eq!(next_test_title(&[]), "测试0");
}

#[test]
fn next_title_fills_smallest_gap() {
    let v = vec!["测试0".to_string(), "测试2".to_string()];
    assert_eq!(next_test_title(&v), "测试1");
}

#[test]
fn next_title_ignores_other_titles() {
    let v = vec!["大肥鱼的家".to_string(), "测试".to_string(), "测试x".to_string()];
    assert_eq!(next_test_title(&v), "测试0", "非数字后缀不算占用编号");
}

#[test]
fn prefix_is_the_sandbox_boundary() {
    assert!(format!("{TEST_PREFIX}0").starts_with(TEST_PREFIX));
    assert!(!format!("我的{}", TEST_PREFIX).starts_with(TEST_PREFIX));
}
