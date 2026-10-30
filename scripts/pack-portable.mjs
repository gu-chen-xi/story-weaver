// 便携版 zip 的打包 + 哈希清单，一条命令出全套。
//
// 为什么要有这个脚本（2026-10-30）：
//   3.11.0 发版前，便携 zip 是**手搓**的（资源管理器/压缩工具随手一压），结果三件事：
//     ① 包里那份 README.txt 忘了同步测试项数（写着 Rust 465，实际 472）—— 用户一解压就看到错数字；
//     ② 换个工具/换个时间再压一次，**哈希就变**，而发布说明里已经写死了 sha256，对不上；
//     ③ SHA256SUMS 由 PowerShell `Out-File` 生成，带 BOM + 混行尾（203 字节），
//        `sha256sum -c` 这类工具会被 BOM 噎住。
//   所以这里把三件事钉死：
//     ① 包内**只准**两个文件（README.txt + Story-Weaver.exe），别的文件一律报错退出；
//     ② 打包前把两个文件的 mtime **钉成同一个值**（默认取 exe 的 mtime，截到整秒）——
//        同样的文件内容 + 同样的 mtime ⇒ 同样的 zip 字节 ⇒ 同样的哈希，重打可复现；
//     ③ 哈希清单自己写：UTF-8 **无 BOM**、CRLF、两行，与发布说明逐字符可比。
//
// 用法：
//   node scripts/pack-portable.mjs            # 版本号取自 package.json
//   node scripts/pack-portable.mjs 3.11.0     # 指定版本
//   node scripts/pack-portable.mjs 3.11.0 --stamp 2026-10-30T23:13:26   # 手动钉时间戳
//   node scripts/pack-portable.mjs 3.11.0 --check    # 只校验，不重打（哈希与清单是否一致）
//
// 前置：dist-package/Story-Weaver-<版本>-portable/ 里已经有 README.txt 与 Story-Weaver.exe
//       （exe 来自 `npm run tauri build` 的 target/release/app.exe）。
// 注意：`dist-package/` 已 gitignore —— 产物不进仓库，脚本进仓库。

import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { execFileSync } from "node:child_process";

const DIST = "dist-package";
const argv = process.argv.slice(2);
const flags = new Set(argv.filter((a) => a.startsWith("--")));
const positional = argv.filter((a) => !a.startsWith("--"));

const pkgVersion = JSON.parse(fs.readFileSync("package.json", "utf8")).version;
const version = positional[0] || pkgVersion;

const stampArgIdx = argv.indexOf("--stamp");
const stampArg = stampArgIdx >= 0 ? argv[stampArgIdx + 1] : null;

const msi = path.join(DIST, `Story-Weaver_${version}_x64_en-US.msi`);
const dir = path.join(DIST, `Story-Weaver-${version}-portable`);
const zip = path.join(DIST, `Story-Weaver-${version}-portable.zip`);
const sumsFile = path.join(DIST, `SHA256SUMS-${version}.txt`);
const sumsCopy = path.join(DIST, "SHA256SUMS.txt");

const EXPECTED = ["README.txt", "Story-Weaver.exe"];

const sha256 = (f) => crypto.createHash("sha256").update(fs.readFileSync(f)).digest("hex");
const human = (n) => (n / 1048576).toFixed(2) + " MB";

function die(msg) {
  console.error("✗ " + msg);
  process.exit(1);
}

// ---------- 0. 前置检查 ----------
if (!fs.existsSync(msi)) die(`找不到安装包：${msi}（先跑 npm run tauri build）`);
if (!fs.existsSync(dir)) die(`找不到便携目录：${dir}`);

const got = fs.readdirSync(dir).sort();
const missing = EXPECTED.filter((f) => !got.includes(f));
const extra = got.filter((f) => !EXPECTED.includes(f));
if (missing.length) die(`便携目录缺文件：${missing.join(", ")}`);
if (extra.length) die(`便携目录多了文件：${extra.join(", ")}（包里只该有两个文件，先清掉再打）`);

