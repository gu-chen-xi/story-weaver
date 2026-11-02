// 三维数值变更测试（2026-10-29）。
//
// 每条对应设计稿里的一条规矩：表外不记 / 单条封顶 / 同日同类型一次 /
// 存在感当日夹 ±5 / 换天不自动恢复 / 单条撤销 / 分支隔离 / 提示词不给数字。

use super::*;
use sqlx::sqlite::SqlitePoolOptions;

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    for ddl in [
        "CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY, date_label TEXT, branch_id TEXT)",
        "CREATE TABLE characters (id INTEGER PRIMARY KEY, name TEXT, stat_stamina INTEGER DEFAULT 50, stat_sanity INTEGER DEFAULT 50, stat_screen INTEGER DEFAULT 50, version INTEGER DEFAULT 1, branch_id TEXT)",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    ensure_table(&pool).await.unwrap();
    sqlx::query("INSERT INTO timeline_nodes(id,date_label) VALUES(1,'第 1 天'),(2,'第 2 天')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO characters(id,name) VALUES(1,'夏婉琴')").execute(&pool).await.unwrap();
    pool
}

async fn stat(pool: &SqlitePool, col: &str) -> i64 {
    let sql = format!("SELECT {col} FROM characters WHERE id = 1");
    sqlx::query_scalar(&sql).fetch_one(pool).await.unwrap()
}

#[test]
fn external_kind_is_ignored() {
    assert!(settle("（她点头）\n〔状态变化〕白给：乱写的类型").is_empty());
}

#[test]
fn settle_reads_one_line_and_strips_it() {
    let reply = "（她揉胳膊）\n〔状态变化〕受伤：被推下台阶";
    let plan = settle(reply);
    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].kind, "受伤");
    assert_eq!(plan[0].dim, "stamina");
    assert_eq!(plan[0].delta, -15);
    assert_eq!(strip_line(reply), "（她揉胳膊）");
}

/// 作息（睡好/熬夜）**不在对话协议里**：2026-10-29 定「只在日结里判」，
/// 否则模型会天天报"没睡好"，变成慢性失血。
#[test]
fn settle_never_emits_sleep_kinds() {
    assert!(settle("x\n〔状态变化〕睡好：她睡到天亮").is_empty());
    assert!(settle("x\n〔状态变化〕熬夜：通宵").is_empty());
    // 但日结要能用它们
    assert_eq!(plan_kind("睡好", "日结").len(), 2);
    assert_eq!(plan_kind("熬夜", "日结").len(), 1);
}

#[test]
fn strip_keeps_text_and_protocol_is_recognised() {
    let reply = "正文\n〔状态变化〕熬夜：她一夜没合眼";
    // 末尾协议区要认得出这一行（attitude::is_protocol_line 已接线）
    assert_eq!(crate::memory::protocol_block_start(reply), 1);
    assert_eq!(strip_line(reply), "正文");
    // 中间的同类字样是剧情文本，不删
    let mid = "她在本子上写〔状态变化〕三个字\n又划掉了";
    assert_eq!(strip_line(mid), mid);
}

#[tokio::test]
async fn apply_clamps_and_records_actual_delta() {
    let pool = pool().await;
    sqlx::query("UPDATE characters SET stat_stamina = 5 WHERE id = 1").execute(&pool).await.unwrap();
    let got = apply(&pool, &None, 1, 10, 1, &settle("x\n〔状态变化〕受伤：摔下台阶"), "model").await;
    assert_eq!(got.len(), 1);
    assert_eq!((got[0].old, got[0].new, got[0].delta), (5, 0, -5), "夹到 0，记实际变化量");
    assert_eq!(stat(&pool, "stat_stamina").await, 0);
    let rows = list_in(&pool, &None, Some(1), 10).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].delta, -5);
}

#[tokio::test]
async fn sleep_counts_once_per_day_and_gives_both_dims() {
    let pool = pool().await;
    let plan = plan_kind("睡好", "日结");
    let first = apply(&pool, &None, 1, 10, 1, &plan, "model").await;
    assert_eq!(first.len(), 2, "睡好同时给体力和理智");
    assert_eq!(stat(&pool, "stat_stamina").await, 65);
    assert_eq!(stat(&pool, "stat_sanity").await, 54);
    let second = apply(&pool, &None, 1, 11, 1, &plan, "model").await;
    assert!(second.is_empty(), "同一故事日同一类型只算一次");
    assert_eq!(stat(&pool, "stat_stamina").await, 65);
}

