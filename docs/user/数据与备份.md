# 存储与数据规划（STORAGE）

> 项目版本：**V3.1.0**（2026-10-18）｜本文档最后同步：**2026-10-18**
> 版本：V1.0 Windows 单机版
> 说明：仅 Windows，无多用户协作。
> ⚠ **2026-10-16 重大修订**：本文档原先描述"所有数据集中在项目根目录"，
> 那套结构**已废弃**。现在数据在一个**数据根**下，且根的位置**按可写性逐级探测**。

---

## 一、数据根：数据到底在哪

**所有数据都在一个"数据根"下**。根的位置按以下顺序解析，**取第一个真能写的**
（见 `src-tauri/src/paths.rs`；判定方式是"真写一个探针文件"，不是看目录存不存在）：

| 顺序 | 位置 | 说明 |
|---|---|---|
| 1 | **环境变量 `DREAMWEAVER_DATA_DIR`** | 显式指定优先；**但它不可写时会继续往后兜底** |
| 2 | `%APPDATA%\com.guchenxi.story-weaver` | 标准位置 |
| 3 | `%LOCALAPPDATA%\com.guchenxi.story-weaver` | 有些环境 Roaming 被拦而 Local 可以 |
| 4 | `<exe 同级>\data` | **便携 / 受限环境兜底** |
| 5 | `<exe 同级>\dreamweaver-data` | 再退一步 |
| 6 | `<当前目录>\data` → 系统临时目录 | 最后手段 |

> **为什么必须"探测可写"**：实测踩到过 —— 数据根指向 `%APPDATA%`，但那台机器上
> 程序**连新建目录都被拒**（安全软件拦截，`os error 5`），于是 sqlite 报
> `attempt to write a readonly database`，应用起不来。而**同一台机器上 Python 能写** ——
> 所以"目录看起来在/权限看起来对"完全不能作为依据。
>
> 用了兜底位置时启动日志会打印 `[startup][warn] 标准数据目录不可写（...），已改用：...`。

### 数据根下的子目录

| 子目录 | 内容 |
|---|---|
| `library/` | `library.db`（故事索引 + 配置）+ `default/projects/<id>/story.db` + `default/shared/portraits/` |
| `exports/` | 导出产物（`story/` `character/` `portrait/` `backup/` `quick/`） |
| `temp/` | 生成过程临时文件（**不在 cargo 的 target 下** —— 打包后 target 不存在） |
| `logs/` | 日志，含启动诊断 `startup-trace.log` / 启动失败 `startup-error.log` |
| `backups/` | 备份 |

**运行期不再使用 `std::env::current_dir()`**（唯一例外：一次性迁移旧数据时用来找旧位置）。

---

## 二、数据存储位置总表

路径均**相对数据根**。API Key 例外（在系统凭据库）。

