// 分支视图的纯逻辑（从 App.tsx 抽出，v3.4）。
//
// 为什么抽这三件事：
//   ① `visibleForBranch` / `branchColor` / `branchLabelOf` 是**纯函数**，和 React 无关，
//      留在组件里既读不进去也测不了；
//   ② `branchPathIds` 的算法踩过两次坑（共享历史被当成"别的分支"跳过、
//      按 branch_id 批量取把兄弟分支卷进来），这种"有历史教训"的逻辑最该能单测；
//   ③ App.tsx 的行数守卫（frontend_guard.rs）本来就要求只许往下压。
//
// 这里只放"输入→输出"的计算，**不碰 React state、不碰 invoke**。

import type { TimelineNodeDto } from "../types";

/** 某个节点是否属于给定分支（`null` = 主线）。 */
export function visibleForBranch(x: TimelineNodeDto, currentBranchId: string | null): boolean {
  return (x.branch_id ?? null) === (currentBranchId ?? null);
}

/** 分支颜色：主线用中性灰，未知分支回退紫色。 */
export function branchColor(
  branches: { id: string; color?: string | null }[],
  bid: string | null | undefined,
): string {
  return bid == null ? "#6B7280" : (branches.find((b) => b.id === bid)?.color ?? "#8B5CF6");
}

/** 分支名：主线 / 分支名 / 未知分支回退成 id 前 8 位。 */
export function branchLabelOf(
  branches: { id: string; name?: string | null }[],
  bid: string | null | undefined,
): string {
  if (bid == null) return "主线";
  return branches.find((b) => b.id === bid)?.name ?? (bid.slice(0, 8) + "…");
}

/**
 * **当前这条线"能看到"的节点 id 集合** = 共享历史（沿父链上溯到分叉点及之前）∪ 本分支自己的节点。
 *
 * 为什么要单独算：时间维只画"这条线"的内容，而**共享历史**与**别的分支**必须区别对待 ——
 * 前者要**逐个画出来**（第 1~11 天是共同经历），后者才收成一行"支线"。
 * 曾经把两者混为一谈 → 共享历史被整段跳成"时间流逝 · 第 2~11 天"（用户实测反馈）。
 *
 * 算法：从"本分支最后一个节点"（没有则分叉点）出发，沿 parent_id 上溯取全路径。
 * **沿 parent_id 逐级上溯**而不是按 branch_id 批量取：每个节点只有一个父，路径唯一，
 * 不会把"同父的兄弟分支"卷进来（按 branch_id 取会）。
 */
export function branchPathIds(
  nodes: TimelineNodeDto[],
  branches: { id: string; fork_node_id?: number | null }[],
  currentBranchId: string | null,
): Set<number> {
  const byId = new Map<number, TimelineNodeDto>();
  nodes.forEach((n) => byId.set(n.id, n));
  const sortKey = (a: TimelineNodeDto, b: TimelineNodeDto) => (a.sort_order - b.sort_order) || (a.id - b.id);

  const out = new Set<number>();
  let startId: number | null = null;

  if (currentBranchId != null) {
    const b = branches.find((x) => x.id === currentBranchId);
    startId = b ? (b.fork_node_id ?? null) : null;
    nodes
      .filter((n) => (n.branch_id ?? null) === currentBranchId)
      .forEach((n) => out.add(n.id));
  } else {
    const m = nodes.filter((n) => n.branch_id == null).sort(sortKey);
    startId = m.length > 0 ? m[m.length - 1].id : null;
  }

  let cur = startId;
  let guard = 0;
  while (cur != null && guard++ < 10000) {
    if (out.has(cur)) break; // 防环
    out.add(cur);
    const node = byId.get(cur);
    cur = node?.parent_id ?? null;
  }
  return out;
}

/** 新建分支时"分叉节点"下拉的分组：主线一组，其余按分支各一组。 */
export type ForkGroup = { key: string; name: string; nodes: TimelineNodeDto[] };

/**
 * 把 `timelineNodes` 按"属于哪条线"分组，给「新建分支 → 分叉节点」下拉用。
 *
 * 为什么必须分组（2026-10-26 用户反馈）：原来是一张平铺清单，标签只有
 * `第 7 天` / `第 7 天（分支内）` —— 存在多条分支时，好几个"第 7 天（分支内）"
 * 长得一模一样，根本不知道自己选的是哪条线上的那天。
 * 现在：主线一组（名字就叫「主线」），每条分支一组（组名 = 分支名），
 * 组内按 `sort_order` 排（= 时间顺序），跨分支不再交错。
 *
 * 找不到归属的节点（`branch_id` 不在 branches 里，比如分支被删了）单独放「未知分支」，
 * 不丢、也不混进主线。
 */
export function forkPickerGroups(
  nodes: TimelineNodeDto[],
  branches: { id: string; name?: string | null }[],
): ForkGroup[] {
  const sortKey = (a: TimelineNodeDto, b: TimelineNodeDto) => (a.sort_order - b.sort_order) || (a.id - b.id);
  const out: ForkGroup[] = [];

  const main = nodes.filter((n) => (n.branch_id ?? null) === null).slice().sort(sortKey);
  if (main.length > 0) out.push({ key: "main", name: "主线", nodes: main });

  for (const b of branches) {
    const own = nodes.filter((n) => n.branch_id === b.id).slice().sort(sortKey);
    if (own.length === 0) continue;
    // 分支名可能是空串（历史数据）—— 空串会让 optgroup 变成一条空白横线，回退成 id 前 8 位
    const label = branchLabelOf(branches, b.id).trim();
    out.push({ key: b.id, name: label || b.id.slice(0, 8) + "…", nodes: own });
  }

  const known = new Set(branches.map((b) => b.id));
  const stray = nodes
    .filter((n) => n.branch_id != null && !known.has(n.branch_id))
    .slice()
    .sort(sortKey);
  if (stray.length > 0) out.push({ key: "unknown", name: "未知分支（分支记录已不在）", nodes: stray });

  return out;
}
