# SPEC · 产品规格

> 受众：开发者 / 验收对照 ｜ 状态：**现行** ｜ 最后同步：**2026-10-20**（按代码实测重写）
>
> **本文描述"软件实际怎么表现"，不是"当初打算怎么设计"。**
> 两者冲突时，以代码为准，并回来改本文。
> 计划中、未实装的东西一律进 §8 的「未实装」表，不混进正文冒充已有功能。

---

## 1. 产品概述

单机 Windows 桌面软件，用于 AI 辅助长文剧情创作。

- **纯本地**：本体不联网；唯一出站是**用户自己配置**的模型 API 与立绘服务
- **无作者服务器**：不存在"数据回流到作者"的路径（架构上没有）
- **核心机制**：剧情 AI 只写故事；设定变更由后台分析器提出候选 → 人工确认 → 落库
- **数据**：SQLite，明文；API Key 只进 Windows 凭据管理器

---

## 2. 技术形态（实装）

| 层 | 实际技术 | 备注 |
|---|---|---|
| 桌面壳 | **Tauri 2**（Rust） | `src-tauri/` |
| UI | **React 19 + TypeScript**，Vite 8 构建 | **全部内联样式**，无 CSS 框架、无状态库 |
| 数据库 | **SQLite + sqlx** | `library.db`（索引/共享）+ 每故事一个 `story.db` |
| 网络 | `reqwest` + `tokio` | 流式 SSE 解析 |
| Key 存储 | `keyring` → Windows 凭据管理器 | `keys.rs` / `keychain.rs` |
| 图片处理 | `image` + `png` crate | AI 标识烧录、格式转换 |
| Token 估算 | 前端字符粗估 + 后端预算计算 | 双端各有一套，口径见 §5.2 |

> 曾规划但**未采用**：Tailwind、Zustand、tiktoken-rs、Electron（原型已废弃）。

---

## 3. 数据模型（实际 15 张表）

### 3.1 归属

| 库 | 位置 | 内容 |
|---|---|---|
| `library.db` | `<数据根>\library\default\library.db` | 故事索引、全局配置、缓存索引 |
| `story.db` | `<数据根>\library\default\projects\<故事>\story.db` | 该故事的全部数据 |

> 数据根 = `%APPDATA%\com.guchenxi.story-weaver`（受限环境兜底到 `<exe同级>\data`），
> 可被环境变量 `DREAMWEAVER_DATA_DIR` 覆盖。详见 [数据与备份.md](../user/数据与备份.md)。

### 3.2 表清单

| 表 | 所在库 | 关键字段 | 用途 |
|---|---|---|---|
| `stories` | library | id, title, time_origin, prologue, current_branch_id | 故事 + 绝对时间起点 + **前史/序章** + 当前分支 |
| `configs` | library | key, value | 全部配置（见 §7.1） |
| `cache_entries` | library | path, size, type, last_access | 缓存索引（**当前从未写入**，见 §8） |
| `timeline_nodes` | story | id, parent_id, branch_id, date_label, sort_order, day_summary | 按日节点（树形，`parent_id` 成链） |
| `messages` | story | id, node_id, branch_id, role, content, mode, meta | 剧情消息；`mode` 实际写 `narr` / `chat` |
| `characters` | story | id, branch_id, name, role, gender, 档案字段, stat_stamina / stat_sanity / stat_emotion / stat_screen, version | 角色档案 + 四属性  + **v3.4 角色卡 v2**：background_past / motivation / goal / dialogue_examples / behavior_rules |
| `relations` | story | id, branch_id, from/to_character_id, relation_type, direction, strength, state, evidence, version | 关系（含出处与版本） |
| `world_settings` | story | id, branch_id, key, value, category, is_hard_rule, meta_json, version | 设定条目（设定/地理/势力/势力关系/术语/**物品**/**世界书**） |
| `setting_snapshots` | story | id, branch_id, version, payload_json, parent_version, temporary, note | 设定版本链（回滚依据；`temporary` 区分会话内临时版本） |
| `change_proposals` | story | id, branch_id, group_id, node_id, target_type, target_id, field, old_value, new_value, reason, evidence, confidence, status, note | 演进提案（`group_id` 供批量落地） |
| `event_logs` | story | id, node_id, event_type, payload_json, confidence, source | 事件日志 |
| `audit_logs` | story | id, action, target, before, after, operator | 审计 |
| `story_summaries` | story | id, branch_id, day_from, day_to, source_days, content | **阶段摘要**（多天压缩成一段） |
| `branches` | story | id(UUID), story_id, name, fork_node_id, source_branch_id, color, note | 分支（`source_branch_id` 指向父分支，支持嵌套） |
| `debug_requests` | story | id, node_id, model, system_prompt, history_json, user_text, ai_reply, estimated_tokens, n_used, status, error | 请求记录（「生成参数」页的数据源；每次生成前写入、超 300 条自动裁剪） |

