// 关系跃迁触发条件的单测（2026-11-01）。
//
// 为什么要单独测这四行：跃迁会把关系数值直接推到 85+，误触发一次就很难回滚；
// 而"要不要触发"完全由三个布尔决定，是最便宜也最该锁住的地方。

use super::relation_jump_triggered;

#[test]
fn same_round_confession_and_acceptance_jumps() {
    assert!(relation_jump_triggered(true, true, false));
}

#[test]
fn flip_jumps_even_without_a_confession_event() {
    // 被拒之后她松口：这一轮没有表白事件，也不是词表意义上的"接受"
    assert!(relation_jump_triggered(false, false, true));
    assert!(relation_jump_triggered(false, true, true));
    assert!(relation_jump_triggered(true, true, true));
}

#[test]
fn bare_acceptance_alone_does_not_jump() {
    // 「我答应你明天早点回来」这种日常承诺不许触发跃迁
    assert!(!relation_jump_triggered(false, true, false));
    assert!(!relation_jump_triggered(false, false, false));
}

#[test]
fn confession_without_acceptance_does_not_jump() {
    assert!(!relation_jump_triggered(true, false, false));
}
