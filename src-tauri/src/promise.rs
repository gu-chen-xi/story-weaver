//! 承诺兑现标记（2026-10-30）：`〔承诺:兑现〕` / `〔承诺:未兑现〕`。
//!
//! 为什么单独一个模块：它和 `〔状态变化〕`、`〔导演代价〕` 一样是**协议行**，不是正文；
//! 解析只认末尾协议区，存库/历史/记忆/回顾/导出都要剥掉。
//!
//! 状态语义（用户 2026-10-30 定）：**未兑现是正式状态**，不是“删掉”或“没发生”。

pub const TAG: &str = "〔承诺〕";

/// 协议行的**前缀**：两种写法共用 —— `〔承诺〕兑现`（规范写法）与 `〔承诺:未兑现〕`（模型自己变出来的写法）。
///
/// 2026-10-30 修：`strip_line` 原来只认 `TAG`（带 `〕`），于是带冒号的写法**解析得到、剥不掉** ——
/// 结算生效了，但那行字还留在正文/历史/导出里。改用前缀后与 `parse_line` 同一判据。
pub const PREFIX: &str = "〔承诺";

/// 给模型的兑现协议（**只在有「待兑现」承诺时注入**，否则零 token）。
pub fn hint() -> &'static str {
    "【承诺结算】如果你这一轮**明确兑现**了之前答应过的事（做完 / 交还 / 履行），在最后单独一行写：〔承诺〕兑现；\
     如果是**明确食言**（反悔 / 没做到 / 毁约），写：〔承诺〕未兑现。\
     没有明确发生就不要写，不要写在正文里，也不要解释。"
}

/// 解析这一轮有没有承诺结算行：`Some(true)` 兑现、`Some(false)` 未兑现、`None` 没写。
pub fn parse_line(text: &str) -> Option<bool> {
    for line in text.lines() {
        let t = line.trim();
        // 兼容两种写法：〔承诺〕兑现 / 〔承诺:未兑现〕
        let Some(rest) = t.strip_prefix(PREFIX) else { continue };
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
        .filter(|l| !l.trim().starts_with(PREFIX))
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

    /// 回归（2026-10-30）：〔承诺:未兑现〕这种带冒号的写法也必须剥掉
    #[test]
    fn strips_colon_form_too() {
        let t = "她把钥匙放回去。\n〔承诺:未兑现〕";
        assert_eq!(strip_line(t), "她把钥匙放回去。");
        assert!(!strip_line(t).contains("承诺"));
    }

    #[test]
    fn strips_only_the_protocol_line() {
        let t = "她说：\"我答应过。\"\n〔承诺〕兑现";
        assert_eq!(strip_line(t), "她说：\"我答应过。\"");
    }

}