#[tokio::test]
async fn presence_is_capped_at_five_per_day() {
    let pool = pool().await;
    let praise = settle("x\n〔状态变化〕被认可：当众被肯定");
    assert_eq!(apply(&pool, &None, 1, 10, 1, &praise, "model").await.len(), 1);
    assert_eq!(stat(&pool, "stat_screen").await, 55);
    // 同一天再来一次：已经 +5，不能再加
    let again = apply(&pool, &None, 1, 11, 1, &praise, "model").await;
    assert!(again.is_empty(), "当天存在感已到 +5");
    // 换一天可以再加
    assert_eq!(apply(&pool, &None, 2, 12, 1, &praise, "model").await.len(), 1);
    assert_eq!(stat(&pool, "stat_screen").await, 60);
}

#[tokio::test]
async fn no_automatic_recovery_when_the_day_changes() {
    let pool = pool().await;
    apply(&pool, &None, 1, 10, 1, &settle("x\n〔状态变化〕受伤：摔下台阶"), "model").await;
    assert_eq!(stat(&pool, "stat_stamina").await, 35);
    // 第二天：没有任何协议行 → 数值不动（不做"每加一天 +15"）
    assert!(apply(&pool, &None, 2, 11, 1, &[], "model").await.is_empty());
    assert_eq!(stat(&pool, "stat_stamina").await, 35);
}

#[tokio::test]
async fn undo_reverses_and_marks_the_original() {
    let pool = pool().await;
    apply(&pool, &None, 1, 10, 1, &settle("x\n〔状态变化〕受伤：摔下台阶"), "model").await;
    assert_eq!(stat(&pool, "stat_stamina").await, 35);
    let id = list_in(&pool, &None, Some(1), 10).await.unwrap()[0].id;
    let r = undo_in(&pool, id).await.unwrap();
    assert_eq!((r.old, r.new), (35, 50));
    assert_eq!(stat(&pool, "stat_stamina").await, 50);
    let rows = list_in(&pool, &None, Some(1), 10).await.unwrap();
    assert_eq!(rows[0].source, "undo");
    assert_eq!(rows[1].revoked, 1);
    assert!(undo_in(&pool, id).await.is_err(), "不能撤两次");
}

#[tokio::test]
async fn branch_scope_is_respected() {
    let pool = pool().await;
    let plan = settle("x\n〔状态变化〕受伤：摔下台阶");
    apply(&pool, &Some("b1".to_string()), 1, 10, 1, &plan, "model").await;
    assert_eq!(list_in(&pool, &None, None, 10).await.unwrap().len(), 0);
    assert_eq!(list_in(&pool, &Some("b1".to_string()), None, 10).await.unwrap().len(), 1);
}

#[test]
fn legend_never_leaks_numbers() {
    let s = legend();
    assert!(s.contains("〔状态变化〕"));
    assert!(s.contains("被当众羞辱") && s.contains("受伤"));
    assert!(!s.contains("睡好") && !s.contains("熬夜"), "作息只在日结里，不该出现在对话协议的提示词里");
    assert!(!s.contains("-10") && !s.contains("+15") && !s.contains("15 "), "提示词里不许出现分值");
}

/// 2026-11-02（用户定）：存在感**正负各自封顶 ±5、互不抵扣**。
/// 原来按"当天净额"夹 ±5 → 先被羞辱 −5、后被认可 +5 被抵成 0，一天两件事等于没发生。
#[tokio::test]
async fn presence_positive_and_negative_caps_are_independent() {
    let pool = pool().await;
    apply(&pool, &None, 1, 10, 1, &settle("x\n〔状态变化〕被当众羞辱：当着人下不来台"), "model").await;
    assert_eq!(stat(&pool, "stat_screen").await, 45, "先 −5");
    let added = apply(&pool, &None, 1, 11, 1, &settle("x\n〔状态变化〕被认可：有人夸她"), "model").await;
    assert_eq!(added.len(), 1, "正负互不抵扣，+5 必须落地：{added:?}");
    assert_eq!(stat(&pool, "stat_screen").await, 50, "同一句话先羞辱后被认可要记两笔");
    // 正向当天已满 5：再来 +3 加不进去
    let more = apply(&pool, &None, 1, 12, 1, &settle("x\n〔状态变化〕被看见：有人注意到她"), "model").await;
    assert!(more.is_empty(), "正向当天已到 +5：{more:?}");
}

/// 2026-11-02：类型口径要进提示词（归类才会稳），而且仍然不带数字。
#[test]
fn legend_carries_kind_hints_and_no_numbers() {
    let s = legend();
    assert!(s.contains("受伤（被打"), "要带口径：{s}");
    assert!(s.contains("被当众羞辱（当着人下不来台）"), "要带口径：{s}");
    // 只查"类型清单"这一行：理由上限的 32 字是说明文字，不算数值泄露
    let line = s.lines().find(|l| l.contains("类型只能写")).expect("要有类型清单行");
    assert!(!line.chars().any(|c| c.is_ascii_digit()), "类型清单里不许出现数字：{line}");
}