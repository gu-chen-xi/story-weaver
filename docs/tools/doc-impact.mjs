#!/usr/bin/env node
/**
 * doc-impact.mjs —— 改完代码，反查该动哪些文档
 *
 * 设计说明见 docs/dev/文档更新顺序.md（§三 的矩阵就是这里的 RULES）。
 * 零依赖，只用 Node 内置模块。放在 docs/tools/ 下：这是文档侧工具，不进根目录。
 *
 * 用法：
 *   node docs/tools/doc-impact.mjs                      # 未提交改动（含暂存）
 *   node docs/tools/doc-impact.mjs --staged             # 只看暂存区
 *   node docs/tools/doc-impact.mjs --range origin/main..HEAD
 *   node docs/tools/doc-impact.mjs --grep 世界书          # 反查：谁提到了它
 *   node docs/tools/doc-impact.mjs --rules              # 打印规则表
 */
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, "..", "..");

const RULES = [
  {
    id: "plugins",
    why: "插件系统 / 插件契约变了（加插件、改宿主数据、改卡片）",
    match: /^src\/plugins(\.tsx|\/)/,
    p0: ["docs/dev/PLUGINS.md", "docs/dev/测试用例.md（守卫项）"],
    p1: ["docs/dev/UI_TREE.md（§七 插件页）", "docs/dev/SPEC.md（插件行为）"],
  },
  {
    id: "ui",
    why: "界面结构变了",
    match: /^src\/(views\/|App\.tsx|plugins\.tsx|hooks\/)/,
    p0: ["docs/dev/UI_TREE.md"],
    p1: ["docs/dev/SPEC.md（§5 页面构成）", "README.md（界面截图/简介）"],
  },
  {
    id: "compliance",
    why: "条款文案 / 许可展示变了（用户点「同意」那层）",
    match: /^(src\/compliance\.ts|src\/views\/CompliancePage\.tsx|LICENSE|LICENSE-CONTENT)$/,
    p0: ["README.md（§十 许可协议）", "SECURITY.md", "docs/user/security/*"],
    p1: ["src/App.tsx（同意弹窗）", "package.json / src-tauri/Cargo.toml（license 字段）"],
  },
  {
    id: "security",
    why: "安全边界变了（权限 / Key 存储）",
    match: /^(src-tauri\/capabilities\/|src-tauri\/src\/(keys|keychain)\.rs)/,
    p0: ["SECURITY.md", "docs/user/security/*（4 份一起改）"],
    p1: ["AGENTS.md（红线那节）", "README.md（§安全说明）"],
  },
  {
    id: "prompt",
    why: "提示词拼装变了",
    match: /^src-tauri\/src\/(prompt|generate_stream)\.rs$/,
    p0: ["docs/dev/提示词规则.md", "docs/dev/SPEC.md（§6 提示词与上下文）"],
    p1: ["docs/dev/错误合集.md（踩坑就加一条）", "docs/dev/测试用例.md"],
  },
  {
    id: "db",
    why: "数据模型 / 建库迁移变了",
    match: /^src-tauri\/src\/(story_db|db)\.rs$/,
    p0: ["docs/dev/SPEC.md（§3 数据模型）", "docs/user/数据与备份.md"],
    p1: ["docs/dev/错误合集.md", "docs/dev/测试用例.md（空库回归）"],
  },
  {
    id: "backend",
    why: "后端逻辑变了",
    match: /^src-tauri\/src\//,
    p0: ["docs/dev/测试用例.md"],
    p1: ["docs/dev/SPEC.md", "docs/dev/错误合集.md"],
  },
  {
    id: "frontend",
    why: "前端逻辑变了",
    match: /^src\//,
    p0: ["docs/dev/测试用例.md（src/lib 纯函数改动需补测试）"],
    p1: ["docs/dev/SPEC.md"],
  },
  {
    id: "selfcheck",
    why: "启动自检工具变了",
    match: /^scripts\/selfcheck\//,
    p0: ["docs/dev/自检工具.md", "docs/dev/测试用例.md"],
    p1: ["README.md（若写了检查项数）"],
  },
  {
    id: "tests",
    why: "测试文件变了",
    match: /^(tests\/|scripts\/test-frontend\.mjs)/,
    p0: ["docs/dev/测试用例.md"],
    p1: [],
  },
  {
    id: "version",
    why: "版本号 / 构建配置变了 → 按发版清单走",
    match: /^(package\.json|package-lock\.json|src-tauri\/(tauri\.conf\.json|Cargo\.toml|Cargo\.lock))$/,
    p0: [
      "README.md（徽章 / 下载链接 / 校验命令 / 版本记录）",
      "SECURITY.md（对应版本 + 支持范围）",
      "docs/README.md（最后同步）",
      "docs/dev/ROADMAP.md（产品版本）",
      "docs/dev/开发流程.md（当前版本）",
      "docs/user/security/ 四份（文件名里的版本号）",
    ],
    p1: ["docs/user/releases/<新版本>.md（发布正文）"],
  },
  {
    id: "ci",
    why: "CI / 依赖门禁变了",
    match: /^(\.github\/|deny\.toml$)/,
    p0: ["SECURITY.md（依赖扫描那节）"],
    p1: ["docs/dev/开发规范.md"],
  },
  {
    id: "docs",
    why: "文档自己变了",
    match: /^(docs\/|README\.md$|SECURITY\.md$|AGENTS\.md$)/,
    p0: ["跑一遍链接校验：npm run doclinks"],
    p1: ["docs/README.md（索引是否需要加/改行）"],
  },
];

