// `lib/promptDrift.ts::driftAgainst` 的单测（2026-11-19）。
//
// 背景：2026-11-14 那版"漂移护栏"拿真实尾部**总字数**比预览**三块子集**的字数，
// 口径不同 → 故事一大就必报红（用户实测 1273 vs 459/264）。现在改成逐行核对。

const test = require("node:test");
const assert = require("node:assert/strict");

const { driftAgainst } = require("../.tmp/fe-tests/src/lib/promptDrift.js");

const REAL = [
  "【数值与当下】（本节为当前真值）",
  "关系强度与状态：",
  "- 夏婉琴 → 顾宸熙：关系强度 60（亲近）· 稳定",
  "- 夏婉琴：体力 80 / 理智 70 / 存在感 55 · 当下：动作靠近",
  "【示例对话】",
  "A：你还在生气？",
  "她：没有。",
  "【当前模式】沉浸对话：用户以顾宸熙的身份与你说话，你扮演夏婉琴回应。",
].join("\n");

const ROLE_VIEW = [
  "【数值与当下·关系强度】",
  "- 夏婉琴 → 顾宸熙：关系强度 60（亲近）· 稳定",
  "【数值与当下·角色属性】",
  "- 夏婉琴：体力 80 / 理智 70 / 存在感 55 · 当下：动作靠近",
  "【示例对话】",
  "A：你还在生气？",
  "她：没有。",
].join("\n");

test("预览的行都在真实尾部里 → 不报警", () => {
  const d = driftAgainst(REAL, ROLE_VIEW, ROLE_VIEW);
  assert.ok(d);
  assert.equal(d.ok, true);
  assert.deepEqual(d.missing, []);
  assert.equal(d.realChars, REAL.trim().length);
});

test("两栏内容相同也能判（全局=角色不冲突）", () => {
  const d = driftAgainst(REAL, ROLE_VIEW, ROLE_VIEW);
  assert.equal(d.ok, true);
});

test("预览多出一行真实尾部没有的 → 报警，并指出缺哪一行", () => {
  const drifted = ROLE_VIEW + "\n- 顾宸熙 → 夏婉琴：关系强度 40 · 紧张";
  const d = driftAgainst(REAL, drifted, drifted);
  assert.ok(d);
  assert.equal(d.ok, false);
  assert.equal(d.missing.length, 1);
  assert.ok(d.missing[0].includes("顾宸熙 → 夏婉琴"), d.missing[0]);
});

test("取缺得少的那一栏（最近一次请求可能是全局也可能是角色）", () => {
  const globalView = ROLE_VIEW + "\n- 顾宸熙：体力 50 / 理智 50 / 存在感 50";
  const d = driftAgainst(REAL, globalView, ROLE_VIEW);
  assert.ok(d);
  assert.equal(d.side, "role", "角色栏完整，应选它");
  assert.equal(d.ok, true);
});

test("真实尾部为空（没有请求记录）→ 不判", () => {
  assert.equal(driftAgainst("", ROLE_VIEW, ROLE_VIEW), null);
  assert.equal(driftAgainst("   ", ROLE_VIEW, ROLE_VIEW), null);
});

test("空行与首尾空白不影响判断", () => {
  const padded = "\n\n" + ROLE_VIEW.split("\n").map((l) => "  " + l + "  ").join("\n") + "\n\n";
  const d = driftAgainst(REAL, padded, padded);
  assert.ok(d);
  assert.equal(d.ok, true);
});

test("数据在请求之后变过（名字对得上、整行不同）→ 不算漂移，记 stale", () => {
  // 真实快照里是「当下：动作靠近」，预览读的是现在的库 → 变成了「动作：转身」
  const changed = ROLE_VIEW.replace("当下：动作靠近", "当下：动作：转身");
  const d = driftAgainst(REAL, changed, changed);
  assert.ok(d);
  assert.equal(d.ok, true, "名字前缀对得上，不该判硬漂移");
  assert.equal(d.missing.length, 0);
  assert.equal(d.stale.length, 1);
  assert.ok(d.stale[0].includes("动作：转身"), d.stale[0]);
});

test("非数据行（示例对话）对不上 → 仍然算硬缺失", () => {
  const changed = ROLE_VIEW.replace("她：没有。", "她：别问了。");
  const d = driftAgainst(REAL, changed, changed);
  assert.ok(d);
  assert.equal(d.ok, false);
  assert.equal(d.missing.length, 1);
  assert.ok(d.missing[0].includes("别问了"), d.missing[0]);
});