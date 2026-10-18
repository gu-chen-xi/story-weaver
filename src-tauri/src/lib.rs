mod recall;
#[cfg(test)]
mod frontend_guard;
mod paths;
mod gen_cancel;
mod db;
mod keychain;
mod keys;
mod story_db;
mod characters;
mod world;
mod relations;
mod api;
mod timeline;
mod generate;
mod generate_stream;
mod check;
mod evaluate;
mod state_extract;
mod evolution;
mod export;
mod portraits;
mod generator;
mod generator_ingest;
mod generator_cloud;
mod image_stamp;
mod branch;

use sqlx::SqlitePool;
use std::str::FromStr;
use tauri::Manager;

/// 第四批 ⑥ P1：把生成结果入库（**唯一同时知道生成与素材库的地方**，内部走 portrait_import）
#[tauri::command]
async fn generator_ingest(req: generator_ingest::IngestRequest) -> Result<portraits::PortraitItem, String> {
    generator_ingest::ingest(&req).await
}


/// 读云端配置（配置表里的键；API Key 若配置表为空则尝试系统凭据库）
async fn read_cloud_config(pool: &SqlitePool) -> generator_cloud::CloudConfig {
    let mut cache: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for k in [
        "portrait.cloud_url", "portrait.cloud_model", "portrait.cloud_api_key",
        "portrait.cloud_request_url", "portrait.cloud_body_template", "portrait.cloud_response_path",
        "portrait.cloud_auth_style", "portrait.cloud_extra_headers", "portrait.cloud_ok_status",
        "portrait.cloud_response_raw_b64",
    ] {
        let v = db::get_config(pool, k).await.ok().flatten().unwrap_or_default();
        cache.insert(k.to_string(), v);
    }
    // **凭据库优先**（Key 的正确位置）。表里那份只在"凭据库还没有"时当遗留兜底 ——
    // 反向优先会让历史明文永远压过用户后来设的新 Key（旧实现就是这个毛病）。
    match keys::get(keys::PORTRAIT_ACCOUNT) {
        Some(k) => {
            cache.insert("portrait.cloud_api_key".to_string(), k);
        }
        None => { /* 保持表里的值（若有），并等启动清理把它搬走 */ }
    }
    generator_cloud::CloudConfig::from_map(&|k: &str| cache.get(k).cloned().unwrap_or_default())
}

/// 用界面当前值覆盖已保存配置（留空的字段保留原值；Key 留空时若用户填了就用界面值）
fn merge_cloud(mut saved: generator_cloud::CloudConfig, ov: &generator_cloud::CloudOverrides) -> generator_cloud::CloudConfig {
    saved.apply_overrides(ov);
    saved
}

/// 第四批 ⑥ P3：保存云端通道配置（前端"保存云端配置"按钮）
#[tauri::command]
async fn generator_save_cloud_config(
    pool: tauri::State<'_, SqlitePool>,
    config: generator_cloud::CloudOverrides,
    apiKey: Option<String>,
) -> Result<String, String> {
    let lib_pool = pool.inner().clone();
    let mut saved = 0;
    let pairs: [(&str, Option<String>); 7] = [
        ("portrait.cloud_url", config.base_url),
        ("portrait.cloud_model", config.model),
        ("portrait.cloud_request_url", config.url),
        ("portrait.cloud_body_template", config.body_template),
        ("portrait.cloud_response_path", config.response_path),
        ("portrait.cloud_auth_style", config.auth_style),
        ("portrait.cloud_extra_headers", config.extra_headers),
        // ⚠ **不要把 Key 写进配置表** —— 那等于明文落盘（见 keys.rs 顶部注释）。
        // Key 一律走系统凭据库（下面的 keychain/keys::set）。
    ];
    for (k, v) in pairs {
        if let Some(val) = v {
            db::set_config(&lib_pool, k, &val).await.map_err(|e| e.to_string())?;
            saved += 1;
        }
    }
    // Key **只**进系统凭据库（配置表里绝不留明文）
    let key_to_store = apiKey.unwrap_or_default();
    let mut key_note = String::new();
    if !key_to_store.trim().is_empty() {
        match keys::set(keys::PORTRAIT_ACCOUNT, &key_to_store) {
            Ok(()) => key_note = "；API Key 已存入系统凭据库".to_string(),
            Err(e) => return Err(format!("配置已保存，但 API Key 写入系统凭据库失败：{e}")),
        }
    }
    Ok(format!("已保存 {saved} 项云端配置{key_note}"))
}

/// 主模型 Key 是否已设置（**绝不回传明文**，只回布尔值）。
///
/// 用途：界面上给用户一个"已保存"的确认 ——
/// Key 存在系统凭据库里，界面无法自己判断，以前只能让用户"留空试试"。
#[tauri::command]
fn main_model_key_status() -> serde_json::Value {
    serde_json::json!({
        "has_key": keys::get(keys::MAIN_MODEL_ACCOUNT).is_some(),
    })
}

/// 第四批 ⑥ P3：读取云端通道配置给界面（Key 不回传明文，只回传"是否已设置"）
#[tauri::command]
async fn generator_get_cloud_config(pool: tauri::State<'_, SqlitePool>) -> Result<serde_json::Value, String> {
    let lib_pool = pool.inner().clone();
    let cfg = read_cloud_config(&lib_pool).await;
    Ok(serde_json::json!({
        "base_url": cfg.base_url,
        "model": cfg.model,
        "url": cfg.url,
        "body_template": cfg.body_template,
        "response_path": cfg.response_path,
        "auth_style": cfg.auth_style,
        "extra_headers": cfg.extra_headers,
        "has_api_key": !cfg.api_key.trim().is_empty(),
    }))
}

/// 第四批 ⑥ 附加项：给生成结果烧录「AI 生成」显式角标 + 隐式元数据标识（**加**标识，不是去标识）
/// 入参为临时文件路径；返回新文件路径（原文件不动）。失败时返回原因，调用方应**降级为按原图入库**。
#[tauri::command]
fn stamp_ai_label(path: String) -> Result<String, String> {
    let src = std::path::Path::new(&path);
    if !src.exists() {
        return Err(format!("文件不存在：{path}"));
    }
    let bytes = std::fs::read(src).map_err(|e| e.to_string())?;
    if !image_stamp::is_png(&bytes) {
        return Err("目前只支持对 PNG 烧录角标（其它格式请保持原样，或用文件名 _AI 标识）".into());
    }
    let stamped = image_stamp::stamp_ai_label(&bytes)?;
    let dir = generator::temp_dir();
    image_stamp::write_stamped(&dir, &stamped, 0)
}

/// 第四批 ⑥ P1：列出立绘生成通道（如实报告每个通道是否已实装）
#[tauri::command]
fn generator_list() -> Vec<generator::GeneratorDescriptor> {
    generator::registry()
}

