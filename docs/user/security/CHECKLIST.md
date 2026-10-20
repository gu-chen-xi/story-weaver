# 安全核对清单（人用）

> 两步走：**3 分钟版**（只想确认"包没被换"）和**深度版**（想确认"代码没问题"）。
> 每项都可核查；不能核查的，请当"未验证"处理，不要当"通过"。

## A. 3 分钟版（只验"我拿到的包是发布者发的那个"）

- [ ] 下载 `.msi`（或便携 zip）与 `SHA256SUMS.txt`
- [ ] `Get-FileHash .\Story-Weaver_3.1.0_x64_en-US.msi -Algorithm SHA256`，与文件里的值**逐字符**一致
- [ ] 安装/运行时遇到 SmartScreen → 「更多信息」→「仍要运行」（没签名，原因见 [WHY-NO-CODE-SIGNING](WHY-NO-CODE-SIGNING.md)）
- [ ] 跑起来后：任务管理器里只有 `app.exe`（+ 系统自带 `msedgewebview2.exe`）

## B. 深度版（10 分钟，验"代码没做坏事"）

- [ ] 仓库能 clone，`src-tauri/src/` 下能读到全部源码（不是只放二进制）
- [ ] `grep -rn "Command::new" src-tauri/src/` → **无匹配**（不启动外部程序）
- [ ] `grep -rn "winreg\|CurrentVersion.Run\|schtasks" src-tauri/src/` → **无匹配**（不驻留）
- [ ] 硬编码域名清单里**没有**统计/埋点/短链域名（对照 [AGENT-AUDIT-GUIDE §3.1](AGENT-AUDIT-GUIDE.md)）
- [ ] `src-tauri/src/api.rs` 的 URL 来自配置（`model.base_url`），不是常量
- [ ] `src-tauri/src/keys.rs` 用 Windows 凭据管理器存 Key，且有清理历史明文的逻辑
- [ ] 自己 `npm ci && npm run tauri build` 能编出可运行的 exe（能编 = 源码完整）
- [ ] 抓包（Fiddler/mitmproxy）只见你配置的 API 域名；静置 10 分钟无心跳
- [ ] 数据只写在 `%APPDATA%\com.guchenxi.story-weaver`（与 `STORAGE.md` 一致）
- [ ] 用 `grep -a -r "sk-" "$APPDATA/com.guchenxi.story-weaver"` → 数据库里没有明文 Key

## C. 如果哪一项对不上

1. 先确认不是你操作问题（路径/工具差异）；
2. 仍对不上 → GitHub Issues 贴 **项号 + 命令 + 实际输出**；
3. 别运行可疑的包（可以先只用源码版）。

---

## 附：本软件**明确不做**的事（可作为核对基准）

| 不做 | 证据位置 |
|---|---|
| 不启动外部程序 / 脚本 | `src-tauri/src/**` 无 `Command::new` |
| 不写注册表自启 / 服务 / 计划任务 | 同上，无 `winreg` / `schtasks` |
| 不上报使用数据（无遥测）| 无统计域名；网络出口仅 `api.rs` / `generator_cloud.rs` |
| 不把 API Key 写入文件/数据库 | `keys.rs` → Windows 凭据管理器 |
| 不修改系统设置 | 无注册表写入代码 |
| 不自动更新 / 不静默下载 | 无 auto-updater 依赖 |

反过来，它**会**做的事（也请核对）：连你配置的大模型 API、读写自己的数据目录、
导出文件到**你指定**的位置、生成图片时加「AI生成」标识（法规要求）。
