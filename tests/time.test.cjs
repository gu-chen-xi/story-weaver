// 时间戳口径的单元测试（对应 src/lib/time.ts）
//
// 背景：写进库/配置的时间字符串必须与 SQLite `datetime('now','localtime')` 同格式，
// 否则同一个库里会混着 `2026-10-18 14:40:02` 和 `2026/10/18 14:40:02` 两种。

const test = require("node:test");
const assert = require("node:assert");
const { nowStamp, isStamp, dayStamp } = require("../.tmp/fe-tests/src/lib/time.js");

test("nowStamp：格式是 YYYY-MM-DD HH:MM:SS（与 SQLite localtime 一致）", () => {
  const s = nowStamp();
  assert.match(s, /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/, `格式不对：${s}`);
  assert.equal(s.length, 19);
  assert.ok(isStamp(s), "自己的 isStamp 必须认可自己的输出");
});

test("nowStamp：用的是本地时间字段，不是 UTC", () => {
  // 构造一个本地时间明确的时刻：本地 2026-03-05 07:08:09
  const d = new Date(2026, 2, 5, 7, 8, 9);
  assert.equal(nowStamp(d), "2026-03-05 07:08:09");
});

test("nowStamp：个位数月/日/时/分/秒都要补零", () => {
  const d = new Date(2026, 0, 2, 3, 4, 5);
  assert.equal(nowStamp(d), "2026-01-02 03:04:05");
});

test("isStamp：拒绝 toLocaleString 那种带斜杠的旧格式", () => {
  assert.equal(isStamp("2026/10/18 14:40:02"), false, "旧格式（斜杠）不该被认作标准时间戳");
  assert.equal(isStamp("2026-10-18T14:40:02"), false, "ISO 带 T 的不是标准口径");
  assert.equal(isStamp("2026-10-18 14:40:02"), true);
});

test("dayStamp：只到天，同样补零", () => {
  assert.equal(dayStamp(new Date(2026, 9, 8, 23, 59, 59)), "2026-10-08");
});
