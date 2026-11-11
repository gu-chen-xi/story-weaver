// 「按配对静默」的前端过滤（2026-11-11，ROADMAP §七 选项 A）。
//
// 演进分析本来就只读"这一轮"，天然是这一对的事；A 这一档只做**归属**：
// 关系提案要落在这段关系上才给看。
//
// 为什么不用改表：关系提案的 `target_id` 就是 `relations.id`，
// 关系表一行一个方向、两端是**角色 id** —— 配对端点直接可查，
// 不需要给 `change_proposals` 加列（也就没有迁移）。
//
// 其它类型（新增角色 / 地点 / 术语 / 世界观条目）不按对子收：新东西是**世界**的事，
// 不属于某一段关系，一律照常显示。

/** 这条关系是不是「这两个人之间」的（顺序无关） */
export function relationTouchesPair(
  rel: { from: number; to: number },
  aiId: number | null,
  myId: number | null,
): boolean {
  if (aiId === null || myId === null) return true; // 「自由」/ 没指定 → 不筛（宁多不少）
  return (rel.from === aiId && rel.to === myId) || (rel.from === myId && rel.to === aiId);
}

/**
 * 一条提案在当前配对下要不要显示。
 *
 * - 关系类：`relations` 里找不到 target（脏数据 / 已删）→ 显示（宁可多一条也别吞）；
 *           找得到 → 看两端是不是这一对。
 * - 其它类型：一律显示。
 */
export function proposalVisibleInPair(
  targetType: string,
  targetId: number,
  relations: { id: number; from: number; to: number }[],
  aiId: number | null,
  myId: number | null,
): boolean {
  if (targetType !== "relation") return true;
  const rel = relations.find((r) => r.id === targetId);
  if (!rel) return true;
  return relationTouchesPair(rel, aiId, myId);
}
