import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Tauri CLI 包装：dev 模式优先使用仓库根部的 data\，避免 worktree / debug 启动时
// 落到 src-tauri/target/debug/data 新建一座空库。打包构建不注入该变量。
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const env = { ...process.env };
const devData = join(root, "data");
const devLibrary = join(devData, "library", "default", "library.db");

if (args[0] === "dev" && !env.DREAMWEAVER_DATA_DIR?.trim() && existsSync(devLibrary)) {
  env.DREAMWEAVER_DATA_DIR = devData;
  console.log(`[tauri] dev 数据根 = ${devData}`);
}

const cli = join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
const result = spawnSync(process.execPath, [cli, ...args], { cwd: root, env, stdio: "inherit" });
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
