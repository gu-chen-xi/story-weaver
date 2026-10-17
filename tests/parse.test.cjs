// 前端纯逻辑单元测试 —— `lib/parse.ts` 的解析 / 格式化 / 预算计算。
//
// 运行：`npm test`
//
// 为什么是 .cjs 纯 JS 而不是 .ts：
//   ① 本机 npm 网络不通，装不了 vitest / @types/node；
//   ② tsc 只负责把 `src/lib` 编译成 `.tmp/fe-tests/src/...`，
//      测试本身用 Node 自带的 `node:test` + `node:assert`，**不需要任何类型包**。
//
// 为什么值得测这些：`parseScriptBlocks` 决定剧情气泡怎么分块渲染，
// `computeBudget` 决定「生成参数」页显示的 token 分布 —— 都是纯函数，
// 一旦改坏，界面上表现得很隐蔽（分块错了 / 数字不对），靠眼睛很难发现。

const test = require("node:test");
const assert = require("node:assert/strict");

const {
  computeBudget,
  estimateTokens,
  eventLabel,
  fmtNum,
  normRelState,
  parseDebugHistory,
  parseDirector,
  parseScriptBlocks,
  sectionColor,
  splitPromptSections,
  stripDirector,
} = require("../.tmp/fe-tests/src/lib/parse.js");

// ─────────────────────────── estimateTokens ───────────────────────────

test("estimateTokens：空文本为 0", () => {
  assert.equal(estimateTokens(""), 0);
});

test("estimateTokens：按字符数 ×1.5 向上取整（中文按字计）", () => {
  assert.equal(estimateTokens("abc"), 5);   // 3 × 1.5 = 4.5 → 5
  assert.equal(estimateTokens("你好"), 3);   // 2 × 1.5 = 3
  assert.equal(estimateTokens("你好呀"), 5); // 3 × 1.5 = 4.5 → 5
});

test("estimateTokens：emoji 按一个字符算（不是按 code unit）", () => {
  // "🎨" 是单个 code point、两个 code unit；用 Array.from 才不会被算成 2 个字符
  assert.equal(estimateTokens("🎨"), 2); // 1 × 1.5 = 1.5 → 2
});

// ─────────────────────────── parseDebugHistory ───────────────────────────

test("parseDebugHistory：对象数组正常解析", () => {
  const out = parseDebugHistory('[{"role":"user","content":"嗨"},{"role":"assistant","content":"在"}]');
  assert.deepEqual(out, [
    { role: "user", content: "嗨" },
    { role: "assistant", content: "在" },
  ]);
});

test("parseDebugHistory：也兼容 [role, content] 数组形式", () => {
  const out = parseDebugHistory('[["user","嗨"],["assistant","在"]]');
  assert.deepEqual(out, [
    { role: "user", content: "嗨" },
    { role: "assistant", content: "在" },
  ]);
});

test("parseDebugHistory：坏 JSON / 非数组 → 空数组（不抛异常）", () => {
  assert.deepEqual(parseDebugHistory("这不是 JSON"), []);
  assert.deepEqual(parseDebugHistory('{"role":"user"}'), []);
  assert.deepEqual(parseDebugHistory(""), []);
});

test("parseDebugHistory：字段缺失用空串补齐", () => {
  const out = parseDebugHistory('[{"role":"user"},{}]');
  assert.deepEqual(out, [
    { role: "user", content: "" },
    { role: "", content: "" },
  ]);
});

// ─────────────────────────── splitPromptSections ───────────────────────────

test("splitPromptSections：空文本 → 空数组", () => {
  assert.deepEqual(splitPromptSections(""), []);
});

test("splitPromptSections：没有任何【】→ 整段算「系统提示词」", () => {
  const out = splitPromptSections("纯文本");
  assert.deepEqual(out, [{ name: "系统提示词", text: "纯文本" }]);
});

test("splitPromptSections：按【】切段，且第一段之前的内容算「基础指令」", () => {
  const out = splitPromptSections("开头一段\n【角色】甲和乙\n【世界观】城市");
  assert.deepEqual(out.map((s) => s.name), ["基础指令", "角色", "世界观"]);
  // 每段正文保留自己的【】前缀（界面按段着色时要用）
  assert.ok(out[1].text.startsWith("【角色】"), "段正文应保留【】前缀：" + out[1].text);
  assert.ok(out[2].text.includes("城市"));
});

test("splitPromptSections：第一段就是【】时没有「基础指令」段", () => {
  const out = splitPromptSections("【角色】甲");
  assert.deepEqual(out.map((s) => s.name), ["角色"]);
});

// ─────────────────────────── sectionColor / normRelState ───────────────────────────

test("sectionColor：认识的段名给对应颜色，不认识的给默认色", () => {
  assert.equal(sectionColor("角色甲"), "#8f6bff"); // 前缀匹配「角色」
  assert.equal(sectionColor("世界观设定"), "#37c978");
  assert.equal(sectionColor("完全没听说过的段"), "#5b8cff");
});

