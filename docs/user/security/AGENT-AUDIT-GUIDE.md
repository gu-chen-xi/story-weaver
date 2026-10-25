# AGENT-AUDIT-GUIDE —— 让 AI agent 逐步核对本软件是否安全

> **给 agent 的指令（人类可以直接把这段贴给 agent）**：
>
> ```text
> 请严格按本文件的步骤核对仓库 https://github.com/gu-chen-xi/story-weaver（或本地已 clone 的目录）。
> 规则：
> 1) 每步都要给出**实际执行的命令**或**读过的文件:行号**，不许凭印象；
> 2) 无法验证的项写「未验证」，不要写「应该没问题」；
> 3) 最后输出一张表：检查项 / 结论（通过 / 不通过 / 未验证）/ 证据位置；
> 4) 发现任何"可疑但不违规"的东西也要列出来（例：测试里的 example.com）。
> ```

> **本项目坐标（供 agent 快速定位）**：Tauri 2 桌面应用；后端 Rust（`src-tauri/src/`，约 35 个模块）；
> 前端 React+TS（`src/`）；数据全部本地 SQLite。**没有服务端**，作者没有任何服务器可接收你的数据。

---

## 0. 先明确"查什么"

一个"良性但被误判"和"真的有问题"，区别只在这几件事上：

| 类别 | 有问题的样子 | 本项目应然 |
|---|---|---|
| 启动外部程序 | `Command::new("powershell")`、`cmd /c`、`wscript` | **一个都没有** |
| 持久化驻留 | 注册表 `Run`、计划任务、服务、开机启动 | **一个都没有** |
| 上传数据 | 硬编码的统计/日志/埋点域名 | **一个都没有** |
| 加密勒索 / 挖矿 | 遍历用户目录加密、CPU 长跑 | **没有** |
| 隐蔽联网 | 后台心跳、隐私字段外发 | 只在你主动生成时连你配置的 API |
| 提权滥用 | 无由头地请求管理员权限 | 仅 MSI 安装到 Program Files 时（便携版不提权）|

下面每一步只回答一件事：**"表里右列是否成立"**。

---

## 1. 检查"不启动外部程序"

**命令（在仓库根目录执行）**：

```powershell
# Windows PowerShell
Select-String -Path .\src-tauri\src\*.rs -Pattern 'Command::new|std::process::Command'
```

```bash
# macOS / Linux / WSL
grep -rn "Command::new\|std::process::Command" src-tauri/src/ || echo "NONE"
```

**期望**：**无匹配**（输出 `NONE`）。

**说明**：`std::process::id()` 会命中"process"但不是启动进程（它只是取 PID 拼临时文件名），
`std::process::exit(1)` 是退出自己。这两者无害，但**必须如实在报告里写明你看到的是哪一种**。

**本项目当前实测**：0 处 `Command::new`；只有 `std::process::id()`（临时文件名）和 1 处 `std::process::exit(1)`（启动失败时退出）。

---

## 2. 检查"不驻留"

```powershell
Select-String -Path .\src-tauri\src\*.rs -Pattern 'winreg|CurrentVersion\\Run|schtasks|Startup'
```

**期望**：`winreg` / `CurrentVersion\Run` / `schtasks` **无匹配**；
`Startup` 只会命中 `startup-trace.log` / `fatal_startup()` 这类**日志与错误处理**代码（与开机自启无关）。

**再验运行时**（更硬的证据）：运行软件、正常关闭，然后：

```powershell
# 开机自启项
Get-CimInstance Win32_StartupCommand | Select-Object Name, Command, Location
# 计划任务里有没有本软件
Get-ScheduledTask | Where-Object { $_.TaskName -match 'Story|Weaver|织梦' }
```

**期望**：都**没有**本软件相关条目。

---

## 3. 检查"出站流量只去你配置的地址"

### 3.1 源码层（先看有没有硬编码域名）

```powershell
Select-String -Path .\src-tauri\src\*.rs -Pattern 'https?://[a-zA-Z0-9\.\-]+' -AllMatches |
  ForEach-Object { $_.Matches.Value } | Group-Object | Sort-Object Count -Descending |
  Select-Object Count, Name
```

**本项目当前实测结果**（你可以对照，多出来的就要追问）：

| 域名 | 次数 | 用途 |
|---|---|---|
| `https://api.deepseek.com` | 7 | **预置默认值**（模型服务），可在设置里改成任何地址 |
| `http://127.0.0.1` | 7 | 本地绘图服务默认地址（Stable Diffusion WebUI 等）|
| `https://api.openai.com` | 1 | 预置默认值（OpenAI 兼容协议）|
| `https://api.example.com` / `https://example.com` / `https://x` | 9 | **测试夹具**（单元测试里的假地址）|
| `https://www.cac.gov.cn` / `https://www.gov.cn` / `https://creativecommons.org` | 4 | 合规页与 LICENSE 的**文档链接**（不会自动访问）|

