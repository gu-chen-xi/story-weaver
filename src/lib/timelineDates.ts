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

/** 一段"无事件"的时间：`from` = 前一个有剧情的天，`to` = 后一个有剧情的天（中间都是空的） */
export type Gap = { from: number; to: number };

/**
 * 由「已有的天数」算出可插入的空档。
 *
 * 2026-11-03 用户实测补的两点：
 *   ① **开头也算空档**：删掉第 1 天后，天数从第 2 天开始 —— 原来只算相邻两天之间，
 *      于是第 1 天永远补不回来（点「插入事件」直接说"时间是连续的"）。
 *      现在补一段 `{ from: 0, to: 第一个天 }`。
 *   ② 末尾**不算**空档：那是「+ 下一天」的事，不归插入。
 */
export function gapsOf(dayNos: number[]): Gap[] {
  const ds = [...new Set(dayNos.filter((d) => Number.isFinite(d) && d > 0))].sort((a, b) => a - b);
  const out: Gap[] = [];
  if (ds.length === 0) return out;
  if (ds[0] > 1) out.push({ from: 0, to: ds[0] });
  for (let i = 0; i + 1 < ds.length; i++) {
    if (ds[i + 1] - ds[i] > 1) out.push({ from: ds[i], to: ds[i + 1] });
  }
  return out;
}

/**
 * 挑一段"要插事件"的空档（v3.5.7，「⏳ 插入事件」按钮的选段规则）。
 *
 * 规则（用户视角的直觉顺序）：
 *   1. 当前这天**落在空档里**（`from < curDay < to`）→ 就这段
 *      ——"我在第 5 天，想补第 5 天的事"最合理；
 *   2. 否则取**当前天之后的第一段**（`from >= curDay`）—— 往前走，别往回跳。
 *      边界：站在这段的 `from` 那天（空档就在今天之后）→ 命中这段；
 *            站在 `to` 那天（空档已经过去）→ 不算，继续往后找；
 *   3. 否则取**最后一段**（唯一的选择）。
 *
 * 没有空档 → `null`（调用方据此提示"时间是连续的，用「+ 下一天」"）。
 */
export function pickGap(gaps: Gap[], curDay: number): Gap | null {
  if (gaps.length === 0) return null;
  const inside = gaps.find((g) => curDay > g.from && curDay < g.to);
  if (inside) return inside;
  const after = gaps.find((g) => g.from >= curDay);
  if (after) return after;
  return gaps[gaps.length - 1];
}
