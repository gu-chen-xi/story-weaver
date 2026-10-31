// 接线守卫（2026-10-31）：账本调用必须带 characterName。
//
// 背景：14b9443 加 currentNodeId 时，这行被注释吃掉（`characterName: aiRole,` 跑到 // 后面），
// 后端必填参数缺失 → Tauri reject → 前端 catch 清空账本 → 面板永远"没记过账"、闸门永远不锁，
// 而 167 条纯函数测试全绿（cost 是手工构造的）。这类"纯函数测了、接线没测"的盲区用源码扫描兜住。

const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const SRC = fs.readFileSync(path.join(__dirname, "..", "src", "views", "StoryInput.tsx"), "utf8");

test("list_director_costs 的实参必须含 characterName / currentNodeId", () => {
  const i = SRC.indexOf('invoke<DirectorLedger>("list_director_costs"');
  assert.ok(i > 0, "找不到 list_director_costs 调用");
  const chunk = SRC.slice(i, i + 400);
  assert.ok(/characterName\s*:/.test(chunk), "缺 characterName：后端必填，缺了会静默失败（2026-10-31 事故）");
  assert.ok(/currentNodeId\s*:/.test(chunk), "缺 currentNodeId：旧账衰减算不对");
});

test("apply_pardon 之后要重拉关系（赦免会回补强度）", () => {
  const i = SRC.indexOf("const applyPardon");
  assert.ok(i > 0);
  const chunk = SRC.slice(i, i + 700);
  assert.ok(/loadRelations\(/.test(chunk), "赦免回补关系后不重拉，闸门还在用旧数");
});
