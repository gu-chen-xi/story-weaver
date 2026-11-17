// `lib/memory.ts` 的分组单测（npm test）。
//
// 背景（2026-11-18）：关键记忆的归属从"一个角色"改成**多参与者**之后（v3.16），
// 角色详情卡的「🧠 记忆」弹窗要按关系组展示。这套分组是后端
// `memory::group_by_participants` 的界面镜像 —— 这里锁的就是"两边口径一致"：
//   a×b 与 b×a 是同一组、无归属 = 全局组、全局组不会被算进某个角色的名字下。

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  groupMemories,
  splitGroups,
  countFor,
  GLOBAL_GROUP_LABEL,
} = require("../.tmp/fe-tests/src/lib/memory.js");

function card(id, characters, extra = {}) {
  return {
    id,
    kind: "confession",
    label: "表白",
    day_label: "第 1 天",
    branch: null,
    importance: 90,
    body: "# 标题\n正文",
    created_at: "2026-11-18T00:00:00Z",
    characters,
    ...extra,
  };
}

test("a×b 与 b×a 是同一组（参与者集合，顺序无关）", () => {
  const groups = groupMemories([card("1", ["夏婉琴", "顾宸熙"]), card("2", ["顾宸熙", "夏婉琴"])]);
  assert.equal(groups.length, 1, "两张卡应落进同一组");
  assert.deepEqual(groups[0].key, ["夏婉琴", "顾宸熙"]);
  assert.equal(groups[0].items.length, 2);
  assert.equal(groups[0].label, "夏婉琴 × 顾宸熙");
});

test("无归属 = 全局组，单独一摞；老卡的空数组也归进去", () => {
  const groups = groupMemories([card("1", []), card("2", ["夏婉琴"]), card("3", undefined)]);
  const global = groups.find((g) => g.key.length === 0);
  assert.ok(global, "应有一个全局组");
  assert.equal(global.label, GLOBAL_GROUP_LABEL);
  assert.deepEqual(global.items.map((x) => x.id), ["1", "3"]);
});

test("重复/空白参与者会被去重清理（脏数据不该拆成两组）", () => {
  const groups = groupMemories([card("1", ["夏婉琴", " 夏婉琴 ", ""]), card("2", ["夏婉琴"])]);
  assert.equal(groups.length, 1);
  assert.deepEqual(groups[0].key, ["夏婉琴"]);
});

test("组内卡片多的排前面；一样多时按组名（顺序稳定）", () => {
  const groups = groupMemories([
    card("1", ["A", "B"]),
    card("2", ["C"]),
    card("3", ["C"]),
    card("4", ["A", "B"]),
    card("5", ["A"]),
  ]);
  assert.deepEqual(
    groups.map((g) => g.label),
    ["A × B", "C", "A"],
    "2 张的组在前，1 张的组按组名字典序",
  );
});

test("按角色切成三摞：含她的 / 全局 / 其它", () => {
  const groups = groupMemories([
    card("1", ["夏婉琴", "顾宸熙"]),
    card("2", ["夏婉琴", "王老师"]),
    card("3", ["顾宸熙", "王老师"]),
    card("4", []),
  ]);
  const { mine, global, others } = splitGroups(groups, "夏婉琴");
  assert.deepEqual(mine.map((g) => g.label).sort(), ["夏婉琴 × 王老师", "夏婉琴 × 顾宸熙"].sort());
  assert.equal(global.length, 1, "全局组永远单独给出来");
  assert.deepEqual(others.map((g) => g.label), ["王老师 × 顾宸熙"]);
});

test("角色一张卡都没有：mine 为空但全局仍在（弹窗不能只显示空）", () => {
  const groups = groupMemories([card("1", []), card("2", ["顾宸熙"])]);
  const { mine, global, others } = splitGroups(groups, "夏婉琴");
  assert.equal(mine.length, 0);
  assert.equal(global.length, 1);
  assert.equal(others.length, 1);
});

test("countFor 只数含她的卡，不把全局卡算进去", () => {
  const items = [
    card("1", ["夏婉琴", "顾宸熙"]),
    card("2", ["夏婉琴"]),
    card("3", []),
    card("4", ["顾宸熙"]),
  ];
  assert.equal(countFor(items, "夏婉琴"), 2);
  assert.equal(countFor(items, "顾宸熙"), 2);
  assert.equal(countFor(items, ""), 0, "空名字不该把所有卡都算成她的");
});
