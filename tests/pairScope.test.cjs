// 「按配对静默」过滤（2026-11-11）。
//
// 关系提案的 target_id 就是 relations.id，两端是角色 id —— 所以不用改表，
// 只在界面上按"这一对"筛：她俩之间的事留下，和别人之间的事不掺进来。

const test = require("node:test");
const assert = require("node:assert/strict");
const { relationTouchesPair, proposalVisibleInPair } = require("../.tmp/fe-tests/src/lib/pairScope.js");

const RELS = [
  { id: 1, from: 10, to: 20 }, // 夏 → 顾
  { id: 2, from: 20, to: 10 }, // 顾 → 夏
  { id: 3, from: 10, to: 30 }, // 夏 → 王
];

test("端点命中，顺序无关", () => {
  assert.equal(relationTouchesPair(RELS[0], 10, 20), true);
  assert.equal(relationTouchesPair(RELS[0], 20, 10), true, "互换也算同一对");
  assert.equal(relationTouchesPair(RELS[2], 10, 20), false, "换了对手就不是");
});

test("没指定扮演者 → 不筛（宁多不少）", () => {
  assert.equal(relationTouchesPair(RELS[0], null, 20), true);
  assert.equal(relationTouchesPair(RELS[0], 10, null), true);
});

test("关系提案按配对过滤", () => {
  assert.equal(proposalVisibleInPair("relation", 1, RELS, 10, 20), true);
  assert.equal(proposalVisibleInPair("relation", 3, RELS, 10, 20), false, "夏×王 的提案不该出现在夏×顾 里");
});

test("非关系类照常显示，脏 target 也不吞", () => {
  assert.equal(proposalVisibleInPair("world_setting", 0, RELS, 10, 20), true, "新设定属于世界");
  assert.equal(proposalVisibleInPair("character", 10, RELS, 10, 20), true, "新角色也照常");
  assert.equal(proposalVisibleInPair("relation", 999, RELS, 10, 20), true, "找不到关系 → 宁可多显示一条");
});
