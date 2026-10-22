// 从 `evolution.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `evolution::third_batch_tests` —— 因为 `evolution.rs` + `evolution/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `evolution`，一行都不用改写。

use super::*;

#[test]
fn new_signals_are_extracted() {
    let ev = extract_events("[夏婉琴]“我保证，我一定会回来。”");
    assert!(ev.iter().any(|e| e.event_type == "promise"), "应识别承诺：{ev:?}");
    let ev2 = extract_events("（他挡在她身前，替她挨了那一拳）");
    assert!(ev2.iter().any(|e| e.event_type == "sacrifice"), "应识别守护：{ev2:?}");
    let ev3 = extract_events("[院长]“你太让我失望了。”他第一次说出很难听的话。");
    assert!(ev3.iter().any(|e| e.event_type == "betrayal"), "应识别伤害：{ev3:?}");
    // 普通叙述不应误触发新信号
    let quiet = extract_events("（她安静地坐在窗边看书，阳光落在书页上）");
    assert!(
        !quiet.iter().any(|e| matches!(e.event_type.as_str(), "promise" | "sacrifice" | "betrayal")),
        "普通叙述不应触发：{quiet:?}"
    );
}

#[test]
fn event_labels_are_chinese() {
    for k in ["confession", "acceptance", "rejection", "care", "conflict", "promise", "sacrifice", "betrayal"] {
        let lb = event_label(k);
        assert!(!lb.is_empty() && lb != "其它事件", "{k} 应有中文名");
    }
    assert_eq!(event_label("promise"), "承诺");
    assert_eq!(event_label("unknown_thing"), "其它事件");
}
