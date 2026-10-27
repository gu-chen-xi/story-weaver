// 导演代价的回归测试（2026-10-27）。
//
// 每一条都对应一条**必须成立的规矩**（用户原话）：
//   表外类型 = 0 / 一轮取最重 / 空收据不记 / 分支隔离 / 阈值够锁闸门 / 导出不含标记。

use super::*;
use sqlx::sqlite::SqlitePoolOptions;

/// 和生产同一份 DDL（改表结构时这里要跟着改，否则测试通过但线上炸）
async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE timeline_nodes (
            id INTEGER PRIMARY KEY, date_label TEXT, branch_id TEXT)",
    )
    .execute(&pool).await.unwrap();
    sqlx::query(
        "CREATE TABLE director_costs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id INTEGER,
            message_id INTEGER,
            character_id INTEGER,
            kind TEXT NOT NULL,
            reason TEXT NOT NULL,
            score INTEGER NOT NULL,
            source TEXT NOT NULL DEFAULT 'model',
            excused INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
            branch_id TEXT)",
    )
    .execute(&pool).await.unwrap();
    pool
}

fn cand(kind: &str, reason: &str, score: i64, source: &'static str) -> Candidate {
    Candidate { kind: kind.to_string(), reason: reason.to_string(), score, source }
}

/// **表外类型 = 0**：模型自创的类型不记账（也不报错），协议行仍然从正文里删掉
#[test]
fn unknown_kind_scores_zero_and_is_not_settled() {
    let reply = "（她点头）「行吧。」\n〔导演代价〕撒娇：她根本拒绝不了";
    let r = parse_receipt(reply).expect("协议行形状对，应该解析得出来");
    assert_eq!(r.score, 0, "表外类型必须 = 0");
    assert!(settle(reply).is_none(), "0 分不是一笔账，不能入账");
    // 但协议行不能留在正文里
    assert_eq!(strip_receipt(reply), "（她点头）「行吧。」");
}

/// **只认最后一行**：正文中间出现的同名标记不解析、不剥离
#[test]
fn receipt_must_be_the_last_line() {
    let mid = "（她皱眉）〔导演代价〕白给：无所谓\n「你走吧。」";
    assert!(parse_receipt(mid).is_none(), "中间的标记不是收据");
    assert_eq!(strip_receipt(mid), mid, "中间的标记不许剥离");

    let last = "「你走吧。」\n\n〔导演代价〕白给：她懒得争了";
    assert_eq!(parse_receipt(last).unwrap().score, 30);
}

/// **理由 32 字够、33 字不收**（24 字那条老上限已被用户否掉）
#[test]
fn reason_length_is_capped_at_32_chars() {
    let ok = format!("〔导演代价〕条件：{}", "先".repeat(32));
    assert!(parse_receipt(&ok).is_some(), "32 字应该收");

    let bad = format!("〔导演代价〕条件：{}", "先".repeat(33));
    assert!(parse_receipt(&bad).is_none(), "33 字必须退回空收据");
}

/// 全角 / 半角冒号都收，类型两边有空格也认
#[test]
fn both_colons_are_accepted() {
    let half = parse_receipt("〔导演代价〕时限: 只试三天").unwrap();
    assert_eq!((half.kind.as_str(), half.score), ("时限", 15));
    let spaced = parse_receipt("〔导演代价〕 交换 ： 他答应帮她一次").unwrap();
    assert_eq!((spaced.kind.as_str(), spaced.score), ("交换", 5));
}

/// **空收据不记**：没有标记 / 理由是空的 → 一律不入账
#[test]
fn empty_receipt_is_not_recorded() {
    assert!(settle("（她别过脸）「别。」").is_none());
    assert!(settle("〔导演代价〕条件：").is_none(), "空理由等于没写");
    assert!(settle("〔导演代价〕条件").is_none(), "没有冒号等于没写");
}

/// **一轮只结一次账（取最重）**：模型写「条件」，正文里却明摆着是「心软」→ 按 25 记，
/// 而不是 10 + 25 记两笔（这就是"一轮双计费"那类坑）
#[test]
fn one_round_settles_once_taking_the_heaviest() {
    let reply = "（她眼眶红了）「我舍不得。」\n（她终究心软了）\n〔导演代价〕条件：她说要先道歉";
    let c = settle(reply).expect("应该结出一笔账");
    assert_eq!(c.kind, "心软", "推断出更重的那一档");
    assert_eq!(c.score, 25);
    assert_eq!(c.source, "rule", "来源要标清楚，账本里能看出是推断的");
}

