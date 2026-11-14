# 提示词拼接说明（只读参考）

> 受众：开发者 ｜ 状态：**现行**（只读文档，不含运行代码）｜ 最后核对：2026-11-14（按代码逐段核对）

**这个文件夹是"拼接说明书"，不是"提示词仓库"。** 提示词原文永远只在代码里（`src-tauri/src/...`），
要看你当前故事的真实请求，用「生成参数 → 双池对比」或「开发者工具 → 请求快照」。

## 怎么读（省时间）

1. **先看对应文件夹里的 `README.md`** —— 确认"这个场景什么时候发、请求长什么样"。
2. **确认没问题之后，再看这个场景里具体负责的文档**（拼接顺序 / 动态内容 / 输出契约 / 已知坑），
   以免一上来就把整棵目录读完。
3. 只想知道"某个场景存在吗" → 看下面这张总表，不用进目录。

## 三条硬规范（新建/修改这类文档时必须遵守）

1. **代码位置写 `文件::符号`，不写行号**（行号会飘）。
   例：`prompt.rs::OUTPUT_FORMAT_RULES`、`characters.rs::performance_rules()`。
   行号可以附一句"最后核对时在第 N 行"，但校验只认符号。
2. **`scripts/check-prompt-docs.mjs` 会扫所有文档里的 `代码：路径::符号`**，符号在源码里找不到就报红
   （第 3 批落地脚本，挂到 doclinks / CI）。
3. **不在文档里抄提示词原文** —— 原文只在代码里，抄一份必然漂移。

## 场景总表

| 场景 | 是否调模型 | 吃 `prompt.roleplay_scope` | 代码入口 | 文档 |
|---|---|---|---|---|
| 主生成 · 旁白 | 是 | **否**（永远全局视角） | `generate_stream.rs::generate_reply_streaming`（`mode = "narr"`） | `01-main-narration/` |
| 主生成 · 对话 | 是 | **是**（默认 `role`；`global` 回退全量） | 同上（`mode = "chat"`） | `02-main-dialogue/` |
| 日摘要 / 日结 | 是 | 否 | `timeline/summary.rs::generate_day_summary` | `03-day-summary/`（第 2 批） |
| 阶段摘要 / 自动压缩 | 是 | 否 | `summary.rs::generate_phase_summary_with` | `04-phase-summary/`（第 2 批） |
| 演进分析（含记忆抓取） | 否（纯本地规则） | 否 | `analyze.rs::auto_analyze` → `evolution.rs::analyze_evolution_in` | `05-evolution/`（第 2 批） |
| 伏笔扫描 | 是（本地 Ollama / 云端主模型两步） | 否 | `foreshadow.rs::scan_proposals_in` | `06-foreshadow-scan/`（第 3 批） |
| 章目标注入 | 否（只注入已有目标） | 否 | `outline.rs::goal_block` | `07-chapter-goal/`（第 3 批） |
| 赦免请求 | 是 | 否 | `pardon.rs::request_pardon` | `08-pardon/`（第 3 批） |

## 跨场景共用

| 文档 | 讲什么 |
|---|---|
| `common/记忆块渲染.md` | 关键记忆为什么压成一行、1200 字预算、按角色/按配对过滤 |
| `common/协议标记清单.md` | `〔态度〕/〔导演代价〕/〔承诺〕/〔作息〕/〔赦免〕` 谁写、谁解析、进不进历史 |
| `common/预算与裁剪.md` | token 估算系数、历史 N 值、降级、前缀缓存为什么这么排 |

## 批次与进度

| 批 | 内容 | 状态 |
|---|---|---|
| 1 | 本 README + `common/` 三篇 + `01-main-narration` + `02-main-dialogue` | ✅ 2026-11-14 |
| 2 | 日摘要 / 阶段摘要 / 演进分析 | 未开始 |
| 3 | 伏笔扫描 / 章目标 / 赦免 + 校验脚本 `check-prompt-docs.mjs` | 未开始 |
