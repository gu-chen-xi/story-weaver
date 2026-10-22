// 从 `timeline.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `timeline::tree_tests` —— 因为 `timeline.rs` + `timeline/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `timeline`，一行都不用改写。

use super::*;
use std::collections::HashSet;

fn n(id: i64, parent: Option<i64>, sort: i64) -> TreeNode {
    TreeNode { id, parent_id: parent, sort_order: sort }
}

/// 线性故事（现在所有故事都是这种）：1→2→3→4，父链就是从根到该节点的顺序
fn linear() -> Vec<TreeNode> {
    vec![n(1, None, 1), n(2, Some(1), 2), n(3, Some(2), 3), n(4, Some(3), 4)]
}

#[test]
fn children_index_groups_and_sorts_by_sort_order() {
    // 手工造一个"乱序传入"的列表：同层应按 (sort_order, id) 排
    let nodes = vec![
        n(10, Some(1), 30),
        n(1, None, 1),
        n(11, Some(1), 20),
        n(12, Some(1), 20), // 与 11 同 sort_order → 按 id
    ];
    let idx = children_index(&nodes);
    assert_eq!(idx.get(&Some(1)).unwrap(), &vec![11, 12, 10], "同层应按 (sort_order, id)");
    assert_eq!(idx.get(&None).unwrap(), &vec![1]);
}

#[test]
fn path_to_root_returns_root_first_order() {
    let nodes = linear();
    assert_eq!(path_to_root(&nodes, 4), vec![1, 2, 3, 4], "应返回从根到目标的顺序");
    assert_eq!(path_to_root(&nodes, 1), vec![1], "根节点的路径只有自己");
}

/// 这是分支系统最关键的一条：**路径只走自己的父链，不含别的分支**
#[test]
fn path_to_root_excludes_other_branches() {
    //        1
    //       / \
    //      2   5     ← 2 与 5 是分叉（共享 parent 1）
    //      |   |
    //      3   6
    //      |
    //      4
    let nodes = vec![
        n(1, None, 1),
        n(2, Some(1), 2),
        n(3, Some(2), 3),
        n(4, Some(3), 4),
        n(5, Some(1), 5),
        n(6, Some(5), 6),
    ];
    assert_eq!(path_to_root(&nodes, 4), vec![1, 2, 3, 4], "分支 A 的路径不应含分支 B 的 5/6");
    assert_eq!(path_to_root(&nodes, 6), vec![1, 5, 6], "分支 B 的路径不应含分支 A 的 2/3/4");
}

#[test]
fn path_to_root_survives_cycle_without_hanging() {
    // 数据异常：3→2→3 成环。函数必须能停下（返回已访问过的部分），不能死循环
    let nodes = vec![n(2, Some(3), 1), n(3, Some(2), 2)];
    let path = path_to_root(&nodes, 3);
    assert!(path.len() <= 2, "成环时应停下，实际 {path:?}");
}

#[test]
fn flatten_tree_respects_expanded_set() {
    let nodes = linear();
    // 什么都没展开 → 只有根
    let none: HashSet<i64> = HashSet::new();
    assert_eq!(flatten_tree(&nodes, None, &none), vec![(1, 0)]);
    // 展开 1、2 → 出现 1(0) 2(1) 3(2)，4 仍未出现（3 未展开）
    let mut e: HashSet<i64> = HashSet::new();
    e.insert(1);
    e.insert(2);
    assert_eq!(flatten_tree(&nodes, None, &e), vec![(1, 0), (2, 1), (3, 2)]);
    // 全展开 → 4 个节点，深度 0..3
    let mut all: HashSet<i64> = HashSet::new();
    for i in 1..=4 {
        all.insert(i);
    }
    assert_eq!(flatten_tree(&nodes, None, &all), vec![(1, 0), (2, 1), (3, 2), (4, 3)]);
}

#[test]
fn flatten_tree_handles_fork_depths() {
    let nodes = vec![
        n(1, None, 1),
        n(2, Some(1), 2),
        n(3, Some(2), 3),
        n(4, Some(3), 4),
        n(5, Some(1), 5),
        n(6, Some(5), 6),
    ];
    let mut all: HashSet<i64> = HashSet::new();
    for i in 1..=6 {
        all.insert(i);
    }
    // 期望：1(0) → 2(1) → 3(2) → 4(3) → 5(1) → 6(2)
    assert_eq!(
        flatten_tree(&nodes, None, &all),
        vec![(1, 0), (2, 1), (3, 2), (4, 3), (5, 1), (6, 2)]
    );
}

