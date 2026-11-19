// 时间戳的统一口径（前端侧）。
//
// 背景（2026-10-18 审计）：写进库/配置里的时间字符串有三种来源：
//   ① SQLite `datetime('now','localtime')`  →  2026-10-18 14:40:02
//   ② Rust `SystemTime + 8h` 手工格式化      →  （已删，见 export.rs 注释）
//   ③ 前端 `new Date().toLocaleString()`     →  2026/10/18 14:40:02（随系统区域变）
// ①②③ 格式不同，同一个库里混着好几种；③ 还受用户区域设置影响，事后无法稳定解析。
//
// 现在统一为 **①的格式**（`YYYY-MM-DD HH:MM:SS`，本地时区），本地时间由 JS 的 Date 取
// （它就是系统本地时区，与 SQLite 的 localtime 一致）。

/** 补零 */
const p2 = (n: number) => String(n).padStart(2, "0");

/**
 * 当前本地时间的标准时间戳：`YYYY-MM-DD HH:MM:SS`
 *
 * 与 SQLite `datetime('now','localtime')` **同格式同口径**，可以放心写进同一个库。
 */
export function nowStamp(d: Date = new Date()): string {
  return (
    `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())} ` +
    `${p2(d.getHours())}:${p2(d.getMinutes())}:${p2(d.getSeconds())}`
  );
}

/** 校验一个字符串是否为标准时间戳格式（导入/兼容旧数据时可用来判断） */
export function isStamp(s: string): boolean {
  return /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(s);
}

/** 日期 → `YYYY-MM-DD`（只到天） */
export function dayStamp(d: Date = new Date()): string {
  return `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}`;
}

/**
 * 首页问候（2026-11-19，按本地时段分 5 档）。
 *
 * 用户参考页的口径，照抄：<5 夜深了 / <11 早上好 / <14 中午好 / <18 下午好 / 其余 晚上好。
 * 纯装饰 —— 首页真正的价值是「继续创作 + 最近编辑」，问候只是让冷启动有一点人味。
 */
export function greetingFor(d: Date = new Date()): string {
  const h = d.getHours();
  if (h < 5) return "夜深了";
  if (h < 11) return "早上好";
  if (h < 14) return "中午好";
  if (h < 18) return "下午好";
  return "晚上好";
}

/**
 * 标准时间戳 → 人话（2026-11-19，首页「最近编辑」用）：`刚刚 / N 分钟前 / 今天 / 昨天 / N 天前 / 上周 / N 周前 / 月日 / 年月日`。
 *
 * - 只认 `YYYY-MM-DD HH:MM:SS`（与 SQLite localtime 同口径）；解析不了 → 空串（界面不显示这段）；
 * - 未来时间（时钟漂移/手改库）按「刚刚」，不显示负数。
 */
export function relativeTime(stamp: string, now: Date = new Date()): string {
  const m = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2}):(\d{2})$/.exec((stamp ?? "").trim());
  if (!m) return "";
  const t = new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]);
  if (Number.isNaN(t.getTime())) return "";
  const diff = Math.max(0, Math.floor((now.getTime() - t.getTime()) / 1000));
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  const that = dayStamp(t);
  if (that === dayStamp(now)) return "今天";
  const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const y = new Date(midnight);
  y.setDate(y.getDate() - 1);
  if (that === dayStamp(y)) return "昨天";
  const days = Math.round(
    (midnight.getTime() - new Date(t.getFullYear(), t.getMonth(), t.getDate()).getTime()) / 86400000,
  );
  if (days < 7) return `${days} 天前`;
  if (days < 14) return "上周";
  if (days < 60) return `${Math.floor(days / 7)} 周前`;
  if (t.getFullYear() === now.getFullYear()) return `${t.getMonth() + 1} 月 ${t.getDate()} 日`;
  return `${t.getFullYear()}-${p2(t.getMonth() + 1)}-${p2(t.getDate())}`;
}