/// 第四批 ⑥ P1：通道连接测试（只读探测，不产图）
#[tauri::command]
async fn generator_probe(
    pool: tauri::State<'_, SqlitePool>,
    generatorId: String,
    baseUrl: Option<String>,
    config: Option<generator_cloud::CloudOverrides>,
) -> Result<generator::GenProbe, String> {
    let lib_pool = pool.inner().clone();
    let saved = read_cloud_config(&lib_pool).await;
    let mut ov = config.unwrap_or_default();
    // 地址优先取"界面上填的"，没填则取已保存的
    if ov.base_url.as_deref().unwrap_or("").trim().is_empty() {
        ov.base_url = Some(baseUrl.unwrap_or_default());
    }
    let merged = merge_cloud(saved, &ov);
    let base = merged.base_url.clone();
    generator::probe(&generatorId, &base, &merged).await
}

/// 第四批 ⑥ P1：生成立绘 → 结果落临时目录（入库由前端调 portrait_import 完成，严守 R1）
#[tauri::command]
async fn generator_generate(
    pool: tauri::State<'_, SqlitePool>,
    generatorId: String,
    req: generator::GenRequest,
    config: Option<generator_cloud::CloudOverrides>,
) -> Result<generator::GenResult, String> {
    let lib_pool = pool.inner().clone();
    // 本地通道用 portrait.base_url；云端通道用云端配置（也会带上界面/已保存的覆盖值）
    let saved = read_cloud_config(&lib_pool).await;
    let merged = merge_cloud(saved, &config.unwrap_or_default());
    let local_url = db::get_config(&lib_pool, "portrait.base_url").await.map_err(|e| e.to_string())?
        .unwrap_or_default();
    let base_url = if merged.base_url.trim().is_empty() { local_url } else { merged.base_url.clone() };
    let api_key = if merged.api_key.trim().is_empty() {
        crate::keychain::get_api_key("portrait_api_key".to_string()).unwrap_or(None).unwrap_or_default()
    } else {
        merged.api_key.clone()
    };
    generator::generate(&generatorId, &base_url, &api_key, &merged, &req).await
}
/// 列出 temp/ 里的中间文件（「临时文件」页用）
#[tauri::command]
fn generator_list_temp() -> Vec<generator::TempFileInfo> {
    generator::list_temp()
}


/// 第四批 ⑥ P1：清理生成临时目录（保留 keep 里的文件）
#[tauri::command]
fn generator_cleanup_temp(keep: Vec<String>) -> usize {
    generator::cleanup_temp(&keep)
}


