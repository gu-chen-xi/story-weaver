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
mod state_extract;
mod evolution;
mod cache;
mod export;
mod portraits;

use sqlx::SqlitePool;
use std::str::FromStr;
use tauri::Manager;

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

#[tauri::command]
async fn create_interlude_node(storyId: String) -> Result<timeline::TimelineNode, String> {
    timeline::create_interlude_node(&storyId).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_timeline_node(storyId: String, nodeId: i64) -> Result<(), String> {
    timeline::delete_timeline_node(&storyId, nodeId).await.map_err(|e| e.to_string())
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
            history_json TEXT NOT NULL, user_text TEXT NOT NULL, ai_reply TEXT,
            estimated_tokens INTEGER NOT NULL DEFAULT 0, n_used INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'ok', error TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )"
    ).execute(&pool).await;
    let rows = sqlx::query(
        "SELECT id, node_id, model, system_prompt, history_json, user_text, ai_reply,
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
            history_json: r.try_get(4).map_err(|e: sqlx::Error| e.to_string())?,
            user_text: r.try_get(5).map_err(|e: sqlx::Error| e.to_string())?,
            ai_reply: r.try_get(6).map_err(|e: sqlx::Error| e.to_string())?,
            estimated_tokens: r.try_get(7).map_err(|e: sqlx::Error| e.to_string())?,
            n_used: r.try_get(8).map_err(|e: sqlx::Error| e.to_string())?,
            status: r.try_get(9).map_err(|e: sqlx::Error| e.to_string())?,
            error: r.try_get(10).map_err(|e: sqlx::Error| e.to_string())?,
            created_at: r.try_get(11).map_err(|e: sqlx::Error| e.to_string())?,
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
            ensure_first_node, list_messages, create_message,
            list_timeline_nodes, create_next_day_node, update_message, create_interlude_node, delete_timeline_node,
            generate_story_reply,
            generate_story_reply_stream,
            list_debug_requests, clear_debug_requests,
            extract_char_states,
            analyze_evolution, list_proposals, confirm_proposal_group, reject_proposal_group,
            list_snapshots, rollback_snapshot, list_audit_logs, list_event_logs,
            list_cache_entries, cache_usage, clear_cache, get_cache_limit, set_cache_limit, auto_cleanup_cache,
            export_story_txt, export_story_md, export_story_html,
            portraits::portrait_list, portraits::portrait_import, portraits::portrait_create, portraits::portrait_bind,
            portraits::portrait_group_set, portraits::portrait_tag_batch, portraits::portrait_export, portraits::portrait_delete,
            portraits::portrait_update, portraits::portrait_update_params,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
