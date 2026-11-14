# 阶段摘要 / 自动压缩

> 受众：开发者 ｜ 状态：现行 ｜ **最后核对：2026-11-14（按代码逐段核对）**

## 一句话

把"连续的若干天日摘要"合并成**一段 300 字总纲**，供很久以后再回读 ——
它替换的不是原文，而是**更早日摘要的细节**（原文早就不在上下文里了）。

## 代码入口

| 职责 | 代码：`文件::符号` |
|---|---|
| 提示词常量 | `timeline/summary.rs::PHASE_SUMMARY_PROMPT` |
| 规划一次压缩（找最老的一段） | `timeline/summary.rs::plan_phase_summary_in` |
| 生成并落库 | `timeline/summary.rs::generate_phase_summary_with` |
| 最短天数配置 | `timeline/summary.rs::phase_min_days_cfg` |
| 去重 / 清理重叠 | `timeline/summary.rs::dedupe_phase_rows` |
| 生成阶段的"最近几天排除" | `timeline::PHASE_KEEP_RECENT_DAYS` |

## 请求长什么样

```
[system]  PHASE_SUMMARY_PROMPT（纯文本：只留"到这一段结束仍然成立"的事实 / 300 字 / 一段）
[user]    若干天的日摘要逐条列出（形如 "第 3 天：…"）
```

**没有**历史、没有角色卡、没有世界观。输入就是**日摘要文本**。

## 什么时候才会压

| 条件 | 值/来源 |
|---|---|
| 至少几天 | `phase_min_days_cfg`（故事级配置，默认值见实现） |
| 只压"最老的一段" | `plan_phase_summary_in` 从最早的未覆盖连续天数开始 |
| 最近 N 天不参与 | `timeline::PHASE_KEEP_RECENT_DAYS`（生成阶段就排除，避免和保留的日摘要重复） |
| 已覆盖的段不再压 | `ignore_covered = false`（2026-11-02 起） |

## 落库与影响

- 写 `story_summaries` 表（阶段摘要行），并在提示词的【前情提要】里**合并成一行**；
- 被覆盖的日摘要**不删除**（原文还在 `timeline_nodes.day_summary`，只是不再单独进提示词）；
- `coalesce_with_phases` 负责把"被阶段摘要覆盖的日摘要行"折叠掉（见 `01-main-narration/动态内容.md`）。

## 与日摘要的分工

| | 日摘要 | 阶段摘要 |
|---|---|---|
| 粒度 | 一天 | 连续若干天 |
| 长度 | 200 字 | 300 字 |
| 内容取向 | 当天事实 | **仍然成立的**关系/立场/约定/伏笔 |
| 触发 | 天结束 / 补跑 | 积累够天数后自动 / 手动"自动压缩" |

## 验收与坑

- 压缩后**不能重复**：同一段天数被两次压缩是历史 bug（`dedupe_phase_rows` + 重叠清理）；
- 压缩后**不能吞掉最近几天**：`PHASE_KEEP_RECENT_DAYS` 是硬边界；
- 测试：`timeline/phase_summary_tests.rs`（含"自动压缩不再重压已覆盖段"的断言）。
