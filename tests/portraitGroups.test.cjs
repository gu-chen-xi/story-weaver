// 立绘分组归属（2026-10-25）。
//
// 为什么要单测：分组从"一对一"改成"多归属"之后，同一张图要同时出现在角色组和自定义组里，
// 这类逻辑出错不会报错 —— 只会让某张图在某个组里莫名消失。

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  customGroupsOf, groupsOf, groupBuckets,
} = require("../.tmp/fe-tests/src/lib/portraitGroups.js");

test("自定义分组：逗号分隔可多个，去空白去空项", () => {
  assert.deepEqual(customGroupsOf({ group: "" }), []);
  assert.deepEqual(customGroupsOf({ group: "女生" }), ["女生"]);
  assert.deepEqual(customGroupsOf({ group: "女生, 夏婉琴 ,," }), ["女生", "夏婉琴"]);
});

test("角色组是派生的：绑定了角色就自动属于该组", () => {
  assert.deepEqual(groupsOf({ group: "", bound_character: "夏婉琴" }), ["夏婉琴"]);
  assert.deepEqual(groupsOf({ group: "女生", bound_character: "夏婉琴" }), ["夏婉琴", "女生"]);
  assert.deepEqual(groupsOf({ group: "女生", bound_character: null }), ["女生"]);
});

test("角色名与自定义组重名时只算一次", () => {
  assert.deepEqual(groupsOf({ group: "夏婉琴", bound_character: "夏婉琴" }), ["夏婉琴"]);
});

test("同一张图同时出现在两个组里（不是复制数据，是多归属）", () => {
  const a = { id: 1, group: "女生", bound_character: "夏婉琴" };
  const b = { id: 2, group: "", bound_character: "夏婉琴" };
  const buckets = groupBuckets([a, b]);
  const names = buckets.map((x) => x.name);
  assert.deepEqual(names, ["夏婉琴", "女生"], "先角色组、后自定义组");
  assert.equal(buckets[0].items.length, 2, "夏婉琴组里两张");
  assert.equal(buckets[1].items.length, 1, "女生组里只有 a");
  assert.equal(buckets[1].items[0].id, 1);
});

test("哪都不属于 → 未分组；空输入 → 空数组", () => {
  const buckets = groupBuckets([{ id: 9, group: "", bound_character: null }]);
  assert.deepEqual(buckets.map((x) => x.name), ["未分组"]);
  assert.deepEqual(groupBuckets([]), []);
});
