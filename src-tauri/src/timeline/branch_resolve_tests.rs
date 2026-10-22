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

/// 节点确实挂在某条分支上 → 以节点为准（防止"人在分支、节点建到主线"的老 bug 复发）
#[test]
fn real_branch_node_wins_over_ui() {
    let derived = Some(Some("branch-x".to_string()));
    let ui = Some("branch-y".to_string());
    assert_eq!(
        resolve_branch_for_new_node(derived, ui),
        Some(Some("branch-x".to_string())),
        "节点自己所属的分支优先（界面状态可能过期）"
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