/// 收据存在、且它是更重的那一档 → 用它（不重复计费）
#[test]
fn receipt_wins_when_it_is_heavier() {
    let reply = "（她随口应了一声）\n〔导演代价〕白给：她根本没听清";
    let c = settle(reply).unwrap();
    assert_eq!((c.kind.as_str(), c.score, c.source), ("白给", 30, "model"));
}

/// 记一笔 → 账本里就一行；同一轮再 settle 一次也不会多出第二行
#[tokio::test]
async fn record_writes_exactly_one_row_per_round() {
    let pool = pool().await;
    let reply = "（她眼眶红了）「我舍不得。」\n（她终究心软了）\n〔导演代价〕条件：她说要先道歉";
    let c = settle(reply).unwrap();
    record_cost(&pool, &None, 7, Some(31), Some(1), &c).await.unwrap();
    let rows = list_rows_in(&pool, &None).await.unwrap();
    assert_eq!(rows.len(), 1, "一轮只能有一笔账：{rows:?}");
    assert_eq!(rows[0].score, 25);
    assert_eq!(rows[0].kind, "心软");
}

/// **分支隔离**：分支里的账不算进主线，也不互相串（额度是各自一条线）
#[tokio::test]
async fn ledger_is_branch_isolated() {
    let pool = pool().await;
    sqlx::query("INSERT INTO timeline_nodes(id, date_label, branch_id) VALUES(1, '第 1 天', NULL)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO timeline_nodes(id, date_label, branch_id) VALUES(2, '第 7 天', 'BR')")
        .execute(&pool).await.unwrap();

    record_cost(&pool, &None, 1, None, None, &cand("交换", "主线一笔", 5, "model")).await.unwrap();
    record_cost(&pool, &None, 1, None, None, &cand("条件", "主线第二笔", 10, "model")).await.unwrap();
    record_cost(&pool, &Some("BR".into()), 2, None, None, &cand("白给", "分支一笔", 30, "model")).await.unwrap();

    let main = ledger_in(&pool, &None, None, 20).await.unwrap();
    let br = ledger_in(&pool, &Some("BR".into()), None, 20).await.unwrap();
    assert_eq!(main.total, 15, "主线只算主线的：{main:?}");
    assert_eq!(br.total, 30, "分支只算分支的：{br:?}");
    assert_eq!(main.rows.len(), 2);
    assert_eq!(br.rows[0].day_label, "第 7 天", "账本要显示这笔发生在哪天");
}

/// 账本按 AI 扮演者分账：换个人就是另一本账
#[tokio::test]
async fn ledger_is_per_character() {
    let pool = pool().await;
    record_cost(&pool, &None, 1, None, Some(1), &cand("交换", "给甲", 5, "model")).await.unwrap();
    record_cost(&pool, &None, 1, None, Some(2), &cand("白给", "给乙", 30, "model")).await.unwrap();

    assert_eq!(ledger_in(&pool, &None, Some(1), 20).await.unwrap().total, 5);
    assert_eq!(ledger_in(&pool, &None, Some(2), 20).await.unwrap().total, 30);
    assert_eq!(ledger_in(&pool, &None, None, 20).await.unwrap().total, 0, "没指定角色时只读「自由」那一本");
}

/// **阈值够锁闸门**：两次「白给」正好 60（= LIMIT），赦免一笔就回到 30
///
/// 这条不测前端，测的是"闸门读的那个数"对不对 —— 前端只是拿 total >= limit 判断。
#[tokio::test]
async fn total_reaches_limit_and_excuse_lowers_it() {
    let pool = pool().await;
    let a = record_cost(&pool, &None, 1, None, None, &cand("白给", "第一次", 30, "model")).await.unwrap();
    record_cost(&pool, &None, 1, None, None, &cand("白给", "第二次", 30, "model")).await.unwrap();

    let led = ledger_in(&pool, &None, None, 20).await.unwrap();
    assert_eq!((led.total, led.limit), (60, LIMIT));
    assert!(led.total >= led.limit, "两笔白给就该顶到闸门");

    assert!(excuse_in(&pool, a).await.unwrap());
    let after = ledger_in(&pool, &None, None, 20).await.unwrap();
    assert_eq!(after.total, 30, "赦免留痕但不计分");
    assert_eq!(after.rows.len(), 2, "账本里两行都还在");
    assert_eq!(after.rows.iter().filter(|r| r.excused == 1).count(), 1);
}