> **分支感知**：`timeline_nodes` / `messages` / `characters` / `relations` / `world_settings` /
> `setting_snapshots` / `change_proposals` / `event_logs` / `audit_logs` / `story_summaries`
> 共 10 张表带 `branch_id`，`NULL` 表示主线。

---

## 4. 功能规格

### 4.1 故事与时间线

- 新建故事时用**内联表单**（不是弹窗）设定标题、**绝对时间起点**与**前史/序章（v3.3，可空）**；"第 N 天"由系统换算，AI 只填内容
- **前史/序章**：故事开始之前发生过的事（例："三年前 A 与 B 在码头分开"）。**故事级**静态设定，不随分支变化；
  进提示词稳定前缀的 `【前史】` 段（在 `【世界观】` 之前），空串则整段不出现。建好后可在故事卡片上编辑/补写
- 按日建 `timeline_nodes`；节点用 `parent_id` 组成树，**跨天装配按父链取**
- **跳号不建空节点**：相邻天数之间没内容时，时间线上显示一条「时间流逝」分隔，点它可插入事件
- 插曲 / 倒叙：可作为独立节点插入，带标签与时空提示
- 修历史：改完后续基于新内容续写；批量落地前自动打 `setting_snapshots`
- **统计按节点局部化**，不做全局统计


#### 世界观的四类条目（`world_settings.category`）

| 分类 | 用途 | 提示词渲染 |
|---|---|---|
| `地理` | 地点（含地图坐标、归属）| `- 地点「x」：说明（区域：…；归属：…）` |
| `势力` | 组织（控制范围 / 规模 / 头目 / 态度）| `- 势力「x」：说明（…）` |
| `势力关系` | 两个势力之间的关系 | `- 势力关系：A 对 B：…` |
| `术语` | 概念 / 组织 / 地名 | `- 术语「x」：释义（别名：…）` |
| **`物品`（v3.4）** | **能拿在手里的东西**（怀表、钥匙、药瓶、信物）| `- 物品「x」：说明（别名：…）` |
| **`世界书`（v3.4）** | 多而散的设定（人物前史 / 器物来历 / 某地习俗）；**触发词命中才注入** | 尾部块的 `【世界书】` 段：`- 「x」：内容`（全量条目**不进** system 世界观段）|
| `设定`（其他）| 杂项设定，带"硬规则"标记 | `- x：说明 [硬规则，不可违反]` |

- **物品库**在「世界观设定」页是独立页签（`🎒 物品库`），带搜索框（支持几十~上百条），
  字段与术语同构：物品名 / 别名（逗号分隔）/ 说明。
- 物品**全量注入**提示词（一行一条，通常只有十几条，不值得做条件注入）；
- 物品名**参与「🔍 回顾」的实体召回**（与地理/术语/势力同等权重）；
- 「🔍 检查」对物品用通用规则（空名 / 重名 / 空说明），无需专门代码。
### 4.2 角色与世界观

