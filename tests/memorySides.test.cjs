// 记忆卡两侧名字（`lib/memory.ts::sidesOf` / `memoryBodyView` 的第二参数）单测（2026-11-18）。
//
// 背景：记忆正文里的 `[用户]` / `[AI]` 是写卡时贴的"操作者"标签，换扮演者后
// 同一张卡的"用户"会指到别人身上。界面按角色名渲染，方向来自 `ai_side`（新卡）
// 或 `characters` 的顺序（老卡，恰好两人时）。

const test = require("node:test");
const assert = require("node:assert/strict");

const { sidesOf, memoryBodyView } = require("../.tmp/fe-tests/src/lib/memory.js");

function card(characters, ai_side, body = "# 第 1 天 · 承诺\n\n[用户] 我答应你。\n[AI] 「嗯。」") {
  return {
    id: "001",
    kind: "promise",
    label: "承诺",
    day_label: "第 1 天",
    branch: null,
    importance: 80,
    body,
    created_at: "",
    characters,
    ...(ai_side === undefined ? {} : { ai_side }),
  };
}

test("显式 ai_side 决定两侧名字（顺序无关）", () => {
  assert.deepEqual(sidesOf(card(["夏婉琴", "顾宸熙"], "顾宸熙")), { ai: "顾宸熙", user: "夏婉琴" });
  assert.deepEqual(sidesOf(card(["顾宸熙", "夏婉琴"], "夏婉琴")), { ai: "夏婉琴", user: "顾宸熙" });
});

test("老卡没有 ai_side：恰好两人时按顺序（[AI, 用户]）推断", () => {
  assert.deepEqual(sidesOf(card(["夏婉琴", "顾宸熙"], undefined)), { ai: "夏婉琴", user: "顾宸熙" });
});

test("一个参与者 / 空参与者：不猜方向（保留 用户/AI 标签）", () => {
  assert.equal(sidesOf(card(["夏婉琴"], undefined)), null);
  assert.equal(sidesOf(card([], undefined)), null);
  assert.equal(sidesOf(card(["夏婉琴"], "夏婉琴")), null, "只有一个名字时凑不出对面那个人");
});

test("正文行首标签换成名字；行中的标签不动", () => {
  const body = "# 第 1 天\n\n[用户] 我答应你。\n[AI] 「嗯。」\n他补了一句 [用户] 只是举例";
  const v = memoryBodyView(body, sidesOf(card(["夏婉琴", "顾宸熙"], "夏婉琴")));
  assert.ok(v.text.includes("[顾宸熙] 我答应你。"), v.text);
  assert.ok(v.text.includes("[夏婉琴] 「嗯。」"), v.text);
  assert.ok(v.text.includes("他补了一句 [用户] 只是举例"), "行中的标签不是我们的：" + v.text);
  assert.ok(!v.text.includes("\n[AI]"), "AI 标签不该留着");
});

test("写死的「她的话：」也换成名字", () => {
  const body = "# 第 1 天 · 表白\n\n第 1 次表白：**被拒绝**。她的话：「别。」";
  const v = memoryBodyView(body, sidesOf(card(["夏婉琴", "顾宸熙"], "夏婉琴")));
  assert.ok(v.text.includes("夏婉琴的话：「别。」"), v.text);
  assert.ok(!v.text.includes("她的话"), v.text);
});

test("拿不到名字时保持原样（老行为不回退）", () => {
  const v = memoryBodyView(card([], undefined).body, null);
  assert.ok(v.text.includes("[用户] 我答应你。"), v.text);
  const v2 = memoryBodyView(card([], undefined).body);
  assert.ok(v2.text.includes("[AI] 「嗯。」"), v2.text);
});

test("已经是人名的正文（新卡）不会被二次改名", () => {
  const body = "# 第 1 天\n\n[顾宸熙] 我答应你。\n[夏婉琴] 「嗯。」";
  const v = memoryBodyView(body, sidesOf(card(["夏婉琴", "顾宸熙"], "夏婉琴")));
  assert.ok(v.text.includes("[顾宸熙] 我答应你。"), v.text);
  assert.ok(v.text.includes("[夏婉琴] 「嗯。」"), v.text);
});
