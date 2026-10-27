// 从 `generate_stream.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `generate_stream::budget_tests` —— 因为 `generate_stream.rs` + `generate_stream/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `generate_stream`，一行都不用改写。

// ===== v3.4 世界书：接线（命中行必须真的进尾部块）=====
use super::build_tail_block;

/// 世界书的行要出现在尾部块里，且排在【示例对话】之前。
///
/// 为什么测这个而不是只测 `lore_hit`：纯函数错了容易发现，
/// 真正的坑是"算出来了却没拼进去"（示例对话就出过这种问题）。
#[test]
fn lore_lines_reach_tail_block() {
    let lore = vec![crate::world::lore_line("怀表的来历", "B 母亲留下的，走时慢三分钟")];
    let block = build_tail_block(
        "chat", "", "", "", "", "简单", "day", 0, None,
        &[], &[], "", "", &lore, 0,
    );
    assert!(block.contains("【世界书】"), "尾部块里应有世界书段：{block}");
    assert!(block.contains("- 「怀表的来历」：B 母亲留下的"), "{block}");
    // 位置：在【示例对话】之前（先补世界，再教语气）
    if let (Some(a), Some(b)) = (block.find("【世界书】"), block.find("【示例对话】")) {
        assert!(a < b, "世界书应排在示例对话之前");
    }
}

/// 没有命中 → 整段不出现（老故事行为零变化）
#[test]
fn no_lore_hits_means_no_block() {
    let block = build_tail_block(
        "chat", "", "", "", "", "简单", "day", 0, None,
        &[], &[], "", "", &[], 0,
    );
    assert!(!block.contains("【世界书】"), "没命中不该出现该段：{block}");
}

/// 被预算挤掉时，块里要注明条数（否则用户以为设定没生效）
#[test]
fn lore_skipped_is_reported_in_tail() {
    let lore = vec![crate::world::lore_line("A", "内容")];
    let block = build_tail_block(
        "chat", "", "", "", "", "简单", "day", 0, None,
        &[], &[], "", "", &lore, 2,
    );
    assert!(block.contains("另有 2 条命中"), "应注明被挤掉的条数：{block}");
}

/// 导演代价（2026-10-27）：**只在用了导演指令的那一轮**才出现，且排在【导演指令】之后。
///
/// 反过来也要守：没有导演指令的轮次里出现这段，等于每轮都要求模型写协议行。
#[test]
fn director_cost_block_follows_the_director_block() {
    let with_dir = build_tail_block(
        "chat", "宸熙", "夏婉琴", "", "", "简单", "day", 0, Some("强制让她答应"),
        &[], &[], "", "", &[], 0,
    );
    assert!(with_dir.contains("【导演代价】"), "用了导演指令就该要求交收据：{with_dir}");
    assert!(with_dir.contains("〔导演代价〕类型：理由"), "模板行要能照抄：{with_dir}");
    let a = with_dir.find("【导演指令】").unwrap();
    let b = with_dir.find("【导演代价】").unwrap();
    assert!(a < b, "代价段要排在指令段之后：{with_dir}");

    let no_dir = build_tail_block(
        "chat", "宸熙", "夏婉琴", "", "", "简单", "day", 0, None,
        &[], &[], "", "", &[], 0,
    );
    assert!(!no_dir.contains("【导演代价】"), "没用导演指令就不该要求收据：{no_dir}");
}
use super::pick_example_owner;

/// 示例对话的归属选择：@提到 > AI 扮演 > 面板焦点 > 放弃（用户 2026-10-21 定的：示例可以为空）
#[test]
fn pick_example_owner_falls_back_in_order() {
    let having = vec!["夏婉琴".to_string(), "顾宸熙".to_string()];

    // ① 用户 @ 了谁就用谁
    assert_eq!(
        pick_example_owner("顾宸熙", "夏婉琴", "", &having).as_deref(),
        Some("顾宸熙")
    );
    // ② 没 @ → AI 扮演的那个
    assert_eq!(
        pick_example_owner("", "夏婉琴", "", &having).as_deref(),
        Some("夏婉琴")
    );
    // ③ 前两个都没有 → 导演监视器「当前角色卡」的焦点角色
    assert_eq!(
        pick_example_owner("", "", "顾宸熙", &["顾宸熙".to_string()]).as_deref(),
        Some("顾宸熙")
    );
    // ④ 都没有 → **不给示例**（不再乱猜"全库唯一"那档）
    assert_eq!(pick_example_owner("", "", "", &having), None, "没有可用焦点时不该乱挑");
    assert_eq!(
        pick_example_owner("", "", "", &["只此一个".to_string()]),
        None,
        "只剩一个写过示例也不该猜"
    );
    // 边界：候选没写示例 / 不认识的名字 / 「自由」 都不算数
    assert_eq!(pick_example_owner("顾宸熙", "", "", &["夏婉琴".to_string()]), None);
    assert_eq!(pick_example_owner("不存在的角色", "", "", &having), None);
    assert_eq!(pick_example_owner("自由", "自由", "自由", &having), None);
    assert_eq!(pick_example_owner("", "", "", &[]), None);
}
use super::{clip_field, examples_block};

