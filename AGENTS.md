# 项目指令：织梦机 / Story-Weaver

> 本文件只管**这个仓库**。全局行为规则（人设、git push 规矩）在 `~/.dsh/AGENTS.md`。
> 由 dsh-agent-instructions 在会话首个请求注入；项目根以 `.git` 为标记。

## 一、这是什么

Tauri 2 + React 19 + Vite 8 + TypeScript 的 Windows 桌面应用（AI 剧情创作）。

| 位置 | 内容 |
|---|---|
| `src-tauri/src/` | Rust 后端，约 30 个模块，`lib.rs` 是命令注册中心 |
| `src/` | 前端；`App.tsx` 仍在拆分中，页面在 `src/views/`，hooks 在 `src/hooks/` |
| 数据 | SQLite 明文（`library.db` + 每故事一个 `story.db`），**这是设计选择，不是缺陷** |
| 构建 | `npm run build`（tsc+vite）· `npm run tauri build`（发版）· **`npm run sync:run`（本地跑，不要手搓 `cargo build --release`）** · `npm test` · `cargo test` |

安全边界的正式说明在 `SECURITY.md`；自证流程在 `docs/user/security/`。

## 二、安全红线（动这些代码前必须对照）

### 1. 不执行外部程序
后端禁止出现 `Command::new` / `std::process::Command` / `schtasks` / `winreg`；
Cargo.toml 禁止引入 `tauri-plugin-shell`、`-fs`、`-process`。
验证：`grep -rn "Command::new\|tauri_plugin_shell\|tauri_plugin_fs" src-tauri/src/` → 期望 **0 命中**。

### 2. AI 输出只能当文本，不能当指令
不解析 `tool_call` / `function_call`，不做"AI 说什么就执行什么"。
任何"AI 内容改库"必须有人工闸门：
- `evolution.require_approval` 默认 `true` —— 改成默认 `false` 算安全回归
- `apply_check_patch` 必须走 `validate_patch` 字段白名单 + `before` 值核对 + 执行前快照
验证：`grep -rn "tool_call\|function_call" src/ src-tauri/src/` → 期望 **0 命中**。

### 3. 前端不出现注入面
禁止 `dangerouslySetInnerHTML`、`innerHTML`、`outerHTML`、`document.write`、`eval`、`new Function`。
验证：`grep -rn "dangerouslySetInnerHTML\|innerHTML\|outerHTML\|eval(" src/` → 期望 **0 命中**。

### 4. CSP 只能收紧，不能放松
`connect-src` 必须只有 `'self' ipc: http://ipc.localhost`；不得加入任何外部域名（CDN、字体、统计都算）。
`object-src 'none'`、`base-uri 'self'`、`frame-ancestors 'none'` 三条不许删。
验证：读 `src-tauri/tauri.conf.json` 的 `app.security.csp`。

### 5. Tauri 权限只加必要的
`src-tauri/capabilities/` 现在只有 `core:default` + `dialog:default`。新增权限须在提交信息里说明"现有权限为什么不够"。
`assetProtocol.scope = ["**"]` 是 `SECURITY.md §3.2` **明确接受的已知风险**：不要顺手改窄（会裂图），
要走 C 方案（`portrait://` 自定义协议）。

### 6. API Key 只进凭据库
Key 只存 Windows 凭据管理器（`keys.rs` / `keychain.rs`）。禁止写进 `configs` 表、日志、错误串、前端存储。
`scrub_legacy_plaintext()` 的历史明文迁移不许退化。
验证：`grep -rn "sk-[A-Za-z0-9]\{20,\}" src-tauri/src/ src/` → 只应命中测试夹具，不应命中真实 key。

### 7. 依赖安全门禁不能拆
- `.github/workflows/security.yml`：每周一定时 + 依赖文件变更时跑 cargo-deny 与 npm audit
- `.github/dependabot.yml`：每周依赖 PR
- `deny.toml`：`[graph] targets` 只留 Windows（避免 Linux-only crate 的假阳性）；`ignore` 每条必须带 `reason`
动过依赖后本地至少跑一次：`npm audit --omit=dev --audit-level=high`。

## 三、仓库卫生

- **绝不提交**：`*.db`、`*.db-wal`、`*.db-shm`、`*.bak-*`、`.env*`、`*.pem`、`dist-package/`
  —— 这些路径里是真实剧情、立绘和密钥。`dist-package/` 已被 gitignore，**不许用 `git add -f` 绕过**。
- 提交信息沿用现有风格：中文 + `feat:` / `fix:` / `refactor:` / `docs:` / `chore(sec)` 前缀。

## 四、结构守卫

`src-tauri/src/frontend_guard.rs` 卡着 `App.tsx` 行数上限（当前 **1300**，实际 **1223**，2026-10-28 按 LF 实测 —— 旧文档写的 1097 是量错了，见 `docs/dev/错误合集.md` E-DOC-04）。
**只许往下调**；要往上调必须在 ROADMAP 里写明理由。

同文件还有几条硬结构守卫，改前端时会被它们拦住（明细见 `docs/dev/测试用例.md`）：
插件页不许按 id 分支、插件模板不许进注册表、抽出的模块不许抄回 `App.tsx`、
导演监视器区块顺序不许变、`scripts/sync-run.mjs` 的 dev 标志判据不许拆。