**判定要点**：以上域名里**没有**统计（GA/友盟/Sentry）、没有短链、没有陌生 IP 段、没有 Base64 混淆的 URL。

### 3.2 请求确实用的是"你填的地址"

```powershell
Select-String -Path .\src-tauri\src\api.rs -Pattern 'base_url|post\(|Client::new'
```

**期望**：见 `let base_url = db::get_config(pool, "model.base_url")` ——
URL 来自配置表，不是常量。

### 3.3 运行时层（最硬的证据：抓包）

1. 装 [Fiddler](https://www.telerik.com/fiddler) 或 `mitmproxy`，开启系统代理；
2. 启动软件、新建故事、发一条消息、点一次生成立绘、导出一次；
3. **期望**：只有两类连接 ——
   * `api.deepseek.com`（或你改成的地址）；
   * `127.0.0.1:7860`（本地绘图服务，若你配了云端则是那个域名）。
4. 没有后台定时心跳（静置 10 分钟，应无新连接）。

> 注意：Tauri 的 IPC 走 `ipc://localhost` / `http://ipc.localhost`，那是**进程内通信**，不是外网。
> 若 Fiddler 显示它，属正常。

---

## 4. 检查"只写自己的数据目录"

```powershell
Select-String -Path .\src-tauri\src\*.rs -Pattern 'fs::write|fs::create_dir_all|File::create'
Select-String -Path .\src-tauri\src\paths.rs -Pattern 'data_root|appdata|APPDATA'
```

**期望**：所有写文件的路径都来自 `paths::*`（数据根/导出目录/日志目录/临时目录），
数据根默认为 `%APPDATA%\com.guchenxi.story-weaver`。

**运行时验证（可选，需要管理员）**：用 [Process Monitor](https://learn.microsoft.com/sysinternals/downloads/procmon) 过滤
`Process Name = app.exe` 且 `Operation = CreateFile`，跑一遍主流程，看 **Path** 是否只落在：

* `%APPDATA%\com.guchenxi.story-weaver\**`
* 你选择的导出目录（导出时才写）
* `%TEMP%\**`（诊断/临时图片）
* Windows 自身的 `C:\Windows\**`（WebView2 运行时，README-only 读取）

**不该出现**：`C:\Users\<你>\Documents`、`Desktop`、`Downloads` 的**自动写入**（除非你点了"导出"到那里）。

---

## 5. 检查"API Key 不会外流"

```powershell
Select-String -Path .\src-tauri\src\keys.rs -Pattern 'keychain|LEGACY_PLAINTEXT|scrub'
```

**期望**：
* Key 只经 `keychain::set_api_key/get_api_key`（Windows 凭据管理器）；
* 有 `LEGACY_PLAINTEXT_CONFIG_KEYS` + `scrub_legacy_plaintext()`（把历史明文键迁移进凭据库并清空表值）；
* 有测试 `legacy_list_covers_known_plaintext_keys` 防漏。

**自己验**（重要）：装好软件、填好 Key 之后，把数据目录里所有 `.db` 搜一遍 Key 的前 8 位：

```powershell
Get-ChildItem "$env:APPDATA\com.guchenxi.story-weaver" -Recurse -Include *.db |
  ForEach-Object { $c = Get-Content $_.FullName -Raw -Encoding Byte; ... }   # 或用 HxD / grep -a
```

更简单的做法（PowerShell 没有 byte-grep，用 git 自带 grep）：

```bash
grep -a -r "sk-" "$APPDATA/com.guchenxi.story-weaver" || echo "no plaintext key in DB"
```

**期望**：`no plaintext key in DB`。

---

## 6. 检查"二进制里没有多余东西"

### 6.1 安装包内容清单

> 📌 下文命令以 3.6.0 为例，按你实际下载的文件名替换版本号。

```powershell
msiexec /a .\Story-Weaver_3.6.0_x64_en-US.msi /qb TARGETDIR=$env:TEMP\sw-extract
Get-ChildItem $env:TEMP\sw-extract -Recurse -File | Select-Object Name, Length
```

**期望**：只有 `Story-Weaver.exe`（+ 图标/许可证等资源），**没有** `.dll` 群、**没有** `node.exe`、
**没有** `python.exe`、**没有** `.ps1`/`.bat` 脚本。

### 6.2 便携版

直接解压 zip 看：应只有 `Story-Weaver.exe` + `使用说明.txt`。

### 6.3 二进制里有没有"别家域名"

```powershell
# 有 strings 工具（Sysinternals / git bash 都能给）
strings .\Story-Weaver.exe | Select-String -Pattern 'https?://' | Sort-Object -Unique
```

**期望**：出现的域名只有 `api.deepseek.com` / `api.openai.com` / `127.0.0.1` / `cac.gov.cn` / `gov.cn` /
`creativecommons.org` 等本文档第 3.1 节列过的；**不应有**统计或短链域名。

> 提醒：Tauri 会把前端资源压缩进 exe，"搜不到某个字符串"**不能**证明代码里没有它（压缩后自然搜不到）。
> 所以**源码检查优先于字符串检查**。

---

## 7. 检查"来源没被换包"

```powershell
Get-FileHash .\Story-Weaver_3.6.0_x64_en-US.msi -Algorithm SHA256
Get-FileHash .\Story-Weaver-3.6.0-portable.zip -Algorithm SHA256
```

与发布页 `SHA256SUMS.txt` 逐字符比对。**不一致 = 不要运行**。

**关于"字节一致"的诚实说明**：让 agent 自己从源码构建再比 hash **必然失败**
（Rust 产物含构建时间/绝对路径/编译器版本）。这不是可疑，是正常现象。
要验"源码 = 二进制"，正确做法是**比行为**（第 3、4 节的运行时检查），
或者信任发布者的 `SHA256SUMS.txt`（它证明"你拿到的包 = 发布页那个包"）。

---

## 8. 检查"有没有偷偷改系统"

```powershell
# 跑软件前后各取一次快照，比对
$a = Get-ChildItem "HKCU:\Software" -Recurse -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name
# （运行软件、正常关闭）
$b = Get-ChildItem "HKCU:\Software" -Recurse -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name
Compare-Object $a $b
```

**期望**：不应出现与 `Story-Weaver` / `织梦` 相关的**自启/服务**键。
（Tauri/WebView2 可能写自己的缓存目录，那是运行库行为，属正常。）

---

## 9. 最后：让它真的跑起来，看有没有异常

1. **Windows 沙盒**里跑便携版（`Windows Sandbox` 功能开启后双击 `.wsb`）；
2. 数据目录指到沙盒内：`set DREAMWEAVER_DATA_DIR=C:\data` 再启动；
3. 走一遍主流程（建故事 → 发消息 → 导出）；
4. **期望**：无异常弹窗、无静默安装、无额外进程常驻（任务管理器里只有 `app.exe` + `msedgewebview2.exe`）。

> `msedgewebview2.exe` 是 **Microsoft Edge WebView2 运行时**（任何 Tauri/Electron 类应用都有），
> 不是本软件自带的浏览器内核，也**不需要**额外下载（Win10 1803+ / Win11 自带或由系统更新安装）。

---

## 10. 报告模板（agent 直接用）

```markdown
| # | 检查项 | 结论 | 证据 |
|---|---|---|---|
| 1 | 不启动外部程序 | 通过 | `grep Command::new src-tauri/src/` → 0 命中；仅有 std::process::id()（PID 拼文件名）|
| 2 | 不驻留（注册表/计划任务/服务）| 通过 | grep 无 winreg/schtasks；运行后 Win32_StartupCommand 无相关项 |
| 3 | 出站只去配置地址 | 通过 | 硬编码域名清单见 §3.1；api.rs:16 base_url 来自 configs |
| 3.3 | 抓包实测 | 通过 / 未验证 | Fiddler 只见 api.deepseek.com + 127.0.0.1 |
| 4 | 只写自己数据目录 | 通过 | 所有写入经 paths::*；ProcMon 见附录 |
| 5 | Key 不进数据库 | 通过 | keys.rs 用 keychain；grep "sk-" 数据目录 → 无 |
| 6 | 二进制无多余载荷 | 通过 | MSI 解包只有 exe |
| 7 | 包未被换 | 通过 / 未验证 | SHA256 与 SHA256SUMS.txt 一致 |
| 8 | 不改系统设置 | 通过 / 未验证 | 注册表前后比对无相关差异 |
| 9 | 沙盒运行无异常 | 通过 / 未验证 | — |

其它观察（不构成风险但要记录）：
- 单元测试里出现 example.com / https://x 等假地址（测试夹具）
- 生成图片左上角固定「AI生成」标识（法规要求，不可关闭；服务方自己的水印原样保留，两者位置不同可区分）
```

---

## 11. 如果你发现了问题

请在 Issues 里贴：**检查项编号 + 你的命令 + 实际输出**。
作者承诺：能复现的问题就修，修不了的写进 `ROADMAP.md` 的「仍未处理的风险」。
（本文件本身就是那次"安全审查"的产物，见 [ROADMAP 归档](../../archive/ROADMAP-已完成.md) §10.102。）
