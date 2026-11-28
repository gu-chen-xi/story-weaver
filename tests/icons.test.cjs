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
const nav = read("src/lib/navigation.ts");
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
  const navStart = nav.indexOf("export function buildStoryNavItems");
  const navBlock = nav.slice(navStart);
  assert.ok(!PICTO.test(navBlock), "navigation.ts 的导航项里还有 emoji");
  for (const icon of ["folder", "calendar", "users", "globe", "book", "clipboard"]) {
    assert.ok(navBlock.includes(`icon: "${icon}"`), `navItems 缺少 ${icon}`);
  }
});

test("两个签名图标在位：whale 在 Logo、fish 在首页", () => {
  assert.ok(sidebar.includes('<Icon name="whale"'), "Logo 没有鲸");
  assert.ok(home.includes('<Icon name="fish"'), "首页没有大肥鱼");
});
test("九个页面标题也换成了图标（2026-11-19）", () => {
  const headers = [
    ["src/views/TimelinePage.tsx", "calendar"],
    ["src/views/CharsPage.tsx", "users"],
    ["src/views/WorldPage.tsx", "globe"],
    ["src/views/ChaptersPage.tsx", "book"],
    ["src/views/ParamsPage.tsx", "sliders"],
    ["src/views/PluginsPage.tsx", "grid"],
    ["src/views/CompliancePage.tsx", "scale"],
    ["src/views/TempPage.tsx", "database"],
    ["src/views/PortraitPage.tsx", "image"],
  ];
  for (const [file, name] of headers) {
    assert.ok(read(file).includes(`<Icon name="${name}"`), `${file} 的页头没换成 ${name}`);
  }
});test("弹窗/区块标题也换了图标（2026-11-19 第三刀）", () => {
  const where = [
    ["src/views/ComplianceModal.tsx", "scale"],
    ["src/views/ComplianceModal.tsx", "book"],
    ["src/views/ExportModal.tsx", "download"],
    ["src/views/InsertEventModal.tsx", "hourglass"],
    ["src/views/PortraitPage.tsx", "target"],
    ["src/views/PortraitPage.tsx", "tag"],
    ["src/views/PortraitPage.tsx", "upload"],
    ["src/views/PortraitPage.tsx", "folder"],
    ["src/views/MemoryModal.tsx", "spark"],
    ["src/views/ProposalsPage.tsx", "hourglass"],
    ["src/views/ProposalsPage.tsx", "clipboard"],
    ["src/views/ProposalsPage.tsx", "list"],
  ];
  for (const [file, name] of where) {
    assert.ok(read(file).includes(`<Icon name="${name}"`), `${file} 缺 ${name} 图标`);
  }
  // 用户明确说不动的：放大镜/缩小、删除、toast 的 ✅❌⚠
  const input = read("src/views/StoryInput.tsx");
  assert.ok(!input.includes('<Icon name="search"'), "放大镜类图标这次不换");
});test("第四批：对话区 / 监视器 / 角色页 / 世界观 / 章节 的图标（2026-11-19）", () => {
  const where = [
    ["src/views/StoryPanel.tsx", "hourglass"],
    ["src/views/StoryPanel.tsx", "film"],
    ["src/views/StoryPanel.tsx", "pencil"],
    ["src/views/DirectorPanel.tsx", "monitor"],
    ["src/views/DirectorPanel.tsx", "card"],
    ["src/views/DirectorPanel.tsx", "network"],
    ["src/views/PhaseManager.tsx", "clipboard"],
    ["src/views/ContextPanel.tsx", "layers"],
    ["src/views/CharsPage.tsx", "dice"],
    ["src/views/CharsPage.tsx", "save"],
    ["src/views/CharsPage.tsx", "message"],
    ["src/views/WorldPage.tsx", "map"],
    ["src/views/WorldPage.tsx", "bookmark"],
    ["src/views/WorldPage.tsx", "warn"],
    ["src/views/ChaptersPage.tsx", "download"],
    ["src/views/AppHeader.tsx", "download"],
  ];
  for (const [file, name] of where) {
    assert.ok(read(file).includes(`name="${name}"`) || read(file).includes(`"${name}"`), `${file} 缺 ${name}`);
  }
});test("第五批：世界观 / 提案 / 立绘 / 生成参数（2026-11-19）", () => {
  const where = [
    ["src/views/ParamsPage.tsx", "chart"],
    ["src/views/ParamsPage.tsx", "file"],
    ["src/views/ParamsPage.tsx", "code"],
    ["src/views/ParamsPage.tsx", "shuffle"],
    ["src/views/ParamsPage.tsx", "ruler"],
    ["src/views/ParamsPage.tsx", "calculator"],
    ["src/views/PortraitPage.tsx", "refresh"],
    ["src/views/PortraitPage.tsx", "info"],
    ["src/views/PortraitPage.tsx", "robot"],
    ["src/views/PortraitPage.tsx", "sort"],
    ["src/views/PortraitPage.tsx", "columns"],
    ["src/views/ProposalsPage.tsx", "broom"],
    ["src/views/ProposalsPage.tsx", "hourglass"],
  ];
  for (const [file, name] of where) {
    assert.ok(read(file).includes(`name="${name}"`) || read(file).includes(`"${name}"`), `${file} 缺 ${name}`);
  }
  // 世界观正文里的「📖 术语词典」也去掉了（图标不往散文里塞）
  assert.ok(!read("src/views/WorldPage.tsx").includes("「📖 术语词典」"), "WorldPage 正文还留着 📖");
});