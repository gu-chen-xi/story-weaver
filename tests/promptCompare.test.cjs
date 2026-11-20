// 双池对比界面的守卫（2026-11-20）。
//
// 用户实测反馈了两件事：
//   ① 关系强度/示例对话在界面里**凭空消失** —— 它们两边一样，被 compareSegments 归进 common，
//      而尾部只渲染了 differing，common 压根没画；
//   ② 护栏只有报红的时候才有条，通过时什么也不显示，用户不知道"到底核没核"。
// 这里用源码断言把这两点焊住（读源码，不跑 DOM）。

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.join(__dirname, "..");
const src = fs.readFileSync(path.join(ROOT, "src", "views", "PromptPoolCompare.tsx"), "utf8");

test("两边一样的尾部块也要渲染（不许再凭空消失）", () => {
  assert.ok(src.includes("tailDiff.common"), "没有渲染 tailDiff.common");
  assert.ok(src.includes("两边一样的尾部块"), "缺少「两边一样的尾部块」标题");
});

test("护栏通过时要有绿条（不只报红）", () => {
  assert.ok(src.includes("drift.ok"), "没有判 ok 的分支");
  assert.ok(src.includes("逐行核对通过"), "通过时没有提示文案");
});

test("对比面板标题不再用 emoji", () => {
  assert.ok(!src.includes("🔀"), "标题里的 🔀 没换掉");
});