- 角色：增删改查 + 四属性（体力/理智/情绪/存在感）+ `version` 版本化
- **角色卡 v2（v3.4）**：新增五个字段 —— `background_past`（过去）/ `motivation`（动机）/ `goal`（目的）/
  `dialogue_examples`（对话示例）/ `behavior_rules`（行为准则）。前四项进 system 的【角色】段（`prompt.rs`），
  `dialogue_examples` 走**尾部块**的【示例对话】（只给焦点角色，且明确要求「不要照抄」）
- 关系：`relation_type` / `direction` / `strength`(0–100，越界自动夹到区间) / `state` / `evidence`
- 关系注入句式：单向 = 「A 是 B 的『X』」；双向自动拆成两条单向；亲属词按性别/年龄判长幼，
  判不出用 from 兜底；空类型 → 「A 和 B 是陌生人」（`relations.rs:55–84`）
- 世界观：条目 + `is_hard_rule` 硬规则标记；冲突检测只查结构性问题，语义问题交 AI

### 4.3 剧情生成

**输入语法（实装）**

| 写法 | 含义 | 处理 |
|---|---|---|
| 裸文本 | 正常剧情/对话内容 | 直接进入上下文 |
| `*……*` 整行 | 角色**心声** | 渲染为心声样式（斜体 + 暖玫瑰色 + 左竖线），不显示星号 |
| `#标签` | **元指令** | 从正文中剥离，不进剧情上下文（`useGeneration.ts:101`） |
| `@角色名` | 本次聚焦 | 仅注入提示词，不改正文 |
| `〔导演指令〕…` + 空行 + 正文 | 导演指令 | 一次性生效，发完清空；带图标单独显示在气泡上方（`parse.ts:143`） |

**两种模式（实装：旁白 / 对话）**

| 模式 | 行为 | 记录值 |
|---|---|---|
| 🎭 旁白 | 叙述者视角生成旁白 | `mode = "narr"` |
| 💬 对话 | 以角色身份直接对话（沉浸） | `mode = "chat"` |

> 系统提示词按模式切换措辞（`generate_stream.rs:50`）。**"上帝视角扩写"模式已放弃**（不是未做，是决定不做），见 [ROADMAP §2.2](ROADMAP.md)。

**其它**

- 输入台：长度档位、敏感词过滤（默认开，可关）、剧情约束、记忆开关、🔍 回顾
- 流式输出；生成中显示 ⏹ 停止；失败保留已生成部分并标记
- 生成完成自动触发：状态提取（`state_extract.rs`）→ 可选自动演进 → 可选自动日摘要

### 4.4 上下文管理

**装配顺序**（`generate_stream.rs:39`）

```
基础指令 + 输出排版 → 数据字典 → 世界观 → 角色 → 方向与关系 → 【前情提要】→ 近期原文 → 本次输入
```

**三条铁律**

1. **绝对不截断**：系统提示词、数据字典、世界观、角色、关系——这五段永不压缩
2. **只截断历史**：近三天原文 + 更早的日摘要可截断
3. **超预算先砍检索块**，绝不挤占近三天原文

**摘要两级**

| 层级 | 存储 | 说明 |
|---|---|---|
| 日摘要 | `timeline_nodes.day_summary` | 每天一条，生成一次即固化 |
| 阶段摘要 | `story_summaries` | 多天压成一段；`coalesce_with_phases` 把被覆盖的日摘要**折叠成一行**再进提示词 |

**近三天口径**：按**节点数**（当前节点 + 前 2 个 day 节点），不按自然日。

**N 值自适应**：按模型窗口与预算动态分配，濒临超限自动降 N。

### 4.5 状态提取

- 来源：最新 AI 回复 + 最新用户输入
- 方式：`state_extract.rs` 规则优先；提取结果更新角色状态卡
- 输出要求：提示词强制"必须自然包含动作 / 心理 / 神态"，**不用 JSON 标记**

### 4.6 后台演进（核心机制）

