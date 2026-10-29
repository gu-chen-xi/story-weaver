# 安全策略（SECURITY）

> 适用于：织梦机 / Story-Weaver（Windows 桌面版）
> 最后更新：2026-10-29 ｜ 对应版本 **v3.10.0**（最新发布版）
>
> 本文件是**威胁模型与安全边界的正式说明**。
> "怎么自己动手验证我说的是真的" 在 [`docs/user/security/`](docs/user/security/README.md)。

---

## 一、报告安全问题

* **首选**：GitHub Issue（公开仓库：<https://github.com/gu-chen-xi/story-weaver/issues>）。
  一般性问题直接开 Issue 即可。
* 如果问题敏感（例如：可能读出你本地任意文件、可能外发数据），
  请用 GitHub 的 [私密漏洞报告](https://github.com/gu-chen-xi/story-weaver/security/advisories/new)。
* 作者是个人开发者（学生），**没有 SLO 承诺**，但会认真看每一条。
  能复现的问题就修；修不了的会写进本文档「已知限制」。

**支持范围**：只支持**最新发布版本**（当前 v3.10.0）。旧版本不打补丁。

---

## 二、我们只验证了什么（每条都给了命令，超出范围的不下结论）

> 下面的"做法"列写的是**我们核对源码后的结论**，不是对运行时的保证。
> 每条都能用"验证方式"列里的命令重现；**运行时到底发生了什么，只有抓包能回答**
> （见 `docs/user/security/AGENT-AUDIT-GUIDE.md` §3.3）。

| 项 | 做法 | 验证方式 |
|---|---|---|
| API Key 存储 | 只进 **Windows 凭据管理器**；`configs` 表不存明文；历史明文键由 `scrub_legacy_plaintext()` 启动时迁移并清空 | `src-tauri/src/keys.rs` |
| 不写文件存 Key | 只验证了：**源码里**没有把 Key 落盘的调用点（依赖层与编译器行为未审）| 同上 + `grep -a "sk-" 数据目录` |
| 不启动外部程序 | 只验证了：**源码里**无 `Command::new` / `cmd /c` / `powershell` / `schtasks`（`Cargo.toml` 也未引入 shell/fs 插件）| `grep -rn "Command::new" src-tauri/src/` |
| 不驻留 | 只验证了：源码里没有写注册表自启 / 装服务 / 建计划任务的调用；"关窗即退出"请用任务管理器实测 | `grep -rn "winreg\|schtasks" src-tauri/src/` |
| 未发现遥测/埋点 | 只验证了：源码里没有统计、上报、短链域名 | `docs/user/security/AGENT-AUDIT-GUIDE.md` §3.1 |
| 出站调用点 | 只验证了：源码里**硬编码**的外部地址只有预置默认值（清单见上条 §3.1），实际请求 URL 来自 `model.base_url` 配置。**不覆盖**你系统上的代理 / DNS / 系统级拦截 | `src-tauri/src/api.rs`（URL 来自 `model.base_url`）|
| WebView 网络收紧 | CSP `connect-src 'self' ipc: http://ipc.localhost` —— 清单里**没有**任何外部域名 | `src-tauri/tauri.conf.json` |
| 禁止页面被嵌入/被脚本注入 | CSP `frame-ancestors 'none'`、`object-src 'none'`、`base-uri 'self'`；前端无 `dangerouslySetInnerHTML`/`innerHTML` | 同上 + `grep -rn innerHTML src/` |
| AI 生成图片标识 | 出图自动打「AI生成」可见标识（法规要求，不可关闭）：固定**左上角**，服务方自带水印原样保留、不覆盖（2026-10-25 从右下角改到左上，避免与平台水印叠在同一角）| `src-tauri/src/image_stamp.rs` |
| 数据只写自己的目录 | 只验证了：源码里的写入都经 `paths::*`（数据根 / 导出目录 / 日志 / 临时目录）| `src-tauri/src/paths.rs` |

---

## 三、威胁模型

### 3.1 信任边界

```
[你的电脑]
  ├─ app.exe（Rust/后端）──── 完全权限：读写自己的数据目录、连你配的 API
  └─ WebView2（前端 UI）───── CSP 限制：不能连外网、不能嵌对象、不能读任意文件
                               唯一出口：ipc:（调后端命令）
[外网]
  └─ 你在设置里填的模型 API / 绘图 API（你自己选的，作者无法干预）
```

作者**没有服务器**。这一条我们只验证了：**源码里没有任何指向作者自有域名的地址**（`grep` 可复现地看到清单，见 §3.1）。
至于"运行时到底连了谁"，请按 `AGENT-AUDIT-GUIDE.md` §3.3 抓包 —— 本文档不对抓包结果下结论，"作者收不到"也不是本文档能保证的事。

### 3.2 威胁 A：`assetProtocol.scope = ["**"]`（**已接受的风险**）

**是什么**：`tauri.conf.json` 里

```json
"assetProtocol": { "enable": true, "scope": ["**"] }
```

允许 webview 通过 `asset://localhost/<任意绝对路径>` 读取**本机任意图片/媒体文件**。
前端 5 处 `convertFileSrc(path)` 用它显示立绘。

**为什么不能直接收紧**：
* 数据根可被环境变量 `DREAMWEAVER_DATA_DIR` 指到任意目录；
* 立绘可能是从任何位置**导入**的外部文件。
→ 静态白名单（如 `$APPDATA/**`）会让这两类正常用法**全部裂图**。

**攻击可行吗？逐条评估**：

| 攻击步骤 | 是否可行 | 原因 |
|---|---|---|
| AI 输出里塞 `<img src="asset://...">` | ❌ 不可行 | AI 文本只作为 React 文本节点渲染；无 `dangerouslySetInnerHTML` / `innerHTML` |
| 用 JS 构造 `asset://` URL 并读取内容 | ❌ 不可行 | 无脚本注入点（CSP 禁 `object-src`，无 `innerHTML`，无 `eval` 用法）|
| 把读到的图片**外发**出去 | ❌ 不可行 | CSP `connect-src` 无外部域名；且 `<img>` 跨源会让 canvas 污染，读不到像素 |
| 导入**恶意 story.db**，让 `portraits.path` 指向别的本地图片 | ⚠ **可行** | 结果：该图片被显示在你自己的屏幕上（**仅展示，不能外发**，也不能批量扫描）|

**结论**：这是**"本地图片被本地显示"级别**的问题，需要攻击者先让你导入一个恶意数据库；
无法窃取、无法外传、无法提权。**风险等级：低。作者接受该风险**（A 方案）。

**未来会改（C 方案，已列入 ROADMAP 后续项）**：
后端提供自定义协议 `portrait://<portrait_id>`，前端不再传任意路径，
由后端按数据库记录解析真实路径 —— 从根上堵死这条路。见 `docs/archive/ROADMAP-已完成.md` §10.107。

### 3.3 威胁 B：本地数据库是**明文**（已知限制，非缺陷）

数据（`library.db` + 各故事 `story.db`）是**未加密的 SQLite**。
任何能读取你 Windows 账户文件的人/程序，都能读到你的剧情、设定、提示词。

* 这是**有意选择**：本地单机、无需密码、方便你随时备份/迁移/直接查库。
* 如果你需要加密，方案是给数据目录套 BitLocker/VeraCrypt，而不是在应用里做"半吊子加密"。

### 3.4 威胁 C：调试记录含完整提示词

`debug_requests` 表会存 system_prompt + 历史 + 你的输入（用于「生成参数」页展示装配）。
它**只在本地**，但会随 `library.db` 一起被复制/分享。

* 默认上限 `debug.max_rows = 300`（每次生成前自动裁剪）；
* 设置 → **数据管理** 可一键清空；
* **分享数据库给别人前，请先清空**。

### 3.5 威胁 D：你配置的第三方 API

本软件会把你写的剧情与提示词发给你在「设置 → API」里填的那个服务。
**那个服务怎么处理你的数据，本软件无法控制，也不做任何承诺**。选谁、信不信，由你判断。

---

## 四、已知限制（如实列出）

| 限制 | 影响 | 状态 |
|---|---|---|
| **无代码签名** | 首次运行 Windows 弹 SmartScreen「未知发布者」 | **不修**（作者学生、软件不赚钱、无证书）；跳过方法与自证方式见 [`docs/user/security/WHY-NO-CODE-SIGNING.md`](docs/user/security/WHY-NO-CODE-SIGNING.md) |
| `assetProtocol.scope = ["**"]` | 见 §3.2（仅"本地图片被显示"） | 接受，C 方案列入后续 |
| 本地库明文 | 见 §3.3 | 设计选择 |
| 调试记录含提示词 | 见 §3.4 | 有上限 + 可清 |
| 无自动更新 | 需手动下载新版本 | 尚未实现；也避免"静默更新"这一类风险 |
| 无多用户/权限 | 单机单人，数据目录权限=你的账户权限 | 设计范围 |

---

## 五、如果你想自己验证（不用信我）

| 入口 | 内容 |
|---|---|
| [`docs/user/security/README.md`](docs/user/security/README.md) | 三条自证路径（哈希 / 源码 / 比行为）|
| [`docs/user/security/AGENT-AUDIT-GUIDE.md`](docs/user/security/AGENT-AUDIT-GUIDE.md) | **给 AI agent 的 10 步核对流程**（命令 + 期望输出 + 报告模板）|
| [`docs/user/security/CHECKLIST.md`](docs/user/security/CHECKLIST.md) | 人用勾选清单（3 分钟 / 10 分钟）|
| [`docs/user/security/WHY-NO-CODE-SIGNING.md`](docs/user/security/WHY-NO-CODE-SIGNING.md) | 为什么没签名、怎么跳过、怎么替代验证 |

一句话：**可验证 > 可信。** 源码全公开，你可以自己编、自己抓包、自己 grep。
