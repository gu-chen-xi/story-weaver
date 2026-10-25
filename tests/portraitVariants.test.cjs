// 立绘「表情差分」的归组与切换（2026-10-25）。
//
// 为什么要单测：切换规则是用户口述定的（按张切 / 没有下一张就保持 / 一张都没有就不切），
// 这类"看起来没反应"的逻辑最容易在改界面时被改坏，而且坏了不会报错。

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  expressionTagOf, variantSlots, defaultVariant, nextVariant, variantIndexOf, labelOfVariant,
} = require("../.tmp/fe-tests/src/lib/portraitVariants.js");

const p = (id, tags, extra = {}) => ({ id, tags, path: `p${id}.png`, ver: `v${id}`, ...extra });

test("表情名 = 第一个非系统标签（AI生成 不算）", () => {
  assert.equal(expressionTagOf({ tags: ["AI生成"] }), "", "只有系统标签 → 没有表情名");
  assert.equal(expressionTagOf({ tags: ["AI生成", "冷静"] }), "冷静");
  assert.equal(expressionTagOf({ tags: [" 愠怒 ", "AI生成"] }), "愠怒", "应 trim");
  assert.equal(expressionTagOf({ tags: [] }), "");
});

test("切换单位是「每一张」：绑 3 张就是 3 格（同标签也不合并）", () => {
  const slots = variantSlots([p(3, ["冷静"]), p(1, ["冷静"]), p(2, ["AI生成"])]);
  assert.equal(slots.length, 3, "3 张图应有 3 个位置");
  assert.deepEqual(slots.map((s) => s.portrait.id), [1, 2, 3], "按 id 升序排");
  assert.deepEqual(slots.map((s) => s.tag), ["冷静", "未命名", "冷静"], "标签只当名字，不参与合并");
});

test("切换：按入库顺序往后走，走到末尾回绕", () => {
  const ps = [p(1, ["冷静"]), p(2, ["愠怒"]), p(3, ["疲惫"])];
  assert.equal(nextVariant(ps, 1).id, 2);
  assert.equal(nextVariant(ps, 2).id, 3);
  assert.equal(nextVariant(ps, 3).id, 1, "末尾回绕到第一个");
});

test("只有一张 → 保持当前（不回绕、不报错）", () => {
  assert.equal(nextVariant([p(7, ["冷静"])], 7).id, 7);
});

test("一张立绘都没有 → 返回 null（界面保持占位）", () => {
  assert.equal(nextVariant([], 1), null);
  assert.equal(nextVariant([], null), null);
  assert.equal(defaultVariant([]), null);
});

test("当前不在列表里（换角色后）→ 给第一张", () => {
  const ps = [p(1, ["冷静"]), p(2, ["愠怒"])];
  assert.equal(nextVariant(ps, 999).id, 1);
});

test("默认显示：优先钉住的，否则最新的", () => {
  assert.equal(defaultVariant([p(1, ["a"]), p(2, ["b"])]).id, 2, "没钉 → 最新");
  assert.equal(defaultVariant([p(1, ["a"]), p(2, ["b"], { pinned: true })]).id, 2);
  const ps = [p(1, ["a"], { pinned: true }), p(2, ["b"])];
  assert.equal(defaultVariant(ps).id, 1, "钉住的优先");
});

test("角标位置与名字", () => {
  const ps = [p(1, ["冷静"]), p(2, ["AI生成"]), p(3, ["疲惫"])];
  assert.equal(variantIndexOf(ps, 1), 1);
  assert.equal(variantIndexOf(ps, 3), 3);
  assert.equal(variantIndexOf(ps, 999), 0, "不在列表里 → 0");
  assert.equal(labelOfVariant(p(1, ["冷静"])), "冷静");
  assert.equal(labelOfVariant(p(1, ["AI生成"])), "", "只有系统标签 → 不显示名字（v1 这类版本号已不再展示）");
  assert.equal(labelOfVariant(null), "");
});
