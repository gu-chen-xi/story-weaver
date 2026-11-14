// 行级 diff（2026-11-14）：双池对比 tab 里"到底改了啥"靠它。
const test = require("node:test");
const assert = require("node:assert/strict");
const { diffLines, diffStats } = require("../.tmp/fe-tests/src/lib/textDiff.js");

test("完全一样 → 全是 same，没有增删", () => {
  const l = diffLines("a\nb\nc", "a\nb\nc");
  assert.equal(l.every((x) => x.kind === "same"), true);
  assert.deepEqual(diffStats(l), { added: 0, removed: 0 });
});

test("删掉中间一行 / 换掉一行", () => {
  const del = diffLines("a\nb\nc", "a\nc");
  assert.equal(del.filter((x) => x.kind === "del").length, 1);
  assert.equal(del.find((x) => x.kind === "del").text, "b");

  const swap = diffLines("a\nb", "a\nB");
  assert.deepEqual(diffStats(swap), { added: 1, removed: 1 });
});

test("角色视角少了几段 → 计出被删的行", () => {
  const before = "【角色】\n- 夏婉琴：…\n- 顾宸熙：…\n【关系】\n- 夏 → 顾";
  const after = "【角色】\n- 夏婉琴：…\n【关系】\n- 夏 → 顾";
  const st = diffStats(diffLines(before, after));
  assert.equal(st.removed, 1);
  assert.equal(st.added, 0);
});

test("顺序保持：same/add/del 按原顺序拼回去能还原两边", () => {
  const l = diffLines("x\ny", "x\nz");
  const fromBefore = l.filter((x) => x.kind !== "add").map((x) => x.text).join("\n");
  const fromAfter = l.filter((x) => x.kind !== "del").map((x) => x.text).join("\n");
  assert.equal(fromBefore, "x\ny");
  assert.equal(fromAfter, "x\nz");
});
