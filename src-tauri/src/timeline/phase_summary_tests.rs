// 从 `timeline.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `timeline::phase_summary_tests` —— 因为 `timeline.rs` + `timeline/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `timeline`，一行都不用改写。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;

async fn temp_db(tag: &str) -> (SqlitePool, std::path::PathBuf) {
    let dir = crate::paths::test_root().join(tag).join("projects").join(tag);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("story.db");
    let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    sqlx::query(
        "CREATE TABLE timeline_nodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT, date_label TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0, node_type TEXT NOT NULL DEFAULT 'day',
            is_flashback INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '',
            day_summary TEXT NOT NULL DEFAULT '',
            parent_id INTEGER, branch_id TEXT)",
    ).execute(&pool).await.unwrap();
    crate::story_db::ensure_story_summaries_table(&pool).await.unwrap();
    // 真实库由 ensure_branch_columns 补这一列；测试库手工补上
    let _ = sqlx::query("ALTER TABLE story_summaries ADD COLUMN branch_id TEXT")
        .execute(&pool).await;
    (pool, dir)
}

async fn add_day(pool: &SqlitePool, day: i64, summary: &str) {
    sqlx::query("INSERT INTO timeline_nodes(date_label, sort_order, node_type, is_flashback, day_summary) VALUES(?1, ?2, 'day', 0, ?3)")
        .bind(format!("第 {day} 天")).bind(day).bind(summary)
        .execute(pool).await.unwrap();
}

/// 用户例子（2026-10-17 定）：日摘要在 1~5 与 8~12（共 10 天，6~7 空档）
async fn build_1to5_8to12(tag: &str) -> (SqlitePool, std::path::PathBuf) {
    let (pool, dir) = temp_db(tag).await;
    for d in 1..=5 { add_day(&pool, d, "有摘要").await; }
    for d in 8..=12 { add_day(&pool, d, "有摘要").await; }
    (pool, dir)
}

/// x=5 → 压 1~5
#[tokio::test]
async fn x5_compresses_days_1_to_5() {
    let (pool, dir) = build_1to5_8to12("phase_x5").await;
    let plan = plan_phase_summary_in(&pool, &None, 5, false).await.unwrap().expect("够 5 天");
    assert_eq!(plan.2.len(), 5, "压 5 天");
    assert_eq!((plan.0, plan.1), (1, 5), "覆盖第 1~5 天");
    let _ = std::fs::remove_dir_all(dir);
}

/// x=7 → 压 1~5 + 8~9（跨过空档），覆盖 1~9
#[tokio::test]
async fn x7_crosses_gap_and_covers_1_to_9() {
    let (pool, dir) = build_1to5_8to12("phase_x7").await;
    let plan = plan_phase_summary_in(&pool, &None, 7, false).await.unwrap().expect("够 7 天");
    assert_eq!(plan.2.len(), 7);
    assert_eq!((plan.0, plan.1), (1, 9), "1~5 + 8~9 → 覆盖 1~9");
    assert_eq!(plan.2.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(),
               vec!["第 1 天","第 2 天","第 3 天","第 4 天","第 5 天","第 8 天","第 9 天"]);
    let _ = std::fs::remove_dir_all(dir);
}

/// x=10 → 压满 10 天，覆盖 1~12
#[tokio::test]
async fn x10_covers_all_ten_days() {
    let (pool, dir) = build_1to5_8to12("phase_x10").await;
    let plan = plan_phase_summary_in(&pool, &None, 10, false).await.unwrap().expect("够 10 天");
    assert_eq!(plan.2.len(), 10);
    assert_eq!((plan.0, plan.1), (1, 12), "覆盖 1~12");
    let _ = std::fs::remove_dir_all(dir);
}

