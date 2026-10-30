//! 承诺兑现标记（2026-10-30）：`〔承诺:兑现〕` / `〔承诺:未兑现〕`。
//!
//! 为什么单独一个模块：它和 `〔状态变化〕`、`〔导演代价〕` 一样是**协议行**，不是正文；
//! 解析只认末尾协议区，存库/历史/记忆/回顾/导出都要剥掉。
//!
//! 状态语义（用户 2026-10-30 定）：**未兑现是正式状态**，不是“删掉”或“没发生”。

pub const TAG: &str = "〔承诺〕";

/// 解析这一轮有没有承诺结算行：`Some(true)` 兑现、`Some(false)` 未兑现、`None` 没写。
pub fn parse_line(text: &str) -> Option<bool> {
    for line in text.lines() {
        let t = line.trim();
        // 兼容两种写法：〔承诺〕兑现 / 〔承诺:未兑现〕
        let Some(rest) = t.strip_prefix("〔承诺") else { continue };
        let v = rest.trim_start_matches([':', '：', '〕']).trim();
        if v.starts_with("兑现") {
            return Some(true);
        }
        if v.starts_with("未兑现") || v.starts_with("没兑现") || v.starts_with("食言") {
            return Some(false);
        }
    }
    None
}

/// 把协议行从正文里剥掉（只剥整行，行内其它内容不动）。
pub fn strip_line(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim().starts_with(TAG))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_states() {
        assert_eq!(parse_line("x\n〔承诺〕兑现"), Some(true));
        assert_eq!(parse_line("x\n〔承诺:未兑现〕"), Some(false));
        assert_eq!(parse_line("x\n〔承诺〕食言"), Some(false));
        assert_eq!(parse_line("没有这一行"), None);
    }

    #[test]
    fn strips_only_the_protocol_line() {
        let t = "她说：\"我答应过。\"\n〔承诺〕兑现";
        assert_eq!(strip_line(t), "她说：\"我答应过。\"");
    }
}
