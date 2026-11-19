// 导航「自动刷新」的接线守卫（2026-11-19 用户要求）。
//
// 只读源码字符串，不跑 DOM：要守的是"点了演进提案必须真的调 loadProposals()"，
// 以及 App 真的把这个函数传给了 Sidebar —— 纯函数测试覆盖不到这种接线。

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.join(__dirname, "..");
const sidebar = fs.readFileSync(path.join(ROOT, "src", "views", "Sidebar.tsx"), "utf8");
const app = fs.readFileSync(path.join(ROOT, "src", "App.tsx"), "utf8");

test("Sidebar 声明了 loadProposals 这个 prop", () => {
  assert.ok(
    sidebar.includes("loadProposals: () => Promise<void>;"),
    "SidebarProps 里缺少 loadProposals —— 点了提案导航不会刷新",
  );
});

test("点「演进提案」时顺手 loadProposals()（重复点也刷）", () => {
  assert.ok(sidebar.includes('it.view === "proposals"'), "Sidebar 里没有针对 proposals 的分支");
  assert.ok(
    /if \(it\.view === "proposals" && currentStory\) void loadProposals\(\);/.test(sidebar),
    "提案导航的 onClick 没有调用 loadProposals()",
  );
});

test("App 把 loadProposals 传给了 Sidebar", () => {
  const i = app.indexOf("const sidebarProps");
  assert.ok(i >= 0, "App.tsx 里找不到 sidebarProps");
  const block = app.slice(i, i + 500);
  assert.ok(block.includes("loadProposals"), "sidebarProps 里没传 loadProposals");
});
