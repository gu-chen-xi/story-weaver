// 时间线的纯日期计算（v3.5.2）。
//
// 从 `hooks/useTimelineView.ts` 拆出来的原因有两个：
//   1. 这三个函数**不依赖 React**，混在 hook 里让它们没法单测
//      （测试只编译 `src/lib`，见 `tsconfig.split.json`）；
//   2. 它们是"按字符串算日子"的逻辑 —— 错了不报错，只会让日历与空档提示错位，
//      必须能用断言盯住（闰年、跨月、跨年、中文写法的时间起点）。

/** "第 3 天" → 3；解析不出来 → 0 */
export function dayNoOf(n: { date_label: string }): number {
  return parseInt(n.date_label.replace(/\D/g, ""), 10) || 0;
}

/** 是不是"占日期的一天"（插曲 / 倒叙不占） */
export function isDayNode(n: { node_type: string }): boolean {
  return n.node_type !== "interlude" && n.node_type !== "flashback";
}

/**
 * 某天在剧情日历上的日期（`MM-DD`）。
 *
 * `time_origin`（YYYY-MM-DD，也接受中文写法）是绝对起点，第 N 天 = 起点 + (N-1) 天。
 * 插曲/倒叙、起点没填/填歪、天数解析不出来 → 返回空串（不猜）。
 */
export function dateDisplayOf(
  n: { node_type: string; date_label: string },
  timeOrigin: string | undefined,
): string {
  if (!isDayNode(n)) return "";
  const origin = (timeOrigin ?? "").trim();
  if (!origin) return "";
  const m = origin.match(/^(\d{4})[-./年](\d{1,2})[-./月](\d{1,2})日?$/);
  if (!m) return "";
  const dayNo = dayNoOf(n);
  if (dayNo <= 0) return "";
  const base = new Date(+m[1], +m[2] - 1, +m[3]);
  base.setDate(base.getDate() + (dayNo - 1));
  return `${String(base.getMonth() + 1).padStart(2, "0")}-${String(base.getDate()).padStart(2, "0")}`;
}
