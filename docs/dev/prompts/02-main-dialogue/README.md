# 主生成 · 对话（角色视角 · 按配对）

> 受众：开发者 ｜ 状态：现行 ｜ **最后核对：2026-11-14（按代码逐段核对）**

## 什么时候发

输入台切到「对话」并提交时，`generate_reply_streaming` 收到 `mode = "chat"`。
对话模式**默认走角色视角**（故事级配置 `prompt.roleplay_scope = "role"`）；
写 `global` / `false` / `0` / `off` 会退回全量（行为与旁白一致）。

## 代码入口

| 职责 | 代码：`文件::符号` |
|---|---|
| 主入口 | `generate_stream.rs::generate_reply_streaming` |
| 视角开关判定 | 同文件内 `roleplay_on`（读 `prompt.roleplay_scope`） |
| 视角参数 | `generate_stream/prompt.rs::PromptScope { role, my_role }` |
| system 装配 | `generate_stream/prompt.rs::build_system_prompt` |
| 尾部装配 | `generate_stream/prompt.rs::build_tail_block` |
| 历史的视角过滤 | `timeline/message.rs::visible_in_role_view` |
| 记忆过滤 | `memory.rs::filter_for_role` |

## 请求长什么样

结构与旁白**完全相同**（system 前缀 / 历史 / 一条 user 消息），差别只在三处：
**角色卡只留她、关系只留她出发的方向、历史只留"这一对 + 旁白 + 无标记"**。
逐段看 [`拼接顺序.md`](拼接顺序.md)，和旁白的对照看 [`与旁白的差异.md`](与旁白的差异.md)。

## 关键概念：配对（pair）

视角的键是 **`ai_role` × `my_role` 这一对**，不是单个角色：

- `a × b` 与 `b × a` 是同一段关系（顺序无关）；
- `a × b` 和 `a × c` 是两段不同的关系，历史彼此不可见；
- 换扮演者会换一段关系视角 —— 界面会弹提示（`lib/roleHint.ts`），旧对话不丢、只是不在当前视角里。

## 验证方式

- 生成参数 →「双池对比」：左全局 / 右角色，下半是尾部依赖视角的块，第③段按关系组看记忆；
- 开发者工具 →「请求快照」：`system_prompt` 里只应有一张卡；
- 单测：`generate_stream/pair_prompt_tests.rs`、`timeline/message.rs::role_view_tests`、`memory::*_tests`