#[tauri::command]
async fn get_config(pool: tauri::State<'_, SqlitePool>, key: String) -> Result<Option<String>, String> {
    let pool = pool.inner().clone();
    db::get_config(&pool, &key).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_config(
    pool: tauri::State<'_, SqlitePool>,
    key: String,
    value: String,
) -> Result<(), String> {
    let pool = pool.inner().clone();
    db::set_config(&pool, &key, &value).await.map_err(|e| e.to_string())
}


#[tauri::command]
async fn list_stories(pool: tauri::State<'_, SqlitePool>) -> Result<Vec<db::StorySummary>, String> {
    let pool = pool.inner().clone();
    db::list_stories(&pool).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_story(
    pool: tauri::State<'_, SqlitePool>,
    title: String,
    time_origin: String,
) -> Result<db::StorySummary, String> {
    let pool = pool.inner().clone();
    let id = db::create_story(&pool, &title, &time_origin).await.map_err(|e| e.to_string())?;
    // 同时建该故事的 story.db
    story_db::init_story_db(&id.to_string()).await.map_err(|e| e.to_string())?;
    let list = db::list_stories(&pool).await.map_err(|e| e.to_string())?;
    list.into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "创建后未找到".to_string())
}

#[tauri::command]
async fn delete_story(pool: tauri::State<'_, SqlitePool>, id: i64) -> Result<(), String> {
    let pool = pool.inner().clone();
    // 删 story.db 文件及其目录
    let path = story_db::story_db_path(&id.to_string());
    let _ = std::fs::remove_file(&path);
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir(parent);
    }
    db::delete_story(&pool, id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_characters(storyId: String) -> Result<Vec<characters::Character>, String> {
    characters::list_characters(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_character(storyId: String, input: characters::CharacterInput) -> Result<i64, String> {
    characters::create_character(&storyId, &input).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_character(storyId: String, id: i64, input: characters::CharacterInput) -> Result<(), String> {
    characters::update_character(&storyId, id, &input).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn character_usage(storyId: String) -> Result<Vec<characters::CharacterUsage>, String> {
    characters::character_usage(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_character(storyId: String, id: i64) -> Result<(), String> {
    characters::delete_character(&storyId, id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_world_settings(storyId: String) -> Result<Vec<world::WorldSetting>, String> {
    world::list_world_settings(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_world_setting(
    storyId: String,
    key: String,
    value: String,
    isHardRule: bool,
    category: String,
    metaJson: String,
) -> Result<i64, String> {
    world::create_world_setting(&storyId, &key, &value, isHardRule, &category, &metaJson)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_world_setting(
    storyId: String,
    id: i64,
    key: String,
    value: String,
    isHardRule: bool,
    category: String,
    metaJson: String,
) -> Result<(), String> {
    world::update_world_setting(&storyId, id, &key, &value, isHardRule, &category, &metaJson)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn detect_world_conflicts(storyId: String) -> Result<Vec<world::WorldConflict>, String> {
    world::detect_world_conflicts(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_world_setting(storyId: String, id: i64) -> Result<(), String> {
    world::delete_world_setting(&storyId, id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_relations(storyId: String) -> Result<Vec<relations::Relation>, String> {
    relations::list_relations(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn create_relation(
    storyId: String,
    fromId: i64,
    toId: i64,
    relationType: String,
    direction: String,
    strength: i64,
    state: String,
    description: String,
    evidence: String,
) -> Result<i64, String> {
    relations::create_relation(&storyId, fromId, toId, &relationType, &direction, strength, &state, &description, &evidence)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn update_relation(
    storyId: String,
    id: i64,
    relationType: String,
    direction: String,
    strength: i64,
    state: String,
    description: String,
    evidence: String,
) -> Result<(), String> {
    relations::update_relation(&storyId, id, &relationType, &direction, strength, &state, &description, &evidence)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_relation(storyId: String, id: i64) -> Result<(), String> {
    relations::delete_relation(&storyId, id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn test_api_connection(pool: tauri::State<'_, SqlitePool>) -> Result<api::TestResult, String> {
    let pool = pool.inner().clone();
    api::test_connection(&pool).await
}

#[tauri::command]
async fn ensure_first_node(storyId: String) -> Result<timeline::TimelineNode, String> {
    timeline::ensure_first_node(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_messages(storyId: String, nodeId: i64) -> Result<Vec<timeline::Message>, String> {
    timeline::list_messages(&storyId, nodeId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_message(
    storyId: String,
    nodeId: i64,
    role: String,
    content: String,
    mode: String,
) -> Result<i64, String> {
    timeline::create_message(&storyId, nodeId, &role, &content, &mode).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_timeline_nodes(
    storyId: String,
    branchId: Option<String>,
) -> Result<Vec<timeline::TimelineNode>, String> {
    // 第五批 ③：**以界面传来的分支为准**，顺手把记录校正过来。
    // 为什么：`stories.current_branch_id` 是隐式状态，一旦与界面不同步，
    // 就会出现"界面在主线、节点却建到分支"这类看不见的错（实测踩到）。
    // 界面每次加载时间线都会调这个命令 → 这里是最自然的"校正点"。
    if let Err(e) = branch::set_current_branch(&storyId, &branchId).await {
        eprintln!("[branch] 校正当前分支失败（忽略）：{e}");
    }
    // 注意：**仍返回全树** —— 树状图与父子关系判断需要看到所有分支的节点；
    // "只显示当前分支这一条线"是界面侧的过滤（filter by branch_id）。
    timeline::list_timeline_nodes(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_next_day_node(
    storyId: String,
    branchId: Option<String>,
    afterNodeId: Option<i64>,
) -> Result<timeline::TimelineNode, String> {
    // 分支怎么定：**节点确实属于某条分支**时按节点推导，否则信界面传来的 branchId。
    //
    // 为什么不能无条件按节点推导（用户实测的 bug）：
    //   新建一条分支时，它的"最后一个节点"就是**分叉点本身** —— 而分叉点属于共享历史
    //   （`branch_id IS NULL`）。`branch_of_node_in_story` 对共享历史返回 `Some(None)`，
    //   即"在主线"，于是「下一天」被建到了**主线**里（数字：建了分支、点下一天，主线多一天）。
    //
    //   共享历史在**每条线**上都成立，它无法区分"主线"和"某条还没写过内容的分支"，
    //   所以只认 `Some(Some(b))`（节点挂在真分支上）；其余情况一律用 branchId。
    let derived = match afterNodeId {
        Some(nid) => timeline::branch_of_node_in_story(&storyId, nid).await,
        None => None,
    };
    let resolved = timeline::resolve_branch_for_new_node(derived, branchId);
    timeline::create_next_day_node_with(&storyId, resolved).await.map_err(|e| e.to_string())
}

/// 第五批：删除单条消息（失败重发时撤掉孤儿用户消息 / 清理半截回复）
#[tauri::command]
async fn delete_message(storyId: String, messageId: i64) -> Result<(), String> {
    timeline::delete_message(&storyId, messageId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_message(storyId: String, messageId: i64, content: String) -> Result<(), String> {
    timeline::update_message(&storyId, messageId, &content).await.map_err(|e| e.to_string())
}

/// C 子项（时间流逝）：在**指定天数**插入 day 节点（用户点日历/时间线上的"时间流逝"区间条时调用）
#[tauri::command]
async fn create_node_at_day(
    storyId: String,
    dayNo: i64,
    branchId: Option<String>,
) -> Result<timeline::TimelineNode, String> {
    timeline::create_node_at_day_with(&storyId, dayNo, Some(branchId)).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_interlude_node(
    storyId: String,
    branchId: Option<String>,
) -> Result<timeline::TimelineNode, String> {
    timeline::create_interlude_node_with(&storyId, Some(branchId)).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_timeline_node(storyId: String, nodeId: i64) -> Result<(), String> {
    timeline::delete_timeline_node(&storyId, nodeId).await.map_err(|e| e.to_string())
}

// ===== D 子项：日摘要 / 阶段摘要 =====

/// 为指定节点生成日摘要（force=true 时覆盖已固化摘要；默认幂等，已有则跳过）
#[tauri::command]
async fn generate_day_summary(
    pool: tauri::State<'_, SqlitePool>,
    storyId: String,
    nodeId: i64,
    force: Option<bool>,
) -> Result<Option<String>, String> {
    let lib_pool = pool.inner().clone();
    timeline::generate_day_summary(&lib_pool, &storyId, nodeId, force.unwrap_or(false)).await
}

/// 当天对话到量后自动生成/刷新当天日摘要（阈值：AI 回复条数，默认 10）
#[tauri::command]
async fn maybe_auto_day_summary(
    pool: tauri::State<'_, SqlitePool>,
    storyId: String,
    nodeId: i64,
    minReplies: Option<i64>,
) -> Result<Option<String>, String> {
    let lib_pool = pool.inner().clone();
    timeline::maybe_auto_day_summary(&lib_pool, &storyId, nodeId, minReplies.unwrap_or(10)).await
}

/// ⑥ 阶段摘要（C 层）：攒够若干天（`PHASE_MIN_DAYS`）后，把**最老的一段**日摘要滚成一段阶段摘要。
///
/// 触发点与日摘要相同（回复完成后，前端顺手调一次）；幂等，不够天数时返回 `null`，前端静默忽略。
/// ⑥ 手动压缩阶段摘要（用户点「压缩」）：可以指定本次用多少天（x），
/// 不传则用配置里的默认值。返回覆盖区间与摘要正文，供界面回显。
/// 摘要管理（导演监视器）：列出本故事的阶段摘要
/// 摘要管理：清理重叠的阶段摘要（保留最新一条）
#[tauri::command]
async fn dedupe_phase_rows(storyId: String) -> Result<usize, String> {
    timeline::dedupe_phase_rows(&storyId).await
}

#[tauri::command]
async fn list_phase_rows(storyId: String) -> Result<Vec<timeline::PhaseRow>, String> {
    timeline::list_phase_rows(&storyId).await
}

/// 摘要管理：删除一条阶段摘要
#[tauri::command]
async fn delete_phase_row(storyId: String, id: i64) -> Result<bool, String> {
    timeline::delete_phase_row(&storyId, id).await
}

#[tauri::command]
async fn run_phase_summary(
    pool: tauri::State<'_, SqlitePool>,
    storyId: String,
    minDays: Option<usize>,
    saveAsDefault: Option<bool>,
) -> Result<Option<timeline::PhaseMade>, String> {
    let lib_pool = pool.inner().clone();
    // 把界面上的 x 记住（下次自动压缩也用它）—— 用户"想要可调整"的核心诉求
    if let (Some(n), Some(true)) = (minDays, saveAsDefault) {
        let _ = crate::db::set_config(&lib_pool, timeline::CFG_PHASE_MIN_DAYS, &n.to_string()).await;
    }
    timeline::run_phase_summary_report(&lib_pool, &storyId, minDays).await
}

/// 读/写"攒够多少天再压缩"
#[tauri::command]
async fn get_phase_min_days(pool: tauri::State<'_, SqlitePool>) -> Result<usize, String> {
    Ok(timeline::phase_min_days_cfg(pool.inner()).await)
}

#[tauri::command]
async fn set_phase_min_days(pool: tauri::State<'_, SqlitePool>, days: usize) -> Result<(), String> {
    crate::db::set_config(pool.inner(), timeline::CFG_PHASE_MIN_DAYS, &days.to_string())
        .await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn maybe_auto_phase_summary(
    pool: tauri::State<'_, SqlitePool>,
    storyId: String,
) -> Result<Option<String>, String> {
    let lib_pool = pool.inner().clone();
    timeline::generate_phase_summary(&lib_pool, &storyId).await
}

/// 补齐历史节点的日摘要（幂等）：切节点 / 回复完成后调用，返回 成功列表 + 失败原因
#[tauri::command]
async fn backfill_day_summaries(
    pool: tauri::State<'_, SqlitePool>,
    storyId: String,
    currentNodeId: i64,
) -> Result<timeline::BackfillReport, String> {
    let lib_pool = pool.inner().clone();
    timeline::backfill_day_summaries(&lib_pool, &storyId, currentNodeId).await
}

/// 阶段摘要（**规划中**：C 层接口预留，界面暂未接入 —— 见 ROADMAP §7 C1）
/// 写入一段覆盖 [dayFrom, dayTo] 的摘要
#[tauri::command]
async fn save_phase_summary(
    storyId: String,
    dayFrom: i64,
    dayTo: i64,
    content: String,
    sourceDays: i64,
) -> Result<i64, String> {
    timeline::save_phase_summary(&storyId, dayFrom, dayTo, &content, sourceDays).await
}

/// 阶段摘要列表（**规划中**：C 层接口预留，界面暂未接入 —— 见 ROADMAP §7 C1）
/// 按 day_from 倒序
#[tauri::command]
async fn list_phase_summaries(storyId: String) -> Result<Vec<(i64, i64, i64, String)>, String> {
    timeline::list_phase_summaries(&storyId).await
}

// ===== 第二批 · ② 检查 =====

/// 本地规则检查（零 API 成本）：只查纯结构问题
#[tauri::command]
async fn check_board_struct(storyId: String, board: String) -> Result<check::CheckReport, String> {
    match board.as_str() {
        "characters" => check::struct_characters(&storyId).await,
        "world" => check::struct_world(&storyId).await,
        _ => Err(format!("未知板块：{board}")),
    }
}

/// AI 检查：查语法 / 歧义 / 关系数值与类型矛盾；每轮输出更新后的完整建议集
#[tauri::command]
async fn check_board_ai(
    pool: tauri::State<'_, SqlitePool>,
    storyId: String,
    board: String,
    content: String,
    previous: Option<String>,
    userInput: Option<String>,
) -> Result<check::CheckReport, String> {
    let lib_pool = pool.inner().clone();
    check::check_board_ai(&lib_pool, &board, &content, &previous.unwrap_or_default(), &userInput.unwrap_or_default()).await
}

/// 一键替换：白名单校验 → 打快照 → 逐条执行（校验不通过则一条都不执行）
#[tauri::command]
async fn apply_check_patch(
    storyId: String,
    items: Vec<check::PatchItem>,
) -> Result<Vec<String>, String> {
    check::apply_check_patch(&storyId, &items).await
}

/// 第三批：自动分析剧情演进（回复后静默调用；纯本地规则，不调 API）
#[tauri::command]
async fn auto_analyze_evolution(storyId: String, nodeId: i64) -> Result<(usize, usize), String> {
    evolution::auto_analyze(&storyId, nodeId).await
}

/// 第三批 ④：评估一条 AI 回复的质量（4 维打分，写 reply_evaluations）
#[tauri::command]
async fn evaluate_reply(
    pool: tauri::State<'_, SqlitePool>,
    storyId: String,
    messageId: i64,
) -> Result<evaluate::Evaluation, String> {
    let lib_pool = pool.inner().clone();
    evaluate::evaluate_message(&lib_pool, &storyId, messageId).await
}

/// 第三批 ④：列出最近的回复评估（倒序，用于看趋势）
#[tauri::command]
async fn list_reply_evaluations(storyId: String, limit: Option<i64>) -> Result<Vec<evaluate::Evaluation>, String> {
    evaluate::list_evaluations(&storyId, limit.unwrap_or(20)).await
}


/// RAG ①：列出可「🔍 回顾」的天（本分支可见 + 有日摘要）
#[tauri::command]
async fn list_recall_days(storyId: String) -> Result<Vec<recall::RecallDay>, String> {
    let pool = story_db::open_story_pool(&storyId).await.map_err(|e| e.to_string())?;
    let branch = branch::resolve_branch(&storyId, &None).await.unwrap_or(None);
    recall::list_days(&pool, &branch).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn generate_story_reply_stream(
    app: tauri::AppHandle,
    pool: tauri::State<'_, SqlitePool>,
    storyId: String,
    nodeId: i64,
    userText: String,
    mode: String,
    myRole: String,
    aiRole: String,
    focusRole: String,
    tags: String,
    constraints: String,
    lengthLevel: String,
    memory: bool,
    recallDay: Option<i64>,
    requestId: String,
) -> Result<(), String> {
    let lib_pool = pool.inner().clone();
    generate_stream::generate_reply_streaming(app, &lib_pool, &storyId, nodeId, &userText, &mode, &myRole, &aiRole, &focusRole, &tags, &constraints, &lengthLevel, memory, recallDay, &requestId).await
}

/// 第五批（《现状说明》5.2）：**停止正在进行的生成**。
///
/// 前端在发送时生成一个 `requestId`（UUID），点「停止」时带着同一个 id 调这里。
/// 后端把该任务标记为已取消，流式循环在下一个 chunk 处退出，
/// 并给前端发 `chat-cancelled`（界面把已生成的部分标成"已停止"而不是"已完成"）。
///
/// 返回 `true` = 找到了任务并已请求取消；`false` = 任务已结束（或 id 不对）。
#[tauri::command]
async fn cancel_generation(requestId: String) -> Result<bool, String> {
    Ok(gen_cancel::cancel(&requestId))
}

#[derive(serde::Serialize)]
struct DebugRequest {
    id: i64,
    node_id: Option<i64>,
    model: Option<String>,
    system_prompt: String,
    /// 尾部「上下文更新」块（当前模式 / 篇幅 / 数值与状态 / 聚焦 / 约束 / 时空 / 导演指令）；老快照为空
    tail_block: Option<String>,
    /// 本次实际使用的预算口径（前端不再自己硬编码算，直接读这几项）
    context_window: i64,
    reserve_ratio: f64,
    max_tokens: i64,
    safety_margin: i64,
    input_budget: i64,
    /// 本次保留了几天的原文（B 子项：跨天历史）
    day_window: i64,
    /// 每天各取几条上限，逗号分隔（如 "10,10,7"）
    day_n: String,
    /// 本次由预算倒推出来的 N（每日条数，用于界面显示"现在的 N 值"）
    n_value: i64,
    /// 单条消息平均 token（界面按不同窗口重算 N 时要用）
    avg_msg_tokens: i64,
    history_json: String,
    user_text: String,
    ai_reply: Option<String>,
    estimated_tokens: i64,
    n_used: i64,
    status: String,
    error: Option<String>,
    created_at: String,
}

#[tauri::command]
async fn list_debug_requests(storyId: String) -> Result<Vec<DebugRequest>, String> {
    use sqlx::Row;
    let path = story_db::story_db_path(&storyId);
    let opts = sqlx::sqlite::SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
        .map_err(|e| e.to_string())?;
    let pool = sqlx::SqlitePool::connect_with(opts).await.map_err(|e| e.to_string())?;
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS debug_requests (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id INTEGER, model TEXT, system_prompt TEXT NOT NULL,
            tail_block TEXT,
            context_window INTEGER NOT NULL DEFAULT 0,
            reserve_ratio REAL NOT NULL DEFAULT 0,
            max_tokens INTEGER NOT NULL DEFAULT 0,
            safety_margin INTEGER NOT NULL DEFAULT 0,
            input_budget INTEGER NOT NULL DEFAULT 0,
            day_window INTEGER NOT NULL DEFAULT 0,
            day_n TEXT NOT NULL DEFAULT '',
            n_value INTEGER NOT NULL DEFAULT 0,
            avg_msg_tokens INTEGER NOT NULL DEFAULT 0,
            history_json TEXT NOT NULL, user_text TEXT NOT NULL, ai_reply TEXT,
            estimated_tokens INTEGER NOT NULL DEFAULT 0, n_used INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'ok', error TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )"
    ).execute(&pool).await;
    // 老库补列（幂等）
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN tail_block TEXT").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN context_window INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN reserve_ratio REAL NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN max_tokens INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN safety_margin INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN input_budget INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN day_window INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN day_n TEXT NOT NULL DEFAULT ''").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN n_value INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE debug_requests ADD COLUMN avg_msg_tokens INTEGER NOT NULL DEFAULT 0").execute(&pool).await;
    let rows = sqlx::query(
        "SELECT id, node_id, model, system_prompt, tail_block,
                context_window, reserve_ratio, max_tokens, safety_margin, input_budget,
                day_window, day_n, n_value, avg_msg_tokens,
                history_json, user_text, ai_reply,
                estimated_tokens, n_used, status, error, created_at
         FROM debug_requests ORDER BY id DESC LIMIT 50"
    ).fetch_all(&pool).await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(DebugRequest {
            id: r.try_get(0).map_err(|e: sqlx::Error| e.to_string())?,
            node_id: r.try_get(1).map_err(|e: sqlx::Error| e.to_string())?,
            model: r.try_get(2).map_err(|e: sqlx::Error| e.to_string())?,
            system_prompt: r.try_get(3).map_err(|e: sqlx::Error| e.to_string())?,
            tail_block: r.try_get(4).map_err(|e: sqlx::Error| e.to_string())?,
            context_window: r.try_get(5).map_err(|e: sqlx::Error| e.to_string())?,
            reserve_ratio: r.try_get(6).map_err(|e: sqlx::Error| e.to_string())?,
            max_tokens: r.try_get(7).map_err(|e: sqlx::Error| e.to_string())?,
            safety_margin: r.try_get(8).map_err(|e: sqlx::Error| e.to_string())?,
            input_budget: r.try_get(9).map_err(|e: sqlx::Error| e.to_string())?,
            day_window: r.try_get(10).map_err(|e: sqlx::Error| e.to_string())?,
            day_n: r.try_get(11).map_err(|e: sqlx::Error| e.to_string())?,
            n_value: r.try_get(12).map_err(|e: sqlx::Error| e.to_string())?,
            avg_msg_tokens: r.try_get(13).map_err(|e: sqlx::Error| e.to_string())?,
            history_json: r.try_get(14).map_err(|e: sqlx::Error| e.to_string())?,
            user_text: r.try_get(15).map_err(|e: sqlx::Error| e.to_string())?,
            ai_reply: r.try_get(16).map_err(|e: sqlx::Error| e.to_string())?,
            estimated_tokens: r.try_get(17).map_err(|e: sqlx::Error| e.to_string())?,
            n_used: r.try_get(18).map_err(|e: sqlx::Error| e.to_string())?,
            status: r.try_get(19).map_err(|e: sqlx::Error| e.to_string())?,
            error: r.try_get(20).map_err(|e: sqlx::Error| e.to_string())?,
            created_at: r.try_get(21).map_err(|e| e.to_string())?,
        });
    }
    Ok(out)
}

#[tauri::command]
async fn clear_debug_requests(storyId: String) -> Result<(), String> {
    let path = story_db::story_db_path(&storyId);
    let opts = sqlx::sqlite::SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
        .map_err(|e| e.to_string())?;
    let pool = sqlx::SqlitePool::connect_with(opts).await.map_err(|e| e.to_string())?;
    let _ = sqlx::query("DELETE FROM debug_requests").execute(&pool).await;
    Ok(())
}


#[tauri::command]
async fn analyze_evolution(storyId: String, nodeId: i64) -> Result<evolution::EvolutionReport, String> {
    evolution::analyze_evolution(&storyId, nodeId).await
}

#[tauri::command]
async fn list_proposals(storyId: String) -> Result<Vec<evolution::Proposal>, String> {
    evolution::list_proposals(&storyId).await
}

/// **测试注入**：不依赖 AI 剧情信号，直接造一条待确认提案（验证拍板流程用）
#[tauri::command]
async fn selftest_inject_proposal(storyId: String, nodeId: Option<i64>) -> Result<String, String> {
    evolution::inject_test_proposal(&storyId, nodeId).await
}

/// ⑤-B：批量落地选中的设定变更（整批打一个快照，可回滚）
#[tauri::command]
async fn confirm_proposals_batch(storyId: String, groupIds: Vec<i64>) -> Result<String, String> {
    evolution::confirm_proposals_batch(&storyId, groupIds).await
}

/// ⑤-B：批量拒绝选中的设定变更
#[tauri::command]
async fn reject_proposals_batch(storyId: String, groupIds: Vec<i64>) -> Result<String, String> {
    evolution::reject_proposals_batch(&storyId, groupIds).await
}

/// ⑤-B：读/写「设定变更是否必须先拍板」开关（默认 true）
#[tauri::command]
async fn get_evolution_require_approval(pool: tauri::State<'_, SqlitePool>) -> Result<bool, String> {
    match db::get_config(pool.inner(), "evolution.require_approval").await {
        Ok(Some(v)) => {
            let v = v.trim().to_ascii_lowercase();
            Ok(!(v == "false" || v == "0" || v == "off" || v == "no"))
        }
        _ => Ok(true),
    }
}

#[tauri::command]
async fn set_evolution_require_approval(
    pool: tauri::State<'_, SqlitePool>,
    on: bool,
) -> Result<(), String> {
    db::set_config(pool.inner(), "evolution.require_approval", if on { "true" } else { "false" })
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn confirm_proposal_group(storyId: String, groupId: i64) -> Result<String, String> {
    evolution::confirm_proposal_group(&storyId, groupId).await
}

#[tauri::command]
async fn reject_proposal_group(storyId: String, groupId: i64) -> Result<String, String> {
    evolution::reject_proposal_group(&storyId, groupId).await
}

#[tauri::command]
async fn list_snapshots(storyId: String) -> Result<Vec<evolution::Snapshot>, String> {
    evolution::list_snapshots(&storyId).await
}

#[tauri::command]
async fn rollback_snapshot(storyId: String, version: i64) -> Result<String, String> {
    evolution::rollback_snapshot(&storyId, version).await
}

/// 第五批 ②：回滚前的差异预览。
/// 回滚是破坏性操作（现存设定会被整表替换），界面必须先拿到差异清单再让用户确认。
#[tauri::command]
async fn preview_snapshot(storyId: String, version: i64) -> Result<evolution::SnapshotDiff, String> {
    evolution::preview_snapshot(&storyId, version).await
}

/// 第五批 ②：把临时设定版本**转为永久**（用户"想留住这个版本"时点）。
#[tauri::command]
async fn keep_snapshot(storyId: String, version: i64) -> Result<String, String> {
    evolution::keep_snapshot(&storyId, version).await
}

/// 第五批 ③：**设定当前分支**（界面切换分支时调用一次）。
///
/// 设计取舍：与其给二十个命令都加 `branchId` 参数（容易漏传、漏传就静默读到主线），
/// 不如"**一个地方设定，其余命令自动沿用**"：
/// 分支存在 `stories.current_branch_id` 里，后端各处通过 `branch::resolve_branch` 读取。
/// 这样新增命令时**默认就是分支正确的**，不依赖调用方记得传参。
#[tauri::command]
async fn set_current_branch(storyId: String, branchId: Option<String>) -> Result<(), String> {
    branch::set_current_branch(&storyId, &branchId).await
}

/// 第五批 ④：分支改名 + 备注（管理平行故事）。
#[tauri::command]
async fn rename_branch(storyId: String, branchId: String, name: String, note: String) -> Result<(), String> {
    branch::rename_branch(&storyId, &branchId, &name, &note).await
}

/// 第五批 ③：删除分支（只删这条分支自己的东西；共享历史/主线/分叉点一律不动）。
///
/// 会被拒绝的情况：**别的分支以它的节点为分叉点**（删了那些分支就断了）。
#[tauri::command]
async fn delete_branch(storyId: String, branchId: String) -> Result<String, String> {
    branch::delete_branch(&storyId, &branchId).await
}

/// 第五批 ③：查询当前分支（`null` = 主线；界面启动时用它恢复上次所在分支）。
#[tauri::command]
async fn get_current_branch(storyId: String) -> Result<Option<String>, String> {
    branch::resolve_branch(&storyId, &None).await
}

/// 第五批 ③：创建故事分支（事务内复制世界状态）。
#[tauri::command]
async fn create_branch(storyId: String, name: String, forkNodeId: i64) -> Result<branch::Branch, String> {
    branch::create_branch(&storyId, &name, forkNodeId).await
}

/// 第五批 ③：列出某故事的全部分支（主线不在此列表）。
#[tauri::command]
async fn list_branches(storyId: String) -> Result<Vec<branch::Branch>, String> {
    branch::list_branches(&storyId).await
}


/// 第五批 ②（B）：直接编辑设定前自动存一个临时版本（界面在保存角色/关系/世界观前调用）。
#[tauri::command]
async fn make_edit_backup(storyId: String, what: String) -> Result<i64, String> {
    evolution::make_edit_backup(&storyId, &what).await
}

/// 第五批 ②：清掉某故事的临时设定版本（界面上的"立即清理"按钮用）。
#[tauri::command]
async fn purge_temporary_snapshots(storyId: String) -> Result<i64, String> {
    evolution::purge_temporary_snapshots(&storyId).await
}

#[tauri::command]
async fn list_audit_logs(storyId: String) -> Result<Vec<evolution::AuditLog>, String> {
    evolution::list_audit_logs(&storyId).await
}

#[tauri::command]
async fn list_event_logs(storyId: String) -> Result<Vec<evolution::EventLogItem>, String> {
    evolution::list_event_logs(&storyId).await
}


/// TXT 导出：按节点范围/模式过滤导出剧情到 exports\story\，返回文件路径。
#[tauri::command]
async fn export_story_txt(storyId: String, nodeIds: Vec<i64>, modeFilter: String, scopeLabel: String) -> Result<String, String> {
    export::export_story_txt(&storyId, &nodeIds, &modeFilter, &scopeLabel).await
}

/// Markdown 导出（对话加工为引用块）。
#[tauri::command]
async fn export_story_md(storyId: String, nodeIds: Vec<i64>, modeFilter: String, scopeLabel: String) -> Result<String, String> {
    export::export_story_md(&storyId, &nodeIds, &modeFilter, &scopeLabel).await
}

/// HTML 导出（浅色阅读主题单文件）。
#[tauri::command]
async fn export_story_html(storyId: String, nodeIds: Vec<i64>, modeFilter: String, scopeLabel: String) -> Result<String, String> {
    export::export_story_html(&storyId, &nodeIds, &modeFilter, &scopeLabel).await
}


/// **无论日志插件是否启用、是否把 stderr 重定向，都能被看见**的输出。
///
/// 为什么需要它：`tauri_plugin_log` 会接管日志，把 `eprintln!` 也写进文件 ——
/// release 版里用户从控制台就**什么都看不到**，只剩"窗口一闪即关"（实测如此）。
/// 这个函数直接写 stderr fd 且**同时**追加到数据目录下的启动日志文件，
/// 保证任何时候都能拿到诊断信息。
fn trace(msg: &str) {
    use std::io::Write;
    // ① 直写 stderr（绕过任何日志库的重定向）
    let line = format!("[dreamweaver] {msg}\n");
    let _ = std::io::stderr().write_all(line.as_bytes());
    let _ = std::io::stderr().flush();
    // ② 同时落盘（用户复制文件给我就行）
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths::logs_root().join("startup-trace.log"))
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// 装一个 panic 钩子：把 panic 信息也写进上面那份日志（release 版默认看不到 panic）。
fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        trace(&format!("PANIC: {info}"));
        prev(info);
    }));
}

/// 启动失败时**必须让用户看见**（依据《现状说明》5.1）。
///
/// 旧行为是 `.expect("初始化 library.db 失败")` —— 在 release 版（日志插件只在 debug 注册）
/// 下，程序会**直接消失**：没有对话框、没有提示、没有日志。
/// 打包后双击（当前目录可能是 `C:\Program Files`，不可写）就正好踩这个。
///
/// 现在：写一份崩溃日志（尽力而为）+ 弹原生对话框说明原因与数据目录 + 返回 Err 让启动中止。
fn fatal_startup(app: &tauri::AppHandle, what: &str, detail: &str) {
    let root = paths::data_root();
    let hint = format!(
        "{what}\n\n原因：{detail}\n\n数据目录：{data_root}\n\n\
         可以把这份数据目录复制到有写权限的位置，然后用环境变量 {env} 指过去：\n\
         （在启动前设置 {env}=D:\\某个有写权限的目录）\n\n日志目录候选的可写状态：\n{cands}",
        data_root = root.display(),
        cands = paths::logs_candidates_report(),
        env = "DREAMWEAVER_DATA_DIR"
    );
    // 崩溃日志：即使对话框弹不出来也留下线索（写不了就算了，不能因为日志再崩一次）。
    // 用 `paths::logs_root()` —— 它是**经过可写性探测的**兜底目录，
    // 直接写 `<数据根>/logs` 在这类环境里必然失败。
    let log_path = paths::logs_root().join("startup-error.log");
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &log_path,
        format!(
            "[{}] {what}\n{detail}\n数据目录：{}\n日志目录：{}\n候选可写状态：\n{}",
            chrono_like_stamp(),
            root.display(),
            paths::logs_root().display(),
            paths::logs_candidates_report()
        ),
    );
    eprintln!("[startup] 崩溃日志已写：{}", log_path.display());
    eprintln!("[startup][FATAL] {what} —— {detail}（数据目录 {root:?}）");
    // 原生对话框（tauri-plugin-dialog 在 setup 里先于数据库初始化注册）
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(hint)
        .title("织梦机 · 启动失败")
        .kind(MessageDialogKind::Error)
        .blocking_show();
}

/// 不引入 chrono：本地时间的可读时间戳（够日志用）
fn chrono_like_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("epoch {secs}")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            install_panic_hook();
            trace("=== setup 开始 ===");
            // ===== 启动第一步：把数据目录建出来 =====
            //
            // 为什么必须最先做：日志插件的**默认目标是 `%LOCALAPPDATA%\<bundle id>\logs`**，
            // 而本机那条路径**拒绝访问**（os error 5）→ 插件初始化失败 → setup 报错 → 进程 panic
            // （exit code 101，用户实测"打不开"）。而且它发生在 `fatal_startup` 之前，
            // 所以连我们的错误对话框都来不及弹 —— 只能靠"先建目录 + 显式指定日志目录"避免。
            for d in [paths::data_root(), paths::logs_root(), paths::temp_root()] {
                if let Err(e) = paths::ensure_dir(&d) {
                    trace(&format!("创建目录失败 {}：{e}", d.display()));
                }
            }
            trace(&format!("数据根 = {}", paths::data_root().display()));
            trace(&format!("日志根 = {}", paths::logs_root().display()));

            // 先把 dialog 插件装上：后面任何失败都能弹对话框告诉用户（而不是无声消失）
            app.handle().plugin(tauri_plugin_dialog::init())?;

            // 日志插件是**可选**的：它装不上（目录不可写）时**绝不能让应用起不来**。
            //
            // 实测事故：用户机器上日志默认目标 `%LOCALAPPDATA%\<bundle id>\logs` 拒绝访问
            // → 插件初始化失败 → setup 报错 → panic（exit code 101），**界面都出不来**。
            // 现在：先探测一个真正可写的目录；仍失败就跳过日志插件，只提示一次。
            if cfg!(debug_assertions) {
                let log_dir = paths::logs_root();   // 已做多级兜底
                // ⚠ 关键：`Builder::default()` 里**预置了两个 target**
                // （`Stdout` + `LogDir`，后者 = `%LOCALAPPDATA%\<bundle id>\logs`），
                // 而 `.target()` 是**追加**不是替换 —— 于是即使我们指定了自己的目录，
                // 插件仍会去建那个写不了的默认目录 → 初始化失败（实测 os error 5）。
                //
                // 正确做法：**先用 `clear_targets()` 清掉预置**，再设我们的目录。
                let mut log_builder = tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .clear_targets();
                log_builder = log_builder.target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Folder {
                        path: log_dir.clone(),
                        file_name: None,
                    },
                ));
                match app.handle().plugin(log_builder.build()) {
                    Ok(_) => trace(&format!("日志插件已启用，目录 = {}", log_dir.display())),
                    Err(e) => {
                        // **不返回错误**：没有日志也要能用
                        trace(&format!("日志插件启用失败（忽略，应用继续）：{e}"));
                        trace(&format!("各候选目录的可写状态：\n{}", paths::logs_candidates_report()));
                    }
                }
            }

            // 第五批「路径模型」修复：数据在"开发期位置"时**先搬过来**（只搬一次，失败不阻断）。
            match paths::migrate_legacy_once() {
                Ok(Some(from)) => eprintln!("[startup] 已把数据从旧位置迁移过来：{}", from.display()),
                Ok(None) => {}
                Err(e) => eprintln!("[startup] 迁移旧数据失败（忽略，继续用新位置）：{e}"),
            }
            eprintln!(
                "[startup] 数据目录 = {}（可用环境变量 DREAMWEAVER_DATA_DIR 覆盖）",
                paths::data_root().display()
            );

            // 初始化 library.db 并注入 State。
            // **失败不再 expect**（那会让程序无声消失）—— 改成弹对话框 + 写日志 + 中止启动。
            trace("开始初始化数据库…");
            let pool = match tauri::async_runtime::block_on(db::init_library_db()) {
                Ok(p) => p,
                Err(e) => {
                    // **优雅退出**：弹对话框说明原因 + 打印数据目录与候选可写状态，
                    // 然后 exit(1)。不再 `return Err` —— 那会让 Tauri 再 panic 一次，
                    // 用户看到的是崩溃而不是"哪里出了问题、怎么解决"。
                    fatal_startup(app.handle(), "无法初始化数据库", &e.to_string());
                    std::process::exit(1);
                }
            };
            app.manage(pool.clone());
            trace("数据库就绪，即将进入事件循环（窗口随后创建）");

            // ===== 安全：清掉配置表里**历史遗留的明文 API Key** =====
            //
            // 早期实现把 Key 明文写进 `configs`（`portrait.cloud_api_key` 等）。
            // 现在 Key 只进系统凭据库，所以升级上来时要把表里那份搬走并删除 ——
            // 否则明文会一直躺在 `library.db` 里，跟着备份/分享一起泄漏。
            //
            // 必须在**任何取 Key 的动作之前**跑完；失败只记日志，绝不阻断启动。
            {
                let lib_pool = pool.clone();
                let (n, notes) = tauri::async_runtime::block_on(keys::scrub_legacy_plaintext(&lib_pool));
                if n > 0 {
                    for line in &notes {
                        trace(&format!("[安全] {line}"));
                    }
                    trace(&format!("[安全] 已清理 {n} 处历史明文 Key"));
                }
            }
            // 第五批 ②：清掉**上一次运行**遗留的"临时设定版本"（回滚前的自动备份）。
            // 这就是"临时版本退出即失效"的实现点 —— 临时版本只在本会话内有意义。
            // 失败只记日志，绝不阻断启动。
            tauri::async_runtime::spawn(async move {
                match evolution::purge_all_temporary_snapshots().await {
                    Ok(n) if n > 0 => eprintln!("[startup] 已清理上次运行遗留的临时设定版本 {n} 个"),
                    Ok(_) => {}
                    Err(e) => eprintln!("[startup] 清理临时设定版本失败（忽略）：{e}"),
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ get_config, set_config,
            keychain::set_api_key, keychain::delete_api_key, main_model_key_status,
            list_stories, create_story, delete_story,
            list_characters, create_character, update_character, delete_character, character_usage,
            list_world_settings, create_world_setting, update_world_setting, delete_world_setting, detect_world_conflicts,
            list_relations, create_relation, update_relation, delete_relation,
            test_api_connection,
            ensure_first_node, list_messages, create_message, create_node_at_day,
            list_timeline_nodes, create_next_day_node, update_message, delete_message, create_interlude_node, delete_timeline_node,
        generate_day_summary, maybe_auto_day_summary, maybe_auto_phase_summary, run_phase_summary, get_phase_min_days, set_phase_min_days, list_phase_rows, delete_phase_row, dedupe_phase_rows, backfill_day_summaries, save_phase_summary, list_phase_summaries,
        check_board_struct, check_board_ai, apply_check_patch,
        auto_analyze_evolution,
        evaluate_reply, list_reply_evaluations,
        generator_list, generator_probe, generator_generate, generator_cleanup_temp, generator_list_temp,
        generator_ingest,
        generator_save_cloud_config, generator_get_cloud_config,
        stamp_ai_label,
            generate_story_reply_stream, list_recall_days,
            list_debug_requests, clear_debug_requests,
            analyze_evolution, list_proposals, confirm_proposal_group, confirm_proposals_batch, selftest_inject_proposal, reject_proposals_batch, get_evolution_require_approval, set_evolution_require_approval, reject_proposal_group,
            cancel_generation,
            create_branch, list_branches, delete_branch, rename_branch, set_current_branch, get_current_branch,
            list_snapshots, rollback_snapshot, preview_snapshot, make_edit_backup, keep_snapshot, purge_temporary_snapshots, list_audit_logs, list_event_logs,
            export_story_txt, export_story_md, export_story_html,
            portraits::portrait_list, portraits::portrait_import, portraits::portrait_create, portraits::portrait_bind,
            portraits::portrait_group_set, portraits::portrait_tag_batch, portraits::portrait_export, portraits::portrait_delete,
            portraits::portrait_update, portraits::portrait_update_params,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}


/// 第五批 ③：**直连真实数据库**验证"设定当前分支 → 读回"（ignored）。
///
/// 为什么单独写：用户实测发现 `stories.current_branch_id` 一直是 NULL ——
/// 说明"界面点分支 → 后端记住"这一环没生效。真实数据库才暴露得出来，
/// 临时库测不出（临时库的 library.db 是进程隔离的）。
/// 跑法：cargo test --lib live_current_branch_write -- --ignored --nocapture
#[cfg(test)]
mod live_branch_state_tests {
    use sqlx::Row;

    #[tokio::test]
    #[ignore]
    async fn live_current_branch_write() {
        let lib = crate::db::library_pool().await.unwrap();
        // 造一个临时故事记录（数字 id，避免与真实故事冲突）
        let sid: i64 = 9_900_002;
        sqlx::query("DELETE FROM stories WHERE id = ?1").bind(sid).execute(&lib).await.unwrap();
        sqlx::query("INSERT INTO stories(id, title, time_origin) VALUES(?1, 'branch-state 测试', '现代')")
            .bind(sid).execute(&lib).await.unwrap();

        let story_key = sid.to_string();
        let fake_branch = "test-branch-uuid-0001".to_string();

        // 1) 初始应为 NULL（主线）
        let initial = crate::branch::resolve_branch(&story_key, &None).await.unwrap();
        println!("[live] 初始 resolve = {:?}", initial);
        assert_eq!(initial, None, "新故事初始应在主线");

        // 2) 设定到分支
        crate::branch::set_current_branch(&story_key, &Some(fake_branch.clone())).await.unwrap();
        let after = crate::branch::resolve_branch(&story_key, &None).await.unwrap();
        println!("[live] set 之后 resolve = {:?}", after);
        assert_eq!(after.as_deref(), Some(fake_branch.as_str()),
            "**set_current_branch 之后必须读得到**（读不到 = 界面切换分支没生效）");

        // 3) 直接用原始 SQL 复核（排除 resolve 自身的问题）
        let raw: Option<String> = sqlx::query("SELECT current_branch_id FROM stories WHERE id = ?1")
            .bind(sid).fetch_one(&lib).await.unwrap().try_get(0).unwrap();
        println!("[live] 原始 SQL 读到的列值 = {:?}", raw);
        assert_eq!(raw.as_deref(), Some(fake_branch.as_str()), "列里必须真的写进去了");

        // 4) 切回主线
        crate::branch::set_current_branch(&story_key, &None).await.unwrap();
        let back = crate::branch::resolve_branch(&story_key, &None).await.unwrap();
        println!("[live] 切回主线后 resolve = {:?}", back);
        assert_eq!(back, None, "切回主线后应读回 NULL");

        sqlx::query("DELETE FROM stories WHERE id = ?1").bind(sid).execute(&lib).await.unwrap();
    }

    /// 第五批 ③：**悬空分支引用要自愈**（用户实测白屏的那个 bug）。
    ///
    /// 场景：`stories.current_branch_id` 指向一条**已被删除**的分支
    /// （跨库引用，库层面没有外键）。界面会拿不存在的分支去过滤 → 白屏/看不到节点。
    /// 这里验证 `resolve_branch` 会把它归主线**并顺手修正记录**。
    #[tokio::test]
    #[ignore]
    async fn live_dangling_branch_self_heals() {
        let lib = crate::db::library_pool().await.unwrap();
        let sid: i64 = 9_900_003;
        sqlx::query("DELETE FROM stories WHERE id = ?1").bind(sid).execute(&lib).await.unwrap();
        sqlx::query("INSERT INTO stories(id, title, time_origin) VALUES(?1, 'dangling 测试', '现代')")
            .bind(sid).execute(&lib).await.unwrap();
        let key = sid.to_string();

        // 指向一条**根本不存在**的分支（模拟"分支被删了但记录还在"）
        sqlx::query("UPDATE stories SET current_branch_id = 'ghost-branch-uuid' WHERE id = ?1")
            .bind(sid).execute(&lib).await.unwrap();

        let got = crate::branch::resolve_branch(&key, &None).await.unwrap();
        println!("[live] 悬空引用 resolve = {got:?}");
        assert_eq!(got, None, "**悬空的分支引用必须归主线**（否则界面会白屏/看不到节点）");

        // 而且记录被**自愈**了（DB 里已经清空）
        let raw: Option<String> = sqlx::query_scalar("SELECT current_branch_id FROM stories WHERE id = ?1")
            .bind(sid).fetch_one(&lib).await.unwrap();
        println!("[live] DB 里的记录 = {raw:?}");
        assert_eq!(raw, None, "悬空引用应被顺手清掉（下次启动不再踩同一个坑）");

        sqlx::query("DELETE FROM stories WHERE id = ?1").bind(sid).execute(&lib).await.unwrap();
    }
}
