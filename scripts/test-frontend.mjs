// 前端纯逻辑测试的运行器（零依赖：Node 自带 `node:test` + `node:assert`）。
//
// 流程：
//   1) 用 tsconfig.split.json 把 `src/lib`（纯逻辑）编译成 CommonJS 到 `.tmp/fe-tests/`
//   2) 在该目录写一个 {"type":"commonjs"} 的 package.json
//      （根 package.json 是 "type":"module"，不加这个 Node 会把 .js 当 ESM 跑，require 会炸）
//   3) `node --test tests` 跑 `tests/*.test.cjs`
//
// 为什么不用 vitest：本机 npm 网络不通（ECONNRESET），装不了；
// 也装不了 @types/node，所以**测试文件写成纯 .cjs**（不需要类型包），
// 只有被测试的 `src/lib` 走 tsc 编译。
//
// 不要用 npx：Node 24 在 Windows 上直接 spawn `npx.cmd` 会 EINVAL；
// 这里用当前 node 直接跑 `node_modules/typescript/bin/tsc`。

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = dirname(here); // scripts/ 的上一级 = 项目根
const outDir = join(root, ".tmp", "fe-tests");
const tsc = join(root, "node_modules", "typescript", "bin", "tsc");

if (!existsSync(tsc)) {
  console.error("[fe-test] 找不到 " + tsc + " —— 先跑一次 npm install");
  process.exit(1);
}

rmSync(outDir, { recursive: true, force: true });

console.log("[fe-test] 编译 src/lib → .tmp/fe-tests …");
execFileSync(process.execPath, [tsc, "-p", "tsconfig.split.json"], {
  cwd: root,
  stdio: "inherit",
});

mkdirSync(outDir, { recursive: true });
writeFileSync(join(outDir, "package.json"), JSON.stringify({ type: "commonjs" }, null, 2));

console.log("[fe-test] 运行 node --test tests …\n");
execFileSync(process.execPath, ["--test", "tests/*.test.cjs"], {
  cwd: root,
  stdio: "inherit",
});
