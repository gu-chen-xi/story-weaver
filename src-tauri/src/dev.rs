//! 开发者工具（**只给作者用**，v3.4）
//!
//! ## 三条硬约束
//!
//! 1. **只在开发构建里真的干活**：release（`debug_assertions = false`）里每个命令都直接返回
//!    「开发者工具仅存在于开发构建」，前端入口也被 `import.meta.env.DEV` 挡住。
//! 2. **只碰测试数据**：所有命令第一件事是 `assert_test_story()` —— 目标故事的标题**必须以
//!    「测试」开头**。真实故事一律拒绝。这不是提示，是硬校验：手滑选错也改不到真数据。
//! 3. **不调模型**：这个工具存在的意义就是"防止 AI 的输出达不到预期"，所以它喂的是**伪造的
//!    AI 响应**，撞的是**真实的解析/落地管线**。任何会花 API 额度的路径都不在这里。
//!
//! ## 为什么"沙箱"是同一个库里的测试故事
//!
//! `paths::data_root()` 是 `OnceLock`（进程内恒定），运行时切不到临时目录；
//! 而每故事本来就一个 `story.db`、快照也是按故事分的。
//! 所以"临时数据" = 标题以「测试」开头的那些故事，删掉即清空，真实故事不受影响。

use sqlx::{Column, Row, SqlitePool, TypeInfo};

/// 测试故事标题前缀。**改这里等于改沙箱边界**，所以集中一处。
pub const TEST_PREFIX: &str = "测试";

const DEV_ONLY: &str = "开发者工具仅存在于开发构建（release 包里没有）";

/// 确保测试故事**真的有 story.db**，返回它的池。
///
/// 为什么需要：`db::create_story` 只插 library.db 的 stories 行；建库是 `init_story_db` 的职责。
/// 早期版本的 dev 命令漏了这一步，于是已经有「测试0」这样的"空壳故事" ——
/// 这里补建，用户不必删了重来。
#[cfg(debug_assertions)]
async fn ensure_dev_story_pool(story_id: i64) -> Result<SqlitePool, String> {
    let path = crate::paths::story_db(&story_id.to_string());
    if !path.exists() {
        crate::story_db::init_story_db(&story_id.to_string())
            .await
            .map_err(|e| format!("补建 story.db 失败：{e}"))?;
    }
    crate::story_db::open_story_pool(&story_id.to_string())
        .await
        .map_err(|e| e.to_string())
}

/// 校验目标故事是测试故事；不是就拒绝。
///
/// 这是整个模块的安全底座：宁可让 dev 命令失败，也不能让注入落到真实剧情上。
pub async fn assert_test_story(lib_pool: &SqlitePool, story_id: i64) -> Result<String, String> {
    let title: Option<String> = sqlx::query_scalar("SELECT title FROM stories WHERE id = ?1")
        .bind(story_id)
        .fetch_optional(lib_pool)
        .await
        .map_err(|e| e.to_string())?;
    match title {
        None => Err(format!("故事 {story_id} 不存在")),
        Some(t) if t.trim().starts_with(TEST_PREFIX) => Ok(t),
        Some(t) => Err(format!(
            "「{t}」不是测试故事（标题须以「{TEST_PREFIX}」开头）。开发者工具只允许操作测试数据。"
        )),
    }
}

/// 下一个测试故事名：从 `测试0` 开始，取**最小未用编号**。
///
/// 取最小未用而不是 max+1：删掉 `测试1` 后下次还叫 `测试1`，编号不会一路涨上去。
pub fn next_test_title(existing: &[String]) -> String {
    let mut used = std::collections::HashSet::new();
    for t in existing {
        if let Some(rest) = t.trim().strip_prefix(TEST_PREFIX) {
            if let Ok(n) = rest.parse::<u32>() {
                used.insert(n);
            }
        }
    }
    let mut n = 0u32;
    while used.contains(&n) {
        n += 1;
    }
    format!("{TEST_PREFIX}{n}")
}

// ==================== 命令 ====================

#[derive(serde::Serialize)]
pub struct DevStory {
    pub id: i64,
    pub title: String,
    pub created_at: String,
}

#[derive(serde::Serialize)]
pub struct DevStatus {
    /// 是否开发构建
    pub dev_build: bool,
    /// 测试故事列表（标题以「测试」开头）
    pub test_stories: Vec<DevStory>,
    /// 下一个会自动使用的测试故事名
    pub next_title: String,
    /// 数据根（给界面上显示，便于确认在操作哪个库）
    pub data_root: String,
}

/// 概览：测试故事列表 + 下一个名字 + 数据根。
#[tauri::command]
pub async fn dev_status(pool: tauri::State<'_, SqlitePool>) -> Result<DevStatus, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = &pool;
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        dev_status_impl(pool.inner()).await
    }
}

