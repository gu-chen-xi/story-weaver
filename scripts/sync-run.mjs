// 把「本地直接能双击跑的那份 exe」一步做出来，并且**验证它不是 dev 版**。
//
// 为什么要有这个脚本（2026-10-25，E-BOOT-03 复发）：
//   当时图快直接跑 `cargo build --release`，绕过了 `npm run tauri build` ——
//   没带 `custom-protocol` 特性，Tauri 就认定这是 dev 版，把前端地址填成
//   http://localhost:5173，双击打开是"拒绝连接 / 白屏"。
//   坑在于它**不报错**：构建成功、产物也生成了，只是那份 exe 永远连不上。
//
// 所以这个脚本把四件事钉死：
//   ① 前端先构建 —— 不然内嵌进 exe 的是上一次的旧 dist（另一类静默错误）
//   ② 一定带 `--features custom-protocol`
//   ③ 构建完**验证**：tauri-build 的构建脚本输出里不许出现 `cargo:rustc-cfg=dev`
//      （判据来源：Cargo.toml 注释里写的 tauri/build.rs 判断逻辑；
//        注意**不能**用"exe 里有没有 localhost:5173"来判 —— devUrl 是配置常量，
//        编不编进去跟 dev/release 无关，这么判会把好构建冤枉成坏的）
//   ④ 通过才复制到运行目录，并核对复制后哈希一致
//
// 用法：
//   npm run sync:run              # 构建 + 验证 + 同步到 %LOCALAPPDATA%\DreamWeaverRun
//   node scripts/sync-run.mjs --build-only   # 只构建 + 验证，不复制
//
// 注意：复制前必须先关掉正在运行的 app.exe（Windows 会锁住文件，报 EBUSY）。

import { spawnSync } from "node:child_process";
import {
  closeSync, copyFileSync, existsSync, mkdirSync, openSync, readFileSync, readdirSync, statSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url))); // scripts/ 的上一级
const isWin = process.platform === "win32";
const manifest = join(root, "src-tauri", "Cargo.toml");
const exePath = join(root, "src-tauri", "target", "release", "app.exe");
const buildDir = join(root, "src-tauri", "target", "release", "build");
const buildOnly = process.argv.includes("--build-only");

function die(msg) {
  console.error("\n❌ " + msg);
  process.exit(1);
}

function step(title) {
  console.log("\n" + title);
}

function run(cmd, args, cwd) {
  const r = spawnSync(cmd, args, { cwd, stdio: "inherit" });
  if (r.error) die(`命令起不来：${cmd}（${r.error.message}）`);
  if (r.status !== 0) die(`命令失败：${cmd} 退出码 ${r.status}`);
}

function sha256(p) {
  return createHash("sha256").update(readFileSync(p)).digest("hex");
}

/** 目标 exe 是否被正在运行的进程锁着（Windows 拒绝写共享 → 打开失败即"在跑"） */
function isLocked(p) {
  if (!existsSync(p)) return false;
  try {
    closeSync(openSync(p, "r+"));
    return false;
  } catch {
    return true;
  }
}

// 目标目录：默认 %LOCALAPPDATA%\DreamWeaverRun；DW_RUN_DIR 可覆盖（自测用）
const localAppData = process.env.LOCALAPPDATA;
const dstDir = process.env.DW_RUN_DIR ?? (localAppData ? join(localAppData, "DreamWeaverRun") : "");
const dst = dstDir ? join(dstDir, "app.exe") : "";

// 先探一次：跑着的话直接说，省得白等一分半的构建
if (!buildOnly) {
  if (!dstDir) die("LOCALAPPDATA 不存在 —— 这个脚本只用于 Windows（或用 DW_RUN_DIR 指定目标目录）");
  if (isLocked(dst)) die(`运行目录里的 app.exe 正在运行 —— 先关掉那个窗口再跑\n   （目标：${dst}）`);
}

// ① 前端：dist 必须是这次的
step("① 构建前端（tsc + vite build → dist/）");
run(isWin ? "cmd.exe" : "npm", isWin ? ["/c", "npm", "run", "build"] : ["run", "build"], root);
if (!existsSync(join(root, "dist", "index.html"))) die("dist/index.html 不存在 —— 前端没构建成功");

// ② exe：特性是"打包版"的开关，漏了就变成 dev 版
step("② 构建 release exe（强制带 custom-protocol）");
const startedAt = Date.now();
run("cargo", ["build", "--release", "--features", "custom-protocol", "--manifest-path", manifest], root);

// ③ 验证：只看构建脚本的 cfg 输出，这是唯一可靠的判据
step("③ 验证：这次构建不能带 dev 标志");
if (!existsSync(buildDir)) die("找不到 " + buildDir + "（target 被清过？先跑一次完整构建）");
const outputs = readdirSync(buildDir, { withFileTypes: true })
  .filter((d) => d.isDirectory() && d.name.startsWith("app-"))
  .map((d) => {
    const p = join(buildDir, d.name, "output");
    return existsSync(p) ? { dir: d.name, path: p, mtime: statSync(p).mtimeMs } : null;
  })
  .filter(Boolean)
  .sort((a, b) => b.mtime - a.mtime);
if (outputs.length === 0) die("找不到 build/app-*/output，无法验证 dev 标志");

// 增量构建可能一个 build script 都没重跑 —— 那就说明磁盘上这份 exe 本来就是按
// 同一套特性编的（特性变了 cargo 必然重编），此时退而检查最近一次的输出。
const fresh = outputs.filter((o) => o.mtime >= startedAt - 2000);
const checked = fresh.length > 0 ? fresh : [outputs[0]];
const devOnes = checked.filter((o) => readFileSync(o.path, "utf8").includes("cargo:rustc-cfg=dev"));
for (const o of checked) {
  console.log(
    `   ${devOnes.includes(o) ? "✗ dev" : "✓ 打包版"}  ${o.dir}` +
      (fresh.length === 0 ? "（本次没有新的构建脚本输出，改查最近一次）" : ""),
  );
}
if (devOnes.length > 0) {
  die(
    "构建出来的是 dev 版（custom-protocol 没生效）—— 这份 exe 会去连 localhost:5173，" +
      "双击打开是白屏/拒绝连接（错误合集 E-BOOT-03）",
  );
}

if (!existsSync(exePath)) die("构建产物不存在：" + exePath);

if (buildOnly) {
  step("（--build-only：跳过复制）");
  console.log(`\n✅ 构建 + 验证通过：${exePath}\n`);
  process.exit(0);
}

// ④ 同步：关着才能复制（Windows 会锁正在运行的 exe）
step("④ 同步到运行目录");
mkdirSync(dstDir, { recursive: true });
try {
  copyFileSync(exePath, dst);
} catch (e) {
  if (["EBUSY", "EPERM", "EACCES"].includes(e.code)) {
    die("运行目录里的 app.exe 正在运行 —— 先关掉那个窗口，再跑一次");
  }
  die("复制失败：" + e.message);
}
const srcHash = sha256(exePath);
const dstHash = sha256(dst);
if (srcHash !== dstHash) die("复制后哈希不一致，别用这份（磁盘/占用问题）");
const conf = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
console.log(`   → ${dst}`);
console.log(`   版本 ${conf.version} · ${(statSync(dst).size / 1048576).toFixed(2)} MB · sha256 ${dstHash}`);
console.log("\n✅ 可以双击运行了（内嵌 dist，不会去连 localhost:5173）\n");
