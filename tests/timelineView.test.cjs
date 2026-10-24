// `hooks/useTimelineView.ts` 里三个纯函数的单测（npm test）。
//
// 为什么测：这三个都是"按字符串算日子"，错了不会报错，只会让日历和空档提示错位
// （比如"第 3 天"被算成第 33 天、或者插曲被当成一天算进空档）。

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  dayNoOf,
  isDayNode,
  dateDisplayOf,
} = require("../.tmp/fe-tests/src/lib/timelineDates.js");

test("dayNoOf：从「第 N 天」取数字，取不到给 0", () => {
  assert.equal(dayNoOf({ date_label: "第 3 天" }), 3);
  assert.equal(dayNoOf({ date_label: "第 12 天" }), 12);
  assert.equal(dayNoOf({ date_label: "插曲 2" }), 2, "插曲里的数字也会被取到（调用方先过滤类型）");
  assert.equal(dayNoOf({ date_label: "回忆·那年夏天" }), 0);
  assert.equal(dayNoOf({ date_label: "" }), 0);
});

test("isDayNode：插曲 / 倒叙不算一天", () => {
  assert.equal(isDayNode({ node_type: "day" }), true);
  assert.equal(isDayNode({ node_type: "interlude" }), false);
  assert.equal(isDayNode({ node_type: "flashback" }), false);
});

test("dateDisplayOf：第 N 天 = 起点 + (N-1) 天，输出 MM-DD", () => {
  const origin = "2026-10-28";
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "第 1 天" }, origin), "10-28");
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "第 2 天" }, origin), "10-29");
  // 跨月
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "第 5 天" }, origin), "11-01");
  // 中文写法的时间起点也认
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "第 1 天" }, "2026年10月28日"), "10-28");
});

test("dateDisplayOf：不该算的日子一律返回空串（不猜）", () => {
  assert.equal(dateDisplayOf({ node_type: "interlude", date_label: "第 2 天" }, "2026-10-28"), "");
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "第 2 天" }, ""), "");
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "第 2 天" }, undefined), "");
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "第 2 天" }, "起点未知"), "");
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "回忆" }, "2026-10-28"), "");
});

test("dateDisplayOf：闰年/年末边界不出错", () => {
  // 2024-02-28 + 2 天 = 2024-03-01（2024 是闰年，2/29 存在）
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "第 3 天" }, "2024-02-28"), "03-01");
  // 2026-12-31 + 1 天 = 2027-01-01
  assert.equal(dateDisplayOf({ node_type: "day", date_label: "第 2 天" }, "2026-12-31"), "01-01");
});