test("normRelState：只认三种状态，其余回落到「稳定」", () => {
  assert.equal(normRelState("稳定"), "稳定");
  assert.equal(normRelState("紧张"), "紧张");
  assert.equal(normRelState("破裂"), "破裂");
  assert.equal(normRelState("乱填的"), "稳定");
  assert.equal(normRelState(""), "稳定");
});

// ─────────────────────────── 导演指令 ───────────────────────────

test("parseDirector：没有标记 → 空串", () => {
  assert.equal(parseDirector("普通正文"), "");
  assert.equal(parseDirector(""), "");
});

test("parseDirector：取出指令内容（不含用户正文）", () => {
  assert.equal(parseDirector("〔导演指令〕让甲接受表白\n\n夏婉琴：好"), "让甲接受表白");
});

test("parseDirector：前导空白不影响识别", () => {
  assert.equal(parseDirector("   \n〔导演指令〕强制剧情\n\n正文"), "强制剧情");
});

test("stripDirector：没有标记 → 原样返回（不做 trim）", () => {
  assert.equal(stripDirector("普通正文"), "普通正文");
  assert.equal(stripDirector("  留白保留"), "  留白保留");
});

test("stripDirector：去掉指令部分，只留用户正文", () => {
  assert.equal(stripDirector("〔导演指令〕强制剧情\n\n夏婉琴：好"), "夏婉琴：好");
});

test("stripDirector：只有指令没有正文 → 空串", () => {
  assert.equal(stripDirector("〔导演指令〕只有指令"), "");
});

// ─────────────────────────── eventLabel / fmtNum ───────────────────────────

test("eventLabel：已知事件给中文名，未知给「其它事件」", () => {
  assert.equal(eventLabel("confession"), "表白");
  assert.equal(eventLabel("coerce"), "其它事件");
});

test("fmtNum：委托给 toLocaleString（本地化千分位）", () => {
  assert.equal(fmtNum(1234), (1234).toLocaleString());
  assert.equal(fmtNum(0), "0");
});

// ─────────────────────────── parseScriptBlocks ───────────────────────────

test("parseScriptBlocks：[角色名]台词 → 对话框，并记住说话人", () => {
  const blocks = parseScriptBlocks("[夏婉琴]你来了。", ["夏婉琴"]);
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].type, "dialog");
  assert.equal(blocks[0].name, "夏婉琴");
  assert.equal(blocks[0].text, "你来了。");
});

test("parseScriptBlocks：「名字：台词」→ 对话框（冒号形式）", () => {
  const blocks = parseScriptBlocks("宸熙：我在等你", ["宸熙"]);
  assert.equal(blocks[0].type, "dialog");
  assert.equal(blocks[0].name, "宸熙");
  assert.equal(blocks[0].text, "我在等你");
});

test("parseScriptBlocks：冒号后的「说/道/问」会被剥掉", () => {
  const blocks = parseScriptBlocks("宸熙说：我在等你", ["宸熙"]);
  assert.equal(blocks[0].name, "宸熙");
});

test("parseScriptBlocks：单独一行的角色名 → speaker 块", () => {
  const blocks = parseScriptBlocks("夏婉琴\n她抬起头", ["夏婉琴"]);
  assert.equal(blocks[0].type, "speaker");
  assert.equal(blocks[0].text, "夏婉琴");
});

test("parseScriptBlocks：整行圆括号 → 动作块", () => {
  const blocks = parseScriptBlocks("（她把杯子放下）", []);
  assert.equal(blocks[0].type, "action");
});

test("parseScriptBlocks：整行 *…* → 心声块（去掉星号）", () => {
  const blocks = parseScriptBlocks("*其实我很紧张*", []);
  assert.equal(blocks[0].type, "inner");
  assert.equal(blocks[0].text, "其实我很紧张");
});

test("parseScriptBlocks：普通句子 → 旁白", () => {
  const blocks = parseScriptBlocks("窗外下起了雨。", []);
  assert.equal(blocks[0].type, "narr");
});

test("parseScriptBlocks：空行被忽略，多段按顺序切块", () => {
  const blocks = parseScriptBlocks("（动作）\n\n[甲]台词\n\n旁白", ["甲"]);
  assert.deepEqual(blocks.map((b) => b.type), ["action", "dialog", "narr"]);
});

