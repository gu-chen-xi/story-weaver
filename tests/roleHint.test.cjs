// 切换「我扮演 / AI 扮演」的提示（2026-11-11）。
//
// 2026-11-11 起对话模式按**配对视角**取料：换扮演者 = 换一段关系的视角。
// 旧对话不会消失，只是不在当前视角里 —— 这必须说一句，否则用户以为历史丢了。

const test = require("node:test");
const assert = require("node:assert/strict");
const { roleSwitchHint } = require("../.tmp/fe-tests/src/lib/roleHint.js");

test("换 AI 扮演：提示写明是谁换了 + 新配对", () => {
  const h = roleSwitchHint({ ai: "夏婉琴", my: "顾宸熙" }, { ai: "王老师", my: "顾宸熙" });
  assert.ok(h, "换了就该有提示");
  assert.match(h, /AI 扮演已切换/);
  assert.match(h, /「王老师」×「顾宸熙」/);
  assert.match(h, /不会消失/, "必须说清旧对话没丢");
});

test("换我扮演：提示指明是我这边换的", () => {
  const h = roleSwitchHint({ ai: "夏婉琴", my: "顾宸熙" }, { ai: "夏婉琴", my: "王老师" });
  assert.match(h, /我扮演已切换/);
  assert.match(h, /「夏婉琴」×「王老师」/);
});

test("两边一起换：说「两边扮演者」", () => {
  const h = roleSwitchHint({ ai: "夏婉琴", my: "顾宸熙" }, { ai: "王老师", my: "老张头" });
  assert.match(h, /两边扮演者已切换/);
});

test("没变就不弹：同一个值 / 自由到自由", () => {
  assert.equal(roleSwitchHint({ ai: "夏婉琴", my: "顾宸熙" }, { ai: "夏婉琴", my: "顾宸熙" }), null);
  assert.equal(roleSwitchHint({ ai: "自由", my: "自由" }, { ai: "自由", my: "自由" }), null);
  // 空白与「自由」视为同一档（界面里 "自由" 就是没指定）
  assert.equal(roleSwitchHint({ ai: "", my: " " }, { ai: "自由", my: "自由" }), null);
});

test("从「自由」切到具体角色也算变化", () => {
  const h = roleSwitchHint({ ai: "自由", my: "顾宸熙" }, { ai: "夏婉琴", my: "顾宸熙" });
  assert.ok(h);
  assert.match(h, /「夏婉琴」×「顾宸熙」/);
});