#[cfg(debug_assertions)]
async fn dev_status_impl(lib_pool: &SqlitePool) -> Result<DevStatus, String> {
    let rows = sqlx::query("SELECT id, title, created_at FROM stories ORDER BY id")
        .fetch_all(lib_pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut all: Vec<DevStory> = Vec::new();
    for r in &rows {
        all.push(DevStory {
            id: r.try_get(0).map_err(|e| e.to_string())?,
            title: r.try_get(1).map_err(|e| e.to_string())?,
            created_at: r.try_get(2).unwrap_or_default(),
        });
    }
    let titles: Vec<String> = all.iter().map(|s| s.title.clone()).collect();
    let test_stories: Vec<DevStory> = all
        .into_iter()
        .filter(|s| s.title.trim().starts_with(TEST_PREFIX))
        .collect();
    Ok(DevStatus {
        dev_build: cfg!(debug_assertions),
        next_title: next_test_title(&titles),
        test_stories,
        data_root: crate::paths::data_root().to_string_lossy().to_string(),
    })
}

/// 新建一个测试故事（名字自动取 `测试0`、`测试1`…）。
#[tauri::command]
pub async fn dev_create_test_story(pool: tauri::State<'_, SqlitePool>) -> Result<DevStatus, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = &pool;
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        let lib = pool.inner().clone();
        let st = dev_status_impl(&lib).await?;
        // 时间起点用今天：测试故事不需要真实时间线。
        // 不发新依赖（项目里没有 chrono），直接问 SQLite 要本地日期。
        let today: String = sqlx::query_scalar("SELECT date('now','localtime')")
            .fetch_one(&lib)
            .await
            .map_err(|e| e.to_string())?;
        let new_id = crate::db::create_story(&lib, &st.next_title, &today, "")
            .await
            .map_err(|e| e.to_string())?;
        // **必须**建 story.db：`db::create_story` 只插 stories 表一行，
        // 建目录与库是 `init_story_db` 的活（正常流程见 lib.rs 的 create_story 命令）。
        // 漏了这步 → 测试故事只有条目没有库 → 后面每个操作都
        // `error returned from database: (code: 14) unable to open database file`（用户实测踩到）。
        crate::story_db::init_story_db(&new_id.to_string())
            .await
            .map_err(|e| e.to_string())?;
        dev_status_impl(&lib).await
    }
}

// ==================== 数据注入 ====================
//
// 通通遵循同一个形状：
//   ① 校验测试故事（assert_test_story）
//   ② dry_run = true → 只解析/校验，**不写库**，把"会不会被接受、会被拒在哪"返回
//   ③ dry_run = false → 先 make_snapshot_for_story（按故事），再写；返回快照号，可回滚

#[derive(serde::Serialize)]
pub struct InjectResult {
    /// 是否只是试运行
    pub dry_run: bool,
    /// 目标描述（用于界面上人话展示）
    pub target: String,
    /// 管线给出的结论（成功提示 / 拒绝原因）
    pub outcome: String,
    /// 被接受的改动（落地前的预览）
    pub changes: Vec<String>,
    /// 若真落地：本次的快照版本号（可回滚）
    pub snapshot: Option<i64>,
}

/// 注入「日摘要」：喂一段"模型本该返回的摘要文本"。
///
/// 真实管线（`timeline::generate_day_summary`）也是 trim 后写 `day_summary`，
/// 所以这里撞的是同一处落地点。
#[tauri::command]
pub async fn dev_inject_summary(
    pool: tauri::State<'_, SqlitePool>,
    story_id: i64,
    node_id: i64,
    text: String,
    dry_run: bool,
) -> Result<InjectResult, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (&pool, story_id, node_id, text, dry_run);
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        dev_inject_summary_impl(pool.inner(), story_id, node_id, &text, dry_run).await
    }
}

#[cfg(debug_assertions)]
async fn dev_inject_summary_impl(
    lib_pool: &SqlitePool,
    story_id: i64,
    node_id: i64,
    text: &str,
    dry_run: bool,
) -> Result<InjectResult, String> {
    let title = assert_test_story(lib_pool, story_id).await?;
    let pool = ensure_dev_story_pool(story_id).await?;
    let row = sqlx::query("SELECT date_label, COALESCE(day_summary,'') FROM timeline_nodes WHERE id = ?1")
        .bind(node_id)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("节点 {node_id} 不存在"))?;
    let label: String = row.try_get(0).unwrap_or_default();
    let before: String = row.try_get(1).unwrap_or_default();

    let after = text.trim().to_string();
    if after.is_empty() {
        // 真实管线里空摘要 = 该天"没有剧情"，回顾会跳过它 —— 这本身是个值得暴露的结论
        return Ok(InjectResult {
            dry_run,
            target: format!("《{title}》{label}"),
            outcome: "拒绝：摘要为空。真实管线里空摘要会让「回顾」跳过这一天".to_string(),
            changes: vec![format!("原摘要 {} 字 → 0 字", before.chars().count())],
            snapshot: None,
        });
    }

    let mut changes = vec![format!(
        "day_summary：{} 字 → {} 字",
        before.chars().count(),
        after.chars().count()
    )];
    if !before.trim().is_empty() {
        changes.push(format!("原内容：{}", before.chars().take(60).collect::<String>()));
    }
    if dry_run {
        return Ok(InjectResult {
            dry_run: true,
            target: format!("《{title}》{label}"),
            outcome: "试运行：可写（未落地）".to_string(),
            changes,
            snapshot: None,
        });
    }

    let snap = crate::evolution::make_snapshot_for_story(&story_id.to_string())
        .await
        .map_err(|e| format!("写入前打快照失败：{e}"))?;
    sqlx::query("UPDATE timeline_nodes SET day_summary = ?1 WHERE id = ?2")
        .bind(&after)
        .bind(node_id)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(InjectResult {
        dry_run: false,
        target: format!("《{title}》{label}"),
        outcome: "已写入 day_summary".to_string(),
        changes,
        snapshot: Some(snap),
    })
}

