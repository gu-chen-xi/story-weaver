# 演进分析（含记忆抓取）

> 受众：开发者 ｜ 状态：现行 ｜ **最后核对：2026-11-14（按代码逐段核对）**

## 一句话（也是最容易误会的一点）

**演进分析不调模型。** 它读"这一轮"的文本 + 本地规则，产出提案与记忆卡 ——
所以它**零 API 成本**，也不吃 `prompt.roleplay_scope`（视角过滤只影响它写卡时的归属）。

## 代码入口

| 职责 | 代码：`文件::符号` |
|---|---|
| 前端触发（回复后静默跑） | `src/hooks/useGeneration.ts` → `invoke("auto_analyze_evolution")` |
| 命令入口 | `lib.rs::auto_analyze_evolution` |
| 自动分析 | `evolution/analyze.rs::auto_analyze` |
| 详细分析（落提案） | `evolution.rs::analyze_evolution_in` |
| 事件抽取 | `evolution::extract_events` / `evolution::memory_events` |
| 记忆抓取 | `memory::capture_first_events_as` |
| 新实体解析（来自日结） | `evolution/new_entity.rs` |
| 提案落库 | `change_proposals` 表（见 `docs/dev/SPEC.md`） |

## 它读什么

| 输入 | 说明 |
|---|---|
| 这一轮的文本 | 最后一条用户消息 + 它的 AI 回复（**不是整天**，也不看历史） |
| 关系/角色现状 | 用来生成"改关系/改属性"提案的 before 值 |
| 日结回复里的 `〔新角色〕` 等 | 只入队，**不自动落库**（人工闸门） |

## 产出

| 产出 | 去哪 | 要不要人确认 |
|---|---|---|
| 关系/属性变更提案 | `change_proposals`（`pending`） | **要**（`evolution.require_approval` 默认 `true`） |
| 关键记忆卡 | `memories/*.md` | 不要（生成即生效，可手动删改） |

## 与视角的关系（2026-11-14）

- 记忆卡的**归属**来自这一轮的两个扮演者（`play.ai_role` + `play.my_role`；
  日结的告白用告白者 + 对面那个人）；
- 提案在界面上**按配对过滤显示**（`lib/pairScope.ts`），但提案本身仍属于整个故事
  （关系提案天然就是两个角色之间的事，`target_id` = `relations.id`）；
- 老消息/老卡没有归属 → 落「全局」，不猜不补。

## 已知坑

| 坑 | 症状 | 处理 |
|---|---|---|
| 同一轮被反复分析 | 表白次数从 4 涨到 10 | 每张卡记 `last_round` 指纹，相同就不再累加 |
| 表白卡缺归属 | 双池对比里看不到 | 已修：`confession_front` 现在写 `characters:` |
| 新实体自动落库 | 模型编一个角色就进库 | 只入队，等人在「演进提案」里确认 |
| 把"她主动告白"记进用户表白卡 | 用户表白次数虚高 | 单独 `ai_confession` 类型 |

## 测试在哪

`evolution/*_tests.rs`、`memory/memory_tests.rs`（指纹、改判、归属）、
`memory::multi_participant_tests`、`memory::confession_participants_tests`。
