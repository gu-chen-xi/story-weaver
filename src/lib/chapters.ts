// 章节「只读聚合」的纯逻辑（2026-10-30）。
//
// 为什么要单独抽出来：按钮传参最容易出错（多选节点、顺序、去重、禁用条件），
// 而这些全是可以离线测的 —— 放进 lib 就能被 `npm test` 覆盖，不用起界面。

export type NodeLike = { id: number; date_label: string };

/// 多选：点一下加入/移出，**保持点击顺序**（不乱排，方便作者按故事顺序圈）
export function toggleNodeId(selected: number[], id: number): number[] {
  if (selected.includes(id)) return selected.filter((x) => x !== id);
  return [...selected, id];
}

/// 选中摘要：给按钮和提示用（「已选 3 天：第 1 天、第 2 天、第 4 天」）
export function selectionSummary(selected: number[], nodes: NodeLike[]): { count: number; labels: string[]; text: string } {
  const byId = new Map(nodes.map((n) => [n.id, n.date_label]));
  const labels = selected.map((id) => byId.get(id) ?? `#${id}`);
  const text = labels.length ? `已选 ${labels.length} 天：${labels.join("、")}` : "还没选天";
  return { count: labels.length, labels, text };
}

/// 创建按钮的可用条件：标题非空 + 至少选一天（前端先拦，后端仍会校验）
export function canCreateChapter(title: string, selected: number[]): { ok: boolean; reason: string } {
  if (!title.trim()) return { ok: false, reason: "先给这一章起个名字" };
  if (selected.length === 0) return { ok: false, reason: "至少选一天" };
  return { ok: true, reason: "" };
}

/// 判断某个天是否已被别的章占用（多对多允许重复，但要提醒作者）
export function ownerChapterOf(nodeId: number, chapters: { order_no: number; node_ids: number[] }[]): number | null {
  for (const c of chapters) if (c.node_ids.includes(nodeId)) return c.order_no;
  return null;
}

/// 字数显示：≥10000 用「万」
export function wordsLabel(n: number): string {
  if (n >= 10000) return `${(n / 10000).toFixed(2)} 万字`;
  return `${n} 字`;
}
