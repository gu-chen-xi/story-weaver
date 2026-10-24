// 逐日节点树的布局（v3.5.2，按用户提供的三张参考图）。
//
// 参考图二「全展开」是另一种呈现：**每个节点一个小药丸**，分叉点加粗成黑点、
// 分支起点用该分支的颜色并加线名标签。它和段框（图一）共享同一套坐标算法，
// 只是"一个单位"从"一段"变成了"一个节点"。
//
// 所以这里把坐标算法抽成通用的 `layoutUnits`，段框与逐日各自提供单位数组。

import type { BranchDto, TimelineNodeDto } from "../types";
import { dayOfLabel } from "./timelineSegments";

/** 一个可布局单位（= 一个段框 / 一个节点药丸） */
export type Unit = {
  key: string;
  depth: number;
  w: number;
  h: number;
  children: Unit[];
};

export type Placed = { key: string; x: number; y: number; w: number; h: number };

export type PlacedEdge = { fromKey: string; toKey: string; x1: number; y1: number; x2: number; y2: number };

export type UnitLayout = { placed: Placed[]; edges: PlacedEdge[]; width: number; height: number };

/**
 * 通用 tidy-tree 布局（从上往下）。
 *
 * 与 `timelineSegments.layoutTree` 同一套规则：
 *   * 叶子按"前面的框实际宽度 + gapX"顺序占位（框宽不等也不重叠）；
 *   * 父居中于子；
 *   * 每层高度取该层最高单位 → 层间对齐、展开也不压下一层。
 */
export function layoutUnits(root: Unit, gapX: number, gapY: number): UnitLayout {
  const byDepth = new Map<number, Unit[]>();
  const collect = (u: Unit) => {
    const arr = byDepth.get(u.depth) ?? [];
    arr.push(u);
    byDepth.set(u.depth, arr);
    u.children.forEach(collect);
  };
  collect(root);

  const maxDepth = Math.max(...byDepth.keys());
  const layerY = new Map<number, number>();
  let y = 0;
  for (let d = 0; d <= maxDepth; d++) {
    layerY.set(d, y);
    const maxH = Math.max(...(byDepth.get(d) ?? [{ h: 0 } as Unit]).map((u) => u.h));
    y += maxH + gapY;
  }

  const placed: Placed[] = [];
  let cursorX = 0;
  const place = (u: Unit): Placed => {
    let x: number;
    if (u.children.length === 0) {
      x = cursorX;
      cursorX += u.w + gapX;
    } else {
      const kids = u.children.map(place);
      // 按**子框中心**居中（不是按左边缘）：宽度不等时左边缘中点会偏心
      const firstC = kids[0].x + kids[0].w / 2;
      const lastC = kids[kids.length - 1].x + kids[kids.length - 1].w / 2;
      x = (firstC + lastC) / 2 - u.w / 2;
    }
    const p: Placed = { key: u.key, x, y: layerY.get(u.depth) ?? 0, w: u.w, h: u.h };
    placed.push(p);
    return p;
  };
  place(root);

  const minX = Math.min(...placed.map((p) => p.x));
  placed.forEach((p) => { p.x -= minX; });

  const byKey = new Map(placed.map((p) => [p.key, p]));
  const edges: PlacedEdge[] = [];
  const link = (u: Unit) => {
    const parent = byKey.get(u.key);
    for (const c of u.children) {
      const child = byKey.get(c.key);
      if (parent && child) {
        edges.push({
          fromKey: parent.key,
          toKey: child.key,
          x1: parent.x + parent.w / 2,
          y1: parent.y + parent.h,
          x2: child.x + child.w / 2,
          y2: child.y,
        });
      }
      link(c);
    }
  };
  link(root);

  const width = Math.max(...placed.map((p) => p.x + p.w)) + 4;
  const height = Math.max(...placed.map((p) => p.y + p.h)) + 4;
  return { placed, edges, width, height };
}

// ── 逐日节点树 ──

export type DayNode = {
  id: number;
  label: string;
  day: number;
  /** 插曲 / 倒叙 */
  isSide: boolean;
  /** 分叉点（多个后继） */
  isFork: boolean;
  /** 属于哪条线（null = 主线） */
  branchId: string | null;
  color: string;
  lineName: string;
  /** 这条线的第一个节点（图上要标"分支A · 活着"这种标签） */
  isLineStart: boolean;
  /** 当前所在节点 */
  isCurrent: boolean;
  /** 直接后继（树结构，供渲染递归用） */
  children: DayNode[];
};

