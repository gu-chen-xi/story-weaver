// 从 `world.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `world::item_tests` —— 因为 `world.rs` + `world/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `world`，一行都不用改写。

use super::*;

#[test]
fn item_line_without_alias() {
    assert_eq!(
        item_line("怀表", "B 送的，走时慢三分钟", ""),
        "- 物品「怀表」：B 送的，走时慢三分钟"
    );
}

#[test]
fn item_line_with_alias_and_trim() {
    assert_eq!(
        item_line("  怀表  ", "  旧东西  ", "  旧表  "),
        "- 物品「怀表」：旧东西（别名：旧表）"
    );
}

/// 别名为纯空白时不该出现空的「（别名：）」
#[test]
fn blank_alias_is_omitted() {
    let s = item_line("钥匙", "铁门那把", "   ");
    assert!(!s.contains("别名"), "空白别名不该出现：{s}");
    assert_eq!(s, "- 物品「钥匙」：铁门那把");
}
