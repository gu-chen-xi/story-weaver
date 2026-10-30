# 文档总入口

> 受众：所有人（用户 / 开发者 / AI agent）｜状态：**现行**｜最后同步：2026-10-29
>
> **三条检索捷径**
> 1. 想知道"现在能做到什么、下一步做什么" → [dev/ROADMAP.md](dev/ROADMAP.md)
> 2. 想知道"哪里容易炸、炸了怎么修" → [dev/错误合集.md](dev/错误合集.md)
> 3. 想知道"这功能为什么这么设计" → [design/](design/分支系统.md) 与 [archive/](archive/历史计划存档.md)

## 一、按"我想干什么"查

| 我想… | 看这份 | 分类 |
|---|---|---|
| 装好后搞清楚数据存哪、怎么备份 | [user/数据与备份.md](user/数据与备份.md) | 用户 |
| 不放心，要自己验证软件没做坏事 | [user/security/README.md](user/security/README.md)（3 分钟 / 10 分钟两版） | 用户 |
| 看这一版改了什么 | [user/releases/v3.10.0.md](user/releases/v3.10.0.md)（最新版）。v3.1.0 / v3.2.0 / v3.4.0 / v3.5.0 / v3.6.0 / v3.7.0 / v3.8.0 / v3.9.0 / v3.10.0 已入库；v3.3.0 未出发布说明 | 用户 |
| 知道现在做到哪、下一步做什么 | [dev/ROADMAP.md](dev/ROADMAP.md) | 开发 |
| 遇到白屏 / 打不开 / 数据不对 / 编译炸了 | [dev/错误合集.md](dev/错误合集.md) → [dev/测试用例.md](dev/测试用例.md) | 开发 |
| 改代码前确认不能碰什么 | [dev/开发规范.md](dev/开发规范.md) + [AGENTS.md](../AGENTS.md) | 开发 |
| **改完代码，不知道要动哪些文档** | [dev/文档更新顺序.md](dev/文档更新顺序.md)（含六层顺序 + 变更矩阵） | 开发 |
| 接口长什么样、行为怎么定义 | [dev/SPEC.md](dev/SPEC.md) | 开发 |
| 页面/组件层级在哪 | [dev/UI_TREE.md](dev/UI_TREE.md) | 开发 |
| 提示词怎么拼、输出格式是什么 | [dev/提示词规则.md](dev/提示词规则.md) | 开发 |
| **要给织梦机加一个插件** / 改插件系统 | [dev/PLUGINS.md](dev/PLUGINS.md)（三步加插件 + 契约 + 守卫） | 开发 |
| 怎么跑、怎么测、怎么提交 | [dev/开发流程.md](dev/开发流程.md) | 开发 |
| 应用打不开，先自检 | [dev/自检工具.md](dev/自检工具.md) | 开发 |
| 分支系统当初怎么设计的 | [design/分支系统.md](design/分支系统.md) | 设计依据 |
| 立绘三条通道怎么用 | [design/立绘系统.md](design/立绘系统.md) | 设计依据 |
| RAG 怎么做的（**① 已落地**，后续待定） | [design/RAG规划.md](design/RAG规划.md) | 设计依据 |
| 早期计划、MVP 范围、文档债记录 | [archive/](archive/历史计划存档.md) | 存档 |
| 已完成功能的逐条实现记录 | [archive/ROADMAP-已完成.md](archive/ROADMAP-已完成.md) | 存档 |

## 二、四个分类：按"谁看"分，不按"写没写"分

| 目录 | 给谁 | 判断标准 | 现状 |
|---|---|---|---|
| `user/` | 用软件但不改代码的人 | "他装完软件会不会想读这份？" | 14 份（数据与备份 1 + security 4 + releases 9）|
| `dev/` | 写代码时手边查 | "改代码前我会不会翻它？" | 11 份 |
| `design/` | 功能已落地，回溯依据 | "功能已在，留着解释为什么这么做" | 3 份 |
| `archive/` | 别再当现状看 | 文档自己写着"存档" | 5 份 |

**根目录只留 4 个**：`README.md`（门面）、`SECURITY.md`（**必须在根目录**，GitHub 只认这个位置）、`AGENTS.md`（AI 约束）、`LICENSE`。

## 三、每份文档顶部的状态头（统一约定）

```markdown
> 受众：用户 / 开发者 / AI agent ｜ 状态：现行 · 设计依据 · 存档 ｜ 最后同步：YYYY-MM-DD（版本）
```

