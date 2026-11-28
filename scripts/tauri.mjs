import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Tauri CLI 包装：dev 模式固定使用 src-tauri/target/debug/data（与直接启动 debug exe 一致），
// 避免 worktree 启动时再生成一套新的空库。打包构建不注入该变量。
const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const env = { ...process.env };
const devData = join(root, "src-tauri", "target", "debug", "data");
const devLibrary = join(devData, "library", "default", "library.db");

if (args[0] === "dev" && !env.DREAMWEAVER_DATA_DIR?.trim()) {
  env.DREAMWEAVER_DATA_DIR = devData;
  console.log(`[tauri] dev 数据根 = ${devData}${existsSync(devLibrary) ? "" : "（尚无 library.db，将初始化）"}`);
}

const cli = join(root, "node_modules", "@tauri-apps", "cli", "tauri.js");
const result = spawnSync(process.execPath, [cli, ...args], { cwd: root, env, stdio: "inherit" });
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
