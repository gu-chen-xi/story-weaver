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

/// 2026-11-01 实测补漏：接受判定必须**并上协议标记**。
///
/// 现网数据（第 13 天）：表白词在用户那句「我喜欢你」里，AI 只回「……好。」+ 〔态度:接受〕，
/// 词表一个都没命中 → "表白+接受"不成立 → 记忆卡改判了、关系跃迁提案却没生成。
#[test]
fn acceptance_counts_the_protocol_marker() {
    // 词表没命中、只有标记 → 这一轮算"接受"
    assert!(super::acceptance_in_round(false, true));
    // 词表命中（模型没用标记的老路径）→ 也算
    assert!(super::acceptance_in_round(true, false));
    // 都没有 → 不算
    assert!(!super::acceptance_in_round(false, false));
}

#[test]
fn marker_acceptance_plus_confession_word_triggers_the_jump() {
    // 就是第 13 天那轮的形状：用户说了"我喜欢你"（表白）、回了个〔态度:接受〕
    let accepted = super::acceptance_in_round(false, true);
    assert!(super::relation_jump_triggered(true, accepted, false));
}

/// 2026-11-01 实测补漏：**单关系故事**不要求双方名字都出现。
/// 他那轮只喊了「夏婉琴」、没提「顾宸熙」，提案就整条丢了。
#[test]
fn single_relation_does_not_need_both_names() {
    assert!(super::relation_is_in_scene(false, 1), "只有一条关系时，名字没提全也该算在场");
    assert!(super::relation_is_in_scene(true, 1));
}

#[test]
fn multiple_relations_still_need_both_names() {
    assert!(!super::relation_is_in_scene(false, 2), "多关系时必须靠名字确认归属");
    assert!(super::relation_is_in_scene(true, 2));
}
