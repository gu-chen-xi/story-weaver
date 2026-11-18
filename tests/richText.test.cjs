// `lib/richText.ts::richTokens` 的单测（2026-11-18）。
//
// 背景：记忆卡正文里既有软件写的 `**已被接受**`，也有 AI 自己写的 `*…*`。
// 拆分规则必须是"宁可不渲染、不可吞字" —— 正文是模型/作者原话，
// 把孤立的 `*` 当成斜体起点会把字吃掉。

const test = require("node:test");
const assert = require("node:assert/strict");

const { richTokens } = require("../.tmp/fe-tests/src/lib/richText.js");

const kinds = (s) => richTokens(s).map((t) => t.kind);
const texts = (s) => richTokens(s).map((t) => t.text);

test("纯文本：一个 text 段", () => {
  assert.deepEqual(kinds("普通正文"), ["text"]);
  assert.deepEqual(texts("普通正文"), ["普通正文"]);
});

test("**加粗** 被认出来", () => {
  assert.deepEqual(kinds("第 1 次：**已被接受**。"), ["text", "bold", "text"]);
  assert.deepEqual(texts("第 1 次：**已被接受**。"), ["第 1 次：", "已被接受", "。"]);
});

test("*斜体* 被认出来（AI 自己写的强调）", () => {
  assert.deepEqual(kinds("*嘴一张就是那三个字*"), ["italic"]);
  assert.deepEqual(texts("*嘴一张就是那三个字*"), ["嘴一张就是那三个字"]);
});

test("加粗和斜体混在一行各归各的", () => {
  assert.deepEqual(kinds("**A** 和 *B*"), ["bold", "text", "italic"]);
  assert.deepEqual(texts("**A** 和 *B*"), ["A", " 和 ", "B"]);
});

test("多个加粗都认", () => {
  assert.deepEqual(kinds("**a**b**c**"), ["bold", "text", "bold"]);
});

test("孤立的 ** 原样输出（不猜、不吞字）", () => {
  assert.deepEqual(kinds("这里有个 ** 没配对"), ["text"]);
  assert.deepEqual(texts("这里有个 ** 没配对"), ["这里有个 ** 没配对"]);
});

test("孤立的 * 原样输出", () => {
  assert.deepEqual(texts("3 * 4 = 12"), ["3 * 4 = 12"]);
  assert.deepEqual(kinds("3 * 4 = 12"), ["text"]);
});

test("空串不给 token", () => {
  assert.deepEqual(richTokens(""), []);
});
