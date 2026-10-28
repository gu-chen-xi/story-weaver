//! token 估算（2026-10-28）。
//!
//! 背景：原来前后端都按「字符数 × 1.5」粗估。2026-10-28 固定区提示词改成英文后，这个口径直接失真：
//! 同样一段话，英文的字数是中文的 ~2.5 倍，却被当成 2.5 倍的 token —— 预算被虚报，
//! 历史会被提前裁掉；而中文又被高估 ~50%（1000 字算成 1500 token）。
//!
//! 现在按**字符类别**加权，并按真实 usage 校准过一次（2026-10-28）：
//! * 非 ASCII（汉字 / 假名 / 全角标点 / emoji）：0.79 token；
//! * ASCII 字母数字：0.20 token（≈ 5 字符 / token）；
//! * ASCII 空白：0.12；
//! * ASCII 标点：0.39。
//!
//! **校准依据**：请求已开 `stream_options.include_usage`，两轮真实记录
//! （估算 5791 / 实测 4567、6315 / 4966）都是**偏高 27%**，于是把四个系数整体乘 0.787。
//! 校准后实测 5 轮落在 **1.05~1.06** —— ±5% 是那几轮中英文比例不同造成的，不是系统性偏差，
//! 所以不再按 1.00 硬凑（继续调只会过拟合这几条样本）。参数页「📐 估算校准」留着观察：
//! 只有稳定 >1.10 或 <0.90 才值得再动系数（宁可略高，别低估）。
//!
//! **这不是真 tokenizer**，是工程近似。要彻底准只能内置真分词器（依赖 + 体积代价）。
//!
//! 前端 `src/lib/parse.ts::estimateTokens` 是同一套系数，改这里必须同步改那边。

/// 非 ASCII 字符每个算多少 token（校准后 ≈ 0.79）
const PER_WIDE: f64 = 0.79;
/// ASCII 字母/数字：≈ 5 字符一个 token（校准后 0.20）
const PER_ALNUM: f64 = 0.20;
/// 空格/换行
const PER_SPACE: f64 = 0.12;
/// ASCII 标点
const PER_PUNCT: f64 = 0.39;

/// 估算一段文本的 token 数（向上取整）。
pub fn estimate(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let mut acc = 0.0f64;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            acc += PER_ALNUM;
        } else if ch.is_ascii_whitespace() {
            acc += PER_SPACE;
        } else if ch.is_ascii() {
            acc += PER_PUNCT;
        } else {
            acc += PER_WIDE;
        }
    }
    acc.ceil().max(1.0) as usize
}

#[cfg(test)]
mod tests {
    use super::estimate;

    #[test]
    fn empty_is_zero() {
        assert_eq!(estimate(""), 0);
    }

    #[test]
    fn chinese_is_about_point_eight_tokens_per_char() {
        assert_eq!(estimate("你好"), 2);   // 1.58 → 2
        assert_eq!(estimate("你好呀"), 3); // 2.37 → 3
        // 全角标点也算进 wide 档（0.79）：3 × 0.79 = 2.37 → 3
        assert_eq!(estimate("你好，"), 3);
    }

    #[test]
    fn english_is_about_four_chars_per_token() {
        // 4 个字母 = 1
        assert_eq!(estimate("abcd"), 1);
        // 一句话：字母/空格/标点分别计权
        let s = "You are a role-play engine.";
        let n = estimate(s);
        assert!((6..=9).contains(&n), "一句英文应在 6~9 token，实际 {n}");
    }

    #[test]
    fn english_is_much_cheaper_than_the_old_char_times_one_point_five() {
        // 同一段英文：旧口径（字数×1.5）会把它算成 3 倍以上
        let en = "Continue the story in natural literary prose, and always weave in the characters' actions.";
        let old = (en.chars().count() as f64 * 1.5).ceil() as usize;
        assert!(estimate(en) * 3 < old, "英文不该再被字数×1.5 高估：new {} old {old}", estimate(en));
    }

    #[test]
    fn cjk_is_not_overestimated_by_fifty_percent() {
        let zh = "她站在门口没有动，手里还攥着那串钥匙。";
        let n = estimate(zh);
        let chars = zh.chars().count();
        // 校准后 ≈ 0.79 token/字（允许 ±1 的取整误差）
        assert!(
            (n as f64 - chars as f64 * 0.79).abs() <= 1.0,
            "汉字应约 0.79 token/字：{chars} 字 → {n}"
        );
    }
}
