# 安全策略（SECURITY）

> 适用于：织梦机 / Story-Weaver（Windows 桌面版）
> 最后更新：2026-11-09 ｜ 对应版本 **v3.15.0**（最新打包版）
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

**支持范围**：只支持**最新发布版本**（当前 v3.15.0）。旧版本不打补丁。

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
| 禁止页面被嵌入/被脚本注入 | CSP `frame-ancestors 'none'`、`object-src 'none'`、`base-uri 'self'`、**`script-src 'self'`（v3.14 起没有 `'unsafe-inline'`）**；前端无 `dangerouslySetInnerHTML`/`innerHTML` | 同上 + `grep -rn innerHTML src/` |
| AI 生成图片标识 | 出图自动打「AI生成」可见标识（法规要求，不可关闭）：固定**左上角**，服务方自带水印原样保留、不覆盖（2026-10-25 从右下角改到左上，避免与平台水印叠在同一角）；解码前设尺寸上限（v3.15，坏图/伪造尺寸只报错不 panic）| `src-tauri/src/image_stamp.rs` |
| 立绘只读素材库 | v3.15 起立绘走 `portrait://`：后端只认**文件名**、落点固定 `portraits_dir()`、越界与缺失分别 403 / 404（原 `assetProtocol.scope=["**"]` 已下线）| `src-tauri/src/portrait_protocol.rs`、`src-tauri/src/security_guards.rs` |
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

### 3.2 已关闭：`assetProtocol.scope = ["**"]` → 改用 `portrait://`（2026-11-05）

**原来的问题**：`assetProtocol.scope = ["**"]` 允许 webview 通过 `asset://localhost/<任意绝对路径>`
读取**本机任意图片/媒体文件**。当时的评估是「不能外发、不能提权，可接受」，但它仍然比**需要**的宽 ——
软件只需要显示立绘。

**现在怎么做**：立绘改走专用协议 `portrait://localhost/<文件名>`，后端 handler 在
`src-tauri/src/portrait_protocol.rs`，只有三条边界：

1. **只认文件名**：URI 里出现 `/`、`\`、`..`、`:` 一律 403（percent-decode **之后**再检查，
   防止 `%2F` 绕过来）；
2. 落点固定在 `portraits_dir()` 里拼，永远出不去；
3. 读不到就 404，不回落、不猜。

`assetProtocol` 与 `tauri` 的 `protocol-asset` feature 都已从配置与 `Cargo.toml` 移除；
CSP 的 `img-src` / `media-src` 里 `asset:` 换成 `portrait: http://portrait.localhost`。

**残留风险**：恶意 story.db 仍可把 `portraits.file` 指到素材库目录里的**另一个文件**（显示成别人的图），
但越不出素材库目录；外发仍然不可行（CSP `connect-src` 无外部域名 + `<img>` 跨源污染 canvas）。

**攻击可行吗？逐条评估**：

| 攻击步骤 | 是否可行 | 原因 |
|---|---|---|
| AI 输出里塞 `<img src="asset://...">` | ❌ 不可行 | AI 文本只作为 React 文本节点渲染；无 `dangerouslySetInnerHTML` / `innerHTML` |
| 用 JS 构造 `asset://` URL 并读取内容 | ❌ 不可行 | 无脚本注入点（CSP 禁 `object-src`，无 `innerHTML`，无 `eval` 用法）|
| 把读到的图片**外发**出去 | ❌ 不可行 | CSP `connect-src` 无外部域名；且 `<img>` 跨源会让 canvas 污染，读不到像素 |
| 导入**恶意 story.db**，让 `portraits.path` 指向别的本地图片 | ⚠ **可行** | 结果：该图片被显示在你自己的屏幕上（**仅展示，不能外发**，也不能批量扫描）|

**结论**：已按 C 方案改完（2026-11-05）—— 立绘走 `portrait://`，后端只认素材库目录里的文件名，
前端不再能传任意路径。剩下的最坏情况是「恶意数据库让某张立绘显示成素材库里的另一张图」，
无法窃取、无法外传、无法提权。**风险等级：低（比改造前更低）**。

实现与边界见 `src-tauri/src/portrait_protocol.rs` 模块头。


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

### 3.6 威胁 E：云端返回的图片地址（2026-10-30 加固）

云端出图时，**图片的下载地址是服务端在响应体里给的**（`data[].url` 或自定义 `response_path`），
不是你在配置里填的。旧实现对这个地址没有任何限制，而且把 `Authorization: Bearer <你的 key>`
一起带了过去 —— 于是一个恶意/被换掉的绘图服务可以：

* 返回 `http://127.0.0.1:<port>/…`、`http://192.168.x.x/…`、`http://169.254.169.254/…`，
  让**本软件从你机器上**发起 GET（端口探测 / 打内网 / 读云元数据）—— 即 SSRF；
