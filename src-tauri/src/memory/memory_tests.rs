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

/// 只记第一次：同一类型第二次出现**不再新建文档**
#[test]
fn captures_each_kind_only_once() {
    let tmp = TmpDir::new("once");
    let text = "[夏婉琴]“我爱你，跟我在一起吧。”";
    let ev = vec![("confession".to_string(), "我爱你".to_string())];

    let made = capture_first_events_in(tmp.path(), &None, 5, "第 4 天", text, &ev).unwrap();
    assert_eq!(made, vec!["表白".to_string()], "第一次表白应该被记下");

    let made2 = capture_first_events_in(tmp.path(), &None, 6, "第 5 天", text, &ev).unwrap();
    assert!(made2.is_empty(), "第二次表白不该再记：{made2:?}");

    let all = list_all_in(tmp.path());
    assert_eq!(all.len(), 1, "应该只有一条记忆");
    assert_eq!(all[0].kind, "confession");
    assert_eq!(all[0].day_label, "第 4 天");
    assert!(all[0].body.contains("我爱你"), "正文应含原文片段：{}", all[0].body);
}

/// 分支隔离：A 分支记的不该出现在 B 分支；但**主线是分叉前的共同历史**，两条分支都看得到
#[test]
fn memories_are_branch_isolated_but_inherit_mainline() {
    let tmp = TmpDir::new("branch");
    let text = "[夏婉琴]“我保证，我一定会回来。”";
    let ev = vec![("promise".to_string(), "我保证".to_string())];

    // 主线先记一条
    capture_first_events_in(tmp.path(), &None, 1, "第 1 天", text, &ev).unwrap();
    // 分支 A 里同一类不再重复记（主线已经记过）
    let a_made = capture_first_events_in(tmp.path(), &Some("BR-A".to_string()), 2, "第 1 天", text, &ev).unwrap();
    assert!(a_made.is_empty(), "主线已记过同类，分支不该重复：{a_made:?}");

    // 分支 A 里发生**新的一类**（背叛）→ 记在 A 名下
    let ev2 = vec![("betrayal".to_string(), "背叛".to_string())];
    let made = capture_first_events_in(tmp.path(), &Some("BR-A".to_string()), 3, "第 2 天", "他背叛了她。", &ev2).unwrap();
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
    let made = capture_first_events_in(tmp.path(), &None, 1, "第 1 天", "（他递伞）他们争吵。", &ev).unwrap();
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
    capture_first_events_in(tmp.path(), &None, 7, "第 9 天", "[院长]他背叛了她。", &ev).unwrap();
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
    capture_first_events_in(&mems, &None, 1, "第 3 天", "[A]“我爱你。”", &ev).unwrap();

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
