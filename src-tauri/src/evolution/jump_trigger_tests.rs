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
    assert!(super::relation_is_in_scene(false, 1, false), "只有一条关系时，名字没提全也该算在场");
    assert!(super::relation_is_in_scene(true, 1, false));
}

#[test]
fn multiple_relations_still_need_both_names_or_an_anchor() {
    assert!(!super::relation_is_in_scene(false, 2, false), "多关系、无锚点、没喊名字 → 不算在场");
    assert!(super::relation_is_in_scene(true, 2, false), "喊了双方名字 → 算");
    assert!(super::relation_is_in_scene(false, 2, true), "有角色锚点 → 不用喊名字也算");
}

/// 2026-11-01：角色锚点（我扮演 / AI 扮演）是**最高优先级**的证据 ——
/// 它来自用户显式选择，比"名字有没有出现"可靠。
#[test]
fn an_explicit_role_anchor_always_counts() {
    assert!(super::relation_is_in_scene(false, 0, true));
    assert!(super::relation_is_in_scene(false, 5, true));
    // 没有任何证据时仍然不认（防止"什么关系都算在场"）
    assert!(!super::relation_is_in_scene(false, 5, false));
}

/// 2026-11-02 用户实测：AI 反过来告白时，不许记成"用户第 1 次表白"。
#[test]
fn ai_confession_alone_not_recorded_as_user_attempt() {
    let ev = vec![super::Event {
        event_type: "confession".into(),
        matched: "我喜欢你".into(),
        confidence: 0.95,
    }];
    assert!(super::memory_events(&ev, false).is_empty(), "用户没表白 → 不建卡");
    assert_eq!(super::memory_events(&ev, true).len(), 1, "用户表白了才建卡");
    // 其它三类不受这条限制（谁做的都该记）
    let other = vec![super::Event {
        event_type: "promise".into(),
        matched: "我保证".into(),
        confidence: 0.88,
    }];
    assert_eq!(super::memory_events(&other, false).len(), 1, "承诺照记");
}