// ===== v3.4 角色卡 v2：字段截断 + 示例对话 =====

/// 空白 → 不输出；短文本原样；超长截断并标注原文长度
#[test]
fn clip_field_truncates_and_marks() {
    assert!(clip_field("", 10).is_none());
    assert!(clip_field("   \n ", 10).is_none());
    assert_eq!(clip_field("短文本", 10).as_deref(), Some("短文本"));

    let long = "字".repeat(50);
    let out = clip_field(&long, 10).expect("应截断而不是 None");
    assert!(out.starts_with(&"字".repeat(10)), "应保留前 10 字：{out}");
    assert!(out.contains("已节略"), "应标注节略：{out}");
    assert!(out.contains("50"), "应说明原文字数：{out}");
}

/// 示例对话：空 → 不出块；有内容 → 必须带【示例对话】与"不要照抄"约束
#[test]
fn examples_block_has_marker_and_constraint() {
    assert!(examples_block("  ").is_none());
    let b = examples_block("A：你来了。\nB：嗯。").expect("应有块");
    assert!(b.contains("【示例对话】"), "{b}");
    assert!(b.contains("A：你来了。"), "{b}");
    assert!(b.contains("不要照抄"), "缺少防照抄约束（否则模型会把示例当剧情续写）：{b}");
    assert!(b.contains("模仿语气"), "缺少用途说明：{b}");
}

/// 示例对话也吃截断（800 字上限），防止一个角色卡吃穿历史预算
#[test]
fn examples_block_is_capped() {
    let huge = "台词".repeat(1000);
    let b = examples_block(&huge).expect("应有块");
    assert!(b.contains("已节略"), "超长示例应被截断：{}", &b[..b.len().min(80)]);
    assert!(b.chars().count() < 1200, "截断后不应仍超过约 1000 字");
}
// ===== v3.3 前史/序章 =====
use super::prologue_block;

/// 空/纯空白 → 不产生块（老故事升级后提示词零变化）
#[test]
fn prologue_block_absent_when_empty() {
    assert!(prologue_block("").is_none());
    assert!(prologue_block("   \n\t  ").is_none());
}

/// 有内容：带【前史】标记、内容被 trim、"不是本轮剧情"的定位说明必须在
#[test]
fn prologue_block_contains_marker_and_trimmed_body() {
    let b = prologue_block("  三年前，A 和 B 在码头分开。  \n").expect("应有块");
    assert!(b.contains("【前史】"), "{b}");
    assert!(b.contains("三年前，A 和 B 在码头分开。"), "内容应保留：{b}");
    assert!(!b.contains("  \n三年前"), "首尾空白应被裁掉：{b}");
    assert!(b.contains("不是本轮剧情"), "缺少定位说明，模型可能当成本轮内容：{b}");
    assert!(b.contains("以前史为准"), "缺少冲突优先级说明：{b}");
}
use super::input_budget_of;

#[test]
fn budget_formula_basic() {
    // 128k 窗口 × 0.8 = 102400，减输出预留 4000 + 余量 800 = 97600
    assert_eq!(input_budget_of(128_000, 0.8, 4000, 800), 97_600);
    // 32k 保守档：32768 × 0.8 = 26214（四舍五入）− 4800 = 21414
    assert_eq!(input_budget_of(32_768, 0.8, 4000, 800), 21_414);
    // 1M 窗口
    assert_eq!(input_budget_of(1_000_000, 0.8, 4000, 800), 795_200);
}

