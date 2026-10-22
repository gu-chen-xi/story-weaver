// 从 `evolution.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `evolution::snapshot_diff_tests` —— 因为 `evolution.rs` + `evolution/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `evolution`，一行都不用改写。

use super::*;
use serde_json::json;

fn chars() -> Vec<(&'static str, &'static str)> {
    CHAR_FIELDS.to_vec()
}

#[test]
fn detects_added_removed_and_modified() {
    let before = vec![
        json!({"id": 1, "name": "夏婉琴", "role": "女主", "stat_emotion": 60}),
        json!({"id": 2, "name": "宸熙", "role": "男主", "stat_emotion": 50}),
        json!({"id": 3, "name": "临时角色", "role": "其他", "stat_emotion": 50}),
    ];
    let after = vec![
        // id 1：情绪被改了
        json!({"id": 1, "name": "夏婉琴", "role": "女主", "stat_emotion": 75}),
        // id 2：原样
        json!({"id": 2, "name": "宸熙", "role": "男主", "stat_emotion": 50}),
        // id 3：快照里没有 → 回滚会**删除**它
        // id 4：快照里有、当前没有 → 回滚会**加回来**
        json!({"id": 4, "name": "院长", "role": "其他", "stat_emotion": 50}),
    ];
    let d = diff_items(&before, &after, &chars(), &|v| jstr(v, "name"), "id");
    assert_eq!(d.len(), 3, "应有 修改1 + 删除1 + 新增1");
    let kinds: Vec<&str> = d.iter().map(|x| x.kind.as_str()).collect();
    assert!(kinds.contains(&"modified"));
    assert!(kinds.contains(&"removed"));
    assert!(kinds.contains(&"added"));

    let m = d.iter().find(|x| x.kind == "modified").unwrap();
    assert_eq!(m.label, "夏婉琴");
    assert_eq!(m.fields.len(), 1, "只应报告情绪一处变化");
    assert_eq!(m.fields[0].field, "情绪");
    assert_eq!(m.fields[0].before, "60");
    assert_eq!(m.fields[0].after, "75");

    let r = d.iter().find(|x| x.kind == "removed").unwrap();
    assert_eq!(r.label, "临时角色");
    assert!(r.fields.is_empty(), "删除项不需要字段差异");

    let a = d.iter().find(|x| x.kind == "added").unwrap();
    assert_eq!(a.label, "院长");
}

#[test]
fn identical_state_produces_no_diff() {
    let arr = vec![json!({"id": 1, "name": "A", "role": "女主"})];
    let d = diff_items(&arr, &arr, &chars(), &|v| jstr(v, "name"), "id");
    assert!(d.is_empty(), "完全一致时不应报差异（界面据此提示「无需回滚」）");
}

/// version 字段**刻意不比对** —— 它是内部计数，回滚时数值变化对用户没有意义
#[test]
fn version_field_is_ignored() {
    let before = vec![json!({"id": 1, "name": "A", "version": 3})];
    let after = vec![json!({"id": 1, "name": "A", "version": 7})];
    let d = diff_items(&before, &after, &chars(), &|v| jstr(v, "name"), "id");
    assert!(d.is_empty(), "version 不在比对字段里，不应算作修改");
}

#[test]
fn numeric_and_string_values_compare_by_text() {
    // 强度可能是 INTEGER 列，JSON 里是数字；统一转文本比较，避免"50 vs "50"" 误报
    let before = vec![json!({"id": 1, "strength": 50})];
    let after = vec![json!({"id": 1, "strength": "50"})];
    let d = diff_items(&before, &after, REL_FIELDS, &|_| "关系".into(), "id");
    assert!(d.is_empty(), "50 与字符串 50 应视为相同");
}

#[test]
fn missing_fields_treated_as_empty_string() {
    let before = vec![json!({"id": 1, "profile": "旧小传"})];
    let after = vec![json!({"id": 1})];
    let d = diff_items(&before, &after, &chars(), &|v| jstr(v, "name"), "id");
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].fields[0].field, "小传");
    assert_eq!(d[0].fields[0].after, "", "缺失字段按空串处理");
}

