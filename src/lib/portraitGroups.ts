// 立绘分组的归属计算（2026-10-25）。
//
// 用户定的规则：**一张图可以同时出现在多个分组里**。
//   · 绑定到「夏婉琴」→ 自动属于「夏婉琴」这个组（派生，不落库）；
//   · 用户又把它加进自定义组「女生」→ 它同时在「女生」里（存 group_name，逗号分隔可多个）；
//   · 两个组视图里都会出现它，详情里两个组也都要显示。
//
// 所以"分组"不是一对一，而是：角色组（派生） ∪ 自定义组（存储）。

export type GroupedItem = {
  group: string;
  bound_character?: string | null;
};

/** 自定义分组（group_name 逗号分隔；老数据的单个值照样能解析） */
export function customGroupsOf(m: { group: string }): string[] {
  return (m.group ?? "")
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

/** 这张图实际归属的所有分组：角色组 + 自定义组（去重、保持顺序） */
export function groupsOf(m: GroupedItem): string[] {
  const out: string[] = [];
  const ch = (m.bound_character ?? "").trim();
  if (ch) out.push(ch);
  for (const g of customGroupsOf(m)) {
    if (!out.includes(g)) out.push(g);
  }
  return out;
}

/** 按分组聚合（同一张图会出现在它归属的每个组里）；哪都不属于 → 「未分组」 */
export function groupBuckets<T extends GroupedItem>(items: T[]): { name: string; items: T[] }[] {
  const map = new Map<string, T[]>();
  const push = (name: string, it: T) => {
    const arr = map.get(name);
    if (arr) arr.push(it);
    else map.set(name, [it]);
  };
  for (const it of items) {
    const gs = groupsOf(it);
    if (gs.length === 0) push("未分组", it);
    else for (const g of gs) push(g, it);
  }
  return Array.from(map.entries()).map(([name, list]) => ({ name, items: list }));
}
