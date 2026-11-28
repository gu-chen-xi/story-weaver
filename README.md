# 织梦机（Story-Weaver）· AI 原生叙事引擎

[![License: PolyForm Noncommercial 1.0.0](https://img.shields.io/badge/License-PolyForm%20Noncommercial%201.0.0-blue.svg)](LICENSE)
[![Content: CC BY-NC-SA 4.0](https://img.shields.io/badge/Content-CC%20BY--NC--SA%204.0-lightgrey.svg)](LICENSE-CONTENT)
[![Version](https://img.shields.io/badge/version-v3.17.0-FF7A18.svg)](#七版本与许可)
[![Tauri 2](https://img.shields.io/badge/Tauri-2-24C8DB.svg)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61DAFB.svg)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-1.77.2%2B-000000.svg)](https://www.rust-lang.org/)

> **品牌**：窗口标题 = 织梦机 · 程序与打包名 = Story-Weaver
> **一句话定位**：在本机运行的 Windows 桌面创作工具。不提供生成式 AI 服务，不代申请接口；模型与绘图服务由你自行配置；创作数据保存在本机，生成时会把必要上下文发送到该服务。
> 当前版本：**v3.17.0** · [下载最新版](https://github.com/gu-chen-xi/story-weaver/releases/latest) · [文档总入口](docs/README.md)

<!-- 软件首页截图待补：拿到截图后放在此处。 -->

## 一、项目是什么、解决什么问题

织梦机是面向长文剧情创作的 Windows 单机桌面工具。它把故事、角色、世界观、时间线、章节和伏笔放进同一套结构化数据里；生成剧情时，由你配置的大模型读取这些上下文，软件负责把结果按日保存、保持前后衔接，并允许回看和修改历史。

它主要解决这些问题：

- **长篇容易忘设定**：角色卡、关系、世界规则、时间线和最近剧情会按预算装配进上下文，减少前后矛盾。
- **修改历史与续写**：剧情按故事日保存；修改历史消息会直接更新该条内容，后续生成基于修改后的历史。当前不会自动把后续内容标记为失效，也不会自动重算已有摘要、记忆或状态。
- **AI 输出只是聊天记录**：结构化管理故事、角色、章节、伏笔和动态状态，让创作资产能长期维护。
- **数据默认保存在本机**：剧情与设定保存在本地 SQLite，API Key 存 Windows 凭据管理器；使用云端模型时，生成所需的上下文会发送到你配置的服务。
- **模型绑定**：支持 OpenAI-compatible 云端接口，也可以切换到本地 Ollama；目前云端调用只验证过 DeepSeek API，其他兼容服务未逐一验证。

## 二、核心功能

- **故事与时间线**：绝对时间起点、按日存储、日历视图、插曲/倒叙，以及历史消息修改与基于新历史续写。
- **角色与世界观**：角色库、关系图、世界设定与硬规则；角色详情、立绘绑定和记忆入口统一管理。
- **剧情生成**：旁白 / 对话两态，支持 `*…*` 心声、`#标签` 元指令、`@角色` 聚焦和 `〔导演指令〕`；流式输出可中途停止，失败会保留半截并写明原因。
- **上下文管理**：按系统提示词、角色、关系、世界观、最近剧情和本轮输入装配；Token 预算、智能截断与回顾检索可控。
- **后台演进与回滚**：事件日志 → 候选变更 → 提案 → 校验 → 人工确认/拒绝 → 快照更新，自动变更可审计、可回滚。
- **章节与伏笔**：章与天手动圈定、拖拽归章、本章目标注入、按章导出，以及伏笔台账和逾期提示。
- **动态状态与承诺**：开启三维数值插件时，体力、理智、存在感会影响台词和行动表现；关键承诺与记忆按分支继承和结算。
- **本地模型**：Ollama 通过原生 `/api/chat` 接入，支持连接检查、测试对话、上下文档位和真实用量回填。
- **安全与调试**：API Key 只进系统钥匙串；提供缓存管理、日志、启动自检和开发期请求快照。

详细规格见 [docs/dev/SPEC.md](docs/dev/SPEC.md)，文档总入口见 [docs/README.md](docs/README.md)。

## 三、下载安装

最新版 **v3.17.0**（2026-11-20） · [全部版本与发行说明](https://github.com/gu-chen-xi/story-weaver/releases)

| 下载 | 适合 |
|---|---|
| [Story-Weaver_3.17.0_x64_en-US.msi](https://github.com/gu-chen-xi/story-weaver/releases/download/v3.17.0/Story-Weaver_3.17.0_x64_en-US.msi) | 安装到系统，开始菜单有快捷方式 |
| [Story-Weaver-3.17.0-portable.zip](https://github.com/gu-chen-xi/story-weaver/releases/download/v3.17.0/Story-Weaver-3.17.0-portable.zip) | 免安装，解压后双击 `Story-Weaver.exe` |
| [SHA256SUMS-3.17.0.txt](https://github.com/gu-chen-xi/story-weaver/releases/download/v3.17.0/SHA256SUMS-3.17.0.txt) | 校验下载是否完整 |

系统要求：Windows 10 1803+ / Windows 11，需 WebView2 运行时；需要联网访问你配置的大模型 API。

> 本软件**没有代码签名**。首次运行若出现 Windows SmartScreen「未知发布者」，点「更多信息」→「仍要运行」。
> 为什么不签名、如何自行核验，见 [安全说明](docs/user/security/README.md) 与 [SECURITY.md](SECURITY.md)。

## 四、快速开始 / 开发环境

### 4.1 环境要求

- Windows 10 1803+ / Windows 11，并安装 WebView2 运行时
- Node.js 22.12+ 与 npm（Vite 8 支持 `^20.19.0 || >=22.12.0`，测试链路推荐 22.12+）
- Rust 1.77.2+ 与 MSVC Build Tools
- Git
- Ollama 可选，仅在使用本地模型时需要
- 你自行准备的大模型 API Key（OpenAI-compatible）

### 4.2 开发模式

```powershell
npm ci
npm run tauri dev
```

开发模式会启动 Vite 与 Tauri，适合改代码和调试；不要直接双击 `target\debug\app.exe`，它需要 `localhost:5173` 的 Vite 服务。

> 如果仓库根部存在 `data\library\default\library.db`，`npm run tauri dev` 会通过 `scripts/tauri.mjs` 自动把 `DREAMWEAVER_DATA_DIR` 指向项目下的 `data\`，直接沿用已有开发数据。

### 4.3 本地构建并运行

```powershell
npm run sync:run
```

这个命令会构建前端、生成带 `custom-protocol` 的 release exe、验证它不是 dev 版，并复制到：

```text
%LOCALAPPDATA%\DreamWeaverRun\app.exe
```

双击复制后的 exe 即可运行。**不要手搓 `cargo build --release`**：漏掉 `custom-protocol` 会得到一个仍去连 `localhost:5173` 的白屏版本。项目目录名含中文时，也建议使用 `sync:run` 复制后的 exe，而不是直接双击 `target\release\app.exe`。

### 4.4 正式打包

```powershell
npm run tauri build
```

正式打包会产出 MSI 与 exe。便携 zip 由维护者用 `node scripts/pack-portable.mjs <版本>` 生成，具体发布流程见 [docs/dev/开发流程.md](docs/dev/开发流程.md)。

### 4.5 常用检查

```powershell
npm test
cd src-tauri; cargo test --lib
npm run doclinks
npm run build
```

## 五、数据、安全与合规

### 5.1 数据在哪里

开发模式下，`npm run tauri dev` 会优先使用仓库根部的 `data\`（当其中已有 `library.db` 时）。其余情况下按顺序探测，使用第一个**真能写入**的目录：

1. 环境变量 `DREAMWEAVER_DATA_DIR`
2. `%APPDATA%\com.guchenxi.story-weaver`
3. `%LOCALAPPDATA%\com.guchenxi.story-weaver`
4. `<exe 同级>\data`
5. `<exe 同级>\dreamweaver-data`
6. `<当前目录>\data` → 系统临时目录

想固定位置，在启动前设置环境变量：

```powershell
$env:DREAMWEAVER_DATA_DIR = "D:\织梦机数据"
```

数据根下主要有：

```text
<数据根>\
├── library\      故事与素材（library.db + 每个故事一个 story.db）
├── exports\      导出产物
├── temp\         临时文件
├── logs\         日志
└── backups\      备份
```

备份、迁移和目录说明见 [docs/user/数据与备份.md](docs/user/数据与备份.md)。启动日志里的 `数据根：...` 一行会显示本次实际使用的位置。

### 5.2 本机安全

- **API Key 只存 Windows 凭据管理器**，不写数据库、日志、备份或导出文件；界面不回显明文。
  - 主模型：`openai_api_key.ai-story-studio`
  - 云端绘图：`portrait_api_key.ai-story-studio`
- **没有代码签名**：首次运行可能触发 SmartScreen，属于发布者未知提示。原因与核验方法见 [docs/user/security/WHY-NO-CODE-SIGNING.md](docs/user/security/WHY-NO-CODE-SIGNING.md)。
- **数据默认落在本机**：源码层没有指向作者服务器的地址、分享接口或遥测调用点；使用云端模型时，必要上下文会发往你配置的服务。若要确认运行时到底连了谁，按 [AGENT-AUDIT-GUIDE.md](docs/user/security/AGENT-AUDIT-GUIDE.md) 自行抓包。
- **打不开先跑自检**：双击 `scripts\selfcheck\selfcheck.cmd`，它会真实启动一次程序并检查窗口、路径和版本；报告写到 `debug-diagnostics\selfcheck-<时间>.txt`。
- 完整安全策略与威胁模型见 [SECURITY.md](SECURITY.md)。

### 5.3 责任边界与合规

本软件是本地桌面工具：不提供生成式 AI 服务，不代申请、代付任何第三方服务，模型推理由你自行配置的服务完成。就现行监管口径，本项目不以「生成式人工智能服务提供者」身份运营；但软件会接收、保存、加工和导出 AI 生成内容，因此仍主动遵守标识与安全边界：

- 不提供去水印、清元数据、绕过第三方内容审核或「免审查」模式；
- 生成图片时添加 AI 显式角标与 PNG 隐式元数据，保留服务方原有标识；导出剧情文件时附「AI 生成内容声明」；
- 内置敏感词过滤，但只覆盖部分明显辱骂/脏话，无法审查全部生成内容，也不承诺生成内容合法；
- 源码公开，过滤词表和实现可自行核查。

使用者需要自行承担：

- 遵守所使用模型与绘图服务的条款，以及内容发布平台的规则；
- 对外发布 AI 生成内容时，按《人工智能生成合成内容标识办法》及发布平台规则主动声明并使用标识功能，不恶意删除、篡改、伪造或隐匿标识；
- 不使用真人姓名、肖像或声音生成未经授权的内容；
- 不生成、不传播违法、暴力、仇恨、自伤诱导等有害内容；
- 不诱导 AI 绕过服务方的安全过滤。

自动化与人工审批的边界：

> **事实进角色，解释进审批；短尾自动，长尾人工；自动必须可撤销。**

- 软件**不替你决定剧情走向**；自动链路只补代价和状态，不改写你的核心指令。
- **生死与不可逆损伤只能人工**处理，自动链路没有这类变更类型。
- **你手改的数值和设定永远优先**；自动变化只在新基线上叠加，并保留可撤销记录。

详细的数值变更边界见 [docs/design/三维数值变更.md](docs/design/三维数值变更.md)。

**本软件严禁未成年人使用。** 内置敏感词过滤只覆盖常见辱骂/脏话，输出侧替换为 `***`，不代表具备内容审查能力；导出的文本保持原样。

涉及的主要规范包括：《生成式人工智能服务管理暂行办法》主要规范面向境内公众提供生成式 AI 服务的提供者；《人工智能生成合成内容标识办法》（2025-09-01 施行）规定显式标识、隐式标识及禁止恶意删除、篡改、伪造、隐匿标识；《互联网信息服务深度合成管理规定》规定深度合成内容的安全与标识要求。此外还需遵守网络安全、数据安全、个人信息保护和未成年人保护等一般性规定。

软件内「合规与责任」页与 [LICENSE](LICENSE) 附录包含更完整的条款。本章节为工程整理，不构成法律意见；具体适用请结合你的发布场景咨询专业人士。

## 六、文档入口

统一入口：[docs/README.md](docs/README.md)

| 你想做什么 | 看这份 |
|---|---|
| 装好后搞清楚数据存哪、怎么备份 | [docs/user/数据与备份.md](docs/user/数据与备份.md) |
| 自行验证软件没有上传或后门 | [docs/user/security/README.md](docs/user/security/README.md) |
| 知道现在做到哪、下一步做什么 | [docs/dev/ROADMAP.md](docs/dev/ROADMAP.md) |
| 改代码前看规范和流程 | [docs/dev/开发规范.md](docs/dev/开发规范.md) · [docs/dev/开发流程.md](docs/dev/开发流程.md) |
| 遇到白屏、打不开、数据不对 | [docs/dev/错误合集.md](docs/dev/错误合集.md) · [docs/dev/自检工具.md](docs/dev/自检工具.md) |
| 提建议或了解贡献政策 | [CONTRIBUTING.md](CONTRIBUTING.md) |
| 查看早期计划与版本规划沿革 | [docs/archive/历史计划存档.md](docs/archive/历史计划存档.md) |

## 七、版本与许可

### 7.1 版本

| 版本 | 日期 | 一句话 |
|---|---|---|
| **V3.17.0** | 2026-11-20 | 首页启动台 + 大肥鱼图标集 v1（50 个手绘线性图标）+ 双池对比尾部同源 |
| **V3.16.0** | 2026-11-14 | 对话模式加入角色视角 + 视角按配对计算 + 双池对比工具 |
| **V3.15.0** | 2026-11-09 | 立绘改走 portrait:// 协议 + 插件命令白名单 + 日结抽取新实体 |

完整历史和每版发行说明见 [GitHub Releases](https://github.com/gu-chen-xi/story-weaver/releases) 与 [docs/user/releases/](docs/user/releases/)。早期版本规划已迁入 [docs/archive/历史计划存档.md](docs/archive/历史计划存档.md)。

### 7.2 许可

本项目分部分授权，完整条款见 [LICENSE](LICENSE) 与 [LICENSE-CONTENT](LICENSE-CONTENT)。

| 部分 | 适用许可 |
|---|---|
| 软件源码（`src/`、`src-tauri/src/`、`scripts/`、构建配置） | **PolyForm Noncommercial 1.0.0**（非 OSI 开源许可） |
| 内容资产（图标、吉祥物、文档、内置条款文案与提示词正文） | **CC BY-NC-SA 4.0** |
| 你创作的剧情、角色、设定、立绘与导出成品 | **归你所有**，不受本仓库许可约束 |

- 可以：个人学习、研究、非商业用途的修改与再分享。
- 不可以：任何以商业优势或金钱报酬为主要目的的用途，包括把本软件或修改版作为商业服务、随商业产品捆绑分发，或收费提供代管服务。
- 名称与 Logo 不随代码许可授权；fork 请改名换图标，见 [TRADEMARK.md](TRADEMARK.md)。
- 第三方依赖遵循各自许可；依赖许可门禁为 `cd src-tauri; cargo deny check licenses` + `npm run licensecheck`。

> 本软件按「现状」提供，不构成法律意见。在法律允许的最大范围内，作者不对因使用本软件产生的直接或间接损害承担责任。