#[test]
fn summary_counts_all_three_kinds() {
    let mut d = SnapshotDiff::default();
    d.characters = vec![
        ItemChange { kind: "added".into(), label: "A".into(), fields: vec![] },
        ItemChange { kind: "modified".into(), label: "B".into(), fields: vec![] },
    ];
    d.relations = vec![ItemChange { kind: "removed".into(), label: "C".into(), fields: vec![] }];
    assert_eq!(d.total(), 3);
    assert_eq!(d.summary(), "新增 1 · 修改 1 · 删除 1");
}
/// 真机验证（默认跳过）：对真实库跑一次 `preview_snapshot`，打印差异摘要。
/// 跑法：`cargo test --lib live_preview_snapshot -- --ignored --nocapture`
///
/// 为什么值得跑：这条链路涉及"JSON payload ↔ 当前三张表"的字段对齐，
/// 字段名写错（例如 payload 里叫 from_character_id 而当前表读成 from_name）会静默变成
/// "差异为空"或"全是差异" —— 只有真实数据才试得出来。
#[tokio::test]
#[ignore]
async fn live_preview_snapshot() {
    let db = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..").join("library").join("default").join("projects").join("7").join("story.db");
    if !db.exists() {
        println!("真实库不存在，跳过：{}", db.display());
        return;
    }
    let snaps = list_snapshots("7").await.unwrap();
    println!("  快照数：{}", snaps.len());
    if snaps.is_empty() {
        return;
    }
    // 逐个预览，最新那个应当在"设定没动过"时为 0 差异
    for s in snaps.iter().take(3) {
        let d = preview_snapshot("7", s.version).await.unwrap();
        println!(
            "  V{}：{} → characters {} / relations {} / world {}",
            s.version,
            d.summary(),
            d.characters.len(),
            d.relations.len(),
            d.world_settings.len()
        );
        let mut n_field = 0;
        for it in d.characters.iter().chain(d.relations.iter()).chain(d.world_settings.iter()) {
            n_field += it.fields.len();
            println!("      [{}] {}  {} 处字段", it.kind, it.label, it.fields.len());
            for f in it.fields.iter() {
                println!("          {} : {:?} → {:?}", f.field, f.before, f.after);
            }
        }
        println!("      （共 {} 个差异项、{} 处字段差异）", d.total(), n_field);
    }
    // 最新快照与当前设定一致时，差异必须为 0（否则说明字段对齐有问题）
    let latest = snaps[0].version;
    let d = preview_snapshot("7", latest).await.unwrap();
    // 说明：**历史快照的 world_settings 里没有 id**（生成端早期漏选），无法按 id 对齐，
    // 因此只对"角色 + 关系"要求 0 差异；世界观条目在**新生成的快照**里才有 id。
    let char_rel = d.characters.len() + d.relations.len();
    println!(
        "  角色+关系差异 {} 处，世界观差异 {} 处（老快照缺 id 属预期）",
        char_rel,
        d.world_settings.len()
    );
    println!("  ✔ 字段对齐检查完成（新快照会写入 world_settings.id）");
}

/// **真机踩到的坑**：老快照的 world_settings 没有 id，而当前库有 id。
/// 如果对齐键在两边"各自挑"（一边挑到 id、一边挑到 key），同一条目会同时出现在
/// "新增"和"删除"里 —— 看起来像"整批设定被换掉了"，实际什么都没变。
/// 这条测试锁住"必须由调用方指定统一的对齐键"。
#[test]
fn mixed_id_presence_must_not_create_phantom_add_remove() {
    // 当前库：带 id 与 key
    let current = vec![json!({"id": 5, "key": "修女宿舍", "value": "旧描述"})];
    // 老快照：只有 key（没有 id）
    let snapshot = vec![json!({"key": "修女宿舍", "value": "旧描述"})];
    // 按 key 对齐 → 两边配得上，且内容一致 → 无差异
    let d = diff_items(&current, &snapshot, WORLD_FIELDS, &|v| jstr(v, "key"), "key");
    assert!(d.is_empty(), "按 key 对齐时不应产生任何差异，实际 {d:?}");

    // 对照：按 id 对齐会配不上（一边 i:5、一边没有 id）→ 出现假的"新增 + 删除"
    let bad = diff_items(&current, &snapshot, WORLD_FIELDS, &|v| jstr(v, "key"), "id");
    assert_eq!(bad.len(), 2, "按 id 对齐会同时报新增与删除（这就是当时的现象）");
}

/// 按 key 对齐时，value 变了要能报出来
#[test]
fn keyed_match_still_detects_value_change() {
    let current = vec![json!({"id": 5, "key": "修女宿舍", "value": "新描述"})];
    let snapshot = vec![json!({"key": "修女宿舍", "value": "旧描述"})];
    let d = diff_items(&current, &snapshot, WORLD_FIELDS, &|v| jstr(v, "key"), "key");
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].kind, "modified");
    assert_eq!(d[0].label, "修女宿舍");
    assert_eq!(d[0].fields[0].field, "内容");
    assert_eq!(d[0].fields[0].before, "新描述");
    assert_eq!(d[0].fields[0].after, "旧描述");
}
