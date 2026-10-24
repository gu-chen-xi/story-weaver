// 时间线「段框树」的纯布局层（v3.5.1）。
//
// 设计依据：用户 2026-10-24 提供的《织梦机-时间线可视化前端实现建议》。
// 注意：同目录的 `timelineTree.ts` 是**旧的**"扁平化成行"（现有树状图用），
// 两者不是一回事，别合并 —— 旧的是列表式缩进，这个是真的二维布局。
//
// ## 只做一件事：数据 → 坐标
//
// 文档最硬的一条约定：**框的宽高必须在渲染之前由数据算出来**，不要渲染后测 DOM。
// 所以这里不碰 React / DOM，只输出 `{ boxes, edges, width, height }`，
// 组件照绝对定位画。好处是段划分的四条边界都能用断言锁住。
//
// ## 四条必须显式处理的边界（文档 §三）
//
// 1. **长段**：点阵封顶 `DOT_CAP`，超出用 `⋯` 省略中段；框宽与节点数解耦；
// 2. **空段**（0 节点）：渲染成占位框，**不能过滤掉**（否则用户以为分支没建成，会重复点）；
// 3. **范围倒挂**：顺序按父链，标签取段首/段尾显示名，允许"第 20 天—第 9 天"；
// 4. **分叉点归属**：归入所在段**末尾**用 ★ 标记，不单独成框。
//
// 另一条容易画错：**主线在分叉点之后继续写的部分，与各分支是并列兄弟，不是父子。**

import type { BranchDto, TimelineNodeDto } from "../types";

/** 点阵上限：超过就不再加点（文档：框宽与节点数解耦） */
export const DOT_CAP = 10;

export type SegNode = {
  id: number;
  /** 第几天；解析不出来时为 -1 */
  day: number;
  label: string;
  /** 插曲 / 倒叙（不等于"当天正片"） */
  isSide: boolean;
  /** 本节点是分叉点 */
  isFork: boolean;
};

export type Segment = {
  key: string;
  lineName: string;
  branchId: string | null;
  color: string;
  nodes: SegNode[];
  /** 0 节点占位 */
  empty: boolean;
  /** "第 1 天—第 7 天"（允许倒挂） */
  rangeLabel: string;
  hasFork: boolean;
  isCurrent: boolean;
  depth: number;
  children: Segment[];
};

export type Box = {
  key: string;
  seg: Segment;
  x: number;
  y: number;
  w: number;
  h: number;
  /** 点阵字符：● 节点 / ★ 分叉点 / ⋯ 省略 */
  dots: string[];
};

export type Edge = {
  fromKey: string;
  toKey: string;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  color: string;
};

export type Layout = { boxes: Box[]; edges: Edge[]; width: number; height: number };

export type LayoutOpts = {
  boxW: number;
  rowH: number;
  padY: number;
  gapX: number;
  gapY: number;
  expanded: ReadonlySet<string>;
};

/** "第 7 天" → 7；解析不出来返回 -1（不猜） */
export function dayOfLabel(label: string): number {
  const m = label.match(/第\s*(\d+)\s*天/);
  return m ? Number(m[1]) : -1;
}

/**
 * 节点 → 段树。
 *
 * * 从根沿"唯一后继"往下走，遇**多后继**（分叉点）就收在本段末尾；
 * * 分叉点的每个后继各开一段 —— 主线续写也是后继之一，所以与分支**并列**；
 * * 分支已建但全树没有它的节点 → 在它的分叉点所在段挂一个**空段占位**。
 */