/// 注入「回复评估」：喂一段"模型本该返回的评估 JSON"。
///
/// 撞的是 `evaluate::apply_eval_response`（解析 + 归一化 + 写 `reply_evaluations`）。
/// 四维缺项、分数越界、非 JSON —— 都能在这里看到真实管线怎么兜。
#[tauri::command]
pub async fn dev_inject_eval(
    pool: tauri::State<'_, SqlitePool>,
    story_id: i64,
    message_id: i64,
    raw: String,
    dry_run: bool,
) -> Result<InjectResult, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (&pool, story_id, message_id, raw, dry_run);
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        let lib = pool.inner().clone();
        let title = assert_test_story(&lib, story_id).await?;
        let pool = ensure_dev_story_pool(story_id).await?;
        let node_id: i64 = sqlx::query_scalar("SELECT COALESCE(node_id,0) FROM messages WHERE id = ?1")
            .bind(message_id)
            .fetch_optional(&pool)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("消息 #{message_id} 不存在"))?;

        if dry_run {
            // 试运行：只解析，不写库。借 apply_eval_response 的解析逻辑 → 用一个临时内存库
            // 太绕，这里直接调解析前的校验：把错误原样抛给用户。
            let cleaned = raw.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
            let parsed: serde_json::Value = serde_json::from_str(cleaned)
                .map_err(|e| format!("拒绝：不是合法 JSON（{e}）"))?;
            let dims = parsed["dims"].as_array().map(|a| a.len()).unwrap_or(0);
            let mut changes = vec![format!("dims 维度数：{dims}（管线固定取 4 维，缺的按 3 分并标注）")];
            for d in parsed["dims"].as_array().into_iter().flatten() {
                let k = d["key"].as_str().unwrap_or("?");
                let s = d["score"].as_i64().unwrap_or(3);
                changes.push(format!("{k}：{s} 分"));
            }
            return Ok(InjectResult {
                dry_run: true,
                target: format!("《{title}》消息 #{message_id}"),
                outcome: "试运行：JSON 可解析（未写库）".to_string(),
                changes,
                snapshot: None,
            });
        }

        let snap = crate::evolution::make_snapshot_for_story(&story_id.to_string())
            .await
            .map_err(|e| format!("写入前打快照失败：{e}"))?;
        let model = crate::db::get_config(&lib, "model.name")
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "dev-inject".to_string());
        let ev = crate::evaluate::apply_eval_response(&pool, message_id, node_id, &model, &raw).await?;
        Ok(InjectResult {
            dry_run: false,
            target: format!("《{title}》消息 #{message_id}"),
            outcome: format!("已写入评估：总分 {:.1}", ev.total),
            changes: ev
                .dims
                .iter()
                .map(|d| format!("{}：{} 分", d.key, d.score))
                .collect(),
            snapshot: Some(snap),
        })
    }
}

/// 注入「静默演进提案」：喂一条"模型认为该改什么"的补丁。
///
/// 撞的是真实闸门：`check::validate_patch`（字段白名单 + 目标存在性 + before 核对）。
/// 通过后按真实流程进 `change_proposals`（status=pending，仍需人工拍板）。
#[tauri::command]
pub async fn dev_inject_proposal(
    pool: tauri::State<'_, SqlitePool>,
    story_id: i64,
    patch_json: String,
    dry_run: bool,
) -> Result<InjectResult, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (&pool, story_id, patch_json, dry_run);
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        let lib = pool.inner().clone();
        let title = assert_test_story(&lib, story_id).await?;
        let item: crate::check::PatchItem = serde_json::from_str(&patch_json)
            .map_err(|e| format!("补丁 JSON 解析失败：{e}"))?;
        let pool = ensure_dev_story_pool(story_id).await?;

        // 真实闸门：白名单 + 目标存在性 + before 核对
        let verdict = crate::check::validate_patch(&pool, std::slice::from_ref(&item)).await;
        let (fixed, notes) = match verdict {
            Ok(v) => v,
            Err(e) => {
                return Ok(InjectResult {
                    dry_run: true,
                    target: format!("《{title}》{} · {}", item.target_name, item.field),
                    outcome: format!("拒绝（与真实流程一致）：{e}"),
                    changes: Vec::new(),
                    snapshot: None,
                })
            }
        };
        let mut changes: Vec<String> = notes;
        for f in &fixed {
            changes.push(format!(
                "{} → {}：{} ⇒ {}",
                f.target_name, f.field, f.before, f.value
            ));
        }
        if dry_run {
            return Ok(InjectResult {
                dry_run: true,
                target: format!("《{title}》{} · {}", item.target_name, item.field),
                outcome: "试运行：通过闸门（未落地）".to_string(),
                changes,
                snapshot: None,
            });
        }

        let snap = crate::evolution::make_snapshot_for_story(&story_id.to_string())
            .await
            .map_err(|e| format!("写入前打快照失败：{e}"))?;
        // 按真实流程落成 pending 提案（不直接改设定 —— 闸门要求人工拍板）
        let branch = crate::branch::resolve_branch(&story_id.to_string(), &None)
            .await
            .unwrap_or(None);
        let group: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(group_id),0) + 1 FROM change_proposals")
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?;
        let target_id: i64 = crate::dev::resolve_target_id(&pool, &item).await?;
        crate::evolution::insert_proposal(
            &pool,
            &branch,
            0,
            group,
            &item.target_kind,
            target_id,
            &item.field,
            &item.before,
            &item.value,
            "【开发者工具注入】伪造的 AI 提议，用于验证接收与闸门",
            "【开发者工具注入】非真实剧情信号",
            0.5,
            "pending",
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(InjectResult {
            dry_run: false,
            target: format!("《{title}》{} · {}", item.target_name, item.field),
            outcome: format!("已落成待确认提案（组 {group}，status=pending）"),
            changes,
            snapshot: Some(snap),
        })
    }
}