**状态只有三种，别自创**：

| 状态 | 含义 | 能不能当现状 |
|---|---|---|
| **现行** | 与代码同步，改动要跟着改 | ✅ 可以 |
| **设计依据** | 功能已落地，留作回溯"为什么这么做" | ⚠️ 描述的是当初的设计，不保证没漂移 |
| **存档** | 当时的计划/结论，历史记录 | ❌ 绝对不要当现状 |

## 四、比文档更可信的东西

文档会过期，测试不会——**能跑的东西优先信**：

| 手段 | 规模 | 命令 |
|---|---|---|
| Rust 单元测试 | **459 项**（439 项常规 + 20 项 `#[ignore]` 诊断 / live） | `cd src-tauri && cargo test --lib` |
| 前端纯逻辑测试 | **167 项**（19 个文件，含 1 条静态守卫） | `npm test` |
| 启动自检 | 16 个逻辑项（看输出「结论」行） | `scripts\selfcheck\selfcheck.cmd` |
| 类型 / 构建 | — | `npx tsc --noEmit`、`npm run build` |

## 五、维护纪律

1. **新增文档必须归到四类之一**，并在本文件加一行；不许再往根目录堆。
2. **改代码顺手改文档**——尤其是 `dev/SPEC.md`、`dev/UI_TREE.md`、`dev/测试用例.md` 这三份"与代码同步"的；
   顺序与影响面见 [dev/文档更新顺序.md](dev/文档更新顺序.md)。
3. **"已完成"的内容不要留在 ROADMAP 正文**：正文只放"还没做的"，做完的明细进 `archive/ROADMAP-已完成.md`。
4. **提交前查断链**：`npm run doclinks`（`scripts/check-doc-links.mjs`，零依赖，扫根目录 + `docs/` 全部 md，
   自动跳过代码块里的示例链接；有断链退出码 1）。
5. **改完代码先跑 `node docs/tools/doc-impact.mjs`**：按 `git diff` 反查受影响文档，输出「必改 / 应查」两档；不确定某个词写在哪就 `--grep <关键词>`。
   完整顺序见 [dev/文档更新顺序.md](dev/文档更新顺序.md)。
6. **错误合集只增不删**：坑修了也别删条目，改成"已修复 + 防复发手段"，下一个人还得靠它。

## 六、已删除 / 已合并的文档（2026-10-18 ~ 10-20）

| 原文档 | 去向 |
|---|---|
| 文档索引.md | **本文件**（内容重写并升级为总入口） |
| PLAN.md + 总计划.md | 合并 → [archive/历史计划存档.md](archive/历史计划存档.md) |
| 立绘系统设计.md + 立绘生成使用指南.md | 合并 → [design/立绘系统.md](design/立绘系统.md) |
| 主界面设计方案.md | 并入 → [dev/UI_TREE.md](dev/UI_TREE.md) 顶部 |
| 已废弃-关闭AI水印建议.md | 删除（内容是"这做法已废弃、请勿操作"，无增量） |
| 测试用例.md（旧版） | 重写 → [dev/测试用例.md](dev/测试用例.md) |
| ROADMAP §10.1–§10.110（4645 行） | 拆出 → [archive/ROADMAP-已完成.md](archive/ROADMAP-已完成.md) |
| `docs/verification/` 四份 | 移到 → [user/security/](user/security/README.md) |

> ⚠ **`user/releases/v3.1.0.md` 故意保持原样**：它是 GitHub Release v3.1.0 的正文存档，
> 要求"与发布页同内容、便于 diff"。里面写的 `docs/verification/` 是**发布当时的路径**，
> 现在实际位置是 `docs/user/security/`。**不要"顺手修"它**，否则 diff 对比就失效了。

## 七、目录结构

```
docs/
├─ README.md          ← 本文件（总入口）
├─ user/              ① 给用户：数据与备份 · security/ 四份 · releases/
├─ dev/               ② 开发时查：SPEC · UI_TREE · 开发规范 · 开发流程 · 测试用例
│                       自检工具 · 提示词规则 · PLUGINS · ROADMAP · 错误合集 · 文档更新顺序
├─ design/            ③ 已落地的设计依据：分支系统 · 立绘系统 · RAG规划
└─ archive/           ④ 历史：历史计划存档 · 最小MVP · 文档债清理 · 功能清点
                        ROADMAP-已完成 · devlog_m5.txt
```