| 数据 / 功能 | 保存位置 | 说明 |
|------------|---------|------|
| 库索引 + 全局配置 | `library/default/library.db` | 故事列表 + `configs` 表（模型参数、上限等） |
| 故事全量数据 | `library/default/projects/<故事>/story.db` | 每故事独立 SQLite：时间线/剧情/角色/关系/世界观/快照/日志/提案/审计/**分支** |
| 共享素材（立绘） | `library/default/shared/portraits/` | 立绘文件 + `portraits` / `portrait_versions` 表 |
| **API Key** | **Windows 凭据管理器** | `keychain.rs`+`keys.rs`；**不进数据库、不进备份、不随作品导出** |
| 立绘云端配置 | `library.db` 的 `configs` 表 | URL / 模型名 / 请求模板等；**Key 单独走凭据库** |
| 日志 | `logs/` | 请求 / Token / 上下文组成 / 启动诊断 |
| 导出 | `exports/` 下分类目录 | quick / backup / story / character / portrait |
| 临时文件 | `temp/` | 程序运行期临时产物（含生成中间图） |
| 备份 | `backups/` | 快照回滚 + 定期备份 |

### ⚠ 未实现 / 遗留（别再按"已实现"理解）

| 项 | 实际状态 |
|---|---|
| `cache/` 目录、`cache_entries` 表 | 表**从来没有被写入**（只有 SELECT/DELETE），缓存面板恒显示 0。上限配置与清理逻辑已实装 |
| `plugins/`（`installed/` `data/`） | **未实现**（插件体系在 ROADMAP 后续批次） |
| `models/`（本地模型文件） | **未实现**（当前立绘走 SD WebUI API / 云端，不读本地模型目录） |
| `resources/`（内置资源） | **未实现**（空壳） |
| `config/` 目录 | **已废弃** —— 全局配置在 `library.db` 的 `configs` 表里 |
| `app/`（打包产物） | **未做** —— 打包暂缓到 V3.0（图标/品牌名未定）；该空目录已于 2026-10-16 删除 |

---

## 三、与 MVP 数据模型的关系

- 数据库：**本地 SQLite**
- 分布：`library.db` 管库索引与全局配置；**每故事独立 `story.db`** 管该故事全部剧情与演进数据
- `story.db` 实际表（**14 张**，MVP 文档写"12 张"是早期口径）：
  `timeline_nodes` / `characters` / `relations` / `world_settings` / `messages` /
  `setting_snapshots` / `event_logs` / `change_proposals` / `audit_logs` / `configs` /
  `cache_entries` / `debug_requests` / `story_summaries` / **`branches`**（分支系统新增）
- 导出：**txt / md / html 三格式已实装**（文件名分钟级时间戳防覆盖）

---

## 四、项目根目录（代码/文档所在，不放数据）

```
D:\project\ai生成\
├── README.md / STORAGE.md / LICENSE
├── start.cmd / index.html / package.json / vite.config.ts / tsconfig.json
├── package.json / vite.config.ts / tsconfig.json
├── index.html
├── start.cmd                  # 日常启动（详见 README「怎么启动」）
├── src\                       # 前端（React + TS）
├── src-tauri\                 # 后端（Rust）
│   ├── Cargo.toml             # ⚠ 含必需的 [features] custom-protocol，别删
│   ├── tauri.conf.json
│   └── src\                   # paths.rs / keys.rs / branch.rs / generate_stream.rs ...
├── docs\
│   └── 开发期的文档（主参考）\
│       ├── ROADMAP.md         # ★ 想知道"现在到哪了"看这个
│       ├── 历史计划存档.md      # 原 PLAN.md + 总计划.md（历史记录）
│       ├── SPEC.md / UI_TREE.md
│       ├── 分支系统实施方案.md / 立绘系统（设计+使用）.md
│       └── 开发流程.md / 开发规范.md / 测试用例.md / 最小MVP实现.md
├── 规则\ai输出规则\AI输入输出规则.md
└── data\                      # ⚠ 只有"兜底到 exe 同级"时才是数据根
```

**遗留空目录已于 2026-10-16 删除**：`app/` `resources/` `config/` `plugins/`
`models/` `cache/` `logs/` `backups/` `src-tauri/logs/` —— 每个都确认过
**0 文件 + git 未跟踪** 才删的（来自旧的"根目录集中"规划，代码 0 引用）。

仍然保留的：`exports/`（里面有 2 个 2026-09-27 的真实导出）、`data/`（兜底数据根）、
`devlog_m5.txt`（历史开发日志）。

---

## 五、文档归档

见上面第四节的项目根目录树。`docs/` 已按用户指定分类归档；`规则/` 放 AI 输入输出规范。

---

## 变更记录

| 日期 | 版本 | 变更 |
|------|------|------|
| **2026-10-16** | **v1.0** | **全面重写**：改为"数据根 + 逐级可写性探测"模型；补"未实现/遗留"清单（cache_entries 无写入、plugins/models/resources 未实现、config 已废弃、app 未打包）；story.db 表数 12→14（含 branches）；标注根目录遗留空目录 |
| 2026-10-16 | v0.4 | 追加"数据根目录"一节（与旧结构并存，本次重写已合并） |
| 2026-09-29 | v0.3 | 同步现状：导出 txt/md/html 三格式已实装；立绘素材存储位置 |
| 2026-09-21 | v0.2 | 按根目录结构参考重写：目录树 + 存储位置总表 + SQLite 关系 |
| 2026-09-21 | v0.1 | 创建占位 |