// ==================== 数据体检 ====================
//
// 只**报告**，附带"修复 SQL 预览"（文本），**绝不执行**。
// 理由：自动改库一旦判断错，坏的是用户的剧情；预览 SQL 让人自己决定。

#[derive(serde::Serialize)]
pub struct HealthItem {
    /// error / warn / info
    pub severity: String,
    /// 哪一块（关系 / 消息 / 分支 / 角色 / 世界观 / 快照）
    pub area: String,
    /// 一句话结论
    pub what: String,
    /// 细节（计数、示例 id）
    pub detail: String,
    /// 修复 SQL 预览（**不执行**；为空表示仅提示）
    pub fix_sql: String,
}

#[derive(serde::Serialize)]
pub struct HealthReport {
    pub story_title: String,
    pub integrity_ok: bool,
    pub integrity_raw: String,
    pub counts: Vec<(String, i64)>,
    pub items: Vec<HealthItem>,
}

#[cfg(debug_assertions)]
async fn dev_health_check_impl(lib_pool: &SqlitePool, story_id: i64) -> Result<HealthReport, String> {
    let title = assert_test_story(lib_pool, story_id).await?;
    let pool = ensure_dev_story_pool(story_id).await?;
        let mut items: Vec<HealthItem> = Vec::new();

        // ── 0. SQLite 自检（页/索引结构层面）──
        let integrity_raw: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_optional(&pool)
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let integrity_ok = integrity_raw.trim() == "ok";
        if !integrity_ok {
            items.push(HealthItem {
                severity: "error".into(),
                area: "库文件".into(),
                what: "SQLite 完整性检查未通过".into(),
                detail: integrity_raw.chars().take(400).collect(),
                fix_sql: String::new(),
            });
        }

        // ── 1. 各表行数（给用户一个"这库多大"的整体感）──
        let mut counts: Vec<(String, i64)> = Vec::new();
        for tbl in [
            "characters", "relations", "world_settings", "timeline_nodes",
            "messages", "branches", "change_proposals", "setting_snapshots",
            "reply_evaluations", "event_logs",
        ] {
            let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {tbl}"))
                .fetch_optional(&pool)
                .await
                .unwrap_or(None)
                .unwrap_or(0);
            counts.push((tbl.to_string(), n));
        }

        // ── 2. 悬空关系（指向不存在的角色）──
        let dangling_rels: Vec<i64> = sqlx::query_scalar(
            "SELECT r.id FROM relations r
             WHERE NOT EXISTS (SELECT 1 FROM characters c WHERE c.id = r.from_character_id)
                OR NOT EXISTS (SELECT 1 FROM characters c WHERE c.id = r.to_character_id)",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        if !dangling_rels.is_empty() {
            let ids = dangling_rels.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
            items.push(HealthItem {
                severity: "error".into(),
                area: "关系".into(),
                what: format!("{} 条关系指向不存在的角色", dangling_rels.len()),
                detail: format!("关系 id：{ids}"),
                fix_sql: format!("-- 删除悬空关系（先看一眼再执行）\nDELETE FROM relations WHERE id IN ({ids});"),
            });
        }

        // ── 3. 孤儿消息（节点已删、消息还在）──
        let orphan_msgs: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages m
             WHERE NOT EXISTS (SELECT 1 FROM timeline_nodes t WHERE t.id = m.node_id)",
        )
        .fetch_optional(&pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0);
        if orphan_msgs > 0 {
            items.push(HealthItem {
                severity: "error".into(),
                area: "消息".into(),
                what: format!("{orphan_msgs} 条消息挂在不存在的节点上"),
                detail: "这些消息永远不会被渲染，但会占空间".into(),
                fix_sql: "DELETE FROM messages WHERE node_id NOT IN (SELECT id FROM timeline_nodes);".into(),
            });
        }

        // ── 4. 分支孤儿（branch_id 指向不存在的分支）──
        for tbl in ["characters", "relations", "world_settings", "timeline_nodes", "messages"] {
            let n: i64 = sqlx::query_scalar(&format!(
                "SELECT COUNT(*) FROM {tbl} x
                 WHERE x.branch_id IS NOT NULL
                   AND NOT EXISTS (SELECT 1 FROM branches b WHERE b.id = x.branch_id)"
            ))
            .fetch_optional(&pool)
            .await
            .unwrap_or(None)
            .unwrap_or(0);
            if n > 0 {
                items.push(HealthItem {
                    severity: "error".into(),
                    area: "分支".into(),
                    what: format!("{tbl} 有 {n} 行属于已不存在的分支"),
                    detail: "分支删了但行没清干净 → 主线视图读不到、也删不掉".into(),
                    fix_sql: format!(
                        "-- 归回主线（也可改成 DELETE）\nUPDATE {tbl} SET branch_id = NULL\n WHERE branch_id IS NOT NULL\n   AND branch_id NOT IN (SELECT id FROM branches);"
                    ),
                });
            }
        }

        // ── 5. 重名（角色 / 同分类的世界条目）──
        let dup_chars: Vec<(String, i64)> = sqlx::query_as(
            "SELECT name, COUNT(*) c FROM characters GROUP BY name HAVING c > 1",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        if !dup_chars.is_empty() {
            let names = dup_chars.iter().map(|(n, c)| format!("{n}×{c}")).collect::<Vec<_>>().join("、");
            items.push(HealthItem {
                severity: "warn".into(),
                area: "角色".into(),
                what: format!("{} 组重名角色", dup_chars.len()),
                detail: names,
                fix_sql: "-- 重名会让 AI 的「改哪个」含混；建议改名，而不是删\n-- UPDATE characters SET name = name || '（二）' WHERE id = ?;".into(),
            });
        }
        let dup_world: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT category, key, COUNT(*) c FROM world_settings GROUP BY category, key HAVING c > 1",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        if !dup_world.is_empty() {
            let listed = dup_world
                .iter()
                .map(|(cat, key, c)| format!("{cat}/{key}×{c}"))
                .collect::<Vec<_>>()
                .join("、");
            items.push(HealthItem {
                severity: "warn".into(),
                area: "世界观".into(),
                what: format!("{} 组同分类重名条目", dup_world.len()),
                detail: listed,
                fix_sql: "-- 同分类同名会让提示词里出现两条一样的设定\n-- 建议人工合并，保留信息更全的那条".into(),
            });
        }

        // ── 6. 有剧情但没摘要的天（回顾会看不到这些天）──
        let no_summary: Vec<String> = sqlx::query_scalar(
            "SELECT t.date_label FROM timeline_nodes t
             WHERE COALESCE(TRIM(t.day_summary),'') = ''
               AND EXISTS (SELECT 1 FROM messages m WHERE m.node_id = t.id)
             ORDER BY t.sort_order, t.id",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        if !no_summary.is_empty() {
            items.push(HealthItem {
                severity: "warn".into(),
                area: "摘要".into(),
                what: format!("{} 天有剧情但没有日摘要", no_summary.len()),
                detail: format!(
                    "{}（「🔍 回顾」看不到这些天）",
                    no_summary.join("、")
                ),
                fix_sql: "-- 用「开发者工具 → 数据注入」给这些天补摘要，或在界面上跑一次摘要回填".into(),
            });
        }

        // ── 7. 临时快照堆积（信息，不算问题）──
        let temp_snaps: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM setting_snapshots WHERE temporary = 1",
        )
        .fetch_optional(&pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0);
        if temp_snaps > 0 {
            items.push(HealthItem {
                severity: "info".into(),
                area: "快照".into(),
                what: format!("{temp_snaps} 个临时快照"),
                detail: "临时快照是编辑前自动打的，可清理".into(),
                fix_sql: "DELETE FROM setting_snapshots WHERE temporary = 1;".into(),
            });
        }

        Ok(HealthReport {
            story_title: title,
            integrity_ok,
            integrity_raw,
            counts,
            items,
        })
}

