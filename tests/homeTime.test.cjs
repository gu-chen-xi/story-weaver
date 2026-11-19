// 「首页」用的两个纯函数（2026-11-19）：问候档位 + 相对时间。
//
// 都是 `src/lib/time.ts` 里的纯函数，不碰 DOM；这里锁边界值，免得"23:59 显示早上好"
// 或"昨天写成 1 天前"这类小毛病上线。

const test = require("node:test");
const assert = require("node:assert/strict");

const { greetingFor, relativeTime } = require("../.tmp/fe-tests/src/lib/time.js");

const at = (y, mo, d, h = 12, mi = 0, s = 0) => new Date(y, mo - 1, d, h, mi, s);

test("greetingFor：5 档的边界都对", () => {
  assert.equal(greetingFor(at(2026, 11, 19, 4, 59)), "夜深了");
  assert.equal(greetingFor(at(2026, 11, 19, 5, 0)), "早上好");
  assert.equal(greetingFor(at(2026, 11, 19, 10, 59)), "早上好");
  assert.equal(greetingFor(at(2026, 11, 19, 11, 0)), "中午好");
  assert.equal(greetingFor(at(2026, 11, 19, 13, 59)), "中午好");
  assert.equal(greetingFor(at(2026, 11, 19, 14, 0)), "下午好");
  assert.equal(greetingFor(at(2026, 11, 19, 17, 59)), "下午好");
  assert.equal(greetingFor(at(2026, 11, 19, 18, 0)), "晚上好");
  assert.equal(greetingFor(at(2026, 11, 19, 23, 59)), "晚上好");
});

test("relativeTime：一分钟内 = 刚刚", () => {
  const now = at(2026, 11, 19, 20, 0, 30);
  assert.equal(relativeTime("2026-11-19 20:00:00", now), "刚刚");
  assert.equal(relativeTime("2026-11-19 20:00:30", now), "刚刚");
});

test("relativeTime：一小时内 = N 分钟前", () => {
  const now = at(2026, 11, 19, 20, 40, 0);
  assert.equal(relativeTime("2026-11-19 20:05:00", now), "35 分钟前");
});

test("relativeTime：同一天 = 今天（超过 1 小时才走这条）", () => {
  const now = at(2026, 11, 19, 20, 0, 0);
  assert.equal(relativeTime("2026-11-19 08:00:00", now), "今天");
});

test("relativeTime：昨天不等于 1 天前", () => {
  const now = at(2026, 11, 19, 9, 0, 0);
  assert.equal(relativeTime("2026-11-18 22:00:00", now), "昨天");
});

test("relativeTime：2~6 天前按天数，7~13 天写上周，再久按周/日期", () => {
  const now = at(2026, 11, 19, 12, 0, 0);
  assert.equal(relativeTime("2026-11-17 12:00:00", now), "2 天前");
  assert.equal(relativeTime("2026-11-13 12:00:00", now), "6 天前");
  assert.equal(relativeTime("2026-11-12 12:00:00", now), "上周");
  assert.equal(relativeTime("2026-10-29 12:00:00", now), "3 周前");
  assert.equal(relativeTime("2026-08-01 12:00:00", now), "8 月 1 日");
  assert.equal(relativeTime("2025-12-31 12:00:00", now), "2025-12-31");
});

test("relativeTime：未来时间按刚刚（不显示负数）", () => {
  const now = at(2026, 11, 19, 12, 0, 0);
  assert.equal(relativeTime("2026-11-20 12:00:00", now), "刚刚");
});

test("relativeTime：不是标准时间戳 → 空串（界面据此不显示）", () => {
  assert.equal(relativeTime("", at(2026, 11, 19)), "");
  assert.equal(relativeTime("2026/11/19 12:00:00", at(2026, 11, 19)), "");
  assert.equal(relativeTime("2026-11-19T12:00:00", at(2026, 11, 19)), "");
});
