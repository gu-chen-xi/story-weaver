// 关键记忆的规则测试（用户 2026-10-22/23 定的三条硬规则）。
//
// 为什么必须有：这几条都是"退化后不会报错、只会慢慢变味"的类型 ——
//   ① 只记第一次 → 退化后会重复刷屏，把尾部块吃满；
//   ② 一个事件一个文档 → 退化后会挤成一个大文件，用户就没法单独删掉"那次背叛"；
//   ③ 分支隔离 → 退化后 A 分支的表白会漏进 B 分支。
//
// 全部走 `_in(dir)` 注入版：`paths::data_root()` 是 `OnceLock` 缓存的，
// 靠环境变量切换数据根在测试里不可靠（而且这台机器的 %TEMP% 子树拒绝建子目录）。

use super::*;

/// 每个测试一个独立目录（放在 cargo 的 target 下 —— %TEMP% 在这台机器上建不了子目录）
struct TmpDir(std::path::PathBuf);

impl TmpDir {
    fn new(tag: &str) -> Self {
        let d = std::env::current_dir()
            .unwrap()
            .join("target")
            .join("sw_memory_tests")
            .join(format!("{tag}_{}_{}", std::process::id(), now_nanos()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TmpDir(d)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 除表白外的三类：只记第一次（第二次不再新建文档）。
///
/// 表白是例外 —— 它按"累计一张卡"记（见下面两条测试），所以这里用「承诺」。
#[test]
fn captures_each_kind_only_once() {
    let tmp = TmpDir::new("once");
    let text = "[夏婉琴]“我保证，我一定会回来。”";
    let ev = vec![("promise".to_string(), "我保证".to_string())];

    let made = capture_first_events_in(tmp.path(), &None, 5, "第 4 天", text, "（她点头）「我保证。」", &ev).unwrap();
    assert_eq!(made, vec!["承诺".to_string()], "第一次承诺应该被记下");

    let made2 = capture_first_events_in(tmp.path(), &None, 6, "第 5 天", text, "（她点头）「我保证。」", &ev).unwrap();
    assert!(made2.is_empty(), "第二次承诺不该再记：{made2:?}");

    let all = list_all_in(tmp.path());
    assert_eq!(all.len(), 1, "应该只有一条记忆");
    assert_eq!(all[0].kind, "promise");
    assert_eq!(all[0].day_label, "第 4 天");
    assert!(all[0].body.contains("我保证"), "正文应含原文片段：{}", all[0].body);
}

/// 分支隔离：A 分支记的不该出现在 B 分支；但**主线是分叉前的共同历史**，两条分支都看得到
#[test]
fn memories_are_branch_isolated_but_inherit_mainline() {
    let tmp = TmpDir::new("branch");
    let text = "[夏婉琴]“我保证，我一定会回来。”";
    let ev = vec![("promise".to_string(), "我保证".to_string())];

    // 主线先记一条
    capture_first_events_in(tmp.path(), &None, 1, "第 1 天", text, "（她点头）「我保证。」", &ev).unwrap();
    // 分支 A 里同一类不再重复记（主线已经记过）
    let a_made = capture_first_events_in(tmp.path(), &Some("BR-A".to_string()), 2, "第 1 天", text, "（她点头）「我保证。」", &ev).unwrap();
    assert!(a_made.is_empty(), "主线已记过同类，分支不该重复：{a_made:?}");

    // 分支 A 里发生**新的一类**（背叛）→ 记在 A 名下
    let ev2 = vec![("betrayal".to_string(), "背叛".to_string())];
    let made = capture_first_events_in(tmp.path(), &Some("BR-A".to_string()), 3, "第 2 天", "他背叛了她。", "（他翻脸）", &ev2).unwrap();
    assert_eq!(made, vec!["背叛".to_string()]);

    let main_view = list_for_branch_in(tmp.path(), &None);
    let a_view = list_for_branch_in(tmp.path(), &Some("BR-A".to_string()));
    let b_view = list_for_branch_in(tmp.path(), &Some("BR-B".to_string()));

    assert_eq!(main_view.len(), 1, "主线只看得到主线的：{main_view:?}");
    assert_eq!(a_view.len(), 2, "A 分支 = 主线 ∪ 自己：{a_view:?}");
    assert_eq!(b_view.len(), 1, "B 分支看不到 A 记的背叛：{b_view:?}");
    assert!(!b_view.iter().any(|m| m.kind == "betrayal"), "A 的背叛不该漏进 B");
}

/// 不在注册表里的事件（关心/冲突）不写记忆
#[test]
fn unregistered_kinds_are_ignored() {
    let tmp = TmpDir::new("ignored");
    let ev = vec![
        ("care".to_string(), "递伞".to_string()),
        ("conflict".to_string(), "争吵".to_string()),
    ];
    let made = capture_first_events_in(tmp.path(), &None, 1, "第 1 天", "（他递伞）他们争吵。", "（他递伞）", &ev).unwrap();
    assert!(made.is_empty(), "关心/冲突不该进关键记忆：{made:?}");
    assert!(list_all_in(tmp.path()).is_empty());
}

/// 前端注入：一条一行；预算不够 → 整段不出现（不给空壳标题）
#[test]
fn render_block_is_one_line_per_memory_and_reports_skips() {
    let m = Memory {
        id: "001-表白".into(),
        kind: "confession".into(),
        label: "表白".into(),
        day_label: "第 4 天".into(),
        node_id: 5,
        branch: None,
        importance: 90,
        matched: "我爱你".into(),
        created_at: "2026-10-23 13:00:00".into(),
        stance: None,
        attempts: 1,
        refusals: 0,
        reasons: Vec::new(),
        quote: String::new(),
        body: "# 第 4 天 · 表白\n\n她说“我爱你”。".into(),
    };
    let block = render_block(std::slice::from_ref(&m), 1200).unwrap();
    assert!(block.starts_with("【关键记忆】"), "{block}");
    assert!(block.contains("- 第 4 天 · 表白："), "{block}");
    assert!(block.contains("不要重演第一次"), "必须提醒模型别重演：{block}");
    assert!(render_block(std::slice::from_ref(&m), 5).is_none(), "预算不够就不出这一节");

    // 超预算的第二条被跳过 → 块里注明
    let m2 = Memory { id: "002-承诺".into(), label: "承诺".into(), day_label: "第 9 天".into(), ..m.clone() };
    let mut m_short = m.clone();
    m_short.body = "短".into();
    let block2 = render_block(&[m_short, m2], 20).unwrap();
    assert!(block2.contains("未列出"), "被挤掉的要注明：{block2}");
}

/// 用户手改过的文件即使格式不完整也不能 panic（这目录用户会动）
#[test]
fn parse_memory_tolerates_hand_edited_files() {
    assert!(parse_memory("x", "没有 front matter").is_none());
    assert!(parse_memory("x", "---\nkind: confession\n---\n\n正文").is_some());
    let m = parse_memory("y", "---\nkind: betrayal\n---\n\n背叛").unwrap();
    assert_eq!(m.label, "背叛", "缺 label 时用注册表的中文名");
    assert_eq!(m.importance, 50, "缺 importance 时默认 50");
    assert!(m.branch.is_none());
}

/// 改正文不动元数据；删掉就真的没了
#[test]
fn update_and_delete_work() {
    let tmp = TmpDir::new("edit");
    let ev = vec![("betrayal".to_string(), "背叛".to_string())];
    capture_first_events_in(tmp.path(), &None, 7, "第 9 天", "[院长]他背叛了她。", "（他翻脸）", &ev).unwrap();
    let id = list_all_in(tmp.path())[0].id.clone();

    update_body_in(tmp.path(), &id, "改过的正文：其实是误会。").unwrap();
    let after = list_all_in(tmp.path());
    assert_eq!(after.len(), 1);
    assert!(after[0].body.contains("其实是误会"), "正文应改掉：{}", after[0].body);
    assert_eq!(after[0].kind, "betrayal", "元数据不该被正文覆盖");

    delete_in(tmp.path(), &id).unwrap();
    assert!(list_all_in(tmp.path()).is_empty(), "删掉后应没有记忆");
}

/// 导出：一条一个 md + INDEX.md，全部在 `key-memories/<故事>_<时间戳>/` 下
#[test]
fn export_writes_folder_with_one_file_per_memory() {
    let tmp = TmpDir::new("export");
    let exports = tmp.path().join("exports");
    let mems = tmp.path().join("memories");
    let ev = vec![("confession".to_string(), "我爱你".to_string())];
    capture_first_events_in(&mems, &None, 1, "第 3 天", "[A]“我爱你。”", "（她别过脸）「别。」", &ev).unwrap();

    let (dir, n) = export_all_in(&exports, &mems, "测试故事/带斜杠").unwrap();
    assert_eq!(n, 1, "应导出一条");
    let p = std::path::Path::new(&dir);
    assert!(p.is_dir(), "导出目录应存在：{dir}");
    assert!(dir.contains("key-memories"), "必须在 key-memories 下：{dir}");
    // 清洗不是删字，而是把 / 换成 _ —— 断言最后一段名字里不含斜杠
    let folder = p.file_name().unwrap().to_string_lossy().to_string();
    assert!(!folder.contains('/') && folder.contains("测试故事_带斜杠"), "故事名里的斜杠要被替换：{folder}");
    let names: Vec<String> = std::fs::read_dir(p)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(names.iter().any(|x| x == "INDEX.md"), "应有 INDEX.md：{names:?}");
    assert_eq!(names.iter().filter(|x| x.ends_with(".md")).count(), 2, "1 条记忆 + 索引");
}

/// **回归锁（2026-10-26）**：记忆正文 = 这一轮的「用户消息 + AI 回复」，原样落盘。
///
/// 之前是"命中词 ±80 字窗口"，摘录会从上一轮 AI 的半句话开头（用户实测截图），
/// 看不出是谁对谁说了什么。
#[test]
fn body_keeps_the_whole_round() {
    let tmp = TmpDir::new("round");
    let round = "[用户] 我爱你\n\n[AI] （她没接，往后退了半步）「别。」";
    let ev = vec![("confession".to_string(), "我爱你".to_string())];
    capture_first_events_in(tmp.path(), &None, 9, "第 1 天", round, "（她没接，往后退了半步）「别。」", &ev).unwrap();
    let all = list_all_in(tmp.path());
    assert_eq!(all.len(), 1);
    assert!(all[0].body.contains("[用户] 我爱你"), "要有用户那条：{}", all[0].body);
    assert!(all[0].body.contains("[AI] （她没接"), "也要有 AI 的回复：{}", all[0].body);
    assert!(!all[0].body.contains("…（"), "不该再带 ±80 字窗口的前缀省略号：{}", all[0].body);
}

/// **表白累计（2026-10-26 用户定的规则）**：同一个 kind 只留一张卡，次数累加，
/// 理由只保留前 3，正文只留**最近一次**的对话。
#[test]
fn confession_accumulates_on_one_card() {
    let tmp = TmpDir::new("confess_sum");
    let ev = vec![("confession".to_string(), "我爱你".to_string())];
    let round1 = "[用户] 我爱你\n\n[AI] （她别过脸）「别。」「别过来。」";
    let r1 = capture_first_events_in(tmp.path(), &None, 1, "第 1 天", round1, "（她别过脸）「别。」「别过来。」", &ev).unwrap();
    assert_eq!(r1, vec!["表白（第 1 次）".to_string()]);

    let round2 = "[用户] 为什么？\n\n[AI] （她背抵到墙）「这话不该跟我说。」「别。」";
    let r2 = capture_first_events_in(tmp.path(), &None, 2, "第 2 天", round2, "（她背抵到墙）「这话不该跟我说。」「别。」", &ev).unwrap();
    assert_eq!(r2, vec!["表白（第 2 次）".to_string()], "第二次是累计，不是新文件");

    let all = list_all_in(tmp.path());
    assert_eq!(all.len(), 1, "表白只留一张卡：{all:?}");
    let raw_md = std::fs::read_to_string(tmp.path().join(format!("{}.md", all[0].id))).unwrap();
    assert!(raw_md.contains("attempts: 2"), "次数要累加：{raw_md}");
    assert!(raw_md.contains("stance: refused"), "{raw_md}");
    assert!(raw_md.contains("refusals: 2"), "{raw_md}");
    assert!(all[0].body.contains("第 2 次表白"), "正文摘要要写第几次：{}", all[0].body);
    assert!(all[0].body.contains("为什么？"), "正文只留最近一次（第 2 次）：{}", all[0].body);
    assert!(!all[0].body.contains("她别过脸"), "上一次的对话不该再留：{}", all[0].body);
}

/// 后来接受了：**同一张卡改判**，不新开一条；已接受的卡不会被后来的拒绝翻回去
#[test]
fn confession_flips_to_accepted_on_the_same_card() {
    let tmp = TmpDir::new("confess_accept");
    let ev = vec![("confession".to_string(), "我爱你".to_string())];
    capture_first_events_in(tmp.path(), &None, 1, "第 1 天", "[用户] 我爱你\n\n[AI] 「别。」「别过来。」", "「别。」「别过来。」", &ev).unwrap();
    let r2 = capture_first_events_in(tmp.path(), &None, 2, "第 4 天", "[用户] 我爱你\n\n[AI] （她低头）「我也喜欢你。」", "（她低头）「我也喜欢你。」", &ev).unwrap();
    assert_eq!(r2, vec!["表白（第 2 次）".to_string()]);

    let all = list_all_in(tmp.path());
    assert_eq!(all.len(), 1, "改判不新开文件：{all:?}");
    let raw_md = std::fs::read_to_string(tmp.path().join(format!("{}.md", all[0].id))).unwrap();
    assert!(raw_md.contains("stance: accepted"), "要改成 accepted：{raw_md}");
    assert!(raw_md.contains("attempts: 2"), "{raw_md}");
    assert!(raw_md.contains("refusals: 1"), "{raw_md}");
    assert!(all[0].body.contains("已被接受"), "正文要写接受：{}", all[0].body);
}

/// **旧卡/未判定卡也要纠正注入措辞**（2026-10-26 用户实测："测试1"那张卡写着「态度未判定」，
/// 于是模型越演越软）。规则：卡里没有 stance 或写着 unclear 时，用**正文里最近一次对话**重算。
#[test]
fn render_marks_refusal_even_for_old_cards() {
    let m = Memory {
        id: "001-表白".into(),
        kind: "confession".into(),
        label: "表白".into(),
        day_label: "第 1 天".into(),
        node_id: 1,
        branch: None,
        importance: 90,
        matched: "我爱你".into(),
        created_at: "2026-10-26 22:00:00".into(),
        stance: Some(Stance::Unclear),
        attempts: 1,
        refusals: 0,
        reasons: Vec::new(),
        quote: String::new(),
        body: "# 第 1 天 · 表白\n\n第 1 次表白：对方态度未判定。\n\n最近一次：\n\n[用户] 我爱你\n\n[AI] （她别过脸）「别说了。」「这种话，我不接。」".into(),
    };
    let block = render_block(std::slice::from_ref(&m), 1200).unwrap();
    assert!(block.contains("被拒绝"), "旧卡也要标成被拒：{block}");
    assert!(!block.contains("态度未判定"), "不该再把未判定丢给模型：{block}");
    assert!(block.contains("关系没有因此确立"), "必须提醒：被拒不等于确立：{block}");
}

/// 新版卡：次数与理由直接进注入行
#[test]
fn render_confession_with_counts_and_reasons() {
    let m = Memory {
        id: "001-表白".into(),
        kind: "confession".into(),
        label: "表白".into(),
        day_label: "第 1 天".into(),
        node_id: 3,
        branch: None,
        importance: 90,
        matched: "我爱你".into(),
        created_at: "2026-10-26 22:00:00".into(),
        stance: Some(Stance::Refused),
        attempts: 3,
        refusals: 3,
        reasons: vec![Signal { phrase: "别说了".into(), weight: 2, count: 2 }],
        quote: String::new(),
        body: "# 第 1 天 · 表白\n\n第 3 次表白：**被拒绝**（累计被拒 3 次）。".into(),
    };
    let block = render_block(std::slice::from_ref(&m), 1200).unwrap();
    assert!(block.contains("被拒绝"), "{block}");
    assert!(block.contains("共 3 次表白"), "要写次数：{block}");
    assert!(block.contains("别说了"), "要写理由：{block}");
}

/// **标记优先于打分**（2026-10-26）：模型写了 `〔态度:拒绝〕`，即使正文的措辞很像接受也按拒绝记。
#[test]
fn marker_overrides_the_scorer() {
    let tmp = TmpDir::new("marker");
    let ev = vec![("confession".to_string(), "我爱你".to_string())];
    let round = "[用户] 我爱你\n\n[AI] （她笑了一下，很轻）「你这个人啊。」\n〔态度:拒绝〕";
    let made = capture_first_events_in(&tmp_path_owned(tmp.path()), &None, 1, "第 1 天", round, "（她笑了一下，很轻）「你这个人啊。」\n〔态度:拒绝〕", &ev).unwrap();
    assert_eq!(made, vec!["表白（第 1 次）".to_string()]);
    let all = list_all_in(tmp.path());
    let raw_md = std::fs::read_to_string(tmp.path().join(format!("{}.md", all[0].id))).unwrap();
    assert!(raw_md.contains("stance: refused"), "标记优先：{raw_md}");
    assert!(!all[0].body.contains("〔态度"), "正文里不许留标记：{}", all[0].body);
    assert!(raw_md.contains("refusals: 1"), "首次被拒要算 1 次：{raw_md}");
    assert!(all[0].body.contains("累计被拒 1 次"), "摘要里的次数要对：{}", all[0].body);
    assert!(all[0].body.contains("她的话：「你这个人啊。」"), "没有词级理由时要写她的原话：{}", all[0].body);
    assert!(!all[0].body.contains("对方理由："), "没有理由时不许留空壳：{}", all[0].body);
    assert!(!all[0].body.contains("对方理由：\n") && !all[0].body.trim_end().ends_with("对方理由："),
        "没有理由时不许留空壳「对方理由：」：{}", all[0].body);
}

fn tmp_path_owned(p: &std::path::Path) -> std::path::PathBuf { p.to_path_buf() }