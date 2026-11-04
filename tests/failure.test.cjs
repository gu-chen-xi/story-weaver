// 流式失败标记（2026-11-04）：后端在正文末尾追加「〔失败原因〕xxx」，
// 正文要剥掉、原因单独用红字显示，导出文件里也不许出现。
const test = require("node:test");
const assert = require("node:assert/strict");
const { failureOf, stripFailure, FAIL_MARK } = require("../.tmp/fe-tests/src/lib/failure.js");

test("没有标记时原样返回", () => {
  assert.equal(stripFailure("她说完了。"), "她说完了。");
  assert.equal(failureOf("她说完了。"), null);
});

test("有标记：正文截断、原因取出", () => {
  const s = "她说到一半。\n\n" + FAIL_MARK + "HTTP 429: rate limited";
  assert.equal(stripFailure(s), "她说到一半。\n\n");
  assert.equal(failureOf(s), "HTTP 429: rate limited");
});

test("标记后面是空的 → 未知原因", () => {
  assert.equal(failureOf("正文" + FAIL_MARK), "未知原因");
});
