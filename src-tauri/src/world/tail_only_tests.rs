// 从 `world.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `world::tail_only_tests` —— 因为 `world.rs` + `world/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `world`，一行都不用改写。

use super::*;

#[test]
fn lore_is_tail_only_others_are_not() {
    assert!(is_tail_only("世界书"));
    assert!(is_tail_only("  世界书  "), "首尾空白也要认");
    for c in ["地理", "术语", "势力", "势力关系", "物品", "设定", ""] {
        assert!(!is_tail_only(c), "{c} 不该被判成只走尾部块");
    }
}
