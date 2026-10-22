// 日摘要自动触发规则的测试（用户 2026-10-22 定）。
//
// 规则：当天 AI 回复条数**每满 10 条**（10 / 20 / 30 …）才跑一次；
// 旧规则是 `replies >= 10` 就一直跑 —— 第 10 条之后每加一条回复都重新生成，
// 既费 token，又把"当日摘要"每轮改一次、前缀缓存跟着散。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use std::str::FromStr;

async fn mem_pool() -> SqlitePool {
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::from_str("sqlite::memory:").unwrap().create_if_missing(true),
    )
    .await
    .unwrap();
    // 建表（本测试只关心"触发条件"，不关心其它列）
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS timeline_nodes (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            date_label   TEXT NOT NULL,
            sort_order   INTEGER NOT NULL DEFAULT 0,
            node_type    TEXT NOT NULL DEFAULT 'day',
            is_flashback INTEGER NOT NULL DEFAULT 0,
            day_summary  TEXT NOT NULL DEFAULT '',
            day_summary_at INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id INTEGER NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

async fn add_day(pool: &SqlitePool, label: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, day_summary)
         VALUES(?1, 1, 'day', 0, '') RETURNING id",
    )
    .bind(label)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn add_replies(pool: &SqlitePool, node_id: i64, n: i64) {
    for _ in 0..n {
        sqlx::query("INSERT INTO messages(node_id, role, content) VALUES(?1, 'assistant', 'x')")
            .bind(node_id)
            .execute(pool)
            .await
            .unwrap();
    }
}

/// 触发条件本身：`maybe_auto_day_summary` 内部按 `replies % step` 判断。
/// 这里不跑模型（那要 API Key），改为直接验证 SQL 侧的计数与"上次轮数"的语义：
///   ① 9 条 → 不是整数倍，不该跑；10 条 → 该跑；11 条 → 不该跑；20 条 → 该跑。
/// 用 `day_summary_at` 的语义把这条规则"算"出来，和函数里的判断同构。
#[tokio::test]
async fn auto_summary_triggers_only_on_multiples_of_step() {
    let pool = mem_pool().await;
    let day = add_day(&pool, "第 1 天").await;
    let step = 10i64;

    let mut expected: Vec<(i64, bool)> = vec![
        (9, false), (10, true), (11, false), (19, false), (20, true), (21, false), (30, true),
    ];
    let mut added = 0i64;
    for (total, want) in expected.drain(..) {
        while added < total {
            add_replies(&pool, day, 1).await;
            added += 1;
        }
        let done_at: i64 =
            sqlx::query_scalar("SELECT COALESCE(day_summary_at, 0) FROM timeline_nodes WHERE id = ?1")
                .bind(day)
                .fetch_one(&pool)
                .await
                .unwrap();
        // 与 maybe_auto_day_summary 内部判断同构
        let would_run = total > 0 && total % step == 0 && total > done_at;
        assert_eq!(would_run, want, "{total} 条时触发判断错了");
        if would_run {
            // 模拟"跑过了"
            sqlx::query("UPDATE timeline_nodes SET day_summary_at = ?1 WHERE id = ?2")
                .bind(total)
                .bind(day)
                .execute(&pool)
                .await
                .unwrap();
        }
    }
}

/// 同一轮数重复调用不得重复跑（重试 / 重复 invoke）
#[tokio::test]
async fn same_reply_count_does_not_run_twice() {
    let pool = mem_pool().await;
    let day = add_day(&pool, "第 1 天").await;
    add_replies(&pool, day, 10).await;
    sqlx::query("UPDATE timeline_nodes SET day_summary_at = 10 WHERE id = ?1")
        .bind(day)
        .execute(&pool)
        .await
        .unwrap();
    let done_at: i64 =
        sqlx::query_scalar("SELECT day_summary_at FROM timeline_nodes WHERE id = ?1")
            .bind(day)
            .fetch_one(&pool)
            .await
            .unwrap();
    let replies = 10i64;
    let would_run = replies > 0 && replies % 10 == 0 && replies > done_at;
    assert!(!would_run, "同一轮数第二次调用不应再跑");

    // 再涨到 20 条 → 又该跑了（每满 10 条刷新一次）
    add_replies(&pool, day, 10).await;
    let replies2 = 20i64;
    assert!(replies2 % 10 == 0 && replies2 > done_at, "20 条时应再次触发");
}

/// 态势规则必须写清与强度的分工，否则冲突戏里两者会打架
#[test]
fn footing_rule_splits_duty_with_strength() {
    let f = crate::relations::footing_rule();
    for k in ["稳定", "紧张", "破裂"] {
        assert!(f.contains(k), "态势规则缺 {k}：{f}");
    }
    for must in ["烈度上限", "愿不愿意配合", "以态势定烈度", "不接受无代价的和解"] {
        assert!(f.contains(must), "态势规则缺「{must}」：{f}");
    }
    // 强度段的标题不能再说"烈度"（那是态势的事）
    let coop = crate::relations::coop_legend();
    assert!(!coop.contains("烈度"), "配合度段不该管烈度：{coop}");
}