* 或者直接把它自己（或中间人）的地址塞进来，白拿你的 Key。

**现在的准入规则**（`generator_cloud.rs::classify_download_url`，纯函数 + 5 条单测）：

| 图片地址 | 结果 |
|---|---|
| 与配置同源（`base_url` / 自定义请求地址：scheme+host+port 全同） | 放行，**照旧带**鉴权头（本地 SD `http://127.0.0.1:7860` 属这一类，不受影响） |
| 跨源但 `https` + 公网 | 放行，**不带任何凭证** |
| 跨源 + `http`（降级）/ 非 `http(s)` scheme | **拒绝** |
| 本机 / 内网 / 链路本地（`127.*`、`10.*`、`172.16-31.*`、`192.168.*`、`169.254.*`、`localhost`、`[::1]`） | **拒绝**（同源除外） |

**大小限制也改了**：旧实现是"先全量读进内存、再查 32MB"，也就是限制只是个事后拒绝；
现在**先看 `Content-Length`**（超限直接不下载），没有长度时用 `chunk()` **流式累计、超限即中止**。

> **已知代价**：某个服务的图床在**另一个域名**且**要求鉴权**时，现在会 401（跨源不带凭证）。
> 正确解法是加一个显式的下载白名单配置键（`portrait.cloud_download_hosts`），而不是把凭证放开 —— **本期没做**，
> 记在 [ROADMAP §2.1](docs/dev/ROADMAP.md) 里。

---

### 3.7 威胁 F：Ollama 地址是你自己填的（2026-10-31）

本地模型插件里那个「地址」**是自由文本**（默认 `http://127.0.0.1:11434`），本软件会往
`{地址}/api/version`、`{地址}/v1/chat/completions`、`{地址}/api/chat` 发请求。也就是说：

* **填局域网里另一台机器是允许的**（这是有意设计，方便你有台带显卡的机器）；
* 但那个地址上跑的是什么，本软件不知道，也管不了 —— 和 §3.5「你配置的第三方 API」同级；
* **本地请求不带任何凭据**：走本地时 `chat_override` **不读钥匙串**，请求里只有一个占位串 `ollama`。
  所以即使地址填错、被换掉，也**不会泄漏你的云端 Key**；
* 它**不会启动/下载任何东西**（红线 1）：没有 `Command::new`，没有 `tauri-plugin-shell`，只是 HTTP 请求。

> 换句话说：本地模型的信任边界 = **你自己填的那个地址**。要连别人的机器，先确认那台机器可信 ——
> 这条和"把数据发给你选的云端服务"是同一类判断。

### 3.8 已收紧（2026-11-07）：命令面 —— 登记表 / 插件白名单 / 原生确认

**这一条回答"前端只要有一次脚本执行，能调多少东西"**（原 §四 里那条"159 个命令全量开放"）。

Tauri 没有"按命令"的 ACL（`capabilities` 只管插件权限），所以 160 条自注册命令对主窗口**天然全开放**，
这一点框架层收不窄。3.15 做的是三件**能做的事**：

| 手段 | 做了什么 | 边界（不要夸大） |
|---|---|---|
| **命令登记表** | `command_registry.rs` 把 **160 条**已注册命令逐条登记，按 Read / Write / Sensitive / Credential / Dev 分类；测试要求它与 `generate_handler![]` **逐条一致**，`delete_/remove_/purge_/clear_/rollback_/undo_` 开头必须标 `Sensitive` | 它**不限制**调用，只保证"不许悄悄多出一条"、并且敏感命令有名册可查 |
| **四条不可逆命令弹原生确认** | `delete_story` / `rollback_snapshot` / `purge_temporary_snapshots` / `delete_branch` 改成 `confirm_destructive`（`tauri-plugin-dialog` 的系统对话框） | 网页里的确认框挡不住注入脚本（它能自己再调一遍 `confirm`），**系统对话框**才行。其它写操作有版本快照兜底，不加确认 |
| **插件命令白名单** | 插件**不再能 `import { invoke }`**：宿主机只交 `host.call(name, args)`，`name` 必须在 `src/lib/pluginCommands.ts` 的白名单里（13 条 = `ollama_*` 4 + `generator_*` 9），表外抛错并列出可用命令；守卫测试扫插件目录，出现 `@tauri-apps/api/core` 就红 | 现在内置插件是**静态 import**，本来就是可信代码；这条是为"以后做外部安装的插件"提前把边界的**那一半**焊上（另外一半是 `lazy()` 动态 import，见 §四） |

