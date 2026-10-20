#!/usr/bin/env node
// 文档相对链接校验（零依赖，Node 内置模块）。
//
// 为什么需要它：docs/README.md 第 5 条维护纪律写着"提交前查断链"，
// 但当时没有脚本 —— 文档一重组（2026-10-20 按受众分成 user/dev/design/archive），
// 相对链接就会成片失效，而**没有任何东西会报错**。
//
// 用法：
//   npm run doclinks          （有断链 → 退出码 1）
//
// 规则：
//   * 只查相对链接；http(s) / mailto / #锚点 / asset: 跳过
//   * 允许带 #锚点（只校验文件是否存在）
//   * 跳过代码块（``` 围栏）与行内代码 `...`，避免把示例里的假链接当成断链

import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { join, dirname, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** 收集所有要检查的 md：仓库根目录 + docs/ 递归 */
function collect() {
  const out = [];
  for (const name of readdirSync(ROOT)) {
    if (name.endsWith(".md")) out.push(join(ROOT, name));
  }
  const walk = (dir) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (name.endsWith(".md")) out.push(p);
    }
  };
  const docs = join(ROOT, "docs");
  if (existsSync(docs)) walk(docs);
  return out;
}

/** 去掉围栏代码块与行内代码，避免示例里的链接被误判 */
function stripCode(md) {
  return md
    .replace(/```[\s\S]*?```/g, (m) => m.replace(/[^\n]/g, " "))
    .replace(/`[^`\n]*`/g, (m) => " ".repeat(m.length));
}

const LINK = /\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;

const files = collect();
const broken = [];
let checked = 0;

for (const file of files) {
  const raw = readFileSync(file, "utf8");
  const text = stripCode(raw);
  for (const m of text.matchAll(LINK)) {
    const label = m[1];
    const url = m[2].trim();
    if (/^(https?:|mailto:|asset:|#)/i.test(url)) continue;
    const target = url.split("#")[0];
    if (!target) continue;
    checked += 1;
    const abs = resolve(dirname(file), target);
    if (!existsSync(abs)) {
      const line = text.slice(0, m.index).split("\n").length;
      broken.push({ file: relative(ROOT, file), line, label: label.slice(0, 40), url, want: relative(ROOT, abs) });
    }
  }
}

console.log(`检查 ${files.length} 份 md，${checked} 条相对链接`);
if (broken.length === 0) {
  console.log("✅ 没有断链");
  process.exit(0);
}
console.log(`❌ 断链 ${broken.length} 条：`);
for (const b of broken) {
  console.log(`  ${b.file}:${b.line}  [${b.label}] → ${b.url}`);
  console.log(`      解析为 ${b.want}（不存在）`);
}
process.exit(1);
