// 命令面审计（2026-11-10）。
//
// 为什么要有它：`#[tauri::command]` 只要注册进 `generate_handler![]`，主窗口的 JS 就调得到。
// 框架层收不窄（同一个 window = 同一套权限），所以**唯一能守的是"只增必要的"**：
// 每新增一条命令，它必须有一个真实的消费者 —— 前端某处 `invoke("x")`、插件白名单、
// 或者明确登记在下面的"已知未用"表里（带理由）。否则这条测试红。
//
// 三个集合：
//   ① 前端**字面量** invoke：`invoke("x")` / `invoke<T>("x")`（含 views/hooks/lib 全部）；
//   ② 插件白名单：`src/lib/pluginCommands.ts` 里的命令名（插件通过 host.call 调）；
//   ③ 动态调用：命令名由变量拼出来的（`export_story_*` 那种），显式登记；
//   ④ 已知未用：真没人调的（现在只剩 10 个 dev_* —— 它们在 release 构建里直接返回错误）。
//
// 加一个新命令却被它拦住时，先问：**谁要用它？** 没人用就别注册。

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const ROOT = path.join(__dirname, "..");

/** 后端注册的命令名（去掉模块前缀） */
function registeredCommands() {
  const lib = fs.readFileSync(path.join(ROOT, "src-tauri/src/lib.rs"), "utf8");
  const start = lib.indexOf("tauri::generate_handler![");
  assert.ok(start >= 0, "lib.rs 里找不到 generate_handler![");
  const rest = lib.slice(start + "tauri::generate_handler![".length);
  const end = rest.indexOf("])");
  assert.ok(end >= 0, "handler 列表没有收尾");
  return rest
    .slice(0, end)
    .split(/[,\s]+/)
    .map((t) => t.trim())
    .filter((t) => t && /^[A-Za-z_]/.test(t))
    .map((t) => t.replace(/^.*::/, ""));
}

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (/\.(ts|tsx)$/.test(e.name)) out.push(p);
  }
  return out;
}

/** 前端字面量 invoke 的命令名 */
function literalInvokes() {
  const names = new Set();
  for (const f of walk(path.join(ROOT, "src"))) {
    const src = fs.readFileSync(f, "utf8");
    for (const m of src.matchAll(/invoke(?:<[^>]*>)?\(\s*["'`]([A-Za-z0-9_:]+)["'`]/g)) {
      names.add(m[1]);
    }
  }
  return names;
}

/** 插件白名单里的命令名 */
function pluginCommands() {
  const src = fs.readFileSync(path.join(ROOT, "src/lib/pluginCommands.ts"), "utf8");
  return new Set([...src.matchAll(/name:\s*"([a-z0-9_]+)"/g)].map((m) => m[1]));
}

/** 命令名由变量拼的：这里显式登记，并写明谁拼的 */
const DYNAMIC = new Map([
  ["export_story_md", "useStoryActions.ts：`const cmd = o.fmt === \"md\" ? \"export_story_md\" : …` 再 invoke(cmd)"],
  ["export_story_html", "同上（动态 cmd 变量）"],
  ["export_story_txt", "同上（动态 cmd 变量）"],
]);

/** 已知没有任何消费者：留着有原因，但**不许再多** */
const KNOWN_UNUSED = new Map([
  ["dev_export_text", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
  ["dev_export_diagnostics", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
  ["dev_seed_sample", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
  ["dev_health_check", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
  ["dev_inject_summary", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
  ["dev_inject_proposal", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
  ["dev_rollback", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
  ["dev_snapshots", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
  ["dev_status", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
  ["dev_create_test_story", "开发者工具专用；release 构建里直接返回错误（dev.rs 的 #[cfg(not(debug_assertions))]）"],
]);

test("每条已注册命令都得有消费者（或明确登记为未用）", () => {
  const registered = registeredCommands();
  assert.ok(registered.length > 140, "只解析到 " + registered.length + " 条命令，lib.rs 的格式可能变了");

  const literal = literalInvokes();
  const plugin = pluginCommands();
  const orphans = registered.filter(
    (c) => !literal.has(c) && !plugin.has(c) && !DYNAMIC.has(c) && !KNOWN_UNUSED.has(c),
  );

  assert.deepEqual(
    orphans,
    [],
    `这些命令注册了但没有任何消费者：\n  ${orphans.join("\n  ")}\n` +
      `要么让前端/插件真的用它，要么从 generate_handler![] 里删掉；\n` +
      `如果是有意保留（例如只在开发构建里用），加到本测试的 KNOWN_UNUSED 并写清原因。`,
  );
});

test("前端/插件调的命令都真的注册了（防拼错字）", () => {
  const registered = new Set(registeredCommands());
  const used = new Set([...literalInvokes(), ...pluginCommands(), ...DYNAMIC.keys()]);
  const missing = [...used].filter((c) => !registered.has(c) && c !== "get_api_key");
  assert.deepEqual(
    missing,
    [],
    `这些命令前端/插件在调，但 generate_handler![] 里没有：\n  ${missing.join("\n  ")}`,
  );
});

test("已知未用清单不许留过期的（命令已经有人用了就该删掉这一行）", () => {
  const literal = literalInvokes();
  const plugin = pluginCommands();
  const stale = [...KNOWN_UNUSED.keys()].filter((c) => literal.has(c) || plugin.has(c));
  assert.deepEqual(
    stale,
    [],
    `这些命令已经有人调了，却还挂在 KNOWN_UNUSED 里：\n  ${stale.join("\n  ")}\n（清单过期比缺清单更糟 —— 它会把"没人用"变成假话）`,
  );
});

test("KNOWN_UNUSED 的每一条都写了理由", () => {
  for (const [name, why] of KNOWN_UNUSED) {
    assert.ok(why && why.length >= 8, `${name} 没写为什么没人用`);
  }
  assert.ok(KNOWN_UNUSED.size <= 15, "已知未用的命令超过 15 条了 —— 命令面在膨胀，先清理");
});
