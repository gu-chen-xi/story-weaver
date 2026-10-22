// 从 `world.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `world::lore_tag_tests` —— 因为 `world.rs` + `world/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `world`，一行都不用改写。

use super::*;

#[test]
fn trigger_hit_is_shown_as_reason() {
    assert_eq!(
        lore_line_tagged("怀表的来历", "B 母亲留下的", "怀表"),
        "- 「怀表的来历」（命中：怀表）：B 母亲留下的"
    );
}

/// 常驻：显示「（常驻）」而不是「（命中：（常驻））」
#[test]
fn always_on_uses_its_own_tag() {
    assert_eq!(
        lore_line_tagged("世界常识", "海水是咸的", "（常驻）"),
        "- 「世界常识」（常驻）：海水是咸的"
    );
}

#[test]
fn trims_whitespace() {
    assert_eq!(
        lore_line_tagged("  A  ", "  B  ", " t "),
        "- 「A」（命中： t ）：B"
    );
}
