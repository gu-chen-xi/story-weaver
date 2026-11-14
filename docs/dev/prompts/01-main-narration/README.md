# 主生成 · 旁白（narr）

> 受众：开发者 ｜ 状态：现行 ｜ **最后核对：2026-11-14（按代码逐段核对）**

## 什么时候发

用户在输入台切到「旁白」并提交时，前端 `useGeneration.ts` 调 `generate_reply_streaming`，
`mode = "narr"`、`myRole` / `aiRole` 传给后端但**不影响装配**（旁白永远是全局视角：
`PromptScope::default()`，不看 `prompt.roleplay_scope`）。

## 代码入口

| 职责 | 代码：`文件::符号` |
|---|---|
| 主入口（流式生成） | `generate_stream.rs::generate_reply_streaming` |
| system 前缀装配 | `generate_stream/prompt.rs::build_system_prompt` |
| 尾部块装配 | `generate_stream/prompt.rs::build_tail_block` |
| 固定输出规则 | `generate_stream/prompt.rs::OUTPUT_FORMAT_RULES` |
| 台词口径 | `characters.rs::performance_rules()` |
| 关系段图例 | `relations.rs::coop_legend()` / `relations.rs::footing_rule()` |

## 请求长什么样

```
[system]      稳定前缀（① 输出排版 → ② 数据字典 → ③ 台词口径 → ④ 时间基准
              → ⑤ 前史 → ⑥ 世界观 → ⑦ 角色（全员）→ ⑨ 配合度 → ⑩ 关系态势
              → ⑪ 情绪表演 → ⑫ 关系图谱（全部方向））
[messages]    历史（最近 3 天原文；更早只有日摘要/阶段摘要，见「动态内容」）
[user]        【上下文更新】尾部块 + 分隔线 + 用户输入（同一条 user 消息）
```

> 段顺序的**逐段表格**在 [`拼接顺序.md`](拼接顺序.md)；哪些段是运行时算的在 [`动态内容.md`](动态内容.md)。

## 与「对话」场景的唯一分界

旁白**不做角色隔离**：所有角色卡、所有关系方向都进提示词。
差异细节（以及角色视角那一套）见 [`../02-main-dialogue/与旁白的差异.md`](../02-main-dialogue/与旁白的差异.md)。

## 验证方式

- 真实请求：开发者工具 →「请求快照」→ 选一条 `mode=narr` 的
- 只看装配：生成参数 →「Prompt 预览」/「双池对比」（旁白那栏就是本场景）
- 单测：`cargo test --lib generate_stream`（预算、装配顺序、世界书/记忆接线）