function git(args) {
  try {
    return execSync(`git ${args}`, { cwd: ROOT, encoding: "utf8" }).trim();
  } catch {
    return "";
  }
}

function changedFiles(argv) {
  if (argv.includes("--staged")) return git("diff --cached --name-only").split("\n").filter(Boolean);
  const i = argv.indexOf("--range");
  if (i >= 0 && argv[i + 1]) return git(`diff --name-only ${argv[i + 1]}`).split("\n").filter(Boolean);
  return git("diff --name-only HEAD").split("\n").filter(Boolean);
}

function walkDocs(dir, acc = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "archive") continue;
      walkDocs(p, acc);
    } else if (entry.name.endsWith(".md")) acc.push(p);
  }
  return acc;
}

function runRules(files) {
  const hit = new Map();
  for (const f of files) {
    const norm = f.replace(/\\/g, "/");
    for (const r of RULES) {
      if (!r.match.test(norm)) continue;
      if (!hit.has(r.id)) hit.set(r.id, { rule: r, files: [] });
      hit.get(r.id).files.push(norm);
    }
  }
  return hit;
}

const argv = process.argv.slice(2);
const norm = (s) => s.replace(/\\/g, "/");

if (argv.includes("--rules")) {
  console.log("路径规则表（对应 docs/dev/文档更新顺序.md §三）：\n");
  for (const r of RULES) {
    console.log(`  ${r.id.padEnd(10)} ${String(r.match).padEnd(58)} ${r.why}`);
  }
  process.exit(0);
}

const gi = argv.indexOf("--grep");
if (gi >= 0) {
  const term = argv[gi + 1];
  if (!term) {
    console.error("用法：--grep <关键词>");
    process.exit(1);
  }
  const roots = [path.join(ROOT, "docs"), ROOT];
  let n = 0;
  for (const r of roots) {
    const files = r === ROOT
      ? ["README.md", "SECURITY.md", "AGENTS.md"].map((f) => path.join(ROOT, f)).filter(fs.existsSync)
      : walkDocs(r);
    for (const f of files) {
      const rel = norm(path.relative(ROOT, f));
      if (rel.startsWith("docs/tools/")) continue;
      const lines = fs.readFileSync(f, "utf8").split(/\r?\n/);
      lines.forEach((l, i) => {
        if (l.includes(term)) {
          n++;
          console.log(`  ${rel}:${i + 1}  ${l.trim().slice(0, 100)}`);
        }
      });
    }
  }
  console.log(`\n命中 ${n} 处（已跳过 docs/archive）`);
  process.exit(0);
}

const files = changedFiles(argv);
if (!files.length) {
  console.log("没有检测到改动（工作区干净）。");
  console.log("想查某个词在哪：node docs/tools/doc-impact.mjs --grep <关键词>");
  process.exit(0);
}

console.log(`检测到 ${files.length} 个改动文件\n`);
const hit = runRules(files);
if (!hit.size) {
  console.log("没有规则命中 —— 可能是新目录，记得手动确认，并考虑在 RULES 里补一条。");
  process.exit(0);
}

const p0 = new Set();
const p1 = new Set();
for (const { rule, files: fs_ } of hit.values()) {
  console.log(`▶ ${rule.why}（${rule.id}）`);
  for (const f of fs_.slice(0, 6)) console.log(`    · ${f}`);
  if (fs_.length > 6) console.log(`    · …另有 ${fs_.length - 6} 个`);
  rule.p0.forEach((d) => p0.add(d));
  rule.p1.forEach((d) => p1.add(d));
}

/**
 * 把「docs/dev/SPEC.md（§3 数据模型）」和「docs/dev/SPEC.md（§6 提示词）」合并成
 * 「docs/dev/SPEC.md（§3 数据模型、§6 提示词）」——同一份文件只出现一次。
 */
function mergeDocList(list) {
  const map = new Map(); // base -> Set(qualifier)
  for (const d of list) {
    const m = d.match(/^([^（(]+?)(?:[（(](.+)[)）])?$/);
    const base = (m?.[1] ?? d).trim();
    const q = m?.[2];
    if (!map.has(base)) map.set(base, new Set());
    if (q) map.get(base).add(q.trim());
  }
  return [...map.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([base, qs]) => (qs.size ? `${base}（${[...qs].sort().join("、")}）` : base));
}

const p0List = mergeDocList([...p0]);
const p1List = mergeDocList([...p1]).filter((d) => !p0List.some((x) => x.startsWith(d.split("（")[0]) && x.includes(d.split("（")[0])));

console.log("\n必改（P0）：");
p0List.forEach((d) => console.log(`  ✱ ${d}`));
if (p1List.length) {
  console.log("\n应该扫一眼（P1）：");
  p1List.forEach((d) => console.log(`  · ${d}`));
}
console.log("\n提醒：改完跑 `npm run doclinks`（链接校验）。");
console.log("完整顺序见 docs/dev/文档更新顺序.md");