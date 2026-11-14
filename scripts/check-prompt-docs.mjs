// 校验「提示词拼接说明」里的代码引用（2026-11-14）。
//
// 文档里约定写 `文件.rs::符号`（不写行号）。这个脚本扫 docs/dev/prompts/**/*.md，
// 逐条检查：
//   ① 文件在 src-tauri/src 下存在；
//   ② 符号能在那个文件里找到（按词边界）。
// 找不到就报红并退出 1 —— 行号会飘，符号不会，所以校验只认符号。
//
// 用法：node scripts/check-prompt-docs.mjs （已挂进 npm run doclinks）

import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const root = process.cwd();
const docsDir = join(root, "docs", "dev", "prompts");
const srcDir = join(root, "src-tauri", "src");

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walk(p));
    else if (name.endsWith(".md")) out.push(p);
  }
  return out;
}

const refRe = /`([A-Za-z0-9_\/\-]+\.rs)::([A-Za-z_][A-Za-z0-9_]*)/g;
let checked = 0;
const problems = [];

for (const file of walk(docsDir)) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(refRe)) {
    const [, relPath, symbol] = m;
    checked++;
    // 允许写 "generate_stream/prompt.rs"（相对 src-tauri/src）或 "prompt.rs"（按文件名找）
    const candidates = [
      join(srcDir, relPath),
      ...walkRust(srcDir).filter((p) => p.endsWith(relPath.replace(/\//g, "\\")) || p.endsWith(relPath)),
    ];
    const found = candidates.filter((p) => existsSync(p));
    if (found.length === 0) {
      problems.push(`${relative(root, file)} → 找不到文件 ${relPath}`);
      continue;
    }
    const hit = found.some((p) => new RegExp(`\\b${symbol}\\b`).test(readFileSync(p, "utf8")));
    if (!hit) {
      problems.push(`${relative(root, file)} → ${relPath} 里找不到符号 ${symbol}`);
    }
  }
}

function walkRust(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...walkRust(p));
    else if (name.endsWith(".rs")) out.push(p);
  }
  return out;
}

if (problems.length === 0) {
  console.log(`提示词文档引用：检查 ${checked} 条，全部对得上代码 ✅`);
} else {
  console.error(`提示词文档引用：检查 ${checked} 条，有 ${problems.length} 条对不上：`);
  for (const p of problems) console.error("  ✗ " + p);
  process.exit(1);
}
