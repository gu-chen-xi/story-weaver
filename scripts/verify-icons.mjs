// 大肥鱼图标集的完整性复核（2026-11-23）。
//
// 用法：
//   node scripts/verify-icons.mjs          # 人看：数量 / 一致性 / 哈希
//   node scripts/verify-icons.mjs --json   # 机器读：输出 JSON（可用于打清单）
//
// 它守四件事：
//   ① 图标名与 path 一一对应（50 个，不重不漏）；
//   ② 创作说明 `docs/design/icon-set-AUTHORSHIP.md` 里的清单与代码同步；
//   ③ `package.json` 里没有引入任何图标库（证明"自己画的"不是"装的"）；
//   ④ 打印当前文件 sha256（与创作说明里记录的 v3.17.0 blob 哈希可比对）。

import fs from "node:fs";
import crypto from "node:crypto";

const ICON_FILE = "src/components/icons.tsx";
const DOC_FILE = "docs/design/icon-set-AUTHORSHIP.md";
const PKG_FILE = "package.json";

const src = fs.readFileSync(ICON_FILE, "utf8");
const names = [...src.match(/export type IconName =([\s\S]*?);/)[1].matchAll(/"([a-z]+)"/g)].map((m) => m[1]);
const paths = [...src.matchAll(/^  ([a-z]+): \(/gm)].map((m) => m[1]);

const doc = fs.readFileSync(DOC_FILE, "utf8");
const docNames = [...doc.matchAll(/^\| \d+ \| `([a-z]+)` \|/gm)].map((m) => m[1]);

const pkg = JSON.parse(fs.readFileSync(PKG_FILE, "utf8"));
const allDeps = { ...(pkg.dependencies || {}), ...(pkg.devDependencies || {}) };
const ICON_LIBS = ["lucide-react", "lucide", "react-icons", "heroicons", "@heroicons/react", "feather-icons", "react-feather", "tabler-icons", "@tabler/icons-react", "@fortawesome/fontawesome-free", "@fortawesome/react-fontawesome", "bootstrap-icons", "material-icons", "@mui/icons-material"];
const foundLibs = ICON_LIBS.filter((d) => d in allDeps);

const hash = crypto.createHash("sha256").update(fs.readFileSync(ICON_FILE)).digest("hex").toUpperCase();
const bytes = fs.statSync(ICON_FILE).size;

const problems = [];
if (new Set(names).size !== names.length) problems.push("IconName 里有重名");
if (new Set(paths).size !== paths.length) problems.push("path 定义里有重名");
const missingPath = names.filter((n) => !paths.includes(n));
const extraPath = paths.filter((p) => !names.includes(p));
if (missingPath.length) problems.push(`有名字没 path：${missingPath.join(", ")}`);
if (extraPath.length) problems.push(`有 path 没名字：${extraPath.join(", ")}`);
if (names.length !== docNames.length) problems.push(`说明清单 ${docNames.length} 个 ≠ 代码 ${names.length} 个`);
const docOnly = docNames.filter((n) => !names.includes(n));
const codeOnly = names.filter((n) => !docNames.includes(n));
if (docOnly.length) problems.push(`清单里有、代码里没有：${docOnly.join(", ")}`);
if (codeOnly.length) problems.push(`代码里有、清单里没有：${codeOnly.join(", ")}`);
if (foundLibs.length) problems.push(`引入了图标库：${foundLibs.join(", ")}（这会让"自己画的"站不住）`);

if (process.argv.includes("--json")) {
  console.log(JSON.stringify({ file: ICON_FILE, bytes, sha256: hash, count: names.length, problems }, null, 2));
} else {
  console.log(`文件 : ${ICON_FILE}（${bytes} 字节）`);
  console.log(`数量 : ${names.length} 个图标（清单 ${docNames.length} 个）`);
  console.log(`哈希 : ${hash}`);
  console.log(`依赖 : ${foundLibs.length ? "✗ " + foundLibs.join(", ") : "✓ 没有图标库"}`);
  if (problems.length) {
    console.log("");
    for (const p of problems) console.log("✗ " + p);
  } else {
    console.log("");
    console.log("✅ 名字/path/清单/依赖 全部一致");
  }
}

process.exit(problems.length ? 1 : 0);
