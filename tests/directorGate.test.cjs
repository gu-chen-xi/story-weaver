// 「导演指令」软件闸门（v3.6.1）。
//
// 为什么要单测：这是一条**硬性拦截**——判错了要么把用户的按钮锁死（该放行时拦），
// 要么白送一个本该拒绝的指令（该拦时放行）。两种都不会报错，只能靠断言盯。
// 而且它的输入是关系数据，方向（单向/双向）和"没有记录"这几种情况很容易写漏。

const test = require("node:test");
const assert = require("node:assert/strict");

const { directorGate } = require("../.tmp/fe-tests/src/lib/directorGate.js");

const chars = [
  { id: 1, name: "夏婉琴" },  // AI 扮演
  { id: 2, name: "宸熙" },    // 用户扮演
  { id: 3, name: "路人" },
];

const rel = (from, to, strength, direction = "单向") => ({
  from_character_id: from,
  to_character_id: to,
  direction,
  strength,
});

const gate = (over = {}) =>
  directorGate({
    mode: "对话",
    myRole: "宸熙",
    aiRole: "夏婉琴",
    chars,
    relations: [],
    ...over,
  });

test("AI → 我 强度 = 0 → 锁住，并说清是谁对谁", () => {
  const g = gate({ relations: [rel(1, 2, 0)] });
  assert.equal(g.blocked, true);
  assert.match(g.reason, /夏婉琴/);
  assert.match(g.reason, /宸熙/);
  assert.match(g.reason, /0/);
  assert.match(g.reason, /导演指令不能添加/);
});

test("强度 1 以上一律放行（1 就是「还有一点希望」）", () => {
  for (const s of [1, 5, 20, 100]) {
    assert.equal(gate({ relations: [rel(1, 2, s)] }).blocked, false, `强度 ${s} 不该拦`);
  }
});

test("没有关系记录 → 放行（数据稀疏不该让功能不可用）", () => {
  assert.equal(gate({ relations: [] }).blocked, false);
});

test("只有 我 → AI 的单向记录 → 放行（那是用户自己的投入，不是 AI 的态度）", () => {
  assert.equal(gate({ relations: [rel(2, 1, 0)] }).blocked, false);
});

test("「双向」记录：正向反向都拦（双向两侧共用一个数值）", () => {
  const forward = gate({ relations: [rel(1, 2, 0, "双向")] });
  assert.equal(forward.blocked, true, "AI→我 的双向 0 必须拦");
  const backward = gate({ relations: [rel(2, 1, 0, "双向")] });
  assert.equal(backward.blocked, true, "反向的双向 0 也算（两侧共用一个数值）");
});

test("AI → 我 有明确记录时优先用它，不被反向那条盖掉", () => {
  const g = gate({ relations: [rel(2, 1, 100, "双向"), rel(1, 2, 0)] });
  assert.equal(g.blocked, true);
});

test("旁白模式不判（导演指令本来就只在沉浸对话里出现）", () => {
  assert.equal(gate({ mode: "旁白", relations: [rel(1, 2, 0)] }).blocked, false);
});

test("任一方是「自由」→ 放行（还没定谁演谁，无从判）", () => {
  assert.equal(gate({ myRole: "自由", relations: [rel(1, 2, 0)] }).blocked, false);
  assert.equal(gate({ aiRole: "自由", relations: [rel(1, 2, 0)] }).blocked, false);
});

test("名字不在角色表里 → 放行（不猜）", () => {
  assert.equal(gate({ aiRole: "查无此人", relations: [rel(1, 2, 0)] }).blocked, false);
});

test("AI 和用户是同一个角色 → 放行（自恋关系不在讨论范围）", () => {
  assert.equal(gate({ myRole: "夏婉琴", relations: [rel(1, 2, 0)] }).blocked, false);
});

test("不拦时 reason 必须是空串（界面靠它判断要不要弹提示）", () => {
  assert.equal(gate().reason, "");
  assert.equal(gate({ relations: [rel(1, 2, 30)] }).reason, "");
});
