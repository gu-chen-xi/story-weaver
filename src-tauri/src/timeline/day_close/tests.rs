use super::*;

/// 时段标记存在 messages.meta：只认三个值，坏数据当"没标"
#[test]
fn parse_tod_accepts_only_three_values() {
    assert_eq!(crate::timeline::parse_tod("{\"tod\":\"午\"}"), "午");
    assert_eq!(crate::timeline::parse_tod("{\"tod\":\"深夜\"}"), "");
    assert_eq!(crate::timeline::parse_tod("not json"), "");
    assert_eq!(crate::timeline::parse_tod("{}"), "");
}

#[test]
fn parses_only_the_two_sleep_kinds() {
    let reply = "这天她一直在写东西。\n〔作息〕夏婉琴：睡好\n〔作息〕顾宸熙：熬夜\n〔作息〕某路人：没睡好";
    let got = parse_sleep_lines(reply);
    assert_eq!(got.len(), 2, "第三种不在三态里，忽略：{got:?}");
    assert_eq!(got[0], ("夏婉琴".to_string(), "睡好".to_string()));
    assert_eq!(got[1], ("顾宸熙".to_string(), "熬夜".to_string()));
}

#[test]
fn strips_sleep_lines_from_summary() {
    let reply = "她写完就睡了。\n〔作息〕夏婉琴：睡好";
    assert_eq!(strip_sleep_lines(reply), "她写完就睡了。");
}

#[test]
fn prompt_tells_model_to_skip_ordinary_nights_and_numbers() {
    assert!(DAY_CLOSE_PROMPT.contains("不要写数字"), "提示词不许让模型报数");
    assert!(DAY_CLOSE_PROMPT.contains("不写 = 不变"), "普通的一夜必须留空");
    assert!(DAY_CLOSE_PROMPT.contains("〔作息〕"));
}

// ===== 2026-11-02：普通的一夜 +10 体力（用户定：恢复端要接上）=====

/// 三态数值：睡好 +15/+4、熬夜 −10、**普通一夜 +10（代码默认，模型不写）**
#[test]
fn normal_night_gives_ten_stamina() {
    let plan = crate::stat_change::plan_kind("普通一夜", "日结");
    assert_eq!(plan.len(), 1, "{plan:?}");
    assert_eq!(plan[0].dim, "stamina");
    assert_eq!(plan[0].delta, 10);
    // 理智不给（用户 2026-11-02 定）
    assert!(!plan.iter().any(|p| p.dim == "sanity"), "普通一夜不加理智");
    // 睡好仍然是更值的那一档
    let good = crate::stat_change::plan_kind("睡好", "日结");
    let good_sta = good.iter().find(|p| p.dim == "stamina").map(|p| p.delta).unwrap_or(0);
    assert!(good_sta > 10, "睡好要明显好过普通一夜：{good:?}");
}