```
回复完成 → 分析器读（设定 + 关系 + 当日剧情 + 节点）
        → 抽候选事件（置信度阈值） → 判是否实质改变设定
        → 写 change_proposals（默认 status = 待确认）
        → 用户确认 → 打快照 → 落库 → 写审计
```

| 参数 | 实际值 | 代码 |
|---|---|---|
| 自动分析 | 默认开（`evolution.auto_analyze = true`） | `db.rs` |
| 要求人工确认 | 默认**开**（`evolution.require_approval`） | `evolution.rs:150` |
| 变更冷却期 | **3 天** | `evolution.rs:28` `COOLDOWN_DAYS` |
| 关系强度范围 | 0–100，越界夹取 | `relations.rs:171` |
| 支持的应用类型 | **仅关系类**（其它类型报"暂不支持"） | `evolution.rs:569` |

### 4.7 分支

- 手动分叉，**不自动**；分支身份用 `branches` 表（UUID / 名称 / 颜色 / 备注）
- 删除规则：只删自己的；被别的分支当分叉点依赖时拒绝；删当前分支则归主线
- `resolve_branch` 两层校验（故事在吗 / 分支在吗），任一不成立 → 归主线并清理悬空记录

### 4.8 RAG 回顾检索（① 方案 A）

- **自动**：本地规则识别"回忆意图"（纯函数、零 API）
- **手动**：界面「🔍 回顾」指定某一天，强制拼进提示词
- 范围：只在本分支可见范围内，且只取**早于近三天原文区**、有 `day_summary` 的天
- 打分权重可配（`recall_get_weights` / `recall_set_weights`），有试算入口 `recall_explain`

### 4.9 设置与安全

- API Key：Windows 凭据管理器，**不进数据库、不进备份、不进导出**
- 缓存：面板展示占用，上限默认 5GB，超限自动清理
- **敏感词过滤**：前端拦截输入 + 输出替换为 `***`，默认开启、可关闭

---

## 5. 页面构成

### 5.1 12 个真实视图（详见 [UI_TREE.md](UI_TREE.md)）

`home` · `stories` · `story` · `timeline` · `chars` · `world` · `proposals` ·
`portrait` · `params` · `plugins` · `compliance` · `temp`

> 旧文档列的"12 个必备页面"把**面板**也算成了页面（关系图 / 焦点角色卡 / 缓存管理 / 日志），
> 同时漏了立绘素材、插件、合规、临时文件。真实清单以上表为准。

### 5.2 Token 预算口径

- 后端算：`context_window` × `reserve_ratio` 得到可用预算，再扣 `safety_margin`
- 前端显示：`ParamsPage` 的「📊 Token 预算」页签 + 输入台角标
- 默认值：`context_window = 32768`、`max_tokens = 4000`、`safety_margin = 800`
  （`reserve_ratio` 库里默认 1.0，UI 未取到值时按 0.8 兜底）

---

## 6. 提示词与上下文

### 系统提示词结构

**稳定前缀**（进 system，逐轮不变 → 利于前缀缓存）

```
基础指令（按模式切换措辞）+ 输出排版要求
  → 【数据字典】（关系强度档位 + 状态含义，只出现一次）
  → 【时间基准】（第 1 天 = 绝对起点；季节/节日/年龄都按它换算）
  → 【前史】（v3.3，故事级；空则整段不出现）
  → 【世界观】（世界书条目**不在这里**）
  → 【角色】（含 过去 / 动机 / 目的）
  → 【方向和关系】
  → 【前情提要】（日摘要 + 阶段摘要折叠）
```

**尾部块**（每次请求重建，易变）

```
【当前模式】→【篇幅要求】→【数值与状态】→【世界书】（命中才出现）
  →【示例对话】→【本次聚焦】→【剧情约束·绝对红线】
  →【时空提示】（插曲/倒叙时）/【导演指令】（有则）
```

