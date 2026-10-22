// 从 `world.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `world::lore_tests` —— 因为 `world.rs` + `world/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `world`，一行都不用改写。

use super::*;

#[test]
fn split_triggers_handles_all_separators() {
    assert_eq!(split_triggers("怀表,旧表，那块表、它;吃药；雨夜"),
               vec!["怀表", "旧表", "那块表", "它", "吃药", "雨夜"]);
    assert!(split_triggers("  ,  ， ").is_empty());
}

#[test]
fn lore_hit_is_case_insensitive_substring() {
    assert_eq!(lore_hit("RTX,显卡", false, "我把显卡拿了出来").as_deref(), Some("显卡"));
    assert_eq!(lore_hit("rtx", false, "那块 RTX 到了").as_deref(), Some("rtx"));
    assert_eq!(lore_hit("怀表", false, "今天天气不错"), None);
}

#[test]
fn lore_hit_always_on_wins() {
    assert_eq!(lore_hit("", true, "").as_deref(), Some("（常驻）"));
    // 常驻即使触发词写错也照样进
    assert!(lore_hit("不存在的词", true, "随便一句").is_some());
}

#[test]
fn lore_block_skips_when_empty_and_marks_skipped() {
    assert!(lore_block(&[], 0).is_none());
    let b = lore_block(&[lore_line("怀表", "B 送的")], 0).expect("应有块");
    assert!(b.contains("【世界书】"), "{b}");
    assert!(b.contains("- 「怀表」：B 送的"), "{b}");
    assert!(!b.contains("未注入"), "没被挤掉就不该出现这句：{b}");

    let b2 = lore_block(&[lore_line("怀表", "B 送的")], 3).expect("应有块");
    assert!(b2.contains("另有 3 条命中"), "被挤掉必须注明：{b2}");
}

#[test]
fn lore_line_trims() {
    assert_eq!(lore_line("  怀表 ", "  B 送的  "), "- 「怀表」：B 送的");
}
