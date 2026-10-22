// 从 `relations.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `relations::tests` —— 因为 `relations.rs` + `relations/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `relations`，一行都不用改写。

use super::{assert_strength_bands, coop_legend, rel_value_line, split_for_relation, state_legend, STRENGTH_BANDS};

#[test]
fn director_tag_parse_and_strip() {
    let content = "〔导演指令〕强制让夏婉琴接受告白\n\n（宸熙攥紧了拳头）我不管，今天你必须给我答案。";
    assert_eq!(super::parse_director(content).as_deref(), Some("强制让夏婉琴接受告白"));
    assert_eq!(super::strip_director(content), "（宸熙攥紧了拳头）我不管，今天你必须给我答案。");
    let only = "〔导演指令〕让雨下得更大";
    assert_eq!(super::strip_director(only), "");
    assert_eq!(super::parse_director(only).as_deref(), Some("让雨下得更大"));
    let multi = "〔导演指令〕先让院长暴怒\n再让夏婉琴摔门\n\n她说不出话。";
    assert_eq!(super::parse_director(multi).as_deref(), Some("先让院长暴怒\n再让夏婉琴摔门"));
    assert_eq!(super::strip_director(multi), "她说不出话。");
    assert_eq!(super::parse_director("（他笑了）你来了。"), None);
    assert_eq!(super::strip_director("（他笑了）你来了。"), "（他笑了）你来了。");
    let dl = super::director_legend();
    assert!(!dl.contains('【') && !dl.contains('】'));
    // 不能自带标题：注入处已有【导演指令】前缀，重复会出现"导演指令：导演指令（…）"
    assert!(!dl.starts_with("导演指令"), "说明文案不应自带「导演指令」标题");
}

#[test]
fn strength_band_boundaries() {
    // 左闭右开 + 100 单独归顶档
    assert_strength_bands();
}

#[test]
fn legend_has_no_section_marker() {
    // 图例正文不能含【】：提示词预览按【】切段，含了会被误拆成独立段
    let legend = super::strength_legend();
    assert!(!legend.contains('【'), "图例不应包含【标记");
    assert!(!legend.contains('】'), "图例不应包含】标记");
    assert!(legend.contains("0-20 疏离") && legend.contains("80-100 羁绊"));
    let sl = super::state_legend();
    assert!(!sl.contains('【') && !sl.contains('】'), "状态图例不应包含【】标记");
    // v3.4 英文化试点后：脚手架是英文，**中文枚举值与含义必须原样保留**
    assert!(
        sl.contains("Relation footing") && sl.contains("稳定 — 冲突止于拌嘴")
            && sl.contains("破裂 — 出现回避、断绝、伤害性言行"),
        "状态图例要英文脚手架 + 中文枚举：{sl}"
    );
    let dl = super::direction_legend();
    assert!(!dl.contains('【') && !dl.contains('】'), "方向读法不应包含【】标记");
    assert!(dl.contains("单向：") && dl.contains("双向：") && dl.contains("各读各的"));
}

#[test]
fn relation_state_meaning_fallback() {
    assert_eq!(super::relation_state_meaning("稳定"), "冲突止于拌嘴");
    assert_eq!(super::relation_state_meaning("紧张"), "容易升级为争执");
    assert_eq!(super::relation_state_meaning("破裂"), "出现回避、断绝、伤害性言行");
    // 老数据的自由文本（动摇 / 待确认 / 空）统一按稳定
    assert_eq!(super::relation_state_meaning("动摇"), "冲突止于拌嘴");
    assert_eq!(super::relation_state_meaning(""), "冲突止于拌嘴");
}

#[test]
fn relation_rows_split_into_two_kinds() {
    // 「方向和关系」行：只有方向 + 说明，不含强度数值与状态
    let d = super::rel_direction_line("宸熙 是 夏婉琴 的「明恋」", "单向", "两人高中同学");
    assert!(d.contains("方向：单向"), "缺少方向：{d}");
    assert!(d.contains("说明：两人高中同学"), "缺少说明：{d}");
    assert!(!d.contains("强度"), "方向行不应出现强度：{d}");
    assert!(!d.contains("83"), "方向行不应出现数值：{d}");
    // 「数值与状态」行：强度 + 档位 + 状态 + 含义
    let v = super::rel_value_line("宸熙", "夏婉琴", 83, "稳定");
    assert!(v.contains("强度 83（羁绊）"), "缺少强度档位：{v}");
    assert!(v.contains("关系态势：稳定（冲突止于拌嘴）"), "缺少态势含义：{v}");
    // 状态含义只随数值行出现，方向行不得带状态
    assert!(!d.contains("状态"), "方向行不应出现状态：{d}");
}

