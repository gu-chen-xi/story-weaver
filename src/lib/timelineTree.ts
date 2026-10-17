// 时间线树的"扁平化"纯函数（节点树 → 带缩进深度的一串行）。
//
// 从 App.tsx 抽出（2026-10-17）：原来 App 里有两份几乎相同的遍历代码
// （`treeRows` 折叠感知、`treeRowsAll` 折叠无关），结果**树状图表格误用了折叠无关那份**，
// 导致"点了收起，树状图不动，倒是上面的时间维区间条动了"。
//
// 现在只留一个函数，用一个参数区分两种用法：
//   `collapsed = Set`  → 折叠感知（树状图用：收起某节点后其子孙不出现）
//   `collapsed = null` → 折叠无关（时间维区间条用：始终显示整条时间线，否则用户会丢日期按钮）

import type { TimelineNodeDto } from "../types";

export type TreeNodeRow = {
  n: TimelineNodeDto;
  depth: number;
  hasKids: boolean;
};

/// 把节点树深度优先展平成行。
///
/// 约定：
/// * 父节点不存在的节点（老库未迁移 / 父被删）**当作根**，避免节点凭空消失；
/// * 用 `seen` 防环（脏数据里 parent_id 可能成环）；
/// * 同级按 `sort_order, id` 排序，保证顺序稳定。
export function buildTreeRows(
  nodes: TimelineNodeDto[],
  collapsed: ReadonlySet<number> | null,
): TreeNodeRow[] {
  const byId = new Map<number, TimelineNodeDto>();
  nodes.forEach((x) => byId.set(x.id, x));

  const kids = new Map<number | null, TimelineNodeDto[]>();
  nodes.forEach((x) => {
    const pid = x.parent_id != null && byId.has(x.parent_id) ? x.parent_id : null;
    if (!kids.has(pid)) kids.set(pid, []);
    kids.get(pid)!.push(x);
  });
  kids.forEach((arr) => arr.sort((a, b) => a.sort_order - b.sort_order || a.id - b.id));

  const rows: TreeNodeRow[] = [];
  const seen = new Set<number>();
  const stack: { n: TimelineNodeDto; depth: number }[] = [];

  // 起点：正常是"没有父节点的那些"。
  // **兜底**：如果没有根却确实有节点，说明数据坏了（例如 parent_id 成环）。
  // 这时把所有节点当根 —— 否则整棵树渲染成空白，比显示得不对更难排查。
  let roots = kids.get(null) ?? [];
  if (roots.length === 0 && nodes.length > 0) {
    roots = nodes.slice().sort((a, b) => a.sort_order - b.sort_order || a.id - b.id);
  }
  // 起点逆序入栈，保证弹出顺序与 sort_order 一致
  roots.slice().reverse().forEach((x) => stack.push({ n: x, depth: 0 }));

  while (stack.length) {
    const { n, depth } = stack.pop()!;
    if (seen.has(n.id)) continue; // 防环
    seen.add(n.id);
    const children = kids.get(n.id) ?? [];
    rows.push({ n, depth, hasKids: children.length > 0 });
    // 只有"被显式收起"的节点才不展开子孙；collapsed=null 时一律展开
    const isCollapsed = collapsed !== null && collapsed.has(n.id);
    if (!isCollapsed) {
      children.slice().reverse().forEach((c) => stack.push({ n: c, depth: depth + 1 }));
    }
  }
  return rows;
}