test("parseScriptBlocks：方括号里带句末标点 → 不走「[角色名]台词」分支", () => {
  // 已知行为（写测试时实测到的，**不是**理想行为，但先钉住，免得改坏了没人知道）：
  //   「[注：这是说明。]正文」里的名字含「。」→ 被「[角色名]台词」分支的正确性检查拒掉；
  //   随后落到「名字：台词」分支（冒号形式），于是 name 被解析成「[注」。
  //   也就是说它仍然是一个 dialog 块，只是说话人名字里带了方括号。
  // 真要修得改解析顺序（先识别方括号包裹的注记），属于行为变更，不在本轮重构范围。
  const blocks = parseScriptBlocks("[注：这是说明。]正文", []);
  assert.equal(blocks[0].type, "dialog");
  assert.equal(blocks[0].name, "[注");
  assert.equal(blocks[0].text, "这是说明。]正文");
});

test("parseScriptBlocks：空文本 → 空数组", () => {
  assert.deepEqual(parseScriptBlocks("", []), []);
});

// ─────────────────────────── computeBudget ───────────────────────────

function dbg(patch) {
  return Object.assign(
    { system_prompt: "", history_json: "[]", user_text: "", estimated_tokens: 0 },
    patch || {},
  );
}

test("computeBudget：dbg 为 null 时全空，但预算仍然可用", () => {
  const b = computeBudget(null, 0);
  assert.deepEqual(b.sysSections, []);
  assert.equal(b.tailSection, null);
  assert.deepEqual(b.history, []);
  assert.equal(b.lastUser, null);
  assert.equal(b.usedTokens, 0);
  assert.equal(b.inputBudget, 1); // ctxWindow=0 → max(0-4800, 1)
  assert.equal(b.remain, 1);
  assert.equal(b.pctUsed, 0);
});

test("computeBudget：优先用快照里后端算好的 input_budget", () => {
  const b = computeBudget(dbg({ input_budget: 1000, estimated_tokens: 250 }), 999999);
  assert.equal(b.inputBudget, 1000);
  assert.equal(b.usedTokens, 250);
  assert.equal(b.remain, 750);
  assert.equal(b.pctUsed, 25);
});

test("computeBudget：老快照没有 input_budget 时回退成窗口 ×0.8 − 4800", () => {
  const b = computeBudget(dbg({ input_budget: 0 }), 10000);
  assert.equal(b.inputBudget, 8000 - 4800);
});

test("computeBudget：最后一轮用户输入会被单独拆出来（不计入 priorHistory）", () => {
  const b = computeBudget(
    dbg({
      system_prompt: "【角色】甲",
      history_json: '[{"role":"user","content":"你好"}]',
      user_text: "新一句",
      estimated_tokens: 0,
      input_budget: 1000,
    }),
    1000,
  );
  assert.equal(b.lastUser.content, "你好");
  assert.deepEqual(b.priorHistory, []);
  assert.equal(b.histTokens, 0, "唯一的 user 消息被当成本次输入，不计入历史");
  assert.ok(b.sysTokens > 0);
  assert.ok(b.userTokens > 0);
});

test("computeBudget：estimated_tokens=0 的老快照会用 0（已知行为，先钉住）", () => {
  // `usedTokens = dbg?.estimated_tokens ?? (自算值)` —— **0 不是 nullish**，
  // 所以老快照（该字段为 0）不会回退到自算值，页面会显示"已用 0"。
  // 而 inputBudget 对 0 有专门回退（snapBudget > 0 ? ... : 窗口×0.8-4800）。
  // 这是一个不对称的老行为：先钉住，真要改属于行为变更。
  const b = computeBudget(
    dbg({ system_prompt: "【角色】甲", user_text: "新一句", estimated_tokens: 0, input_budget: 1000 }),
    1000,
  );
  assert.equal(b.usedTokens, 0);
  assert.equal(b.pctUsed, 0);
  assert.equal(b.remain, 1000);
  assert.ok(b.sysTokens > 0, "自算的分项仍然是有的，只是 usedTokens 没采用");
});

test("computeBudget：历史最后一条不是 user 时，全部计入 priorHistory", () => {
  const b = computeBudget(
    dbg({
      history_json: '[{"role":"user","content":"a"},{"role":"assistant","content":"b"}]',
      user_text: "c",
    }),
    1000,
  );
  assert.equal(b.lastUser, null);
  assert.equal(b.priorHistory.length, 2);
  assert.equal(b.histTokens, estimateTokens("a") + estimateTokens("b"));
  assert.ok(b.userTokens > 0, "没有 lastUser 时用 dbg.user_text 估 token");
});

test("computeBudget：尾部块单独成节并计入 sysTokens", () => {
  const b = computeBudget(dbg({ system_prompt: "【角色】甲", tail_block: "尾部说明" }), 1000);
  assert.equal(b.tailSection.name, "上下文更新（尾部）");
  assert.equal(b.tailSection.text, "尾部说明");
  assert.ok(b.sysTokens >= estimateTokens("尾部说明"));
});

test("computeBudget：pctUsed 上限 100，remain 不为负", () => {
  const b = computeBudget(dbg({ input_budget: 100, estimated_tokens: 500 }), 1000);
  assert.equal(b.pctUsed, 100);
  assert.equal(b.remain, 0);
});
