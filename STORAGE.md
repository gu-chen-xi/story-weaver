# 存储与数据规划（STORAGE）

> 项目版本：**V1.9**（2026-10-11）｜本文档最后同步：2026-10-11

> 版本：V1.0 Windows 单机版
> 说明：仅 Windows，无多用户协作，所有数据集中在根目录 `D:\project\ai生成` 下。
> 依据：根目录结构参考（用户提供，已按项目实际修正）+ 最小MVP实现.md（SQLite 数据模型）

---

## 一、根目录结构（落地规划）

```
D:\project\ai生成\
├─ app\                    # 程序本体、可执行文件、构建产物
├─ resources\              # 内置资源、默认模板、预设、图标
├─ config\                 # 全局配置（模型参数、默认值等）
├─ library\                # 故事库
│  └─ default\             # 默认故事库
│     ├─ library.db        # 库索引数据库（故事列表/共享条目索引）
│     ├─ projects\         # 故事项目（每个故事一个子目录，含独立 SQLite 数据库）
│     ├─ shared\           # 共享角色、世界观、预设
│     ├─ backups\          # 库级备份
│     └─ trash\            # 回收站
├─ plugins\                # 插件
│  ├─ installed\           # 已安装插件
│  └─ data\                # 插件运行数据
├─ models\                 # 本地模型文件
├─ cache\                  # 缓存（模型响应、临时渲染等）
├─ logs\                   # 日志（请求/Token/分析器/错误）
├─ backups\                # 全局备份
├─ temp\                   # 临时文件
├─ exports\                # 默认导出位置
│  ├─ quick\               # 快速导出
│  ├─ backup\              # 完整备份导出
│  ├─ story\               # 剧情、文本导出
│  ├─ character\           # 角色卡导出
│  └─ portrait\            # 立绘、图片导出
└─ docs\                   # 项目文档（见下方"文档归档"映射）
```

> 说明：`docs\` 为参考结构中的项目文档位；当前实际文档按用户指定分类归档（见文末"文档归档"），后续可迁入 `docs\`。

---

## 二、数据存储位置总表

| 数据 / 功能 | 保存位置 | 说明 |
|------------|---------|------|
| 库索引 | `library\default\library.db` | 故事列表、共享条目索引 |
| 故事项目（时间线/剧情/角色/关系/世界观/快照/日志/提案/审计） | `library\default\projects\<故事>\story.db` | 每故事独立 SQLite，表结构见《最小MVP实现.md》§3 |
| 共享角色 / 世界观 / 预设 | `library\default\shared\` | 跨故事复用 |
| 回收站 | `library\default\trash\` | 删除的片段/角色暂存 |
| 库级备份 / 全局备份 | `library\default\backups\` · `backups\` | 快照回滚 + 定期备份 |
| 全局配置（模型/默认参数） | `config\` | 与 MVP 表 configs 对应 |
| API Key | 系统钥匙串 | **不落盘到项目目录**（MVP 4.7 硬性要求） |
| 缓存 | `cache\` | 上限/清理见 MVP 4.7 |
| 日志 | `logs\` | 请求 / Token / 上下文组成 / 分析器 / 错误 |
| 导出 | `exports\` 下分类目录 | quick / backup / story / character / portrait |
| 插件 | `plugins\installed\` + `plugins\data\` | 插件本体与运行数据分离 |
| 本地模型文件 | `models\` | Ollama / llama.cpp 模型 |
| 临时文件 | `temp\` | 程序运行期临时产物 |

---

## 三、与 MVP 数据模型的关系

- 数据库：**本地 SQLite**（MVP §3 全部表：stories / timeline_nodes / characters / relations / world_settings / messages / setting_snapshots / event_logs / change_proposals / audit_logs / configs / cache_entries）
- 分布：`library.db` 管库索引与共享；**每故事独立 `story.db`**（`library\default\projects\<故事>\story.db`）管该故事全部剧情与演进数据【已确认 2026-09-21】
- 导出：`exports\` 分类目录；**txt / md / html 均已实装**（导出弹窗三格式可选，MD 引用块、HTML 浅色阅读主题、文件名时间戳防覆盖）【html/md 原计划后置，2026-09-27 已实装 418f216】

---

## 四、待确认点

- [x] 故事项目数据：每故事独立 `story.db`【确认】
- [x] `docs\` 收纳现有文档：`开发期的文档（主参考）\` 已移入【确认 2026-09-21】
- [x] 导出格式：MVP 仅 txt，html / md 后置为最终计划【确认 2026-09-21】→ **2026-09-27 已实装三格式导出（418f216）**
- [ ] （预留：新规划补充项）

---

## 五、文档归档（当前实际布局）

```
D:\project\ai生成\
├── README.md                     # 根目录
├── STORAGE.md                    # 根目录（本文件）
├── docs\
│   └── 开发期的文档（主参考）\
│       ├── SPEC.md / UI_TREE.md / PLAN.md
│       ├── 主界面设计方案.md / 开发流程.md / 开发规范.md
│       ├── 测试用例.md / 最小MVP实现.md
└── 规则\
    └── ai输出规则\
        └── AI输入输出规则.md
```

---

## 变更记录

| 日期 | 版本 | 变更 |
|------|------|------|
| 2026-09-29 | v0.3 | 同步现状：导出格式改为 txt/md/html 三格式已实装（418f216）；立绘素材存储（library\default\shared\portraits\，portraits 表在 library.db，版本历史 portrait_versions） |
| 2026-09-21 | v0.2 | 按根目录结构参考重写：目录树 + 存储位置总表 + SQLite 关系 + 待确认点 |
| 2026-09-21 | v0.1 | 创建占位 |