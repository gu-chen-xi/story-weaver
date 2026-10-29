use super::*;

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
