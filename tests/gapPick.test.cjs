// 「⏳ 插入事件」的选段规则（v3.5.7）。
//
// 为什么要单独测：这段逻辑藏在组件里时，只能靠"点一下看弹窗对不对"来验 ——
// 而它的三种分支（落在空档里 / 往后找 / 只剩最后一段）很容易在改 UI 时被改坏。

const test = require("node:test");
const assert = require("node:assert/strict");

const { pickGap } = require("../.tmp/fe-tests/src/lib/timelineDates.js");

const gaps = [
  { from: 2, to: 5 },   // 第 3、4 天空
  { from: 9, to: 12 },  // 第 10、11 天空
];

test("当前天落在空档里 → 就选那段", () => {
  assert.deepEqual(pickGap(gaps, 3), { from: 2, to: 5 });
  assert.deepEqual(pickGap(gaps, 4), { from: 2, to: 5 });
  assert.deepEqual(pickGap(gaps, 10), { from: 9, to: 12 });
});

test("当前天不在空档里 → 往后找第一段（不往回跳）", () => {
  assert.deepEqual(pickGap(gaps, 6), { from: 9, to: 12 });
  assert.deepEqual(pickGap(gaps, 1), { from: 2, to: 5 });
});

test("当前天在最后一段之后 → 只剩最后一段可选", () => {
  assert.deepEqual(pickGap(gaps, 20), { from: 9, to: 12 });
});

test("边界：站在空档起点那天 → 选这段（空档就在今天之后）", () => {
  // 第 2 天**有剧情**，空档是第 3、4 天 —— 就在今天之后，选它最合理
  assert.deepEqual(pickGap(gaps, 2), { from: 2, to: 5 });
});

test("边界：站在空档终点那天 → 往后找（那段已经过去了）", () => {
  // 第 5 天有剧情，第 3、4 天已经过去 → 不回头，选后面的第二段
  assert.deepEqual(pickGap(gaps, 5), { from: 9, to: 12 });
});

test("没有空档 → null（调用方提示用「下一天」）", () => {
  assert.equal(pickGap([], 1), null);
  assert.equal(pickGap([], 0), null);
});