#[test]
fn recap_drops_oldest_when_over_budget() {
    let lines = vec![
        (0usize, "- 第 1 天：".to_string() + &"甲".repeat(100)),
        (1, "- 第 2 天：".to_string() + &"乙".repeat(100)),
        (2, "- 第 3 天：".to_string() + &"丙".repeat(100)),
    ];
    // 空间充足 → 一条不丢
    let (kept, dropped) = super::fit_recap(&lines, 100_000, 4800, 3000, 600, 200);
    assert_eq!(kept.len(), 3);
    assert_eq!(dropped, 0);

    // 空间只够 1 条 → 从最旧的开始丢
    let tight = 4800 + 3000 + 600 + 200 + 250; // 只剩约 250 token 给摘要
    let (kept2, dropped2) = super::fit_recap(&lines, tight, 4800, 3000, 600, 200);
    assert_eq!(dropped2, 2, "应丢掉最旧的两条");
    assert_eq!(kept2.len(), 1);
    assert!(kept2[0].1.contains("第 3 天"), "保留的应是最新的那条");
}

#[test]
fn day_markers_one_per_day_on_first_kept_message() {
    let days = vec![
        vec![("user".to_string(), "第一天第一句".to_string()), ("assistant".to_string(), "第一天回复".to_string())],
        vec![("user".to_string(), "第二天第一句".to_string()), ("assistant".to_string(), "第二天回复".to_string())],
    ];
    let labels = vec!["第 1 天".to_string(), "第 2 天".to_string()];
    let out = super::mark_days(&days, &[2, 2], &labels);
    assert_eq!(out.len(), 4, "不应增加消息条数");
    assert_eq!(out[0].0, "user");
    assert!(out[0].1.starts_with("【第 1 天】"), "每天首条应带标记：{}", out[0].1);
    assert!(!out[1].1.contains("【第 1 天】"), "同一天的第二条不加标记");
    assert!(out[2].1.starts_with("【第 2 天】"), "第二天首条应带标记：{}", out[2].1);

    // 只保留每天最后 1 条时，标记应落在"实际保留的首条"上
    let out2 = super::mark_days(&days, &[1, 1], &labels);
    assert_eq!(out2.len(), 2);
    assert!(out2[0].1.starts_with("【第 1 天】"), "截取后标记应跟随保留的首条：{}", out2[0].1);
    assert!(out2[1].1.starts_with("【第 2 天】"));

    // 条数为 0 的天不产生任何消息
    let out3 = super::mark_days(&days, &[0, 2], &labels);
    assert_eq!(out3.len(), 2);
    assert!(out3[0].1.starts_with("【第 2 天】"), "被跳过的天不应产生标记");
}

/// 回归（2026-10-18 用户要求）：单条均 token 有 **400 下限**。
///
/// 起因：用户刚开始写，只有两条很短的话（实测 30 token/条），
/// 「每日 N 值」被算成几百条/天 —— 那个数字既不可信、也让"保留天数"的语义失真。
#[test]
fn avg_msg_tokens_has_floor_of_400() {
    // 每条只有 30 token（很短的用户输入）
    let days = vec![vec![30usize, 30], vec![30], vec![30, 30, 30]];
    let (limits, _, _, avg) = super::plan_day_limits(&days, 100_000, 10);
    assert_eq!(avg, super::MIN_AVG_MSG_TOKENS, "实测 30 也要夹到 400");
    assert_eq!(avg, 400);

    // 预算足够大时，N 应当按 400 算而不是按 30 算（否则 N 会大到没意义）
    // 每日预算 = 100000/3 ≈ 33333 → N = 33333/400 ≈ 83 → 再受上限 10 与当天条数约束
    assert_eq!(limits, vec![2, 1, 3], "不应超过当天实际条数：{limits:?}");

    // 长消息不受下限影响（5000 > 400 用实测值）
    let long_days = vec![vec![5000usize], vec![5000], vec![5000]];
    let (_, _, _, avg2) = super::plan_day_limits(&long_days, 1_000_000, 10);
    assert_eq!(avg2, 5000, "长消息应使用实测平均值");
}

#[test]
fn day_limits_respect_cap_and_budget() {
    // 三天、每天 12 条、每条 100 token（共 3600），预算充足 → 封顶在用户上限 10
    let days = vec![vec![100usize; 12], vec![100; 12], vec![100; 12]];
    let (limits, removed, shrunk, _avg) = super::plan_day_limits(&days, 100_000, 10);
    assert_eq!(limits, vec![10, 10, 10], "应封顶到用户上限");
    assert_eq!(removed, 0);
    assert!(!shrunk);

    // 预算压到 900：每天 300 token，而单条按**下限 400** 算 → 300/400=0 → 每天至少留 1 条
    // （2026-10-18 起 avg 有 400 的下限，所以这里不再是 3 条；见 MIN_AVG_MSG_TOKENS）
    let (limits2, removed2, _, _) = super::plan_day_limits(&days, 900, 10);
    assert_eq!(limits2, vec![1, 1, 1], "预算极紧时每天保底 1 条：{limits2:?}");
    assert_eq!(removed2, 0);

    // 条数不足时不超过当天实际条数
    let uneven = vec![vec![100usize; 2], vec![100; 12], vec![100; 5]];
    let (limits3, _, _, _) = super::plan_day_limits(&uneven, 100_000, 10);
    assert_eq!(limits3, vec![2, 10, 5], "不应超过当天实际条数");
}

