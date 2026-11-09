// 插件命令**能力域**（2026-11-10）。
//
// 守两件事：
//   ① 表本身要自洽：每条命令都有域、有 why、名字不重复；域清单与命令表对得上；
//   ② 插件**只调自己声明的域**：扫 `src/plugins/builtin/<id>/` 里真实的 `call("…")` 调用，
//      逐条比对那个目录 manifest 里的 `commandCaps` —— 多调、少声明、声明了不用，都算红。
//
// 为什么要有 ②：光有运行时拒绝，很容易"某天有人图省事把命令加进白名单，manifest 忘了改"，
// 或者反过来声明了一堆域却一条不用（等于悄悄放宽）。静态扫源码能在提交前拦住。

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const {
  PLUGIN_COMMANDS,
  PLUGIN_COMMAND_SPECS,
  PLUGIN_CAP_DOMAINS,
  commandCapOf,
  commandsOfCap,
  checkPluginCall,
} = require("../.tmp/fe-tests/src/lib/pluginCommands.js");

const ROOT = path.join(__dirname, "..");

test("表自洽：每条命令都有域、有 why，名字不重复", () => {
  const seen = new Set();
  for (const s of PLUGIN_COMMAND_SPECS) {
    assert.ok(PLUGIN_CAP_DOMAINS.includes(s.cap), `${s.name} 的域不认识：${s.cap}`);
    assert.ok(s.why && s.why.length >= 6, `${s.name} 没写 why（review 时就看这一栏）`);
    assert.ok(!seen.has(s.name), `${s.name} 重复了`);
    seen.add(s.name);
  }
  assert.equal(PLUGIN_COMMANDS.length, PLUGIN_COMMAND_SPECS.length);
  for (const cap of PLUGIN_CAP_DOMAINS) {
    assert.ok(commandsOfCap(cap).length > 0, `域 ${cap} 一条命令都没有 —— 要么删域，要么补命令`);
  }
});

test("commandCapOf：认得的给域，不认得的给 undefined", () => {
  assert.equal(commandCapOf("ollama_get"), "ollama");
  assert.equal(commandCapOf("generator_generate"), "portrait-gen");
  assert.equal(commandCapOf("delete_story"), undefined);
  assert.equal(commandCapOf("generator_delete"), undefined);
});

test("checkPluginCall：声明了才放行，跨域与表外都拒绝", () => {
  assert.equal(checkPluginCall("ollama", ["ollama"], "ollama_get").ok, true);
  assert.equal(checkPluginCall("portrait-gen", ["portrait-gen"], "generator_generate").ok, true);

  // 跨域：立绘插件不能碰 ollama_*
  const cross = checkPluginCall("portrait-gen", ["portrait-gen"], "ollama_set");
  assert.equal(cross.ok, false);
  assert.match(cross.message, /没有声明能力域「ollama」/);

  // 一条都没声明
  const none = checkPluginCall("stat-growth", [], "ollama_probe");
  assert.equal(none.ok, false);
  assert.match(none.message, /没有声明能力域/);

  // 表外（危险命令不在白名单里）
  const outside = checkPluginCall("evil", ["ollama", "portrait-gen"], "delete_story");
  assert.equal(outside.ok, false);
  assert.match(outside.message, /不在插件命令白名单里/);
});

// ── ② 静态扫源码：插件的真实调用 vs 它声明的域 ──

function pluginDirs() {
  const base = path.join(ROOT, "src/plugins/builtin");
  return fs
    .readdirSync(base, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => path.join(base, e.name));
}

function readTree(dir) {
  const out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) out.push(...readTree(p));
    else if (/\.(ts|tsx)$/.test(e.name)) out.push([p, fs.readFileSync(p, "utf8")]);
  }
  return out;
}

/** 这个插件目录里**真实**调了哪些插件命令 */
function calledCommands(files) {
  const names = new Set();
  for (const [, src] of files) {
    for (const m of src.matchAll(/\bcall(?:<[^>]*>)?\(\s*["']([a-z0-9_]+)["']/g)) {
      names.add(m[1]);
    }
  }
  return names;
}

/** 这个插件 manifest 里声明了哪些能力域（`commandCaps: ["a", "b"]`） */
function declaredCaps(files) {
  for (const [, src] of files) {
    const m = src.match(/commandCaps:\s*\[([^\]]*)\]/);
    if (m) {
      return [...m[1].matchAll(/["']([a-z-]+)["']/g)].map((x) => x[1]);
    }
  }
  return []; // 没写 = 默认 []
}

test("每个内置插件：只调自己声明的域，且声明的域都用得上", () => {
  let checked = 0;
  for (const dir of pluginDirs()) {
    const id = path.basename(dir);
    const files = readTree(dir);
    const declared = declaredCaps(files);
    const called = calledCommands(files);
    checked += 1;

    for (const name of called) {
      const cap = commandCapOf(name);
      assert.ok(cap, `插件「${id}」调了白名单外的命令「${name}」`);
      assert.ok(
        declared.includes(cap),
        `插件「${id}」调了「${name}」（域 ${cap}），但 manifest 的 commandCaps 里没声明 ${cap}（现在声明的是 [${declared.join(", ")}]）`,
      );
    }

    // 声明了却没用到 —— 相当于白拿权限，不允许
    for (const cap of declared) {
      const used = [...called].some((n) => commandCapOf(n) === cap);
      assert.ok(
        used,
        `插件「${id}」声明了能力域 ${cap} 却一条命令都没调 —— 用不上就把声明删掉（只申请真需要的）`,
      );
    }
  }
  assert.ok(checked >= 3, "只扫到 " + checked + " 个插件目录，路径可能不对");
});

test("三维数值插件不申请任何后端命令", () => {
  const dir = path.join(ROOT, "src/plugins/builtin/stat-growth");
  const files = readTree(dir);
  assert.deepEqual(declaredCaps(files), []);
  assert.equal(calledCommands(files).size, 0, "它不该调后端命令");
});

test("宿主按插件绑定 call，不许退回「全量白名单」", () => {
  const src = fs.readFileSync(path.join(ROOT, "src/views/PluginsPage.tsx"), "utf8");
  assert.match(
    src,
    /makePluginCall\(d\.id,\s*caps\.commandCaps\)/,
    "PluginsPage 必须按插件绑定 call（makePluginCall(d.id, caps.commandCaps)）",
  );
  assert.doesNotMatch(
    src,
    /call:\s*pluginCall\b/,
    "`call: pluginCall` 是全量白名单的旧写法 —— 会把能力域绕过去",
  );
});