/** 药丸宽度：按字数估（不测 DOM —— 与文档约定一致） */
export function pillWidth(label: string, isFork: boolean): number {
  const n = Array.from(label).length;
  return Math.max(44, 16 + n * 8) + (isFork ? 14 : 0);
}

/** 当前节点的 id 由调用方给（它可能不在本树里，则不高亮） */
export function buildDayTree(
  nodes: TimelineNodeDto[],
  branches: BranchDto[],
  _currentBranchId: string | null,   // 逐日模式下"当前线"由节点高亮体现，这里保留签名以便扩展
  currentNodeId: number | null,
): DayNode | null {
  if (nodes.length === 0) return null;

  const byId = new Map<number, TimelineNodeDto>();
  nodes.forEach((n) => byId.set(n.id, n));
  const kids = new Map<number, TimelineNodeDto[]>();
  nodes.forEach((n) => {
    if (n.parent_id == null || !byId.has(n.parent_id)) return;
    const arr = kids.get(n.parent_id) ?? [];
    arr.push(n);
    kids.set(n.parent_id, arr);
  });
  kids.forEach((arr) => arr.sort((a, b) => a.sort_order - b.sort_order || a.id - b.id));

  const branchById = new Map<string, BranchDto>();
  branches.forEach((b) => branchById.set(b.id, b));

  const roots = nodes
    .filter((n) => n.parent_id == null || !byId.has(n.parent_id))
    .sort((a, b) => a.sort_order - b.sort_order || a.id - b.id);
  const rootNode = roots[0] ?? nodes.slice().sort((a, b) => a.id - b.id)[0];
  if (!rootNode) return null;

  const seen = new Set<number>();

  /** 递归建树；`lineStart` 标记"这条线的第一个节点" */
  const build = (n: TimelineNodeDto, lineStart: boolean): DayNode => {
    seen.add(n.id);
    const bid = n.branch_id ?? null;
    const b = bid ? branchById.get(bid) : null;
    const color = bid == null ? "#cfd6e4" : b?.color ?? "#8B5CF6";
    const children: DayNode[] = [];
    for (const c of kids.get(n.id) ?? []) {
      if (seen.has(c.id)) continue;      // 防环
      children.push(build(c, (c.branch_id ?? null) !== bid));
    }
    return {
      id: n.id,
      label: n.date_label,
      day: dayOfLabel(n.date_label),
      isSide: n.node_type !== "day" || n.is_flashback === 1,
      isFork: (kids.get(n.id) ?? []).length > 1,
      branchId: bid,
      color,
      lineName: lineNameOf(bid, b),
      isLineStart: lineStart,
      isCurrent: currentNodeId != null && n.id === currentNodeId,
      children,
    };
  };

  return build(rootNode, true);
}

/** 深搜展平成"渲染顺序"（顺序与树一致，方便上层遍历） */
export function flattenDays(root: DayNode): DayNode[] {
  const out: DayNode[] = [];
  const walk = (n: DayNode) => { out.push(n); n.children.forEach(walk); };
  walk(root);
  return out;
}

/** 节点树 → 布局单位（药丸尺寸） */
export function dayTreeToUnits(root: DayNode, pillH: number, depthOf: (n: DayNode) => number): Unit {
  const mk = (n: DayNode): Unit => ({
    key: `d${n.id}`,
    depth: depthOf(n),
    w: pillWidth(n.label, n.isFork),
    h: pillH + (n.isLineStart ? 16 : 0) + (n.isFork ? 6 : 0),
    children: n.children.map(mk),
  });
  return mk(root);
}
/// 线名：主线 / 分支名（带备注，参考图上写作「分支A · 活着」这种）
function lineNameOf(bid: string | null, b: BranchDto | null | undefined): string {
  if (bid == null) return "主线";
  const name = b?.name ?? `分支 ${bid.slice(0, 6)}`;
  const note = (b?.note ?? "").trim();
  return note ? `${name} · ${note}` : name;
}