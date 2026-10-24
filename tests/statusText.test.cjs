// `lib/chars.ts::statusText` 的单测（npm test）。
//
// 背景：`current_status` 是自由文本，常见写法是
//      "情绪：开心 ｜ 动作：后退"
// 界面已经把「情绪」渲染成 chip、「动作」渲染成单独一行 ——
// 原文再原样显示就会重复（用户截图里「动作：后退」出现了两次）。

const test = require("node:test");
const assert = require("node:assert/strict");

const { statusText } = require("../.tmp/fe-tests/src/lib/chars.js");

test("剥掉情绪/动作段落，保留描述", () => {
  assert.equal(statusText("情绪：开心 ｜ 动作：后退"), "");
  assert.equal(statusText("刚被拒绝 ｜ 情绪：崩溃"), "刚被拒绝");
  assert.equal(statusText("情绪：麻木 ｜ 在等雨停"), "在等雨停");
  assert.equal(statusText("动作：转过身 ｜ 手在抖"), "手在抖");
});

test("没有标签时原样返回（不能把正常描述吃掉）", () => {
  assert.equal(statusText("在窗边等雨停"), "在窗边等雨停");
  assert.equal(statusText(""), "");
});

test("分隔符收尾干净（不留 ｜ · 逗号）", () => {
  assert.equal(statusText("情绪：委屈 ｜ 。"), "");
  assert.equal(statusText("｜ 情绪：紧张 ｜ 说不出话 ｜"), "说不出话");
});

test("半角冒号与全角冒号都认", () => {
  assert.equal(statusText("情绪: 委屈 动作: 后退"), "");
  assert.equal(statusText("情绪: 委屈 ｜ 她在忍"), "她在忍");
});

test("只写了「动作」两个字但没有值时不误删后文", () => {
  // 没有冒号就不算标签段落
  assert.equal(statusText("她的动作很轻"), "她的动作很轻");
});
