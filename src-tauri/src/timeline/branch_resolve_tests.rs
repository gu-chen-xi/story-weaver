// 从 `timeline.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `timeline::branch_resolve_tests` —— 因为 `timeline.rs` + `timeline/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `timeline`，一行都不用改写。

use super::resolve_branch_for_new_node;

/// **回归测试（用户实测的 bug）**：新建分支后点「下一天」，节点被建到主线。
///
/// 现场：新分支还没有自己的节点 → 它的"最后一个节点"是分叉点（**共享历史**，
/// `branch_id IS NULL`）→ 旧逻辑把 `Some(None)` 当成"在主线" → 建到主线。
#[test]
fn shared_history_node_must_not_override_ui_branch() {
    let derived = Some(None); // 上一个节点是共享历史（分叉点）
    let ui = Some("branch-b".to_string());
    assert_eq!(
        resolve_branch_for_new_node(derived, ui.clone()),
        Some(Some("branch-b".to_string())),
        "共享历史节点不能把新节点推回主线"
    );
}

/// **界面优先**：嵌套子分支的分叉点属于父分支 —— 节点推导会推成父分支，界面才是对的。
///
/// 现场（用户实测）：在"从6分支的第8天再分一支"里新建第9/10天，结果建到了「从6分支」。
#[test]
fn ui_branch_wins_over_derived_for_nested_branches() {
    let derived = Some(Some("parent-branch".to_string()));   // 分叉点属于父分支
    let ui = Some("child-branch".to_string());               // 用户站在子分支里
    assert_eq!(
        resolve_branch_for_new_node(derived, ui),
        Some(Some("child-branch".to_string())),
        "嵌套子分支：界面分支优先，不能被父分支抢走"
    );
}

/// 界面说主线、但节点确实在某条分支上（界面状态过期）→ 按节点走，
/// 防止"人在分支、节点却建到主线"的老 bug 复发
#[test]
fn derived_branch_still_used_when_ui_says_main() {
    assert_eq!(
        resolve_branch_for_new_node(Some(Some("branch-x".to_string())), None),
        Some(Some("branch-x".to_string()))
    );
}

/// 节点不存在 / 库打不开 → 退回界面分支
#[test]
fn missing_node_falls_back_to_ui() {
    assert_eq!(
        resolve_branch_for_new_node(None, Some("branch-c".to_string())),
        Some(Some("branch-c".to_string()))
    );
}

/// 界面在主线（branchId = None）且节点在共享历史 → 主线（正常情况不能被破坏）
#[test]
fn main_branch_stays_main() {
    assert_eq!(resolve_branch_for_new_node(Some(None), None), Some(None));
}