> **顺序有讲究**：世界书排在【示例对话】之前 —— 先补"她背后的世界"，再教"她怎么说话"；
> 示例对话排在【本次聚焦】之前 —— 先给"她怎么说话"，再说"这一段围绕她"。
> 两处都有测试守着（`generate_stream/budget_tests.rs`）。

- 分支感知：世界观 / 角色 / 关系都读**该分支自己的版本**（主线 = `branch_id IS NULL`）
- 世界书**只走尾部块**（触发词命中才注入），**绝不能进 system 的【世界观】**——这条有测试守着

---

## 7. 存储与导出

| 数据 | 位置（相对数据根） |
|---|---|
| 故事数据 | `library\default\projects\<故事>\story.db` |
| 全局配置 | `library\default\library.db` 的 `configs` 表 |
| API Key | Windows 凭据管理器 |
| 日志 | `logs\`（含 `startup-trace.log`） |
| 导出 | `exports\story\` |

**导出三格式已实装**：TXT / MD / HTML（`export.rs:197/276/332`）。
MD 把对话加工成引用块；HTML 是浅色阅读主题单文件；文件名带分钟级时间戳；
文末附【AI 生成内容声明】（不可关闭）。

### 7.1 配置默认值

| 键 | 默认值 |
|---|---|
| `model.base_url` | `https://api.openai.com/v1` |
| `model.name` | `gpt-4o-mini` |
| `model.context_window` | `32768` |
| `model.max_tokens` | `4000` |
| `model.safety_margin` | `800` |
| `context.n_recent` | `10` |
| `evolution.auto_analyze` | `true` |
| `evaluate.auto` | `false` |
| `cache.max_bytes` | 5 GB |
| `portrait.base_url` | `http://127.0.0.1:7860` |

---

## 8. 未实装（不要按本文假设它存在）

> **已明确放弃、不再实现**：上帝视角扩写模式、裸文本元指令、蝴蝶效应预览（2026-10-20 用户决定）。
> 见 [ROADMAP §2.2](ROADMAP.md)。

| 项 | 说明 |
|---|---|
| **表情差分 ×4 / 主色卡** | 未实装 |
| **`cache_entries` 从未写入** | 缓存面板恒显示 0；上限与清理逻辑已实装但无数据来源 |
| **429/5xx 自动重试** | 未实装（只有超时与取消） |
| **结构化抽取（非关系类）** | 未实装；`apply_group` 对非 relation 类型直接报错 |

---

## 9. 异常与边界

> 下表是**设计编号**（E1–E12），代码里**没有**这些常量名。对照时看"处理策略"这一列。

| # | 异常 | 处理 |
|---|---|---|
| E1 | 空输入 | 不发送 |
| E2 | 括号/引号未闭合 | 提示位置，不提交 |
| E3 | ~~模式冲突~~ | **已放弃**：上帝视角模式不做，此条不再适用 |
| E4 | Token 超限 | 自动降 N / 动态不传世界观与关系；仍超则明确提示 |
| E5 | 模型不可用 | 错误提示 + 保留输入可重试，不丢数据 |
| E6 | 提案校验失败 | 拒绝并给原因（不半截执行） |
| E7 | 冷却期内重复变更 | 3 天内不自动应用，只记录 |
| E8 | 低置信度候选 | 忽略，不生成提案 |
| E9 | 回滚不可用 | 禁用并提示 |
| E10 | 存储写入失败 | 报错，数据保留在界面 |
| E11 | 未配置 Key | 引导到设置页 |
| E12 | 缓存超限 | 自动清理最久未用 |

---

## 10. 版本记录

本文件自 2026-10-20 起**只记录规格层面的变更**。产品版本与功能变更见 [ROADMAP.md](ROADMAP.md)。

| 日期 | 变更 |
|---|---|
| 2026-10-20 | 按代码实测重写：表清单 12 → 15；修正"双模式""三重语法"；补分支/RAG/阶段摘要；页面按真实视图重列 |