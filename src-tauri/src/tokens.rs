//! token 估算（2026-10-28）。
//!
//! 背景：原来前后端都按「字符数 × 1.5」粗估。2026-10-28 固定区提示词改成英文后，这个口径直接失真：
//! 同样一段话，英文的字数是中文的 ~2.5 倍，却被当成 2.5 倍的 token —— 预算被虚报，
//! 历史会被提前裁掉；而中文又被高估 ~50%（1000 字算成 1500 token）。
//!
//! 现在按**字符类别**加权：
//! * 非 ASCII（汉字 / 假名 / 全角标点 / emoji）：1.0 token —— 取偏保守的上界（常见区间 0.6~1.0）；
//! * ASCII 字母数字：0.25 token（≈ 4 字符 / token）；
//! * ASCII 空白：0.15；
//! * ASCII 标点：0.5。
//!
//! **这不是真 tokenizer**，是工程近似：宁可略高估（预算浪费一点），也不要低估（上下文溢出直接报错）。
//! 要更准，只能拿 API 返回的 `usage.prompt_tokens` 做回归校准 —— 见 ROADMAP 的欠账。
//!
//! 前端 `src/lib/parse.ts::estimateTokens` 是同一套系数，改这里必须同步改那边。

/// 非 ASCII 字符每个算多少 token（保守取 1.0）
const PER_WIDE: f64 = 1.0;
/// ASCII 字母/数字：约 4 字符一个 token
const PER_ALNUM: f64 = 0.25;
/// 空格/换行
const PER_SPACE: f64 = 0.15;
/// ASCII 标点：多半自己成 token 或与相邻字符合并，取半
const PER_PUNCT: f64 = 0.5;

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
    fn chinese_is_one_token_per_char() {
        assert_eq!(estimate("你好"), 2);
        assert_eq!(estimate("你好呀"), 3);
        // 全角标点也算 1（它通常自己成 token）
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
        assert_eq!(n, chars, "汉字按 1 token/字：{chars} 字 → {n}");
    }
}