#[test]
fn legends_do_not_repeat_titles() {
    // 各图例都不带【】标题、也不重复「关系强度档位 / 关系状态」之外的标题，
    // 保证注入后每个标题只出现一次（数据字典只放一次）
    let s = super::strength_legend();
    let st = super::state_legend();
    // 标题改为英文（英文化试点），但档位名与状态名仍是中文
    assert!(s.starts_with("Relationship strength"), "强度图例标题异常：{s}");
    assert!(st.starts_with("Relation footing"), "态势图例标题异常：{st}");
    assert!(s.contains("疏离") && s.contains("羁绊"), "档位名应保留中文：{s}");
    for text in [&s, &st] {
        assert!(!text.contains('【') && !text.contains('】'), "图例不得含【】：{text}");
    }
    // 合成数据字典后，标题各自只出现一次
    let dict = format!("{}\n{}", s, st);
    assert_eq!(dict.matches("Relationship strength").count(), 1, "强度标题重复");
    assert_eq!(dict.matches("Relation footing").count(), 1, "态势标题重复");
}

#[test]
fn split_kinship_by_gender() {
    let s = split_for_relation("双向", "院长", "夏婉琴", "男", "女", "", "", "养父女");
    assert_eq!(s.len(), 2);
    assert!(s[0].text.contains("院长 是 夏婉琴 的 父"));
    assert!(s[1].text.contains("夏婉琴 是 院长 的 女"));
}

#[test]
fn split_same_gender_by_age() {
    let s = split_for_relation("双向", "甲", "乙", "女", "女", "45", "20", "母女");
    assert_eq!(s.len(), 2);
    assert!(s[0].text.contains("甲 是 乙 的 母"));
    assert!(s[1].text.contains("乙 是 甲 的 女"));
}

#[test]
fn split_fallback_from_when_unknown() {
    let s = split_for_relation("双向", "甲", "乙", "", "", "", "", "姐弟");
    assert_eq!(s.len(), 2);
    assert!(s[0].text.contains("甲 是 乙 的 姐"));
}

#[test]
fn split_generic_bilateral_and_stranger() {
    let g = split_for_relation("双向", "甲", "乙", "", "", "", "", "同学");
    assert_eq!(g.len(), 2);
    assert!(g[0].text.contains("甲 是 乙 的「同学」"));
    let s = split_for_relation("双向", "甲", "乙", "", "", "", "", "陌生人");
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].text, "甲 和 乙 是陌生人");
}

#[test]
fn single_direction_is_not_split() {
    assert!(split_for_relation("单向", "甲", "乙", "男", "女", "", "", "父女").is_empty());
}

// ===== v3.4：配合度段与强度档位必须同源 =====

/// **反漂移锁**：配合度段的每个档位名/边界都必须来自 STRENGTH_BANDS。
///
/// 为什么需要：配合度段曾经在提示词里**手写**了一份档位（写成「客气/熟识」、
/// 边界 0-19/20-39），而真相是「相识/亲近/交心」、0-20/20-40。
/// 两套映射并存时模型会自己做隐式映射，映射错了就出现"疏离档却放行"（用户实测）。
/// 现在由常量生成，这条测试守住"再有人手写一份"。
#[test]
fn coop_legend_is_generated_from_bands() {
    let s = coop_legend();
    for (name, lo, hi, _d, _a, coop) in STRENGTH_BANDS.iter() {
        assert!(
            s.contains(&format!("{}-{} {}", lo, hi, name)),
            "配合度段缺少档位 {lo}-{hi} {name}：{s}"
        );
        assert!(s.contains(coop), "配合度段缺少 {name} 的配合说明：{s}");
    }
    for wrong in ["客气", "熟识"] {
        assert!(!s.contains(wrong), "配合度段出现了非官方档位名「{wrong}」：{s}");
    }
    for must in ["默认拒绝", "必须**明确拒绝**", "拒绝之后就停住", "低理智 ≠ 好说话", "输出前自检"] {
        assert!(s.contains(must), "配合度段缺少约束「{must}」：{s}");
    }
}

/// 关系"态势"与角色"当下"必须用不同的词，避免同一个「状态」三义互相污染

#[test]
fn state_and_present_moment_use_distinct_words() {
    let footing = state_legend();
    assert!(footing.contains("Relation footing"), "关系段的英文标题应改掉 state：{footing}");
    let line = rel_value_line("A", "B", 83, "稳定");
    assert!(line.contains("关系态势"), "关系值行应用「关系态势」而不是「状态」：{line}");
    assert!(!line.contains("状态"), "关系值行不该再出现「状态」：{line}");

    let stat = crate::characters::stat_value_line("夏婉琴", 30, 55, 25, "刚被拒绝");
    assert!(stat.contains("当下"), "角色属性行应用「当下」：{stat}");
    assert!(!stat.contains("状态"), "角色属性行不该再出现「状态」：{stat}");
}
