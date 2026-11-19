// `lib/portraitVariants.ts::boundForCharacter` 的单测（2026-11-19）。
//
// 背景：建分支会复制 characters（新 id），但**不复制立绘绑定**（立绘按 character_id 绑）。
// 于是切到分支后所有立绘都消失。界面按"同名角色"回退到主线那份，规则在这里锁住。

const test = require("node:test");
const assert = require("node:assert/strict");

const { boundForCharacter } = require("../.tmp/fe-tests/src/lib/portraitVariants.js");

const P = (id, story_id, character_id, bound_character, path = `p${id}.png`) => ({
  id,
  story_id,
  character_id,
  bound_character,
  tags: ["AI生成"],
  path,
});

test("直接绑在这个角色 id 上 → 用它，不走回退", () => {
  const ps = [P(1, 2, 9, "夏婉琴"), P(2, 2, 1, "夏婉琴")];
  const r = boundForCharacter(ps, { id: 9, name: "夏婉琴" }, 2);
  assert.deepEqual(r.list.map((p) => p.id), [1]);
  assert.equal(r.fromName, false);
});

test("分支角色没有绑定 → 按同名回退到主线那份", () => {
  const ps = [P(1, 2, 1, "夏婉琴"), P(2, 2, 1, "夏婉琴"), P(3, 2, 2, "顾宸熙")];
  const r = boundForCharacter(ps, { id: 9, name: "夏婉琴" }, 2);
  assert.deepEqual(r.list.map((p) => p.id), [1, 2]);
  assert.equal(r.fromName, true);
});

test("不跨故事回退（别的故事同名也不借）", () => {
  const ps = [P(1, 5, 1, "夏婉琴")];
  const r = boundForCharacter(ps, { id: 9, name: "夏婉琴" }, 2);
  assert.deepEqual(r.list, []);
  assert.equal(r.fromName, false);
});

test("没绑角色 / 没名字 / 没故事 → 空", () => {
  const ps = [P(1, 2, null, null), P(2, 2, 1, "")];
  assert.deepEqual(boundForCharacter(ps, { id: 9, name: "夏婉琴" }, 2).list, []);
  assert.deepEqual(boundForCharacter(ps, { id: 9, name: "  " }, 2).list, []);
  assert.deepEqual(boundForCharacter(ps, { id: 9, name: "夏婉琴" }, null).list, []);
});

test("同名有多份（多条分支的副本）都回退到同一批图", () => {
  const ps = [P(1, 2, 1, "夏婉琴"), P(2, 2, 11, "夏婉琴")];
  const r = boundForCharacter(ps, { id: 13, name: "夏婉琴" }, 2);
  assert.deepEqual(r.list.map((p) => p.id), [1, 2]);
});