export function buildSegments(
  nodes: TimelineNodeDto[],
  branches: BranchDto[],
  currentBranchId: string | null,
): Segment | null {
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

  // 根：没有父节点的节点里取最早的一个（正常只有一个）。
  // 兜底：若完全没有根（脏数据成环），把最早的节点当根 —— 别渲染成空白。
  const roots = nodes
    .filter((n) => n.parent_id == null || !byId.has(n.parent_id))
    .sort((a, b) => a.sort_order - b.sort_order || a.id - b.id);
  const rootNode = roots[0] ?? nodes.slice().sort((a, b) => a.id - b.id)[0];
  if (!rootNode) return null;

  const lineNameOf = (bid: string | null) =>
    bid == null ? "主线" : branchById.get(bid)?.name ?? `分支 ${bid.slice(0, 6)}`;
  const colorOf = (bid: string | null) =>
    bid == null ? "#6B7280" : branchById.get(bid)?.color ?? "#8B5CF6";

  let seq = 0;
  const mkSeg = (branchId: string | null): Segment => {
    seq += 1;
    return {
      key: `s${seq}`,
      lineName: lineNameOf(branchId),
      branchId,
      color: colorOf(branchId),
      nodes: [],
      empty: false,
      rangeLabel: "",
      hasFork: false,
      isCurrent: branchId === (currentBranchId ?? null),
      depth: 0,
      children: [],
    };
  };

  const segNodeOf = (n: TimelineNodeDto): SegNode => ({
    id: n.id,
    day: dayOfLabel(n.date_label),
    label: n.date_label,
    isSide: n.node_type !== "day" || n.is_flashback === 1,
    isFork: (kids.get(n.id) ?? []).length > 1,
  });

  const visited = new Set<number>();
  const walk = (start: TimelineNodeDto, seg: Segment) => {
    let cur: TimelineNodeDto | undefined = start;
    let guard = 0;
    while (cur && guard++ < 100000) {
      if (visited.has(cur.id)) break; // 防环
      visited.add(cur.id);
      seg.nodes.push(segNodeOf(cur));
      const children: TimelineNodeDto[] = kids.get(cur.id) ?? [];
      if (children.length > 1) {
        seg.hasFork = true;
        for (const c of children) {
          const child = mkSeg(c.branch_id ?? null);
          child.depth = seg.depth + 1;
          walk(c, child);
          seg.children.push(child);
        }
        return;
      }
      cur = children[0];
    }
  };

  const root = mkSeg(rootNode.branch_id ?? null);
  root.depth = 0;
  walk(rootNode, root);

  // 空段占位：分支存在，但全树里没有它的节点 → 挂在它的分叉点所在段上
  const usedBranches = new Set<string>();
  nodes.forEach((n) => { if (n.branch_id) usedBranches.add(n.branch_id); });
  const pending = branches.filter((b) => !usedBranches.has(b.id));
  const attachEmpty = (seg: Segment) => {
    for (const b of pending) {
      if (usedBranches.has(b.id)) continue;
      if (seg.nodes.some((n) => n.id === b.fork_node_id)) {
        const s = mkSeg(b.id);
        s.depth = seg.depth + 1;
        s.empty = true;
        s.rangeLabel = "尚未开始";
        seg.children.push(s);
        usedBranches.add(b.id);
      }
    }
    seg.children.forEach(attachEmpty);
  };
  attachEmpty(root);

  // 范围标签（允许倒挂：文档明确要求"不要修正"）
  const fillRanges = (seg: Segment) => {
    if (!seg.empty && seg.nodes.length > 0) {
      const a = seg.nodes[0];
      const b = seg.nodes[seg.nodes.length - 1];
      const la = a.day >= 0 ? `第 ${a.day} 天` : a.label;
      const lb = b.day >= 0 ? `第 ${b.day} 天` : b.label;
      seg.rangeLabel = seg.nodes.length === 1 ? la : `${la}—${lb}`;
    }
    seg.children.forEach(fillRanges);
  };
  fillRanges(root);

  return root;
}

/** 点阵：超过上限就前 5 + ⋯ + 后 4（框宽稳定） */
export function dotsOf(nodes: SegNode[]): string[] {
  const ch = (n: SegNode) => (n.isFork ? "★" : "●");
  if (nodes.length <= DOT_CAP) return nodes.map(ch);
  return [...nodes.slice(0, 5).map(ch), "⋯", ...nodes.slice(-4).map(ch)];
}