#[test]
fn branches_and_fork_detection() {
    let nodes = vec![
        n(1, None, 1),
        n(2, Some(1), 2),
        n(5, Some(1), 5),
        n(3, Some(2), 3),
    ];
    assert_eq!(branches_of(&nodes, 1), vec![2, 5], "1 有两个子节点");
    assert!(is_fork(&nodes, 1), "1 是分叉点");
    assert!(!is_fork(&nodes, 2), "2 只有一个子节点，不是分叉点");
    assert!(branches_of(&nodes, 3).is_empty(), "叶子节点没有子节点");
}
/// 真机验证（默认跳过）：对**真实故事库**跑一次迁移，检查父链是否正确串起来。
/// 跑法：`cargo test --lib live_parent_links -- --ignored --nocapture`
///
/// 为什么值得单独跑：老库里 `sort_order` 有重复值（例如两个节点都叫「第 8 天」），
/// 回填必须带 id 作次级键才稳定 —— 这一点在构造数据上容易"碰巧对"，真实数据才试得出来。
#[tokio::test]
#[ignore]
async fn live_parent_links() {
    use sqlx::sqlite::SqliteConnectOptions;
    use std::str::FromStr;
    let db = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..").join("library").join("default").join("projects").join("7").join("story.db");
    if !db.exists() {
        println!("真实库不存在，跳过：{}", db.display());
        return;
    }
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display())).unwrap();
    let pool = SqlitePool::connect_with(opts).await.unwrap();

    // 迁移只补 NULL，不覆盖已有值 —— 可安全反复跑
    crate::story_db::ensure_timeline_parent_column(&pool).await.unwrap();

    let rows = sqlx::query("SELECT id, date_label, sort_order, parent_id FROM timeline_nodes ORDER BY sort_order ASC, id ASC")
        .fetch_all(&pool).await.unwrap();
    println!("  节点数：{}", rows.len());
    let mut roots = 0;
    for r in &rows {
        let id: i64 = r.try_get(0).unwrap();
        let label: String = r.try_get(1).unwrap();
        let sort: i64 = r.try_get(2).unwrap();
        let parent: Option<i64> = r.try_get(3).unwrap();
        if parent.is_none() { roots += 1; }
        println!("    id={id:<3} sort={sort:<3} parent={:<6} {label}", parent.map(|x| x.to_string()).unwrap_or("NULL".into()));
    }
    assert!(roots <= 1, "线性故事应只有 1 个根节点，实际 {roots} 个");

    // 用纯函数复核：从最后一个节点走回根，路径长度应等于节点数
    let nodes: Vec<TreeNode> = rows.iter().map(|r| TreeNode {
        id: r.try_get(0).unwrap(),
        parent_id: r.try_get(3).unwrap(),
        sort_order: r.try_get(2).unwrap(),
    }).collect();
    if let Some(last) = nodes.last() {
        let path = path_to_root(&nodes, last.id);
        println!("  从最后节点 {} 回溯：{:?}（长度 {}）", last.id, path, path.len());
        assert_eq!(path.len(), nodes.len(), "线性链的路径应包含全部节点");
    }
}

/// **本批最重要的一条约束：线性故事的行为必须与改动前完全一致。**
///
/// 改动前，跨天装配是"按全表 sort_order 位置取近 KEEP_DAYS 个天数节点、更早的天进 recap"；
/// 改动后改为"按父链取"。这条测试用**等价性**把这个约束钉住：
/// 只要故事还是线性链（每个节点至多一个后继），两种算法的结果必须逐项相同。
#[test]
fn linear_story_path_equals_positional() {
    const KEEP: usize = 3;
    // 造一条线性链，天数节点与插曲混排（更贴近真实数据：2026-10-11 的库里就有插曲）
    // 1(day) 2(day) 3(插曲) 4(day) 5(day) 6(day) 7(day)
    let nodes = vec![
        n(1, None, 1),
        n(2, Some(1), 2),
        n(3, Some(2), 3),
        n(4, Some(3), 4),
        n(5, Some(4), 5),
        n(6, Some(5), 6),
        n(7, Some(6), 7),
    ];
    // 天数节点（插曲 3 不算）—— 顺序与 sort_order 一致
    let day_ids: Vec<i64> = vec![1, 2, 4, 5, 6, 7];

    for target in [1i64, 2, 4, 5, 6, 7] {
        // —— 新算法：父链上的天数节点 ——
        let chain = ancestor_ids(&nodes, target);
        let by_chain: Vec<i64> = day_ids.iter().copied().filter(|d| chain.contains(d)).collect();

        // —— 旧算法：全表位置在 target 及其之前的天数节点 ——
        let pos = day_ids.iter().position(|d| *d == target).unwrap();
        let by_pos: Vec<i64> = day_ids[..=pos].to_vec();

        assert_eq!(by_chain, by_pos, "节点 {target}：父链与按位置取的结果必须一致");

        // 近 KEEP 天：两边各取末尾 KEEP 个
        let keep_chain = &by_chain[by_chain.len().saturating_sub(KEEP)..];
        let keep_pos = &by_pos[by_pos.len().saturating_sub(KEEP)..];
        assert_eq!(keep_chain, keep_pos, "节点 {target}：近 KEEP 天必须一致");

        // recap：两边都取"剩下的更早的天"
        let recap_chain: Vec<i64> = by_chain.iter().copied().filter(|d| !keep_chain.contains(d)).collect();
        let recap_pos: Vec<i64> = by_pos[..by_pos.len() - keep_pos.len()].to_vec();
        assert_eq!(recap_chain, recap_pos, "节点 {target}：recap 必须一致");
    }
}