// ---------- 1. 只校验模式 ----------
if (flags.has("--check")) {
  if (!fs.existsSync(zip) || !fs.existsSync(sumsFile)) die("zip 或哈希清单不存在，没法校验");
  const z = sha256(zip);
  const m = sha256(msi);
  const listed = fs.readFileSync(sumsFile, "utf8");
  const ok = listed.includes(z) && listed.includes(m);
  console.log(`msi  ${fs.statSync(msi).size}  ${m}`);
  console.log(`zip  ${fs.statSync(zip).size}  ${z}`);
  console.log(ok ? "✅ 清单与产物一致" : "✗ 清单与产物不一致");
  process.exit(ok ? 0 : 1);
}

// ---------- 2. 钉 mtime（可复现的关键一步）----------
const exePath = path.join(dir, "Story-Weaver.exe");
const exeStat = fs.statSync(exePath);
// DOS 时间戳的粒度是 2 秒，截到整秒再对齐偶数，避免不同文件系统的小数部分造成差异
const base = stampArg ? new Date(stampArg) : exeStat.mtime;
if (Number.isNaN(base.getTime())) die(`--stamp 不是合法时间：${stampArg}`);
const stamp = new Date(Math.floor(base.getTime() / 1000) * 1000);
for (const f of EXPECTED) fs.utimesSync(path.join(dir, f), stamp, stamp);
console.log(`mtime 钉死：${stamp.toISOString()}（本地 ${stamp.toLocaleString()}）`);

// ---------- 3. 打 zip ----------
if (fs.existsSync(zip)) fs.rmSync(zip);
execFileSync(
  "powershell.exe",
  [
    "-NoProfile",
    "-Command",
    `Compress-Archive -Path "${dir}\\*" -DestinationPath "${zip}" -CompressionLevel Optimal`,
  ],
  { stdio: "inherit" }
);

// ---------- 4. 回读包内清单（防"打进去一堆临时文件"）----------
const entries = execFileSync(
  "powershell.exe",
  [
    "-NoProfile",
    "-Command",
    `Add-Type -AssemblyName System.IO.Compression.FileSystem; ` +
      `$z=[IO.Compression.ZipFile]::OpenRead("${path.resolve(zip)}"); ` +
      `$z.Entries | ForEach-Object { "$($_.FullName)|$($_.Length)" }; $z.Dispose()`,
  ],
  { encoding: "utf8" }
)
  .trim()
  .split(/\r?\n/)
  .filter(Boolean);
const names = entries.map((l) => l.split("|")[0]).sort();
if (JSON.stringify(names) !== JSON.stringify([...EXPECTED].sort())) {
  die(`包内文件不对：${names.join(", ")}（应为 ${EXPECTED.join(", ")}）`);
}

// ---------- 5. 写哈希清单（无 BOM、CRLF）----------
const msiHash = sha256(msi);
const zipHash = sha256(zip);
const lines =
  [`${msiHash}  ${path.basename(msi)}`, `${zipHash}  ${path.basename(zip)}`].join("\r\n") + "\r\n";
fs.writeFileSync(sumsFile, lines, "utf8");
fs.writeFileSync(sumsCopy, lines, "utf8");

// ---------- 6. 汇总 ----------
console.log("");
console.log(`msi   ${fs.statSync(msi).size} 字节（${human(fs.statSync(msi).size)}）  ${msiHash}`);
console.log(`zip   ${fs.statSync(zip).size} 字节（${human(fs.statSync(zip).size)}）  ${zipHash}`);
console.log(`清单  ${fs.statSync(sumsFile).size} 字节（无 BOM）  ${path.basename(sumsFile)} + SHA256SUMS.txt`);
console.log("");
console.log("包内：");
for (const l of entries.sort()) console.log("  " + l.replace("|", "  "));
console.log("");
console.log("下一步：把上面两行 sha256 填进 docs/user/releases/<版本>.md 与 README 下载表，再跑 npm run doclinks。");