/// 撤销是**删掉**（和赦免不是一回事）
#[tokio::test]
async fn delete_removes_the_row_for_good() {
    let pool = pool().await;
    let id = record_cost(&pool, &None, 1, None, None, &cand("时限", "先试三天", 15, "model")).await.unwrap();
    assert!(delete_in(&pool, id).await.unwrap());
    let led = ledger_in(&pool, &None, None, 20).await.unwrap();
    assert!(led.rows.is_empty());
    assert_eq!(led.total, 0);
    assert!(!delete_in(&pool, id).await.unwrap(), "删不存在的行要返回 false，不能假装成功");
}

/// 单次上限：分值被 MAX_SINGLE 夹住（改表时别悄悄破了上限）
#[tokio::test]
async fn single_score_is_clamped() {
    let pool = pool().await;
    record_cost(&pool, &None, 1, None, None, &cand("白给", "超重", 999, "model")).await.unwrap();
    let rows = list_rows_in(&pool, &None).await.unwrap();
    assert_eq!(rows[0].score, MAX_SINGLE);
}

/// 注入段：只给类型、不给分值（模型不碰数字），也没有 JSON
#[test]
fn cost_legend_never_leaks_numbers() {
    let legend = cost_legend();
    assert!(legend.starts_with("【导演代价】"), "段名要和尾部块一致：{legend}");
    assert!(legend.contains("〔导演代价〕类型：理由"), "模板行要能照抄：{legend}");
    for (name, _, score) in COST_KINDS.iter() {
        assert!(legend.contains(name), "类型「{name}」必须列出来");
        assert!(
            !legend.contains(&format!("{score} 分")) && !legend.contains(&format!("={score}")),
            "分值不能出现在提示词里（模型不碰数字）：{name} = {score}"
        );
    }
    assert!(!legend.contains("JSON") && !legend.contains("json"));
}


/// 生产建库路径（init_story_db_at）出来的库，必须真的有 director_costs 且有这几列。
///
/// 为什么单列一条：上面的测试用的是**测试自己写的 DDL**，和生产那份不是同一个字符串 ——
/// 生产那边改列名/漏建表，测试照样全绿，装到用户机器上才炸（E-DB-05 的同一类）。
#[tokio::test]
async fn production_schema_has_the_ledger_table() {
    use sqlx::Row;
    let dir = crate::paths::test_root().join("director_schema");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("story.db");
    let pool = crate::story_db::init_story_db_at(path).await.unwrap();

    let rows = sqlx::query("PRAGMA table_info(director_costs)")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(!rows.is_empty(), "新建的故事库必须有 director_costs 表");
    let cols: Vec<String> = rows
        .iter()
        .map(|r| r.try_get::<String, _>(1).unwrap_or_default())
        .collect();
    for c in ["node_id", "message_id", "character_id", "kind", "reason", "score", "source", "excused", "branch_id"] {
        assert!(cols.iter().any(|x| x == c), "director_costs 缺列 {c}（实际：{cols:?}）");
    }

    pool.close().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// **老库升级**：打开一个还没有 director_costs 的库，跑完整迁移后表必须被补出来。
///
/// 老库（v3.8.0 及更早）里根本没有这张表 —— 少这一步，用户升级后一用导演指令就报 no such table。
#[tokio::test]
async fn migrations_add_the_ledger_table_to_an_old_db() {
    use sqlx::Row;
    let pool = pool().await;
    sqlx::query("DROP TABLE director_costs").execute(&pool).await.unwrap();

    crate::story_db::apply_all_story_migrations(&pool).await;

    let rows = sqlx::query("PRAGMA table_info(director_costs)")
        .fetch_all(&pool)
        .await
        .unwrap();
    let cols: Vec<String> = rows
        .iter()
        .map(|r| r.try_get::<String, _>(1).unwrap_or_default())
        .collect();
    assert!(cols.iter().any(|c| c == "branch_id"), "老库迁移后也要带 branch_id：{cols:?}");
}
/// 导出前统一剥协议行（导出文件会离开本软件，标记不能跟着走）
///
/// 这里做**源码守卫**：真正调用点在 export.rs 的 load_story_export 里，
/// 而它依赖整库数据、单测不好造 —— 所以直接盯那行调用别被删掉（和 stream_guards 一个路子）。
#[test]
fn export_strips_the_receipt() {
    let src = include_str!("../export.rs");
    assert!(
        src.contains("director::strip_receipt"),
        "load_story_export 必须剥〔导演代价〕协议行，否则导出文件里会留着它"
    );
}
