// `lib/memory.ts::memoryBodyView` 的单测（npm test）。
//
// 背景：记忆文件是 Markdown，但面板上方已经显示了「标签 · 日期」——
// 标题再渲染一遍就是重复，引用块按纯文本渲染会把 `>` 露出来。

const test = require("node:test");
const assert = require("node:assert/strict");

const { memoryBodyView } = require("../.tmp/fe-tests/src/lib/memory.js");

const BODY = [
  "# 第 4 天 · 表白",
  "",
  "[夏婉琴]“我爱你，跟我在一起吧。”",
  "",
  "> 由系统从剧情里提炼（命中「我爱你」）。",
].join("\n");

test("标题被去掉（上面已显示标签与日期），引用块单独成为备注", () => {
  const v = memoryBodyView(BODY);
  assert.equal(v.text, "[夏婉琴]“我爱你，跟我在一起吧。”");
  assert.equal(v.note, "由系统从剧情里提炼（命中「我爱你」）。");
  assert.ok(!v.text.includes("#"), "不该把 # 露出来");
  assert.ok(!v.text.includes(">"), "不该把 > 露出来");
});

test("没有标题/引用块时原样保留（多行不合并）", () => {
  const v = memoryBodyView("第一行\n\n第二行");
  assert.equal(v.text, "第一行\n第二行");
  assert.equal(v.note, "");
});

test("空内容 / 只有标题 都不炸", () => {
  assert.deepEqual(memoryBodyView(""), { text: "", note: "" });
  assert.deepEqual(memoryBodyView("# 只有标题"), { text: "", note: "" });
});

test("CRLF 也能处理（Windows 上写的文件）", () => {
  const v = memoryBodyView("# 标题\r\n正文\r\n> 备注");
  assert.equal(v.text, "正文");
  assert.equal(v.note, "备注");
});
