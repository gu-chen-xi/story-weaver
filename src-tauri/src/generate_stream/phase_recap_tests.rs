// 从 `generate_stream.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `generate_stream::phase_recap_tests` —— 因为 `generate_stream.rs` + `generate_stream/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `generate_stream`，一行都不用改写。

use super::{coalesce_with_phases, build_time_base_block, parse_calendar_date};

fn day(i: usize, d: i64) -> (usize, i64, String) {
    (i, d, format!("- 第 {} 天：第{}天的摘要内容", d, d))
}

/// 被阶段摘要覆盖的"老"日摘要 → 合并成一行；最近 3 天保持原样
#[test]
fn old_days_collapse_into_one_phase_line() {
    // 生产情况：阶段只覆盖"更早的那一坨"（1~9），最近 3 天（10/11/12）不覆盖
    let days: Vec<(usize, i64, String)> = (1..=12).map(|d| day(d as usize, d)).collect();
    let phases = vec![(1i64, 9i64, 9i64, "阶段摘要正文".to_string())];
    let out = coalesce_with_phases(&days, &phases);

    assert_eq!(out.len(), 4, "应为 1 行阶段摘要 + 最近 3 天：{:?}", out.iter().map(|(_, s)| s.as_str()).collect::<Vec<_>>());
    assert!(out[0].1.contains("第 1~9 天（阶段摘要）"), "第一行应是阶段摘要：{}", out[0].1);
    assert!(out[1].1.contains("第 10 天") && out[3].1.contains("第 12 天"), "最近 3 天应保留日摘要原文");

    // 合并后必须更省 token（否则不该合并）
    let before: usize = days.iter().map(|(_, _, s)| s.chars().count()).sum();
    let after: usize = out.iter().map(|(_, s)| s.chars().count()).sum();
    assert!(after < before, "合并后应更短：{after} vs {before}");
}

/// 没有阶段摘要 → 原样返回
#[test]
fn no_phases_keeps_everything() {
    let days: Vec<(usize, i64, String)> = (1..=5).map(|d| day(d as usize, d)).collect();
    let out = coalesce_with_phases(&days, &[]);
    assert_eq!(out.len(), 5);
    assert_eq!(out[0].1, days[0].2);
}

/// 阶段摘要覆盖不到这些天 → 原样返回（不能误伤）
#[test]
fn non_covered_days_are_untouched() {
    let days: Vec<(usize, i64, String)> = (20..=25).map(|d| day(d as usize, d)).collect();
    let phases = vec![(1i64, 12i64, 10i64, "x".to_string())];
    let out = coalesce_with_phases(&days, &phases);
    assert_eq!(out.len(), 6, "覆盖区间是 1~12，20~25 不该被动");
}

/// 只有一两天被覆盖时不合并（合并成一行的收益还不如留原文，且"一堆阶段行"更难读）
#[test]
fn single_covered_day_is_not_collapsed() {
    let days: Vec<(usize, i64, String)> = (1..=6).map(|d| day(d as usize, d)).collect();
    let phases = vec![(1i64, 1i64, 1i64, "x".to_string())];
    let out = coalesce_with_phases(&days, &phases);
    assert_eq!(out.len(), 6, "只覆盖第 1 天，不值得合并");
}

/// 时间基准：日期写法 → 输出绝对起点
#[test]
fn time_base_with_calendar_date() {
    let s = build_time_base_block("2026-03-01");
    assert!(s.contains("【时间基准】"), "应有小节标题：{s}");
    assert!(s.contains("第 1 天 = 2026-03-01"), "应含绝对起点：{s}");
    assert!(s.contains("起点 + (N-1) 天"), "应含换算规则：{s}");
}

/// 时间基准：中文日期写法也要认
#[test]
fn time_base_accepts_chinese_date() {
    assert_eq!(parse_calendar_date("2026年3月1日"), Some((2026, 3, 1)));
    assert_eq!(parse_calendar_date("2026/3/1"), Some((2026, 3, 1)));
    assert_eq!(parse_calendar_date("2026.03.01"), Some((2026, 3, 1)));
}

/// 时间基准：非日期 → 当作"时代背景"，不能崩、也不能编造年份
#[test]
fn time_base_with_era_text() {
    let s = build_time_base_block("民国初年");
    assert!(s.contains("民国初年"), "应原样带出时代描述：{s}");
    assert!(!s.contains("第 1 天 ="), "非日期时不该编造绝对日期：{s}");
}

/// 时间基准：空字符串 → 什么都不输出（老故事可能没填）
#[test]
fn time_base_empty_outputs_nothing() {
    assert_eq!(build_time_base_block(""), "");
    assert_eq!(build_time_base_block("   "), "");
}

/// 非法日期（月/日越界）→ 退化成"时代背景"，不静默算错
#[test]
fn invalid_date_falls_back_to_era() {
    assert_eq!(parse_calendar_date("2026-13-40"), None);
    assert!(build_time_base_block("2026-13-40").contains("时代背景"));
}

/// 在**真实库副本**上验证【前情提要】的合并效果（`#[ignore]`，手动触发；不调模型）。
/// 跑法：DW_DIAG_DB=<story.db> cargo test --lib diag_phase_recap -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn diag_phase_recap_on_real_db() {
    use sqlx::sqlite::SqliteConnectOptions;
    let Ok(dir) = std::env::var("DW_DIAG_DB") else { eprintln!("未设置 DW_DIAG_DB"); return };
    // 注意：副本写**系统临时目录**，不写真实数据目录（测试中断会留下垃圾文件）
    let tmp = std::env::temp_dir().join("dw_diag_recap.db");
    let _ = std::fs::copy(&dir, &tmp);
    let opts = SqliteConnectOptions::new().filename(&tmp);
    let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();
    let _ = sqlx::query("ALTER TABLE story_summaries ADD COLUMN branch_id TEXT").execute(&pool).await;

    // 造出与生产一致的 day_lines（第 N 天：摘要），排除最近一天（当前节点还在写）
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT date_label, COALESCE(day_summary,'') FROM timeline_nodes
         WHERE node_type IN ('day','main') AND is_flashback=0
         ORDER BY sort_order, id").fetch_all(&pool).await.unwrap();
    let mut day_lines: Vec<(usize, i64, String)> = Vec::new();
    for (i, (label, summary)) in rows.iter().enumerate() {
        if summary.trim().is_empty() { continue; }
        let Some(d) = crate::timeline::parse_day_number(label) else { continue };
        day_lines.push((i, d, format!("- {}：{}", label, summary.trim())));
    }
    let phases = crate::timeline::list_phase_summaries_in(&pool, &None).await.unwrap_or_default();

    let before: usize = day_lines.iter().map(|(_, _, s)| s.chars().count()).sum();
    let out = super::coalesce_with_phases(&day_lines, &phases);
    let after: usize = out.iter().map(|(_, s)| s.chars().count()).sum();

    eprintln!("\n=== 合并前（{} 行，{} 字）===", day_lines.len(), before);
    for (_, _, s) in &day_lines { eprintln!("  {}", s.chars().take(50).collect::<String>()); }
    eprintln!("\n=== 合并后（{} 行，{} 字）===", out.len(), after);
    for (_, s) in &out { eprintln!("  {}", s.chars().take(90).collect::<String>()); }
    eprintln!("\n  省下 {} 字（约 {} token）", before.saturating_sub(after),
              ((before.saturating_sub(after)) as f64 * 1.5) as usize);
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
    let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
}