/// 有分支时，父链**只含自己那条线** —— 这正是本次改动的目的
#[test]
fn forked_story_path_excludes_other_branch() {
    //        1
    //       / \
    //      2   5
    //      |   |
    //      3   6
    let nodes = vec![
        n(1, None, 1),
        n(2, Some(1), 2),
        n(3, Some(2), 3),
        n(5, Some(1), 4),   // 注意 sort_order 排在 3 前面（模拟既有数据的乱序）
        n(6, Some(5), 5),
    ];
    let day_ids: Vec<i64> = vec![1, 2, 3, 5, 6];

    // 站在分支 A 的末端 3 上：只应看到 1、2、3
    let chain_a = ancestor_ids(&nodes, 3);
    let days_a: Vec<i64> = day_ids.iter().copied().filter(|d| chain_a.contains(d)).collect();
    assert_eq!(days_a, vec![1, 2, 3], "分支 A 不应看到 5/6");

    // 站在分支 B 的末端 6 上：只应看到 1、5、6
    let chain_b = ancestor_ids(&nodes, 6);
    let days_b: Vec<i64> = day_ids.iter().copied().filter(|d| chain_b.contains(d)).collect();
    assert_eq!(days_b, vec![1, 5, 6], "分支 B 不应看到 2/3");

    // 对照：若继续用"全表位置"，站在 3 上会把 5、6（排在其后）也算进来 —— 顺序错乱的风险也在这里
    let by_pos_at_3: Vec<i64> = day_ids[..=day_ids.iter().position(|d| *d == 3).unwrap()].to_vec();
    assert_eq!(by_pos_at_3, vec![1, 2, 3]);
    // 反过来，若 5 的 sort_order 更小，就会出现"读到别的分支"
    let nodes2 = vec![n(1, None, 1), n(2, Some(1), 2), n(3, Some(2), 3), n(5, Some(1), 0)];
    let day_ids2: Vec<i64> = vec![5, 1, 2, 3]; // 按 sort_order 排
    let pos5 = day_ids2.iter().position(|d| *d == 5).unwrap();
    let chain_a2 = ancestor_ids(&nodes2, 3);
    assert!(
        !chain_a2.contains(&5),
        "分支 A 的父链不含 5（即便 5 的 sort_order 最小）"
    );
    assert_eq!(pos5, 0, "而按位置取会把 5 当成'最早的天'，这就是必须改按父链的原因");
}

/// 真机验证（默认跳过）：打印 `list_timeline_nodes` 对真实库的真实返回。
/// 跑法：`cargo test --lib live_list_nodes -- --ignored --nocapture`
///
/// 用途：界面"看不到插曲"这类问题时，先确认**后端到底返回了什么** ——
/// 把"数据没返回"和"界面没渲染"两种可能分开，避免靠猜。
#[tokio::test]
#[ignore]
async fn live_list_nodes() {
    let db = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..").join("library").join("default").join("projects").join("7").join("story.db");
    if !db.exists() {
        println!("真实库不存在，跳过：{}", db.display());
        return;
    }
    let nodes = list_timeline_nodes("7").await.unwrap();
    println!("  list_timeline_nodes 返回 {} 个节点：", nodes.len());
    let mut interludes = 0;
    for n in &nodes {
        if n.node_type != "day" {
            interludes += 1;
        }
        println!(
            "    id={:<4} {:<10} type={:<10} parent={:<6} sort={:<4} {}",
            n.id,
            n.date_label,
            n.node_type,
            n.parent_id.map(|x| x.to_string()).unwrap_or("NULL".into()),
            n.sort_order,
            if n.is_flashback != 0 { "[flashback]" } else { "" }
        );
    }
    println!("  其中非 day 节点（插曲/倒叙）：{interludes} 个");
    assert_eq!(nodes.len(), 15, "真实库应有 15 个节点");
    assert!(interludes >= 2, "真实库应有 2 个插曲，实际 {interludes}");
}
