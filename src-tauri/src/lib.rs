mod db;
mod keychain;
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
mod cache;
mod export;
mod portraits;
mod generator;
mod generator_ingest;
mod generator_cloud;
mod image_stamp;

use sqlx::SqlitePool;
use std::str::FromStr;
use tauri::Manager;

/// 第四批 ⑥ P1：把生成结果入库（**唯一同时知道生成与素材库的地方**，内部走 portrait_import）
#[tauri::command]
async fn generator_ingest(req: generator_ingest::IngestRequest) -> Result<portraits::PortraitItem, String> {
    generator_ingest::ingest(&req).await
}

/// 第四批 ⑥ P1：来源角标文案（素材库显示"AI·本地 / AI·云端 / 手工 / 占位"）
#[tauri::command]
fn generator_source_label(source: String) -> String {
    generator_ingest::source_label(&source).to_string()
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
    // 配置表没存 Key 时，回落到系统凭据库（与主模型 Key 分开存，互不影响）
    if cache.get("portrait.cloud_api_key").map(|s| s.trim().is_empty()).unwrap_or(true) {
        if let Ok(Some(k)) = keychain::get_api_key("portrait_api_key".to_string()) {
            cache.insert("portrait.cloud_api_key".to_string(), k);
        }
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
    let pairs: [(&str, Option<String>); 8] = [
        ("portrait.cloud_url", config.base_url),
        ("portrait.cloud_model", config.model),
        ("portrait.cloud_request_url", config.url),
        ("portrait.cloud_body_template", config.body_template),
        ("portrait.cloud_response_path", config.response_path),
        ("portrait.cloud_auth_style", config.auth_style),
        ("portrait.cloud_extra_headers", config.extra_headers),
        ("portrait.cloud_api_key", config.api_key),
    ];
    for (k, v) in pairs {
        if let Some(val) = v {
            db::set_config(&lib_pool, k, &val).await.map_err(|e| e.to_string())?;
            saved += 1;
        }
    }
    // 用户明确提供的 Key 同时写入系统凭据库（更安全；配置表留空也能用）
    let key_to_store = apiKey.unwrap_or_default();
    if !key_to_store.trim().is_empty() {
        let _ = keychain::set_api_key("portrait_api_key".to_string(), key_to_store);
    }
    Ok(format!("已保存 {saved} 项云端配置"))
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

/// 第四批 ⑥ P1：清理生成临时目录（保留 keep 里的文件）
#[tauri::command]
fn generator_cleanup_temp(keep: Vec<String>) -> usize {
    generator::cleanup_temp(&keep)
}

#[tauri::command]
async fn db_health(pool: tauri::State<'_, SqlitePool>) -> Result<String, String> {
    let pool = pool.inner().clone();
    let (path, n) = db::health(&pool).await.map_err(|e| e.to_string())?;
    Ok(format!("{} (configs 行数={})", path, n))
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
async fn all_configs(pool: tauri::State<'_, SqlitePool>) -> Result<Vec<(String, String)>, String> {
    let pool = pool.inner().clone();
    db::all_configs(&pool).await.map_err(|e| e.to_string())
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
async fn list_timeline_nodes(storyId: String) -> Result<Vec<timeline::TimelineNode>, String> {
    timeline::list_timeline_nodes(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_next_day_node(storyId: String) -> Result<timeline::TimelineNode, String> {
    timeline::create_next_day_node(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_message(storyId: String, messageId: i64, content: String) -> Result<(), String> {
    timeline::update_message(&storyId, messageId, &content).await.map_err(|e| e.to_string())
}

/// C 子项（时间流逝）：在**指定天数**插入 day 节点（用户点日历/时间线上的"时间流逝"区间条时调用）
#[tauri::command]
async fn create_node_at_day(storyId: String, dayNo: i64) -> Result<timeline::TimelineNode, String> {
    timeline::create_node_at_day(&storyId, dayNo).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_interlude_node(storyId: String) -> Result<timeline::TimelineNode, String> {
    timeline::create_interlude_node(&storyId).await.map_err(|e| e.to_string())
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

/// 阶段摘要（C 层接口预留）：写入一段覆盖 [dayFrom, dayTo] 的摘要
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

/// 阶段摘要列表（day_from 倒序）
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

#[tauri::command]
async fn generate_story_reply(
    pool: tauri::State<'_, SqlitePool>,
    storyId: String,
    nodeId: i64,
    userText: String,
) -> Result<String, String> {
    let lib_pool = pool.inner().clone();
    generate::generate_reply(&lib_pool, &storyId, nodeId, &userText).await
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
) -> Result<(), String> {
    let lib_pool = pool.inner().clone();
    generate_stream::generate_reply_streaming(app, &lib_pool, &storyId, nodeId, &userText, &mode, &myRole, &aiRole, &focusRole, &tags, &constraints, &lengthLevel, memory).await
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
async fn extract_char_states(storyId: String, nodeId: i64) -> Result<Vec<state_extract::ExtractedState>, String> {
    state_extract::update_states_from_node(&storyId, nodeId).await
}

#[tauri::command]
async fn analyze_evolution(storyId: String, nodeId: i64) -> Result<evolution::EvolutionReport, String> {
    evolution::analyze_evolution(&storyId, nodeId).await
}

#[tauri::command]
async fn list_proposals(storyId: String) -> Result<Vec<evolution::Proposal>, String> {
    evolution::list_proposals(&storyId).await
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

#[tauri::command]
async fn list_audit_logs(storyId: String) -> Result<Vec<evolution::AuditLog>, String> {
    evolution::list_audit_logs(&storyId).await
}

#[tauri::command]
async fn list_event_logs(storyId: String) -> Result<Vec<evolution::EventLogItem>, String> {
    evolution::list_event_logs(&storyId).await
}

#[tauri::command]
async fn list_cache_entries(storyId: String) -> Result<Vec<cache::CacheEntry>, String> {
    cache::list_cache_entries(&storyId).await
}

#[tauri::command]
async fn cache_usage(storyId: String) -> Result<i64, String> {
    cache::cache_usage(&storyId).await
}

#[tauri::command]
async fn clear_cache(storyId: String) -> Result<String, String> {
    cache::clear_cache(&storyId).await
}

#[tauri::command]
async fn get_cache_limit() -> Result<i64, String> {
    cache::get_cache_limit().await
}

#[tauri::command]
async fn set_cache_limit(gb: i64) -> Result<String, String> {
    cache::set_cache_limit(gb).await
}

#[tauri::command]
async fn auto_cleanup_cache(storyId: String) -> Result<String, String> {
    cache::auto_cleanup_cache(&storyId).await
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

/// M0 自测：建一个 __selftest__/story.db，返回路径与表数量。
#[tauri::command]
async fn selftest_init_story_db() -> Result<String, String> {
    let story_id = "__selftest__";
    let _pool = story_db::init_story_db(story_id).await.map_err(|e| e.to_string())?;
    let (path, n, names) = story_db::story_db_tables(story_id).await.map_err(|e| e.to_string())?;
    Ok(format!("{} ({} 表): {}", path, n, names.join(", ")))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            app.handle().plugin(tauri_plugin_dialog::init())?;
            // 初始化 library.db 并注入 State
            let pool = tauri::async_runtime::block_on(db::init_library_db())
                .expect("初始化 library.db 失败");
            app.manage(pool);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            db_health, get_config, set_config, all_configs,
            keychain::set_api_key, keychain::get_api_key, keychain::delete_api_key,
            selftest_init_story_db,
            list_stories, create_story, delete_story,
            list_characters, create_character, update_character, delete_character, character_usage,
            list_world_settings, create_world_setting, update_world_setting, delete_world_setting, detect_world_conflicts,
            list_relations, create_relation, update_relation, delete_relation,
            test_api_connection,
            ensure_first_node, list_messages, create_message, create_node_at_day,
            list_timeline_nodes, create_next_day_node, update_message, create_interlude_node, delete_timeline_node,
        generate_day_summary, maybe_auto_day_summary, backfill_day_summaries, save_phase_summary, list_phase_summaries,
        check_board_struct, check_board_ai, apply_check_patch,
        auto_analyze_evolution,
        evaluate_reply, list_reply_evaluations,
        generator_list, generator_probe, generator_generate, generator_cleanup_temp,
        generator_ingest, generator_source_label,
        generator_save_cloud_config, generator_get_cloud_config,
        stamp_ai_label,
            generate_story_reply,
            generate_story_reply_stream,
            list_debug_requests, clear_debug_requests,
            extract_char_states,
            analyze_evolution, list_proposals, confirm_proposal_group, reject_proposal_group,
            list_snapshots, rollback_snapshot, preview_snapshot, list_audit_logs, list_event_logs,
            list_cache_entries, cache_usage, clear_cache, get_cache_limit, set_cache_limit, auto_cleanup_cache,
            export_story_txt, export_story_md, export_story_html,
            portraits::portrait_list, portraits::portrait_import, portraits::portrait_create, portraits::portrait_bind,
            portraits::portrait_group_set, portraits::portrait_tag_batch, portraits::portrait_export, portraits::portrait_delete,
            portraits::portrait_update, portraits::portrait_update_params,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
