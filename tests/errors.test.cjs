const test = require("node:test");
const assert = require("node:assert/strict");
const { bindErrorSink, errorText, expectedError, reportError } = require("../.tmp/fe-tests/src/lib/errors.js");

test("errorText: string / Error / object / null", () => {
  assert.equal(errorText("  boom  "), "boom");
  assert.equal(errorText(new Error("bad")), "bad");
  assert.equal(errorText({ message: "obj bad" }), "obj bad");
  assert.equal(errorText({ code: 7 }), '{"code":7}');
  assert.equal(errorText(null), "未知错误");
});

test("reportError: 默认写日志 + 发 T1，并返回用户文案", () => {
  const oldError = console.error;
  const got = [];
  const off = bindErrorSink((text) => got.push(text));
  console.error = () => {};
  try {
    const text = reportError("加载分支", "disk failed");
    assert.equal(text, "❌ 加载分支：disk failed");
    assert.deepEqual(got, ["❌ 加载分支：disk failed"]);
  } finally {
    console.error = oldError;
    off();
  }
});

test("reportError: silent 不发 T1，但仍返回文案", () => {
  const oldError = console.error;
  const got = [];
  const off = bindErrorSink((text) => got.push(text));
  console.error = () => {};
  try {
    const text = reportError("读取设置", new Error("bad config"), { silent: true });
    assert.equal(text, "❌ 读取设置：bad config");
    assert.deepEqual(got, []);
  } finally {
    console.error = oldError;
    off();
  }
});

test("reportError: warn 使用黄色前缀", () => {
  const oldError = console.error;
  const got = [];
  const off = bindErrorSink((text) => got.push(text));
  console.error = () => {};
  try {
    reportError("可选统计", "not ready", { level: "warn" });
    assert.deepEqual(got, ["⚠ 可选统计：not ready"]);
  } finally {
    console.error = oldError;
    off();
  }
});

test("expectedError: 明确忽略不抛异常", () => {
  const oldDebug = console.debug;
  console.debug = () => {};
  try {
    assert.doesNotThrow(() => expectedError("可选词表失败，回退内置值", "missing"));
  } finally {
    console.debug = oldDebug;
  }
});