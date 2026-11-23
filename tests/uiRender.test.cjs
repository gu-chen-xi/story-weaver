// 界面「渲染」测试（2026-11-23，零依赖：react-dom/server 的 renderToString）。
//
// 为什么用 SSR 而不是 Testing Library：这一包先补"**能不能渲染出来、关键文案在不在**"，
// 不引入 jsdom / vitest（作者 2026-11-23 定：先不碰依赖）。点击/输入那层留给下一包。
//
// 能抓到：组件直接崩（props 形状变了、依赖的纯函数抛错）、关键按钮/文案消失、
//         富文本把 ** 星号漏出来、图标不再是 <svg>。
// 抓不到：点击、输入、弹窗开关、路由跳转。

const test = require("node:test");
const assert = require("node:assert/strict");
const React = require("react");
const { renderToString } = require("react-dom/server");

const { HomePage } = require("../.tmp/fe-tests/src/views/HomePage.js");
const { MemoryCards } = require("../.tmp/fe-tests/src/views/KeyMemories.js");
const { StoriesPage } = require("../.tmp/fe-tests/src/views/StoriesPage.js");
const { Icon } = require("../.tmp/fe-tests/src/components/icons.js");

const h = React.createElement;
/// React 19 SSR 在相邻文本节点之间插 `<!-- -->`，比对文案前先去掉（否则「共 4 个故事」会被拆开）
const plain = (html) => html.replace(/<!-- -->/g, "");

const story = (id, title) => ({
  id,
  title,
  time_origin: "2026-01-01",
  prologue: "",
  created_at: "2026-11-01 10:00:00",
  updated_at: "2026-11-19 10:00:00",
});

const noop = () => {};
const asyncNoop = async () => {};

test("HomePage：没有故事时渲染空态与「新建故事」", () => {
  const html = renderToString(h(HomePage, { stories: [], onContinue: asyncNoop, onNew: noop, onAll: noop }));
  assert.match(html, /还没有故事/);
  assert.match(html, /新建故事/);
  assert.match(html, /全部故事|最近编辑/);
});

test("HomePage：有故事时「继续创作」用最近更新的那条，最近编辑只显示 3 条", () => {
  const stories = [1, 2, 3, 4].map((i) => story(i, `故事${i}`));
  const html = renderToString(h(HomePage, { stories, onContinue: asyncNoop, onNew: noop, onAll: noop }));
  assert.ok(html.includes("继续创作 · 故事1"), "最近更新的是第一条，按钮应带它的标题");
  assert.ok(plain(html).includes("共 4 个故事"));
  assert.ok(html.includes("全部故事 →"));
  assert.ok(html.includes("故事3"), "前 3 条都要在");
  assert.ok(!html.includes("故事4"), "第 4 条不该出现在「最近编辑」里");
});

test("StoriesPage：空列表时给出引导文案，并保留「新建故事」按钮", () => {
  const html = renderToString(
    h(StoriesPage, {
      createStory: asyncNoop,
      enterStory: asyncNoop,
      loadStories: asyncNoop,
      newOrigin: "",
      newPrologue: "",
      newTitle: "",
      removeStory: asyncNoop,
      savePrologue: asyncNoop,
      setNewOrigin: noop,
      setNewPrologue: noop,
      setNewTitle: noop,
      setShowNew: noop,
      showNew: false,
      stories: [],
    }),
  );
  assert.match(html, /新建故事/);
  assert.match(html, /还没有故事/);
});

test("MemoryCards：正文里的 **加粗** 与 *斜体* 渲染成标签，星号不裸露", () => {
  const m = {
    id: "001",
    kind: "confession",
    label: "表白",
    day_label: "第 3 天",
    branch: null,
    importance: 90,
    body: "**顾宸熙**第 1 次向*夏婉琴*表白：他说喜欢她。",
    created_at: "2026-11-19 10:00:00",
    characters: ["夏婉琴", "顾宸熙"],
    ai_side: "顾宸熙",
  };
  const html = renderToString(
    h(MemoryCards, {
      items: [m],
      actions: { save: asyncNoop, remove: asyncNoop, togglePromise: asyncNoop },
      busy: false,
    }),
  );
  assert.match(html, /<b[^>]*>顾宸熙<\/b>/, "加粗应渲染成 <b>");
  assert.match(html, /<i>夏婉琴<\/i>/, "斜体应渲染成 <i>");
  assert.ok(!plain(html).includes("**"), "星号不该漏到界面上");
  assert.ok(html.includes("表白"));
});

test("Icon：渲染成内联 SVG（带 path/rect），不是 emoji 文本", () => {
  const html = renderToString(h(Icon, { name: "folder", size: 16 }));
  assert.match(html, /^<svg/);
  assert.match(html, /<path/);
  assert.match(html, /viewBox="0 0 24 24"/);
  const whale = renderToString(h(Icon, { name: "whale", size: 20 }));
  assert.match(whale, /^<svg/);
  assert.ok(!whale.includes("🐟") && !whale.includes("🐋"), "签名图标也不该回退成 emoji");
});
