// 守卫：不许用 `String(currentStory?.id)` / `currentStory!.id`（2026-11-28，E-UI-35）。
//
// 为什么守：从首页点故事进来时，`setCurrentStory(s)` 只是排队更新 state，
// **当前渲染里的闭包仍然拿旧值（null）**。这时 `String(currentStory?.id)` 得到的是
// 字符串 `"undefined"`，后端会去开 `projects/undefined/story.db`，报
// `(code: 14) unable to open database file` —— 而且点**任何一个**故事都中招，
// 看起来像"库坏了"，其实是传参坏了。
//
// 正确写法（两种都行）：
//   ① 调用方显式传 id：`loadProposals(s.id)`，函数内 `const sid = arg ?? currentStory?.id;`
//   ② 函数开头先守卫：`if (!currentStory) return;` 之后用 `currentStory.id`（不用 `?.` / `!`）
//
// 判据：扫 `src/**` 的 .ts/.tsx，跳过注释行 ——
//   ① 不许 `String(currentStory?.id)`；
//   ② 不许 `currentStory!.id`（非空断言会把这个坑藏起来，运行时才炸）。

const test = require("node:test");
const assert = require("node:assert/strict");
const { readdirSync, readFileSync, statSync } = require("node:fs");
const { join, relative } = require("node:path");

const root = join(__dirname, "..");
const srcDir = join(root, "src");

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (/\.(ts|tsx)$/.test(name)) out.push(p);
  }
  return out;
}

const files = walk(srcDir);

const RULES = [
  { re: /String\(currentStory\?\.id\)/, why: "空值会变成字符串 \"undefined\"，后端去打 projects/undefined/story.db" },
  { re: /currentStory!\./, why: "非空断言把风险藏到运行时；先用 `if (!currentStory) return;` 收窄" },
];

test("扫得到源文件（不然这条守卫等于没跑）", () => {
  assert.ok(files.length > 40, `只扫到 ${files.length} 个源文件，路径是不是变了？`);
});

test("不许把可能为 null 的 currentStory.id 拼成字符串或断言非空", () => {
  const bad = [];
  for (const f of files) {
    const lines = readFileSync(f, "utf8").split(/\r?\n/);
    lines.forEach((line, i) => {
      const t = line.trim();
      if (t.startsWith("//") || t.startsWith("*") || t.startsWith("/*")) return; // 注释里提到不算
      for (const r of RULES) {
        if (r.re.test(line)) bad.push(`${relative(root, f)}:${i + 1} → ${r.why}`);
      }
    });
  }
  assert.deepEqual(bad, [], "这些地方会让 storyId 变成 undefined（见 E-UI-35）：\n  " + bad.join("\n  "));
});
