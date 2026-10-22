// 从 `lib.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `lib::live_branch_state_tests` —— 因为 `lib.rs` + `lib/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `lib`，一行都不用改写。

use sqlx::Row;

/// 把数据根强制指到 `target/test-library/<name>`，并建出 library.db + story.db。
///
/// **为什么不再直接用 `library_pool()`**：那会连真实 `%APPDATA%\...\library.db`，
/// 而本模块会往 `stories` 表插一行假故事 —— 中途 panic 就会把它留在用户的故事列表里
/// （点进去还打不开，因为没有对应的 story.db）。
/// 与 `branch.rs` 的临时故事测试保持同一口径：**测试只碰 `target/`**。
async fn setup(root_name: &str, sid: i64, branch_id: Option<&str>) -> String {
    let root = crate::paths::test_root().join(root_name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        crate::paths::force_test_root(root.clone()),
        "数据根已被其它测试固定，继续跑会写进**真实库**。请单独运行本测试：\n  \
         cargo test --lib {root_name} -- --ignored --exact --nocapture"
    );

    // library.db：本模块要验的是 stories.current_branch_id 这一列
    let lib = crate::db::init_library_db().await.unwrap();
    sqlx::query("INSERT INTO stories(id, title, time_origin) VALUES(?1, '测试故事', '现代')")
        .bind(sid)
        .execute(&lib)
        .await
        .unwrap();

    // story.db：resolve_branch 的第二层校验要看 branches 表里这条分支在不在
    let story_key = sid.to_string();
    let pool = crate::story_db::init_story_db(&story_key).await.unwrap();
    if let Some(b) = branch_id {
        sqlx::query(
            "INSERT INTO branches(id, story_id, name, fork_node_id, color, note) \
             VALUES(?1, ?2, '测试分支', 0, '#6B7280', '')",
        )
        .bind(b)
        .bind(&story_key)
        .execute(&pool)
        .await
        .unwrap();
    }
    story_key
}

async fn teardown(root_name: &str, sid: i64) {
    if let Ok(lib) = crate::db::library_pool().await {
        let _ = sqlx::query("DELETE FROM stories WHERE id = ?1")
            .bind(sid)
            .execute(&lib)
            .await;
    }
    let _ = std::fs::remove_dir_all(crate::paths::test_root().join(root_name));
}

/// 第五批 ③：设了当前分支之后必须读得到（界面切分支不生效的那次就是这个）。
///
/// 单独跑：`cargo test --lib live-branch-state-write -- --ignored --exact --nocapture`
#[tokio::test]
#[ignore]
async fn live_current_branch_write() {
    const NAME: &str = "live-branch-state-write";
    let sid: i64 = 9_900_002;
    let fake_branch = "test-branch-uuid-0001".to_string();
    let key = setup(NAME, sid, Some(&fake_branch)).await;
    let lib = crate::db::library_pool().await.unwrap();

    // 1) 初始应为主线
    let initial = crate::branch::resolve_branch(&key, &None).await.unwrap();
    println!("[live] 初始 resolve = {initial:?}");
    assert_eq!(initial, None, "新故事初始应在主线");

    // 2) 设定到分支后必须读得到
    crate::branch::set_current_branch(&key, &Some(fake_branch.clone())).await.unwrap();
    let after = crate::branch::resolve_branch(&key, &None).await.unwrap();
    println!("[live] set 之后 resolve = {after:?}");
    assert_eq!(
        after.as_deref(),
        Some(fake_branch.as_str()),
        "**set_current_branch 之后必须读得到**（读不到 = 界面切换分支没生效）"
    );

    // 3) 原始 SQL 复核（排除 resolve 自身的问题）
    let raw: Option<String> = sqlx::query("SELECT current_branch_id FROM stories WHERE id = ?1")
        .bind(sid)
        .fetch_one(&lib)
        .await
        .unwrap()
        .try_get(0)
        .unwrap();
    println!("[live] 原始 SQL 读到的列值 = {raw:?}");
    assert_eq!(raw.as_deref(), Some(fake_branch.as_str()), "列里必须真的写进去了");

    // 4) 切回主线
    crate::branch::set_current_branch(&key, &None).await.unwrap();
    let back = crate::branch::resolve_branch(&key, &None).await.unwrap();
    println!("[live] 切回主线后 resolve = {back:?}");
    assert_eq!(back, None, "切回主线后应读回 NULL");

    teardown(NAME, sid).await;
}

/// 第五批 ③：**悬空分支引用要自愈**（用户实测白屏的那个 bug）。
///
/// 场景：`stories.current_branch_id` 指向一条已被删除的分支
/// （跨库引用，库层面没有外键）。界面拿不存在的分支去过滤 → 白屏/看不到节点。
/// 这里验证 `resolve_branch` 会归主线**并顺手修正记录**。
///
/// 单独跑：`cargo test --lib live-branch-state-dangling -- --ignored --exact --nocapture`
#[tokio::test]
#[ignore]
async fn live_dangling_branch_self_heals() {
    const NAME: &str = "live-branch-state-dangling";
    let sid: i64 = 9_900_003;
    // 故意**不建**这条分支行 → 模拟"分支被删了但记录还在"
    let key = setup(NAME, sid, None).await;
    let lib = crate::db::library_pool().await.unwrap();

    sqlx::query("UPDATE stories SET current_branch_id = 'ghost-branch-uuid' WHERE id = ?1")
        .bind(sid)
        .execute(&lib)
        .await
        .unwrap();

    let got = crate::branch::resolve_branch(&key, &None).await.unwrap();
    println!("[live] 悬空引用 resolve = {got:?}");
    assert_eq!(got, None, "**悬空的分支引用必须归主线**（否则界面会白屏/看不到节点）");

    // 记录应被自愈（DB 里已经清空）
    let raw: Option<String> =
        sqlx::query_scalar("SELECT current_branch_id FROM stories WHERE id = ?1")
            .bind(sid)
            .fetch_one(&lib)
            .await
            .unwrap();
    println!("[live] DB 里的记录 = {raw:?}");
    assert_eq!(raw, None, "悬空引用应被顺手清掉（下次启动不再踩同一个坑）");

    teardown(NAME, sid).await;
}