#[test]
fn day_limits_degrade_then_drop_oldest_day() {
    // 每天都只有 1 条、但每条 5000 token：预算 6000 → 先降到每天 1 条仍超 → 从最老的一天开始丢弃
    let days = vec![vec![5000usize], vec![5000], vec![5000]];
    let (limits, removed, shrunk, _) = super::plan_day_limits(&days, 6000, 10);
    assert!(removed >= 1, "应至少移出最老的一天（removed={removed}）");
    assert!(limits.len() <= 3);
    // 被移出的天不参与限额列表
    assert_eq!(limits.len(), days.len() - removed);
    let _ = shrunk;

    // 极端：预算为 0 → 只留最后一天，且限额不低于 1（保证当前天有上下文）
    let (limits2, removed2, _, _) = super::plan_day_limits(&days, 0, 10);
    assert_eq!(removed2, 2, "预算为 0 时应只剩最后一天");
    assert_eq!(limits2, vec![1], "最后一天至少留 1 条");
}

#[test]
fn budget_formula_guards() {
    // 系数非法 → 回退 0.8，不产生 NaN/panic
    assert_eq!(input_budget_of(1_000_000, 0.0, 4000, 800), 795_200);
    assert_eq!(input_budget_of(1_000_000, 1.5, 4000, 800), 795_200);
    assert_eq!(input_budget_of(1_000_000, f64::NAN, 4000, 800), 795_200);
    // 窗口小于预留 → 下溢为 0，不 panic
    assert_eq!(input_budget_of(1000, 0.8, 4000, 800), 0);
    assert_eq!(input_budget_of(0, 0.8, 4000, 800), 0);
}

/// **回归锁（2026-10-26）**：关键记忆必须真的进尾部块，且落在【示例对话】之前。
///
/// 之前 `build_tail_block` 的实参把 `memory_block` 和 `focus_examples` **传反了** ——
/// 两个都是 `&str`，编译期毫无提示，于是【关键记忆】永远拿到空串（界面上"记了但没注入"）。
/// 这条测试直接锁"两段各自到位、位置正确"。
#[test]
fn memory_block_reaches_tail_block_and_examples_do_not_mix() {
    let mem = "【关键记忆】本故事里已经发生、不可重来的事\n- 第 1 天 · 表白：我爱你";
    let ex = "[夏婉琴]「你别多想。」";
    let block = build_tail_block(
        "chat", "我", "夏婉琴", "", "", "简单", "day", 0, None,
        &[], &[], ex, mem, &[], 0,
    );
    assert!(block.contains("【关键记忆】"), "关键记忆段必须出现：{block}");
    assert!(block.contains("第 1 天 · 表白"), "{block}");
    assert!(block.contains("【示例对话】"), "示例对话段也要在：{block}");
    assert!(!block.contains("【示例对话】（以下对话只用于**模仿语气、用词、停顿与反应方式**，不要照抄内容、也不要直接接续它）\n【关键记忆】"),
        "两段不许串位：{block}");
    let (a, b) = (block.find("【关键记忆】").unwrap_or(0), block.find("【示例对话】").unwrap_or(0));
    assert!(a < b, "关键记忆应排在示例对话之前：{block}");
}

/// 表态标记的提示词：**只给两个例子照抄**，不解释 JSON、不改正文写法（用户 2026-10-26 定）。
#[test]
fn stance_hint_gives_two_verbatim_examples() {
    let h = super::prompt::stance_hint("confession");
    assert!(h.contains("〔态度:接受〕"), "{h}");
    assert!(h.contains("〔态度:拒绝〕"), "{h}");
    assert!(h.contains("最后一行"), "必须指定位置：{h}");
    assert!(h.contains("不会被展示"), "要说明不影响正文/不会被看到：{h}");
    assert!(!h.contains("json") && !h.contains("JSON"), "不许给模型讲 JSON：{h}");
    let p = super::prompt::stance_hint("promise");
    assert!(p.contains("一个承诺"), "{p}");
}