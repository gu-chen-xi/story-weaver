---
# 二、最小 MVP 的全部必备

> 项目版本：**V1.8**（2026-10-10）｜本文档最后同步：2026-10-10
## 1. MVP 目标
做一个**端到端最小闭环**：
> 创建故事 → 设定时间起点 → 创建角色和世界观 → 写剧情 → 调用模型生成 → 按日保存 → 重启恢复 → 修改历史 → 基于修改续写 → 后台生成设定变更提案 → 人工确认 → 快照更新 → 可回滚。

只要这个闭环成立，MVP 就成立。
---
## 2. MVP 必备范围
### 必须有
- 故事管理
- 绝对时间起点
- 角色库
- 角色关系
- 世界观
- 按日剧情存储
- 模型调用配置
- 上下文组装
- Token 估算与智能截断
- 内容包含性提示词
- 状态提取基础版
- 角色状态卡
- 事件日志
- 设定快照
- 变更提案
- 人工确认 / 拒绝 / 回滚
- 审计日志
- 本地 SQLite
- API Key 系统钥匙串
- 基础缓存管理
- 日志与调试面板
- 打包
### 可以后置
- 全自动后台演进
- 复杂关系图可视化
- 蝴蝶效应分支合并
- 插件系统
- 本地生图
- 多模型全适配
- 独立角色对话完整版
- 高级动态情绪
- 云同步
### 明确不做
- 插件市场
- 多用户协作
- 复杂权限系统
- 本地大模型训练
- 自动生图工作流编排
- 完整分支时间线合并
---
## 3. 核心数据模型
### stories
- id
- title
- time_origin
- created_at
- updated_at
### timeline_nodes
- id
- story_id
- date_label
- sort_order
- node_type
- is_flashback
### characters
- id
- story_id
- name
- profile
- current_status
- version
### relations
- id
- story_id
- from_character_id
- to_character_id
- relation_type
- direction
- strength
- state
- active_from
- active_to
- version
### world_settings
- id
- story_id
- key
- value
- is_hard_rule
- version
### messages
- id
- story_id
- node_id
- role
- content
- mode
- meta
- created_at
### setting_snapshots
- id
- story_id
- version
- payload_json
- parent_version
- created_at
### event_logs
- id
- story_id
- node_id
- event_type
- payload_json
- confidence
- source
### change_proposals
- id
- story_id
- target_type
- target_id
- field
- old_value
- new_value
- reason
- evidence
- confidence
- status
- created_at
### audit_logs
- id
- story_id
- action
- target
- before
- after
- operator
- created_at
### configs
- key
- value
### cache_entries
- path
- size
- type
- last_access
---
## 4. 必备功能模块
### 4.1 故事与时间线
- 创建故事
- 设定绝对时间起点
- 自动计算 X 天后
- 按日存储剧情
- 日历视图
- 插入插曲 / 倒叙
- 修改历史剧情
- 标记后续失效
### 4.2 角色与世界观
- 角色增删改查
- 角色初始设定
- 角色状态
- 关系增删改查
- 世界观条目
- 硬规则标记
### 4.3 剧情生成
- 输入框
- 三重语法解析
- 圆括号：动作、表情、心理
- 双引号：台词
- 裸文本：元指令
- 元指令拦截
- 模式切换：沉浸式 / 上帝视角
- 调用模型
- 流式输出
- 保存到当前时间节点
### 4.4 上下文管理
- 系统提示词
- 角色设定
- 关系图
- 世界观
- 最近 N 轮对话
- 动态上传世界观和关系图
- Token 估算
- 智能降级 N 值
- 防超限
- 锁定设定快照版本
### 4.5 状态提取与状态卡
- 从文本提取情绪、动作、神态
- 规则优先
- 模型辅助
- 更新角色状态卡
- UI 叠加效果
- 不重新生图
### 4.6 后台演进
- 事件日志
- 候选事件提取
- 实质性判断
- 变更提案
- 校验
- 人工确认 / 拒绝
- 应用变更
- 生成新快照
- 回滚
- 审计
### 4.7 设置与安全
- API Key 系统钥匙串
- 不保存到剧情文件夹
- 模型参数
- 缓存管理
- 缓存上限
- 手动清理
- 自动清理
### 4.8 调试与日志
- 请求日志
- Token 使用
- 上下文组成
- 分析器结果
- 变更提案
- 错误日志
---
## 5. 必备页面
1. 故事列表 / 创建故事
2. 主剧情界面
3. 日历 / 时间线
4. 角色库
5. 世界观
6. 关系图
7. 设定演进提案面板
8. 角色状态卡侧栏
9. 设置
10. 缓存管理
11. 日志 / 调试面板
---
## 6. AI 上下文与提示词必备
### 系统提示词必须包含
- 角色身份
- 当前关系
- 世界观
- 输出要求
- 必须自然包含动作、心理、神态
- 不得输出 JSON 或死板格式
- 不得自行修改设定
### 上下文顺序
1. 系统提示词
2. 角色设定
3. 关系图
4. 世界观
5. 最近 N 轮对话
6. 当前用户输入
### 截断策略
- 绝对不截断：系统提示词、人物设定、关系图、世界观
- 只截断：AI 历史生成 + 用户历史输入
- 超限时自动降 N
- 动态决定是否上传世界观和关系图
---
## 7. 技术栈建议
- 桌面壳：Tauri 2
- 核心后端：Rust
- UI：React + TypeScript + Vite
- 样式：Tailwind CSS
- 数据库：SQLite
- Rust 数据库：sqlx
- 状态管理：Zustand
- API Key：系统钥匙串
- 网络：reqwest + tokio
- Token：tiktoken-rs
- 本地模型：Ollama / llama.cpp
- 云端模型：OpenAI-compatible
- 分析器：规则引擎 + 本地小模型，可选 Python sidecar
- 打包：Tauri Bundler
建议：
- Tauri + Rust 做壳和核心
- Python sidecar 做 NLP 和分析器
- 边界清晰，后期可替换
---
## 8. MVP 验收标准
必须全部通过：
1. 能创建故事并设定绝对时间起点。
2. 能创建角色 A、B，并设置关系：暗恋。
3. 能写第 1 天剧情，调用模型生成，保存。
4. 重启软件后，数据恢复。
5. 能续写第 2 天，上下文包含设定和最近对话。
6. 修改第 1 天剧情后，第 2 天能基于修改续写。
7. 后台能生成关系变更提案。
8. 用户能接受、拒绝、回滚。
9. 应用后生成新快照，后续剧情读取新快照。
10. 连续写 3 天剧情，角色设定不明显崩坏。
11. Token 不超限，N 值能自动降级。
12. API Key 不在剧情文件夹中。
13. 缓存管理能看到占用并清理。
14. 日志面板能看到上下文组成和分析器结果。
---
## 9. 开发顺序
1. M0：项目骨架、SQLite、设置、API Key
2. M1：故事、角色、世界观 CRUD
3. M2：剧情生成闭环
4. M3：时间线按日存储
5. M4：上下文管理与截断
6. M5：状态提取与状态卡
7. M6：事件日志、快照、变更提案、人工确认
8. M7：回滚、审计、缓存管理
9. M8：测试、打包、验收
---