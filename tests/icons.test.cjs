// 大肥鱼图标集的接线守卫（2026-11-19）。
//
// 读源码字符串，不跑 DOM：守的是"导航层不再用 emoji + 两个签名图标在位 + 没碰安全红线"。

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.join(__dirname, "..");
const read = (p) => fs.readFileSync(path.join(ROOT, p), "utf8");

const icons = read("src/components/icons.tsx");
const sidebar = read("src/views/Sidebar.tsx");
const app = read("src/App.tsx");
const home = read("src/views/HomePage.tsx");
const PICTO = /\p{Extended_Pictographic}/u;

test("图标集：12 个功能图标 + recall + 两个签名", () => {
  for (const name of [
    "folder", "calendar", "users", "globe", "book", "clipboard",
    "image", "sliders", "grid", "scale", "terminal", "gear",
    "recall", "fish", "whale",
  ]) {
    assert.ok(icons.includes(`"${name}"`), `IconName 里缺少 ${name}`);
  }
  assert.ok(!/dangerouslySetInnerHTML\s*=/.test(icons), "图标集不许用 dangerouslySetInnerHTML");
});

test("导航层不再用 emoji", () => {
  assert.ok(!PICTO.test(sidebar), "Sidebar 里还有 emoji");
  const navStart = app.indexOf("const navItems");
  const navBlock = app.slice(navStart, app.indexOf("];", navStart) + 2);
  assert.ok(!PICTO.test(navBlock), "App 的 navItems 里还有 emoji");
  for (const icon of ["folder", "calendar", "users", "globe", "book", "clipboard"]) {
    assert.ok(navBlock.includes(`icon: "${icon}"`), `navItems 缺少 ${icon}`);
  }
});

test("两个签名图标在位：whale 在 Logo、fish 在首页", () => {
  assert.ok(sidebar.includes('<Icon name="whale"'), "Logo 没有鲸");
  assert.ok(home.includes('<Icon name="fish"'), "首页没有大肥鱼");
});