#[tauri::command]
pub async fn dev_health_check(
    pool: tauri::State<'_, SqlitePool>,
    story_id: i64,
) -> Result<HealthReport, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (&pool, story_id);
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        let lib = pool.inner().clone();
        dev_health_check_impl(&lib, story_id).await
    }
}

// ==================== 样例数据 ====================

/// 一键造样例数据：角色 / 关系 / 世界观（含物品）/ 若干天剧情 + 摘要。
///
/// 只对测试故事生效；重复调用会在已有数据上**追加**（天数接着往后排），
/// 方便"先造 3 天看看，再多造 5 天"。
#[tauri::command]
pub async fn dev_seed_sample(
    pool: tauri::State<'_, SqlitePool>,
    story_id: i64,
    chars: i64,
    days: i64,
) -> Result<String, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (&pool, story_id, chars, days);
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        let lib = pool.inner().clone();
        assert_test_story(&lib, story_id).await?;
        let pool = ensure_dev_story_pool(story_id).await?;

        let chars = chars.clamp(1, 12);
        let days = days.clamp(1, 30);

        // 角色
        let names = ["夏婉琴", "顾宸熙", "林知微", "周砚秋", "陈见川", "苏晚照"];
        for i in 0..chars {
            // 名字固定取自 names，超出后加序号后缀（保证不重名，体检里不会误报）
            let base_name = names[(i as usize) % names.len()];
            let name = if i >= names.len() as i64 {
                format!("{base_name}{i}")
            } else {
                base_name.to_string()
            };
            sqlx::query(
                "INSERT INTO characters(name, role, gender, one_liner, profile, speech_style, traits,
                    background_past, motivation, goal, dialogue_examples, behavior_rules)
                 VALUES(?1, ?2, '未定', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )
            .bind(&name)
            .bind(if i == 0 { "女主" } else if i == 1 { "男主" } else { "配角" })
            .bind(format!("样例角色 {i}"))
            .bind("【样例数据】用于测试提示词与界面，可随时删除")
            .bind("说话偏短句，偶尔停顿")
            .bind("克制，认真")
            .bind("【样例】在一个下雨的院子里长大")
            .bind("【样例】想被人记住")
            .bind("【样例】把欠下的那句话说完")
            .bind("A：你还在生气？\nB：没有。（把杯子放得比平时重一点）")
            .bind("【样例】绝不先开口道歉")
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        }

        // 关系：相邻两个角色各一条
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM characters ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap_or_default();
        for w in ids.windows(2) {
            sqlx::query(
                "INSERT INTO relations(from_character_id, to_character_id, relation_type, strength, state, description)
                 VALUES(?1, ?2, '旧识', 55, '稳定', '【样例】认识很久，话不多')",
            )
            .bind(w[0])
            .bind(w[1])
            .execute(&pool)
            .await
            .map_err(|e| e.to_string())?;
        }

        // 世界观：地点 / 势力 / 术语 / 物品 各一条
        let world_rows = [
            ("夏家老宅", "【样例】三进院子，西厢长期空着", "地理", "{\"region\":\"城西\"}"),
            ("修女院", "【样例】收留孤儿的地方", "势力", "{\"leader\":\"老院长\"}"),
            ("回声", "【样例】故事里对「没说完的话」的说法", "术语", "{\"aliases\":\"余音,回响\"}"),
            ("怀表", "【样例】B 送的，走时慢三分钟", "物品", "{\"aliases\":\"旧表,那块表\"}"),
            ("显卡", "【样例】工作需要才买的", "物品", "{\"aliases\":\"rtx\"}"),
        ];
        for (key, value, cat, meta) in world_rows {
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM world_settings WHERE key = ?1 AND category = ?2",
            )
            .bind(key)
            .bind(cat)
            .fetch_one(&pool)
            .await
            .unwrap_or(0);
            if exists == 0 {
                sqlx::query(
                    "INSERT INTO world_settings(key, value, category, meta_json) VALUES(?1, ?2, ?3, ?4)",
                )
                .bind(key)
                .bind(value)
                .bind(cat)
                .bind(meta)
                .execute(&pool)
                .await
                .map_err(|e| e.to_string())?;
            }
        }

        // 剧情：每天 2 条消息 + 一个摘要（天数接在已有的后面）
        let existing_days: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes WHERE node_type = 'day'")
                .fetch_optional(&pool)
                .await
                .unwrap_or(None)
                .unwrap_or(0);
        let mut created_days = 0;
        for d in 1..=days {
            let label = format!("第 {} 天", existing_days + d);
            let sort = existing_days + d;
            let node_id: i64 = sqlx::query_scalar(
                "INSERT INTO timeline_nodes(date_label, sort_order, node_type, day_summary)
                 VALUES(?1, ?2, 'day', ?3) RETURNING id",
            )
            .bind(&label)
            .bind(sort)
            .bind(format!("【样例】{label}：日常，埋了一句没说完的话"))
            .fetch_one(&pool)
            .await
            .map_err(|e| e.to_string())?;
            for (role, text) in [
                ("user", format!("【样例】{label}的白天，随口提了一句怀表。")),
                ("assistant", format!("【样例】{label}的夜里，把话咽了回去。")),
            ] {
                sqlx::query("INSERT INTO messages(node_id, role, content, mode) VALUES(?1, ?2, ?3, 'chat')")
                    .bind(node_id)
                    .bind(role)
                    .bind(text)
                    .execute(&pool)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            created_days += 1;
        }

        Ok(format!("已造：{chars} 个角色、{} 条关系、5 条世界观、{created_days} 天剧情", (chars - 1).max(0)))
    }
}

