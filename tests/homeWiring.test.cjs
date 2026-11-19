// 首页接线守卫（2026-11-19）：读源码字符串，不跑 DOM。
//
// 守三件事：
//   ① `home` 视图真的渲染 HomePage（此前 home 没有渲染分支，打开软件是一片空白）；
//   ② 点 Logo 能回首页；
//   ③ 首页**不自己查数据** —— 用户定"以故事列表为底座"，只用 stories 这一份，
//      不许为了首页去开每个故事的 story.db（所以这里禁止出现 invoke）。

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.join(__dirname, "..");
const app = fs.readFileSync(path.join(ROOT, "src", "App.tsx"), "utf8");
const sidebar = fs.readFileSync(path.join(ROOT, "src", "views", "Sidebar.tsx"), "utf8");
const home = fs.readFileSync(path.join(ROOT, "src", "views", "HomePage.tsx"), "utf8");

test("App 在 home 视图里渲染 HomePage", () => {
  assert.ok(app.includes('{view === "home" && ('), "App.tsx 里没有 home 的渲染分支");
  assert.ok(app.includes("<HomePage {...homePageProps} />"), "home 分支没渲染 HomePage");
});

test("点左上角 Logo 回首页", () => {
  assert.ok(sidebar.includes("goHome: () => void;"), "Sidebar 没声明 goHome");
  assert.ok(sidebar.includes("onClick={goHome}"), "Logo 没有绑定 goHome");
  assert.ok(app.includes("goHome: () =>"), "App 没给 Sidebar 传 goHome");
});

test("首页只用故事列表数据，不自己查库", () => {
  assert.ok(home.includes("stories: Story[]"), "HomePage 应该从 props 拿 stories");
  assert.ok(!home.includes("invoke("), "首页不该自己 invoke —— 数据以故事列表为底座");
});

test("回到首页会重拉故事列表（排序/相对时间才是新的）", () => {
  const fx = fs.readFileSync(path.join(ROOT, "src", "hooks", "useViewEffects.ts"), "utf8");
  assert.ok(fx.includes('view === "home"'), "useViewEffects 里没有 home 的分支");
  assert.ok(fx.includes("loadStories()"), "回到首页没有重拉故事列表");
  assert.ok(app.includes("loadChars, loadStories"), "App 没把 loadStories 传给 useViewEffects");
});
