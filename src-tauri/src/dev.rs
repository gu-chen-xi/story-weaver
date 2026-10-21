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

use sqlx::{Row, SqlitePool};

/// 测试故事标题前缀。**改这里等于改沙箱边界**，所以集中一处。
pub const TEST_PREFIX: &str = "测试";

const DEV_ONLY: &str = "开发者工具仅存在于开发构建（release 包里没有）";

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
        crate::db::create_story(&lib, &st.next_title, &today, "")
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
    let pool = crate::story_db::open_story_pool(&story_id.to_string())
        .await
        .map_err(|e| e.to_string())?;
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
        let pool = crate::story_db::open_story_pool(&story_id.to_string())
            .await
            .map_err(|e| e.to_string())?;
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
        let pool = crate::story_db::open_story_pool(&story_id.to_string())
            .await
            .map_err(|e| e.to_string())?;

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
