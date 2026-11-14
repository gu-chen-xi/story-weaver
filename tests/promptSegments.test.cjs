// 提示词分段与「通用 / 有差异」分组（2026-11-14）：双池对比的上半/下半靠它。
const test = require("node:test");
const assert = require("node:assert/strict");
const { splitSegments, compareSegments } = require("../.tmp/fe-tests/src/lib/promptSegments.js");

test("按【标题】切段，首个标题前的正文算「基础指令」", () => {
  const segs = splitSegments("You are a role-play engine.\n【输出排版】\n- 台词用引号\n【数据字典】\n- 强度 0-100");
  assert.deepEqual(segs.map((s) => s.title), ["基础指令", "【输出排版】", "【数据字典】"]);
  assert.match(segs[0].body, /role-play engine/);
  assert.match(segs[1].body, /台词用引号/);
});

test("标题行后面紧跟的内容也算这一段", () => {
  const segs = splitSegments("【关系】- 夏 → 顾 强度 81");
  assert.equal(segs.length, 1);
  assert.equal(segs[0].title, "【关系】");
  assert.equal(segs[0].body, "- 夏 → 顾 强度 81");
});

test("两边一样的段进 common，只保留一份", () => {
  const g = "基础\n【排版】规则A\n【角色】- 夏\n- 顾";
  const r = "基础\n【排版】规则A\n【角色】- 夏";
  const d = compareSegments(g, r);
  assert.deepEqual(d.common.map((s) => s.title), ["基础指令", "【排版】"]);
  assert.equal(d.differing.length, 1);
  assert.equal(d.differing[0].title, "【角色】");
});

test("同标题但内容不同 → 配对到同一行；一侧缺失 → null", () => {
  const g = "【角色】- 夏\n- 顾\n【你对面的他】阿婆";
  const r = "【角色】- 夏";
  const d = compareSegments(g, r);
  const role = d.differing.find((x) => x.title === "【角色】");
  assert.ok(role && role.global && role.role, "两边都有这一段");
  const opp = d.differing.find((x) => x.title === "【你对面的他】");
  assert.ok(opp && opp.global && opp.role === null, "只在一侧出现的段，另一侧为 null");
});