/// 把补丁里的 `target_name` 解析成数据库 id（与 `check::validate_patch` 的判定口径一致）。
#[cfg(debug_assertions)]
pub async fn resolve_target_id(pool: &SqlitePool, item: &crate::check::PatchItem) -> Result<i64, String> {
    let id: Option<i64> = match item.target_kind.as_str() {
        "character" => sqlx::query_scalar("SELECT id FROM characters WHERE name = ?1 LIMIT 1")
            .bind(&item.target_name)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?,
        "world_entry" | "term" | "faction" => {
            sqlx::query_scalar("SELECT id FROM world_settings WHERE key = ?1 LIMIT 1")
                .bind(&item.target_name)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?
        }
        _ => sqlx::query_scalar(
            "SELECT id FROM relations ORDER BY id LIMIT 1",
        )
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?,
    };
    id.ok_or_else(|| format!("找不到目标「{}」", item.target_name))
}

// ==================== 诊断包 ====================

#[derive(serde::Serialize)]
pub struct DiagBundle {
    pub dir: String,
    pub files: Vec<String>,
    pub bytes: i64,
}

/// 导出诊断包（**不脱敏**）：schema / 各表 CSV / 配置 / 提示词记录 / 体检报告。
///
/// 写到 `paths::temp_root()/diag-<时间戳>/`，返回目录与文件清单，由界面显示路径。
#[tauri::command]
pub async fn dev_export_diagnostics(
    pool: tauri::State<'_, SqlitePool>,
    story_id: i64,
) -> Result<DiagBundle, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (&pool, story_id);
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        let lib = pool.inner().clone();
        let title = assert_test_story(&lib, story_id).await?;
        let pool = ensure_dev_story_pool(story_id).await?;

        let stamp = format!(
            "diag-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
        let dir = crate::paths::temp_root().join(&stamp);
        std::fs::create_dir_all(&dir).map_err(|e| format!("建目录失败：{e}"))?;
        let mut files: Vec<String> = Vec::new();

        // ① 说明
        let mut readme = String::new();
        readme.push_str(&format!("# 诊断包\n\n故事：{title}（id={story_id}）\n"));
        readme.push_str(&format!("数据根：{}\n", crate::paths::data_root().display()));
        readme.push_str(&format!("版本：{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
        readme.push_str("构建：");
        readme.push_str(if cfg!(debug_assertions) { "debug\n" } else { "release\n" });
        readme.push_str("\n文件说明：\n- schema.sql：story.db 的表结构（含索引）\n- tables/*.csv：各表全量内容\n- config.json：library.db 里的配置项（**不含 API Key**，Key 只在凭据管理器里）\n- prompts.jsonl：最近的请求记录（若表存在）\n- health.json：本次数据体检结果\n");
        let f = dir.join("README.md");
        std::fs::write(&f, readme).map_err(|e| e.to_string())?;
        files.push("README.md".into());

        // ② schema
        let ddl: Vec<(String, String)> = sqlx::query_as(
            "SELECT type, COALESCE(sql, '') FROM sqlite_master WHERE sql IS NOT NULL ORDER BY type, name",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        let schema = ddl.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>().join(";\n\n");
        let f = dir.join("schema.sql");
        std::fs::write(&f, schema).map_err(|e| e.to_string())?;
        files.push("schema.sql".into());

        // ③ 各表 CSV（用 SQLite 的引号规则：值里的双引号翻倍）
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        let tdir = dir.join("tables");
        std::fs::create_dir_all(&tdir).map_err(|e| e.to_string())?;
        for tbl in &tables {
            let rows = sqlx::query(&format!("SELECT * FROM {tbl}"))
                .fetch_all(&pool)
                .await
                .unwrap_or_default();
            let mut csv = String::new();
            if let Some(first) = rows.first() {
                let cols: Vec<String> = first.columns().iter().map(|c| c.name().to_string()).collect();
                csv.push_str(&cols.join(","));
                csv.push('\n');
                for r in &rows {
                    let mut cells = Vec::new();
                    for (idx, c) in r.columns().iter().enumerate() {
                        let cell = match c.type_info().name() {
                            "INTEGER" | "REAL" => r
                                .try_get::<i64, _>(idx)
                                .map(|v| v.to_string())
                                .unwrap_or_else(|_| {
                                    r.try_get::<f64, _>(idx).map(|v| v.to_string()).unwrap_or_default()
                                }),
                            _ => r.try_get::<String, _>(idx).unwrap_or_default(),
                        };
                        cells.push(format!("\"{}\"", cell.replace('"', "\"\"")));
                    }
                    csv.push_str(&cells.join(","));
                    csv.push('\n');
                }
            }
            let f = tdir.join(format!("{tbl}.csv"));
            std::fs::write(&f, csv).map_err(|e| e.to_string())?;
            files.push(format!("tables/{tbl}.csv"));
        }

        // ④ 配置（library.db 的 configs 表；**Key 不在里面**，只在凭据管理器）
        let cfgs: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM configs ORDER BY key")
            .fetch_all(&lib)
            .await
            .unwrap_or_default();
        let cfg_json = serde_json::to_string_pretty(
            &cfgs.iter().map(|(k, v)| serde_json::json!({"key": k, "value": v})).collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".into());
        let f = dir.join("config.json");
        std::fs::write(&f, cfg_json).map_err(|e| e.to_string())?;
        files.push("config.json".into());

        // ⑤ 最近请求记录（表名可能是 debug_requests；不存在就跳过）
        let has_dbg: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='debug_requests'",
        )
        .fetch_one(&pool)
        .await
        .unwrap_or(0);
        if has_dbg > 0 {
            let rows: Vec<(i64, String, String)> = sqlx::query_as(
                "SELECT id, COALESCE(request_json,''), COALESCE(created_at,'') FROM debug_requests ORDER BY id DESC LIMIT 30",
            )
            .fetch_all(&pool)
            .await
            .unwrap_or_default();
            let mut jl = String::new();
            for (id, req, at) in rows {
                jl.push_str(&format!("{{\"id\":{id},\"created_at\":\"{at}\",\"request\":{req}}}\n"));
            }
            let f = dir.join("prompts.jsonl");
            std::fs::write(&f, jl).map_err(|e| e.to_string())?;
            files.push("prompts.jsonl".into());
        }

        // ⑥ 体检结果
        let health = dev_health_check_impl(&lib, story_id).await?;
        let hj = serde_json::to_string_pretty(&health).unwrap_or_else(|_| "{}".into());
        let f = dir.join("health.json");
        std::fs::write(&f, hj).map_err(|e| e.to_string())?;
        files.push("health.json".into());

        let bytes = files
            .iter()
            .map(|n| std::fs::metadata(dir.join(n)).map(|m| m.len() as i64).unwrap_or(0))
            .sum();

        Ok(DiagBundle {
            dir: dir.to_string_lossy().to_string(),
            files,
            bytes,
        })
    }
}

/// 导出任意文本（给"导出提示词 .txt"用；也走 temp_root，避免动到用户目录）。
#[tauri::command]
pub async fn dev_export_text(
    pool: tauri::State<'_, SqlitePool>,
    story_id: i64,
    name: String,
    text: String,
) -> Result<String, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (&pool, story_id, name, text);
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        let lib = pool.inner().clone();
        assert_test_story(&lib, story_id).await?;
        let safe: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
            .collect();
        let dir = crate::paths::temp_root().join("export");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let f = dir.join(if safe.is_empty() { "export.txt".to_string() } else { safe });
        std::fs::write(&f, text).map_err(|e| e.to_string())?;
        Ok(f.to_string_lossy().to_string())
    }
}

