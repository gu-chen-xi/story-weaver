// 前端依赖许可门禁（2026-11-27，零依赖）。
//
// 为什么需要：`npm audit` 只管"有没有漏洞"，不管"能不能这样分发"。GPL/AGPL/LGPL/SSPL 这类
// 传染性许可如果混进运行时依赖，闭源分发就会违约；带专利条款的（Apache-2.0 / MPL-2.0）反而是
// **明示授权**，允许但要在文档里说清楚。
//
// 做法：扫 node_modules 里每个包的 license 字段，按 SPDX 表达式拆开：
//   - 命中 DENY（GPL 系 / SSPL / 商业源码许可）且没有可选的宽松项 → 红；
//   - 出现不在 ALLOW 里的许可证 → 红（逼我们显式加白名单并写理由）；
//   - 全部在 ALLOW 里 → 绿。
//
// 用法：node scripts/check-licenses.mjs [--json]
// 只读，不写文件；CI 在 security.yml 的 npm job 里跑。

import fs from "node:fs";
import path from "node:path";

const ALLOW = new Set([
  "0BSD",
  "Apache-2.0",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "BSL-1.0",
  "CC0-1.0",
  "ISC",
  "MIT",
  "MIT-0",
  "MPL-2.0", // 文件级 copyleft：不改它的文件就没事；安装包里带的是未修改的构建产物
  "Unicode-3.0",
  "Unlicense",
  "Zlib",
  "BlueOak-1.0.0",
]);

const DENY = new Set([
  "GPL-2.0",
  "GPL-2.0-only",
  "GPL-2.0-or-later",
  "GPL-3.0",
  "GPL-3.0-only",
  "GPL-3.0-or-later",
  "AGPL-3.0",
  "AGPL-3.0-only",
  "AGPL-3.0-or-later",
  "LGPL-2.1",
  "LGPL-2.1-only",
  "LGPL-2.1-or-later",
  "LGPL-3.0",
  "LGPL-3.0-only",
  "LGPL-3.0-or-later",
  "SSPL-1.0",
  "CDDL-1.0",
  "CDDL-1.1",
  "EPL-2.0",
  "EUPL-1.2",
  "CC-BY-SA-4.0",
  "BUSL-1.1",
  "Elastic-2.0",
  "Commons-Clause",
  "JSON",
]);

const packages = [];
function walk(dir, depth) {
  if (depth > 6) return;
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    if (!e.isDirectory() || e.name.startsWith(".")) continue;
    const p = path.join(dir, e.name);
    if (e.name.startsWith("@")) {
      walk(p, depth + 1);
      continue;
    }
    const pj = path.join(p, "package.json");
    if (fs.existsSync(pj)) {
      try {
        const j = JSON.parse(fs.readFileSync(pj, "utf8"));
        let lic = j.license;
        if (!lic && Array.isArray(j.licenses)) lic = j.licenses.map((x) => (typeof x === "string" ? x : x?.type)).filter(Boolean).join(" OR ");
        if (!lic && typeof j.license === "object") lic = j.license?.type;
        packages.push({ name: j.name || e.name, version: j.version || "?", license: String(lic || "").trim() });
      } catch {
        // 坏 package.json 直接跳过，不因为一个坏包炸掉整个检查
      }
    }
    const nested = path.join(p, "node_modules");
    if (fs.existsSync(nested)) walk(nested, depth + 1);
  }
}
walk("node_modules", 0);

function tokensOf(expr) {
  return expr
    .replace(/[()]/g, " ")
    .split(/\s+(?:OR|AND)\s+/i)
    .map((t) => t.trim())
    .filter(Boolean);
}

const problems = [];
const summary = new Map();
for (const p of packages) {
  const expr = p.license || "(none)";
  const tokens = tokensOf(expr);
  for (const t of tokens) summary.set(t, (summary.get(t) || 0) + 1);
  if (!expr || expr === "UNLICENSED" || expr === "(none)") {
    problems.push(`${p.name}@${p.version}: 没有 license 字段`);
    continue;
  }
  const denied = tokens.filter((t) => DENY.has(t));
  const allowed = tokens.filter((t) => ALLOW.has(t));
  const unknown = tokens.filter((t) => !DENY.has(t) && !ALLOW.has(t));
  if (denied.length && !allowed.length) problems.push(`${p.name}@${p.version}: 传染性/受限许可 ${expr}`);
  else if (denied.length) problems.push(`${p.name}@${p.version}: 表达式含 ${denied.join(" OR ")}（虽可改选宽松项，但请人工确认后再放行）`);
  else if (unknown.length) problems.push(`${p.name}@${p.version}: 未知许可 ${unknown.join(", ")}（表达式 ${expr}）`);
  else if (!allowed.length) problems.push(`${p.name}@${p.version}: 解析不出许可（表达式 ${expr}）`);
}

const json = process.argv.includes("--json");
if (json) {
  console.log(JSON.stringify({ packages: packages.length, licenses: Object.fromEntries(summary), problems }, null, 2));
} else {
  console.log(`扫描 node_modules：${packages.length} 个包`);
  console.log("许可分布：" + [...summary.entries()].sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k}×${v}`).join(" · "));
  if (problems.length) {
    console.log("\n✗ 有问题的包：");
    for (const x of problems) console.log("  - " + x);
  } else {
    console.log("\n✅ 全部在白名单内（无 GPL/AGPL/LGPL/SSPL 等传染性许可）");
  }
}
process.exit(problems.length ? 1 : 0);
