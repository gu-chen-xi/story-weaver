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
