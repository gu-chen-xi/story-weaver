// `memory_round` 的测试（2026-10-26）。
//
// 关键记忆的摘录必须是「用户那条消息 + AI 的回复」，不能把上一轮的半句话带进来。

use super::*;

fn m(role: &str, text: &str) -> (String, String) {
    (role.to_string(), text.to_string())
}

#[test]
fn takes_last_user_message_and_its_reply() {
    let msgs = vec![
        m("user", "前面那句"),
        m("assistant", "（杯子搁回桌上）上一轮的回复"),
        m("user", "我爱你"),
        m("assistant", "（她没接，往后退了半步）「别。」"),
    ];
    let round = memory_round(&msgs);
    assert!(round.starts_with("[用户] 我爱你"), "应从最后一条用户消息开始：{round}");
    assert!(round.contains("[AI] （她没接"), "要带上 AI 的回复：{round}");
    assert!(!round.contains("上一轮"), "上一轮内容不许进来：{round}");
    assert!(!round.contains("前面那句"), "更早的也不许：{round}");
}

#[test]
fn includes_every_reply_after_the_last_user_message() {
    let msgs = vec![m("user", "你还在吗"), m("assistant", "嗯"), m("assistant", "（补了一句）在。")];
    let round = memory_round(&msgs);
    assert!(round.contains("[用户] 你还在吗"));
    assert_eq!(round.matches("[AI]").count(), 2, "两条 AI 回复都要在：{round}");
}


// ===== 2026-10-27：演进分析也按「轮」取文本 =====

/// `last_round` 只给「最后一条用户消息 + 它之后的回复」——
/// 上一轮的「我爱你」不许进来（否则会和本轮的「我也…」配成「表白+接受」）。
#[test]
fn last_round_excludes_previous_rounds() {
    let msgs = vec![
        m("user", "我爱你"),
        m("assistant", "[夏婉琴]「你认错人了。」"),
        m("user", "〔导演指令〕她同意了\n\n我可以亲你嘛？"),
        m("assistant", "[夏婉琴]「就一下。」"),
    ];
    let round = last_round(&msgs);
    assert_eq!(round.len(), 2, "只该剩最后一条用户消息 + 回复：{round:?}");
    assert!(round[0].1.contains("我可以亲你嘛"), "从最后一条用户消息起：{round:?}");
    assert!(!round.iter().any(|(_, c)| c.contains("我爱你")), "上一轮的告白不许进来：{round:?}");
}

/// 这一轮带导演指令 → 走「只记事件、不提议」的分支
#[test]
fn director_round_is_detected() {
    let with = vec![m("user", "〔导演指令〕强制让她答应\n\n可以吗？"), m("assistant", "嗯。")];
    assert!(round_has_director(&with));
    let without = vec![m("user", "可以吗？"), m("assistant", "嗯。")];
    assert!(!round_has_director(&without), "没带标记就不是导演轮次");
}
#[test]
fn without_user_message_falls_back_to_the_last_one() {
    let msgs = vec![m("assistant", "只有旁白"), m("assistant", "最后一句")];
    let round = memory_round(&msgs);
    assert!(round.contains("最后一句"));
    assert!(!round.contains("只有旁白"), "只取最后一条：{round}");
}

#[test]
fn empty_input_gives_empty_round() {
    assert_eq!(memory_round(&[]), "");
}

#[test]
fn long_message_is_clipped_with_ellipsis() {
    let long = "啊".repeat(500);
    let msgs = vec![m("user", &long), m("assistant", "嗯")];
    let round = memory_round(&msgs);
    assert!(round.contains("…"), "超长要截断并加省略号");
    assert!(round.chars().count() < 500 + 40, "截断后不该还是 500 字：{}", round.chars().count());
}

// ===== 表态标记的触发条件（2026-10-26：只有表白/承诺才要求模型输出标记）=====

#[test]
fn stance_marker_kind_covers_confession_and_promise_only() {
    assert_eq!(stance_marker_kind("我爱你，跟我在一起吧"), Some("confession"));
    assert_eq!(stance_marker_kind("我保证，我一定会回来"), Some("promise"));
    assert_eq!(stance_marker_kind("今天天气不错"), None);
    assert_eq!(stance_marker_kind(""), None);
}