/// 压过之后，下次从**断点之后**接着算：x=5 压了 1~5 → 下一次从第 8 天起
#[tokio::test]
async fn next_phase_starts_after_the_break() {
    let (pool, dir) = build_1to5_8to12("phase_next").await;
    let first = plan_phase_summary_in(&pool, &None, 5, false).await.unwrap().unwrap();
    save_phase_summary_in(&pool, &None, first.0, first.1, "第一段", 5).await.unwrap();

    // 剩 8~12 共 5 天 → x=5 正好再压一段，从第 8 天起
    let second = plan_phase_summary_in(&pool, &None, 5, false).await.unwrap().expect("剩 5 天");
    assert_eq!((second.0, second.1), (8, 12), "下一段从第 8 天开始");
    assert_eq!(second.2[0].0, "第 8 天");
    let _ = std::fs::remove_dir_all(dir);
}

/// 攒不够 x 天 → 不生成
#[tokio::test]
async fn below_threshold_means_none() {
    let (pool, dir) = temp_db("phase_below").await;
    for d in 1..=4 { add_day(&pool, d, "有摘要").await; }
    assert!(plan_phase_summary_in(&pool, &None, 5, false).await.unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// 手动重压（ignore_covered=true）：旧阶段覆盖了全部天时，仍能重新规划
/// —— 否则界面永远提示"攒不够"（用户实测踩到）
#[tokio::test]
async fn manual_rebuild_ignores_existing_phases() {
    let (pool, dir) = build_1to5_8to12("phase_rebuild").await;
    save_phase_summary_in(&pool, &None, 1, 12, "旧的阶段摘要", 10).await.unwrap();

    assert!(plan_phase_summary_in(&pool, &None, 5, false).await.unwrap().is_none(),
            "增量模式下已被覆盖，没有可压的");
    let plan = plan_phase_summary_in(&pool, &None, 5, true).await.unwrap().expect("手动重压应能重算");
    assert_eq!((plan.0, plan.1), (1, 5));
    let _ = std::fs::remove_dir_all(dir);
}

/// 在**真实库副本**上跑一次规划（`#[ignore]`，手动触发；不调模型）。
/// 用途：确认"这本书会不会生成阶段摘要、覆盖哪几天"。
#[tokio::test]
#[ignore]
async fn diag_phase_plan_on_real_db() {
    let Ok(dir) = std::env::var("DW_DIAG_DB") else {
        eprintln!("未设置 DW_DIAG_DB，跳过");
        return;
    };
    // 复制一份，避免污染真实库
    // 注意：副本必须写**系统临时目录**，不能写真实数据目录 ——
    // 测试中断会留下几 MB 的垃圾文件在用户的 projects/ 里（2026-10-18 实测踩到）。
    let tmp = std::env::temp_dir().join("dw_diag_phase.db");
    let _ = std::fs::copy(&dir, &tmp);
    let opts = SqliteConnectOptions::new().filename(&tmp);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    let _ = sqlx::query("ALTER TABLE story_summaries ADD COLUMN branch_id TEXT").execute(&pool).await;

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT date_label, COALESCE(day_summary,'') FROM timeline_nodes
         WHERE node_type IN ('day','main') AND is_flashback = 0
         ORDER BY sort_order, id").fetch_all(&pool).await.unwrap();
    eprintln!("\n=== 库里的天 ===");
    for (label, s) in &rows {
        eprintln!("  {label:<10} 摘要长度={}", s.chars().count());
    }
    let with_sum = rows.iter().filter(|(_, s)| !s.trim().is_empty()).count();
    eprintln!("  天数节点 {} 个，有日摘要 {} 个（阈值 {}）", rows.len(), with_sum, PHASE_MIN_DAYS);

    match plan_phase_summary_in(&pool, &None, PHASE_MIN_DAYS, false).await {
        Ok(Some((from, to, days))) => {
            eprintln!("\n=== 规划结果 ===");
            eprintln!("  ✅ 覆盖第 {from} ~ {to} 天，素材 {} 天：", days.len());
            for (l, _) in &days { eprintln!("     · {l}"); }
        }
        Ok(None) => eprintln!("\n  规划结果：None（还攒不够 {} 天有摘要的天）", PHASE_MIN_DAYS),
        Err(e) => eprintln!("  规划出错：{e}"),
    }
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
    let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
    let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
    let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
    let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
}

/// 真实库副本：验证"手动重压"能绕过旧阶段覆盖（用户实测卡住的地方）。
#[tokio::test]
#[ignore]
async fn diag_manual_rebuild_on_real_db() {
    use sqlx::sqlite::SqliteConnectOptions;
    let Ok(dir) = std::env::var("DW_DIAG_DB") else { eprintln!("未设置 DW_DIAG_DB"); return };
    // 注意：副本必须写**系统临时目录**，不能写真实数据目录 ——
    // 测试中断会留下几 MB 的垃圾文件在用户的 projects/ 里（2026-10-18 实测踩到）。
    let tmp = std::env::temp_dir().join("dw_diag_rebuild.db");
    let _ = std::fs::copy(&dir, &tmp);
    let opts = SqliteConnectOptions::new().filename(&tmp);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    let _ = sqlx::query("ALTER TABLE story_summaries ADD COLUMN branch_id TEXT").execute(&pool).await;

    let covered = list_phase_summaries_in(&pool, &None).await.unwrap_or_default();
    eprintln!("\n=== 现有阶段摘要 ===");
    for (f, t2, n, c) in &covered {
        eprintln!("  第 {f}~{t2} 天（source_days={n}）：{}…", c.chars().take(40).collect::<String>());
    }
    eprintln!("\n=== 增量模式（现状，就是界面报「攒不够」的原因）===");
    match plan_phase_summary_in(&pool, &None, 10, false).await {
        Ok(Some((f, t2, d))) => eprintln!("  可压 第{f}~{t2} 天 / {} 天", d.len()),
        Ok(None) => eprintln!("  None（没有可压的天：都被旧阶段覆盖了）"),
        Err(e) => eprintln!("  ERR {e}"),
    }
    eprintln!("\n=== 手动重压（ignore_covered = true）===");
    match plan_phase_summary_in(&pool, &None, 10, true).await {
        Ok(Some((f, t2, d))) => {
            eprintln!("  ✅ 可压 第{f}~{t2} 天，素材 {} 天：", d.len());
            for (l, _) in &d { eprintln!("     · {l}"); }
        }
        Ok(None) => eprintln!("  None"),
        Err(e) => eprintln!("  ERR {e}"),
    }
    let _ = std::fs::remove_file(&tmp);
}

/// 真实库副本：按用户例子打印 x=5 / 7 / 10 各自会覆盖哪几天（不调模型）。
#[tokio::test]
#[ignore]
async fn diag_x_cases_on_real_db() {
    use sqlx::sqlite::SqliteConnectOptions;
    let Ok(dir) = std::env::var("DW_DIAG_DB") else { eprintln!("未设置 DW_DIAG_DB"); return };
    // 注意：副本必须写**系统临时目录**，不能写真实数据目录 ——
    // 测试中断会留下几 MB 的垃圾文件在用户的 projects/ 里（2026-10-18 实测踩到）。
    let tmp = std::env::temp_dir().join("dw_diag_x.db");
    let _ = std::fs::copy(&dir, &tmp);
    let opts = SqliteConnectOptions::new().filename(&tmp);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    let _ = sqlx::query("ALTER TABLE story_summaries ADD COLUMN branch_id TEXT").execute(&pool).await;

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT date_label, COALESCE(day_summary,'') FROM timeline_nodes
         WHERE node_type IN ('day','main') AND is_flashback=0 ORDER BY sort_order, id")
        .fetch_all(&pool).await.unwrap();
    let with_sum = rows.iter().filter(|(_, s)| !s.trim().is_empty()).count();
    eprintln!("\n库里：{} 个天数节点，其中 {} 天有日摘要", rows.len(), with_sum);

    for x in [5usize, 7, 10] {
        match plan_phase_summary_in(&pool, &None, x, true).await {
            Ok(Some((f, to, d))) => eprintln!(
                "  x={x:<3} → 覆盖 第 {f}~{to} 天，素材 {} 天：{:?}",
                d.len(), d.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>()),
            Ok(None) => eprintln!("  x={x:<3} → 攒不够（需要 {x} 天有摘要）"),
            Err(e) => eprintln!("  x={x:<3} → ERR {e}"),
        }
    }
    let _ = std::fs::remove_file(&tmp);
}
