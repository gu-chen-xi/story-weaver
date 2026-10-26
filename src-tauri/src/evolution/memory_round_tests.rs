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
