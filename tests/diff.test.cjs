// `lib/diff.ts` 的单元测试（`npm test`）。
//
// 为什么要测：这个函数决定"两次请求差在哪"，一旦算错，界面上会漏掉改动行
// —— 而这种错**看起来很正常**（少显示几行没人发现）。所以用真值表锁住。

const test = require("node:test");
const assert = require("node:assert/strict");

const { diffLines, diffStat } = require("../.tmp/fe-tests/src/lib/diff.js");

test("两边完全一样 → 全部 same，无增删", () => {
  const rows = diffLines("a\nb\nc", "a\nb\nc");
  assert.equal(rows.length, 3);
  assert.ok(rows.every((r) => r.kind === "same"));
  assert.deepEqual(diffStat(rows), { added: 0, removed: 0 });
});

test("只加了一行 → 一行 add，其余 same", () => {
  const rows = diffLines("a\nc", "a\nb\nc");
  assert.deepEqual(diffStat(rows), { added: 1, removed: 0 });
  assert.equal(rows.find((r) => r.kind === "add").text, "b");
  // 未变化的行必须保留（界面要上下文）
  assert.equal(rows.filter((r) => r.kind === "same").length, 2);
});

test("只删了一行 → 一行 del", () => {
  const rows = diffLines("a\nb\nc", "a\nc");
  assert.deepEqual(diffStat(rows), { added: 0, removed: 1 });
  assert.equal(rows.find((r) => r.kind === "del").text, "b");
});

test("整段替换 → 删旧加新，不误判成 same", () => {
  const rows = diffLines("旧1\n旧2", "新1\n新2");
  assert.deepEqual(diffStat(rows), { added: 2, removed: 2 });
});

test("空对空 / 空对内容 / 内容对空", () => {
  assert.equal(diffLines("", "").length, 1, "空串 split 出 1 个空行");
  assert.deepEqual(diffStat(diffLines("", "a")), { added: 1, removed: 1 });
  assert.deepEqual(diffStat(diffLines("a", "")), { added: 1, removed: 1 });
});

test("顺序敏感：同样的行挪了位置要体现出来", () => {
  const rows = diffLines("a\nb", "b\na");
  const st = diffStat(rows);
  assert.ok(st.added > 0 && st.removed > 0, "顺序变化不该被判成完全一致");
});

test("中文与长段落也能跑（提示词就是中文）", () => {
  const before = "【世界观】\n- 地点「老宅」：三进院子";
  const after = "【世界观】\n- 地点「老宅」：三进院子\n- 物品「怀表」：B 送的";
  const rows = diffLines(before, after);
  const st = diffStat(rows);
  assert.deepEqual(st, { added: 1, removed: 0 });
  assert.ok(rows.some((r) => r.kind === "add" && r.text.includes("怀表")));
});
