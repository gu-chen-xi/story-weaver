// 三段固定区（基础指令 + 输出排版 / 数据字典 / 情绪表演）的契约测试。
//
// 为什么单独锁：这三段是**用户数据波及不到、100% 吃缓存**的部分，
// 改动只发生在开发者侧 —— 也就是说，**没人会靠"用着不对"发现它们退化**。
// 只能靠测试盯着。2026-10-22 这两处都出过真问题：
//   ① 「状态」一词三义（关系态势 / 角色的当下 / 输出姿态）互相污染；
//   ② 心声被写死成「必须与台词相反」→ 所有角色都被写成傲娇，且反向助攻"该拒没拒"。

use super::OUTPUT_FORMAT_RULES;

/// 心声必须是「与台词的关系之一」，而不是只有「相反」一种。
#[test]
fn inner_voice_offers_multiple_relations_not_only_opposite() {
    let s = OUTPUT_FORMAT_RULES;
    // 六种关系都要在（这是「活人感」的来源）
    for kind in ["反向", "旁支", "自欺", "掐断", "钝", "算计"] {
        assert!(s.contains(kind), "心声缺少关系类型「{kind}」：{s}");
    }
    // 旧的死板写法不能再回来
    assert!(
        !s.contains("必须写出与台词相反或矛盾的真实想法"),
        "心声又回到了「必须相反」的老写法（会把所有角色写成傲娇）"
    );
    assert!(!s.contains("傲娇型"), "不该再有「傲娇型」的特例表述");
    // 两条保留的约束
    assert!(s.contains("同向"), "缺少「疏离/拒绝时心声必须与台词同向」的边界");
    assert!(s.contains("忌**：写成对剧情的总结"), "缺少「不得写成总结陈词」的禁令");
}

/// 术语定死：关系态势 / 当下 / 情绪 三者不许混用同一个词。
#[test]
fn terminology_is_fixed_and_unambiguous() {
    let s = OUTPUT_FORMAT_RULES;
    // 输出排版只管格式，不许出现「关系态势」这类数据术语
    assert!(!s.contains("关系态势"), "输出排版不该出现数据术语「关系态势」");

    // 数据字典的术语表必须写清三者的分工
    let dict = crate::relations::strength_legend();
    assert!(dict.contains("Relationship strength"), "强度图例标题异常");
    let footing = crate::relations::state_legend();
    assert!(footing.contains("Relation footing"), "态势图例标题应改掉 state");
    assert!(
        footing.contains("BETWEEN THEM"),
        "态势必须明确是「他们之间」的，避免与角色的当下混淆：{footing}"
    );

    // 角色的「当下」与关系「态势」是两个词
    let char_line = crate::characters::stat_value_line("夏婉琴", 50, 50, 50, "动作靠近");
    assert!(char_line.contains("当下"), "角色行应用「当下」：{char_line}");
    let rel_line = crate::relations::rel_value_line("A", "B", 50, "稳定");
    assert!(rel_line.contains("关系态势"), "关系行应用「关系态势」：{rel_line}");
}

/// 情绪表演公式：一句话 + 四象限 + 两句话的反差。
#[test]
fn emotion_formula_has_veto_and_four_quadrants() {
    let s = crate::relations::coop_legend();
    // 强度有否决权这一点在配合度段里
    assert!(s.contains("默认拒绝"), "配合度段缺少「默认拒绝」的默认方向：{s}");

    // 公式本身在系统提示词里（情绪表演段由 build_system_prompt 拼装，这里直接查源码常量区）
    // 用 include_str! 读本文件所属模块的源，避免为了测试去跑整条装配链
    const SRC: &str = include_str!("../prompt.rs");
    assert!(SRC.contains("【情绪表演】"), "缺少情绪表演段");
    assert!(SRC.contains("以强度优先"), "公式必须写明「冲突时以强度优先」");
    for quad in ["强度低 · 理智低", "强度低 · 理智高", "强度高 · 理智低", "强度高 · 理智高"] {
        assert!(SRC.contains(quad), "缺少四象限之一：{quad}");
    }
    assert!(SRC.contains("崩溃不一定哭"), "缺少「往反方向演」的提示");
}