/// 回滚到某个快照（注入时自动打的那些）。
#[tauri::command]
pub async fn dev_rollback(
    pool: tauri::State<'_, SqlitePool>,
    story_id: i64,
    version: i64,
) -> Result<String, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (&pool, story_id, version);
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        let lib = pool.inner().clone();
        let title = assert_test_story(&lib, story_id).await?;
        let msg = crate::evolution::rollback_snapshot(&story_id.to_string(), version).await?;
        Ok(format!("《{title}》{msg}"))
    }
}

/// 列出测试故事最近的快照（便于回滚）。
#[tauri::command]
pub async fn dev_snapshots(
    pool: tauri::State<'_, SqlitePool>,
    story_id: i64,
) -> Result<Vec<(i64, String, i64)>, String> {
    #[cfg(not(debug_assertions))]
    {
        let _ = (&pool, story_id);
        return Err(DEV_ONLY.to_string());
    }
    #[cfg(debug_assertions)]
    {
        let lib = pool.inner().clone();
        assert_test_story(&lib, story_id).await?;
        let rows = crate::evolution::list_snapshots(&story_id.to_string()).await?;
        Ok(rows
            .into_iter()
            .map(|s| (s.version, s.created_at, s.temporary))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    /// 诊断（手动跑）：空壳测试故事（只有 stories 行、没有 story.db）会 CANTOPEN，
    /// 而 `ensure_dev_story_pool` 必须能补建并成功打开。
    ///   DW_DIAG_STORY_DB 不需要；用 DREAMWEAVER_DATA_DIR 指定临时数据根
    #[tokio::test]
    #[ignore]
    async fn diag_shell_story_is_repaired() {
        // ① 建 library，插一个故事（只插行，不建库）
        let lib = crate::db::init_library_db().await.unwrap();
        let id = crate::db::create_story(&lib, "测试0", "2026-01-01", "").await.unwrap();
        let path = crate::paths::story_db(&id.to_string());
        println!("故事 id={id} 期望路径={}", path.display());
        println!("插行后 story.db 存在？{}", path.exists());

        // ② 修复前：直接 open → 应当 CANTOPEN
        let raw = crate::story_db::open_story_pool(&id.to_string()).await;
        match &raw {
            Ok(_) => println!("open 直接成功（说明路径被别的东西建好了）"),
            Err(e) => println!("open 直接失败（复现用户报错）：{e}"),
        }

        // ③ 修复后：ensure 能补建并成功
        let repaired = ensure_dev_story_pool(id).await;
        match &repaired {
            Ok(_) => println!("ensure_dev_story_pool 成功补齐 ✅"),
            Err(e) => println!("ensure 仍失败 ❌：{e}"),
        }
        println!("补建后 story.db 存在？{}", path.exists());
        assert!(repaired.is_ok(), "ensure 应该能补齐空壳故事");

        // ④ 补建后能正常读写
        let pool = repaired.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM timeline_nodes")
            .fetch_one(&pool).await.unwrap();
        println!("timeline_nodes 行数 = {n}（0 表示表已建好、只是空的）");
        pool.close().await;
    }

    use super::*;

    #[test]
    fn next_title_starts_at_zero() {
        assert_eq!(next_test_title(&[]), "测试0");
    }

    #[test]
    fn next_title_fills_smallest_gap() {
        let v = vec!["测试0".to_string(), "测试2".to_string()];
        assert_eq!(next_test_title(&v), "测试1");
    }

    #[test]
    fn next_title_ignores_other_titles() {
        let v = vec!["大肥鱼的家".to_string(), "测试".to_string(), "测试x".to_string()];
        assert_eq!(next_test_title(&v), "测试0", "非数字后缀不算占用编号");
    }

    #[test]
    fn prefix_is_the_sandbox_boundary() {
        assert!(format!("{TEST_PREFIX}0").starts_with(TEST_PREFIX));
        assert!(!format!("我的{}", TEST_PREFIX).starts_with(TEST_PREFIX));
    }
}