/** 段框行数：基准 3 行 + 分叉 +1 + 当前 +1（文档 §二）；展开时加节点行 */
export function rowsOf(seg: Segment, expanded: ReadonlySet<string>): number {
  let rows = 3;
  if (seg.hasFork) rows += 1;
  if (seg.isCurrent) rows += 1;
  if (expanded.has(seg.key) && !seg.empty) rows += Math.min(seg.nodes.length, 30);
  return rows;
}

/**
 * 坐标（从上往下的 tidy tree）。
 *
 * 两个关键点：
 * * 兄弟**按前一个框的实际宽度 + gapX 累加**（文档：不能按固定槽位分配，框宽不等会重叠）；
 * * 每层高度取该层最高框 → 层与层对齐，展开态也不会压到下一层。
 */
export function layoutTree(root: Segment, opts: LayoutOpts): Layout {
  // ① 先算每个框的高度与层号
  type Draft = { seg: Segment; h: number };
  const byDepth = new Map<number, Draft[]>();
  const walkDraft = (seg: Segment) => {
    const h = opts.padY * 2 + rowsOf(seg, opts.expanded) * opts.rowH;
    const arr = byDepth.get(seg.depth) ?? [];
    arr.push({ seg, h });
    byDepth.set(seg.depth, arr);
    seg.children.forEach(walkDraft);
  };
  walkDraft(root);

  // ② 每层 y：上面所有层的高度 + 间隙
  const maxDepth = Math.max(...byDepth.keys());
  const layerY = new Map<number, number>();
  let y = 0;
  for (let d = 0; d <= maxDepth; d++) {
    layerY.set(d, y);
    const maxH = Math.max(...(byDepth.get(d) ?? [{ h: 0 }]).map((x) => x.h));
    y += maxH + opts.gapY;
  }

  // ③ x：叶子顺序占位，父居中于子
  const boxes: Box[] = [];
  let cursorX = 0;
  const place = (seg: Segment): Box => {
    const h = opts.padY * 2 + rowsOf(seg, opts.expanded) * opts.rowH;
    let x: number;
    if (seg.children.length === 0) {
      x = cursorX;
      cursorX += opts.boxW + opts.gapX;
    } else {
      const placed = seg.children.map(place);
      x = (placed[0].x + placed[placed.length - 1].x) / 2;
    }
    const box: Box = {
      key: seg.key,
      seg,
      x,
      y: layerY.get(seg.depth) ?? 0,
      w: opts.boxW,
      h,
      dots: dotsOf(seg.nodes),
    };
    boxes.push(box);
    return box;
  };
  place(root);

  // ④ 归一化到正坐标
  const minX = Math.min(...boxes.map((b) => b.x));
  boxes.forEach((b) => { b.x -= minX; });

  // ⑤ 连线：父底边中点 → 子顶边中点
  const byKey = new Map(boxes.map((b) => [b.key, b]));
  const edges: Edge[] = [];
  const link = (seg: Segment) => {
    const parent = byKey.get(seg.key);
    for (const c of seg.children) {
      const child = byKey.get(c.key);
      if (parent && child) {
        edges.push({
          fromKey: parent.key,
          toKey: child.key,
          x1: parent.x + parent.w / 2,
          y1: parent.y + parent.h,
          x2: child.x + child.w / 2,
          y2: child.y,
          color: c.color,
        });
      }
      link(c);
    }
  };
  link(root);

  const width = Math.max(...boxes.map((b) => b.x + b.w)) + 4;
  const height = Math.max(...boxes.map((b) => b.y + b.h)) + 4;
  return { boxes, edges, width, height };
}

/** 点阵的文字说明（无障碍：●★ 是纯图形，读屏读不出来） */
export function dotsAria(seg: Segment): string {
  if (seg.empty) return "这个分支还没有节点";
  const first = seg.nodes[0];
  const last = seg.nodes[seg.nodes.length - 1];
  const fork = seg.hasFork ? "，末尾为分叉点" : "";
  return `${seg.rangeLabel}，共 ${seg.nodes.length} 个节点${fork}` +
    (first && last && first.day > last.day ? "（含倒叙/插曲，范围倒挂属正常）" : "");
}