> 仍未关闭的部分：**主窗口自己的 JS 依然能调那 160 条**。前提仍是"前端没有注入点"（红线 3），
> 以及 CSP 不再放松（§3.2 结尾 / 红线 4）。做外部插件生态之前，要做的是"按插件声明的 `caps` 授权"，
> 那时 §四 里那条风险会从"信息级"变成"高"。

---

## 四、已知限制（如实列出）

| 限制 | 影响 | 状态 |
|---|---|---|
| **无代码签名** | 首次运行 Windows 弹 SmartScreen「未知发布者」 | **不修**（作者学生、软件不赚钱、无证书）；跳过方法与自证方式见 [`docs/user/security/WHY-NO-CODE-SIGNING.md`](docs/user/security/WHY-NO-CODE-SIGNING.md) |
| ~~`assetProtocol.scope = ["**"]`~~ | **已修复（2026-11-05）**：改用 `portrait://`，只认素材库目录里的文件名（见 §3.2） | 已关闭 |
| 本地库明文 | 见 §3.3 | 设计选择 |
| 调试记录含提示词 | 见 §3.4 | 有上限 + 可清 |
| 无自动更新 | 需手动下载新版本 | 尚未实现；也避免"静默更新"这一类风险 |
| 无多用户/权限 | 单机单人，数据目录权限=你的账户权限 | 设计范围 |
| ~~**CSP 的 `script-src` 带 `'unsafe-inline'`**~~ | 万一将来引入 markdown→HTML 渲染或第三方组件注入，内联脚本能直接跑，CSP 兜不住 | **已修（v3.14.0，2026-11-04）**：生产 `script-src` 只剩 **`'self'`**；`style-src` 的内联豁免保留（React 内联样式要用）。**开发模式另用一份 `devCsp`**（内联脚本 + `ws://localhost:5173`，Vite HMR 需要）—— 这两样不该进生产策略，否则等于没收紧。验证：`scripts\selfcheck\selfcheck.cmd` 15 项通过（窗口能起、没连 5173） |
| **主窗口能调全部已注册命令** | 前端只要有一次脚本执行，就能调删库 / 清数据 / 改配置的全部能力 | **部分收紧（3.15）**：注册 **160** 条 / 标注 **161** 处（新增 `scan_foreshadow_cloud`；唯一没注册的仍是 `get_api_key`，故意的）；这 160 条逐条登记并分类，`delete_/purge_/rollback_/undo_` 必须标 `Sensitive`，四条不可逆命令改弹**原生**确认，插件侧只给 13 条白名单（§3.8）。**但主窗口自己的 JS 仍然调得到全部** —— 框架层收不窄，前提仍是"前端没有注入点"（红线 3）。详见 [E-DOC-08](docs/dev/错误合集.md) 与 ROADMAP §2.1 |
| ~~跨源图床必须免鉴权~~ | 需要鉴权的跨域图床一律 401，用户只能将就 | **已支持（3.15，2026-11-08）**：配置键 `portrait.cloud_download_hosts`（逗号 / 分号 / 空白分隔，`*.example.com` 支持子域），命中才带鉴权头；**白名单不解除本机 / 内网禁令**（防 SSRF 的底线，见 §3.6） |
| ~~恶意图片字节喂解码器~~ | 几十字节的畸形 PNG 让解码器按声明尺寸分配内存，进程被吃死 | **已修（3.15，2026-11-08）**：解码前设 `image::Limits`（宽 / 高各 16384），超限报「解码失败」；空 / 垃圾 / 截断 / 伪造 IHDR 都有测试，`image_size()` 对坏文件返回 `(0,0)` 不 panic |

> 2026-10-30 做过一轮完整安全审计（静态 + 动态实测，红线逐条回归）：
> [docs/dev/安全审计-2026-10-30.md](docs/dev/安全审计-2026-10-30.md)。上面两条就是从那份报告里留在"已知限制"里的。

---

## 五、如果你想自己验证（不用信我）

| 入口 | 内容 |
|---|---|
| [`docs/user/security/README.md`](docs/user/security/README.md) | 三条自证路径（哈希 / 源码 / 比行为）|
| [`docs/user/security/AGENT-AUDIT-GUIDE.md`](docs/user/security/AGENT-AUDIT-GUIDE.md) | **给 AI agent 的 10 步核对流程**（命令 + 期望输出 + 报告模板）|
| [`docs/user/security/CHECKLIST.md`](docs/user/security/CHECKLIST.md) | 人用勾选清单（3 分钟 / 10 分钟）|
| [`docs/user/security/WHY-NO-CODE-SIGNING.md`](docs/user/security/WHY-NO-CODE-SIGNING.md) | 为什么没签名、怎么跳过、怎么替代验证 |

一句话：**可验证 > 可信。** 源码全公开，你可以自己编、自己抓包、自己 grep。
