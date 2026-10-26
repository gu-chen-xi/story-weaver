// 冲突检测的回归测试（2026-10-26 新增）。
//
// 每条对应一个界面上真出现过的假冲突：
//   ① 跨分支重复（同一地点在主线和分支各一份）→ 全报成坐标冲突；
//   ② 没填坐标的地点默认 (50,50) → 两个没坐标的地点必然互报；
//   ③ 同坐标 3 个地点只报前两个，第三个静默消失。

use super::*;
use sqlx::sqlite::SqlitePoolOptions;

fn w(id: i64, key: &str, value: &str, category: &str, meta: &str) -> WorldSetting {
    WorldSetting {
        id,
        key: key.to_string(),
        value: value.to_string(),
        is_hard_rule: 0,
        version: 1,
        category: category.to_string(),
        meta_json: meta.to_string(),
    }
}

fn place(id: i64, key: &str, x: &str, y: &str) -> WorldSetting {
    let meta = format!(r#"{{"x":"{}","y":"{}"}}"#, x, y);
    w(id, key, "说明", CATEGORY_GEO, &meta)
}

/// 同一分支、同坐标的两个地点 = 一处冲突（基本盘，别修坏了）
#[test]
fn same_coords_in_one_branch_is_a_conflict() {
    let rows = vec![place(1, "夏家老宅", "50", "50"), place(2, "王家祠堂", "50", "50")];
    let out = detect_conflicts_in(&rows);
    assert_eq!(out.len(), 1, "应报 1 处冲突：{out:?}");
    assert_eq!((out[0].left_id, out[0].right_id), (1, 2));
    assert!(out[0].title.contains("夏家老宅") && out[0].title.contains("王家祠堂"));
}

/// **没填坐标 = 画在 (50,50)** —— 和地图上的图钉位置一致，所以两个都要报出来。
///
/// 这条是 2026-10-26 用户实测的反例：dev 样例故事「测试1」两个地点都显示在 50,50，
/// 但上一版"跳过没坐标的" → 检测不报，界面和数据对不上。
#[test]
fn missing_coords_fall_back_to_fifty() {
    let rows = vec![
        w(1, "旧仓库", "没坐标", CATEGORY_GEO, r#"{}"#),
        w(2, "废教堂", "x 是空的", CATEGORY_GEO, r#"{"x":""}"#),
        place(3, "码头", "10", "20"),
    ];
    let out = detect_conflicts_in(&rows);
    assert_eq!(out.len(), 1, "两个没填坐标的地点都落在 (50,50)，应报 1 处：{out:?}");
    assert_eq!((out[0].left_id, out[0].right_id), (1, 2));
    assert!(out[0].title.contains("50,50"), "标题要显示它实际被判的位置：{}", out[0].title);
}

/// 老数据的 `map_x` / `map_y` 也要认 —— 不能因为键名不同就当成"没填坐标"
#[test]
fn legacy_map_coord_keys_are_recognized() {
    let rows = vec![
        w(1, "东塔", "x", CATEGORY_GEO, r#"{"map_x":"30","map_y":"40"}"#),
        place(2, "西塔", "30", "40"),
    ];
    let out = detect_conflicts_in(&rows);
    assert_eq!(out.len(), 1, "map_x/map_y 应与 x/y 等价：{out:?}");
    assert!(out[0].title.contains("30,40"));
}

/// 写入时补默认坐标：地理条目一律带 x/y，其它分类不动
#[test]
fn normalize_meta_fills_default_coords_for_places_only() {
    let got = super::normalize_meta(CATEGORY_GEO, r#"{"region":"城西"}"#);
    let v: serde_json::Value = serde_json::from_str(&got).unwrap();
    assert_eq!(v["x"], "50");
    assert_eq!(v["y"], "50");
    assert_eq!(v["region"], "城西", "其它键要保留");

    let got2 = super::normalize_meta(CATEGORY_GEO, r#"{"x":"" ,"y":"70"}"#);
    let v2: serde_json::Value = serde_json::from_str(&got2).unwrap();
    assert_eq!(v2["x"], "50", "空串也要补成 50");
    assert_eq!(v2["y"], "70", "已有值不动");

    assert_eq!(
        super::normalize_meta("势力", r#"{}"#),
        r#"{}"#,
        "其它分类不许被塞坐标"
    );
}

/// 同一坐标 3 个地点 → 两两全报（3 条），不能漏掉第三个
#[test]
fn three_at_same_coords_report_all_pairs() {
    let rows = vec![
        place(1, "A", "50", "50"),
        place(2, "B", "50", "50"),
        place(3, "C", "50", "50"),
    ];
    let out = detect_conflicts_in(&rows);
    assert_eq!(out.len(), 3, "3 个地点应有 3 对：{out:?}");
    let pairs: Vec<(i64, i64)> = out.iter().map(|c| (c.left_id, c.right_id)).collect();
    assert_eq!(pairs, vec![(1, 2), (1, 3), (2, 3)], "成对要全、且顺序稳定");
}

/// 坐标不同 / 不是地理分类 —— 一律不算
#[test]
fn other_coords_and_categories_are_ignored() {
    let rows = vec![
        place(1, "码头", "10", "20"),
        place(2, "车站", "30", "40"),
        w(3, "术语甲", "x", "术语", r#"{"x":"10","y":"20"}"#),
        w(4, "术语乙", "x", "术语", r#"{"x":"10","y":"20"}"#),
    ];
    assert!(detect_conflicts_in(&rows).is_empty());
}

/// 坐标可能是数字（导入 / 老数据），不能只认字符串
#[test]
fn numeric_coords_are_accepted() {
    let rows = vec![
        w(1, "东塔", "x", CATEGORY_GEO, r#"{"x":40,"y":50}"#),
        place(2, "西塔", "40", "50"),
    ];
    assert_eq!(detect_conflicts_in(&rows).len(), 1, "数字与字符串同值应算同坐标");
}

/// 分支隔离（用户实测的根因）：建分支会把世界观整份复制，
/// 同一个地点在主线和分支各一份 —— 那是同一处地方，不是冲突。
#[tokio::test]
async fn branch_copies_do_not_conflict() {
    let pool = pool_with_places().await;
    assert!(
        detect_conflicts_with(&pool, &None).await.unwrap().is_empty(),
        "主线上只有一个地点，不该报冲突"
    );
    assert!(
        detect_conflicts_with(&pool, &Some("b1".to_string())).await.unwrap().is_empty(),
        "分支里也只有那一份拷贝，不该报冲突"
    );
}

/// 分支内部真有两个地点抢同一坐标 —— 那个还是要报；主线不受影响
#[tokio::test]
async fn conflict_inside_a_branch_is_still_reported() {
    let pool = pool_with_places().await;
    sqlx::query(
        "INSERT INTO world_settings(key, value, category, meta_json, branch_id) \
         VALUES(?1, ?2, ?3, ?4, ?5)",
    )
    .bind("王家祠堂")
    .bind("说明")
    .bind(CATEGORY_GEO)
    .bind(r#"{"x":"50","y":"50"}"#)
    .bind("b1")
    .execute(&pool)
    .await
    .unwrap();
    let out = detect_conflicts_with(&pool, &Some("b1".to_string())).await.unwrap();
    assert_eq!(out.len(), 1, "分支里两个地点同坐标应报 1 处：{out:?}");
    assert!(detect_conflicts_with(&pool, &None).await.unwrap().is_empty(), "主线不受影响");
}

async fn pool_with_places() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE world_settings (
            id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT, value TEXT,
            is_hard_rule INTEGER DEFAULT 0, version INTEGER DEFAULT 1,
            category TEXT, meta_json TEXT DEFAULT '{}', branch_id TEXT)",
    )
    .execute(&pool)
    .await
    .unwrap();
    // 主线：夏家老宅 (50,50)；分支 b1：同一份拷贝（分叉时复制出来的）
    for br in [None::<&str>, Some("b1")] {
        sqlx::query(
            "INSERT INTO world_settings(key, value, category, meta_json, branch_id) \
             VALUES(?1, ?2, ?3, ?4, ?5)",
        )
        .bind("夏家老宅")
        .bind("三进院子")
        .bind(CATEGORY_GEO)
        .bind(r#"{"x":"50","y":"50"}"#)
        .bind(br)
        .execute(&pool)
        .await
        .unwrap();
    }
    pool
}
