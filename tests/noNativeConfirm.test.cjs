// 守卫：页面里不许再出现 WebView 原生确认框（2026-10-25）。
//
// 为什么要守：`window.confirm` 弹出来的是 tauri.localhost 的浅色原生框 ——
// 标题难看、贴窗口顶部、颜色跟系统走，用户点名要换掉，于是全部改成了 `useConfirm()`
// （自绘深色弹窗，见 src/confirm.tsx）。这种替换最怕的是**下次谁图省事又写一句原生
// confirm**：编译不报错、跑起来也能用，只是界面里又冒出一个白色系统框，
// 而且只有在点到那个按钮时才会发现。所以直接用静态扫描挡住。
//
// 判据：src/ 下所有 .ts/.tsx 里，
//   ① 不许出现 `window.confirm(` / `window.alert(`；
//   ② 不许出现没带 await 的 `confirm(`（也就是漏改回原生调用的写法）。

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

test("扫得到源文件（不然这条守卫等于没跑）", () => {
  assert.ok(files.length > 40, `只扫到 ${files.length} 个源文件，路径是不是变了？`);
});

test("不许用 window.confirm / window.alert（要自绘的 useConfirm）", () => {
  const bad = [];
  for (const f of files) {
    const text = readFileSync(f, "utf8");
    if (/window\.(confirm|alert)\s*\(/.test(text)) bad.push(relative(root, f));
  }
  assert.deepEqual(bad, [], `这些文件里还有原生弹窗调用，请改用 useConfirm()：${bad.join(", ")}`);
});

test("confirm( 必须带 await（漏了就是漏改回原生调用）", () => {
  const bad = [];
  for (const f of files) {
    const lines = readFileSync(f, "utf8").split(/\r?\n/);
    lines.forEach((line, i) => {
      // 只看真正的调用点：`confirm(`，且前面不是 await / useConfirm / 定义
      if (/(?<!await )\bconfirm\s*\(/.test(line) && !line.includes("useConfirm")) {
        bad.push(`${relative(root, f)}:${i + 1}`);
      }
    });
  }
  assert.deepEqual(bad, [], `这些地方调用了 confirm 却没 await（改用 useConfirm 后必须 await）：${bad.join(", ")}`);
});
