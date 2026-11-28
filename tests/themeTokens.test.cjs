// 守卫：界面颜色只许从 `src/theme.ts` 的 `T` 取（2026-11-28，配色收口）。
//
// 为什么要守：收口前全仓 455 处裸 hex，同一个 `#191c22`（控件底）手写了 99 遍；
// 更糟的是「主色 vs 危险色」色相只差 22.6° 这件事，一直没人发现 ——
// 因为色值散在几十个文件里，谁也没法一眼看出它们挨这么近。
// 收口做完后最怕的是**下次谁又图省事手写一个色值**：编译不报错、界面也不立刻炸，
// 只是慢慢退回一锅粥，等发现时又要重新普查一遍。
//
// 判据（对应 docs/dev/DESIGN.md）：
//   ① `theme.ts` 里定义过的色值，不许在别的文件里以**完整字符串字面量**出现
//      （`"#191c22"` / `"rgba(55,201,120,.08)"` 这种）；
//   ② 允许出现在**复合字符串**里（渐变、`1px dashed #xxx` 之类）——
//      那是"视觉配方"不是"引用某个单色"。典型：`constants.ts` 的 `g1..g6` 占位渐变、
//      `WorldPage` 的 `FAC_COLORS` 势力色。动它们请整块改，别单点抠一个色值出来。
//   ③ `theme.ts` 自身是唯一允许写这些字面量的地方。
//
// 为什么只查"主题里有的值"而不是禁掉一切裸 hex：还有一批装饰色/内容色
// （分支色、情绪色、占位渐变）本来就不该进 `T`，见 DESIGN.md §7。
// 这条守卫管的是**防回退**，不是"一次扫平所有裸色值"。

const test = require("node:test");
const assert = require("node:assert/strict");
const { readdirSync, readFileSync, statSync } = require("node:fs");
const { join, relative } = require("node:path");

const root = join(__dirname, "..");
const srcDir = join(root, "src");
const themePath = join(srcDir, "theme.ts");

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

// 从 theme.ts 里把"定义值"抠出来：完整的 `"#rrggbb"` 与 `"rgba(...)"` 字符串字面量。
// 注意：`GRAD_ACCENT` 那种复合串不会被匹配 —— 它整串是渐变，不是单色。
function themeLiterals() {
  const text = readFileSync(themePath, "utf8");
  const hexes = [...text.matchAll(/"#[0-9a-fA-F]{6}"/g)].map((m) => m[0]);
  const rgbas = [...text.matchAll(/"rgba\([^)]*\)"/g)].map((m) => m[0]);
  return [...new Set([...hexes, ...rgbas])];
}

test("扫得到源文件（不然这条守卫等于没跑）", () => {
  assert.ok(files.length > 40, `只扫到 ${files.length} 个源文件，路径是不是变了？`);
});

test("theme.ts 里能解析出足够多的色值（解析规则是不是失效了）", () => {
  const lits = themeLiterals();
  assert.ok(lits.length >= 25, `只从 theme.ts 解析出 ${lits.length} 个色值，正则是不是被改了？`);
});

test("主题里定义过的色值，不许在别处再手写一遍", () => {
  const forbidden = themeLiterals();
  const bad = [];
  for (const f of files) {
    if (f === themePath) continue;
    const text = readFileSync(f, "utf8");
    for (const lit of forbidden) {
      if (text.includes(lit)) bad.push(`${relative(root, f)} → ${lit}`);
    }
  }
  assert.deepEqual(
    bad,
    [],
    "这些地方手写了主题里已有的色值，请改用 T.xxx（见 docs/dev/DESIGN.md）：\n  " + bad.join("\n  ")
  );
});

test("T 的每个键名附近都有注释（新加 token 时说明用途）", () => {
  const text = readFileSync(themePath, "utf8");
  const lines = text.split(/\r?\n/);
  const bare = [];
  lines.forEach((line, i) => {
    // 形如 `  accent: "#FF7A18",`
    const m = line.match(/^\s{2}(\w+):\s*"/);
    if (!m) return;
    // 同一组 token 共用一条注释是允许的 —— 往上找 3 行内有没有 `//` 就算数
    const near = [0, 1, 2, 3].some((k) => (lines[i - k] || "").includes("//"));
    if (!near) bare.push(`${m[1]}（theme.ts:${i + 1}）`);
  });
  assert.deepEqual(bare, [], `这些 token 附近没有注释说明用途，加一句：${bare.join("、")}`);
});
