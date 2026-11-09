//! 后端命令登记表（2026-11-07，安全审计 F-05 的**主窗口那一半**）。
//!
//! 为什么需要这张表：Tauri 没有「按命令」的 ACL（capabilities 只管插件权限），
//! `generate_handler!` 里列了 160 条命令 → **主窗口的 JS 全都调得到**。这没法从框架层收窄，
//! 但可以做到两件实事：
//!   ① **不许悄悄多出一条** —— 登记表必须和 `lib.rs` 的 handler 列表**逐条一致**（测试守着）；
//!   ② **敏感命令显式归类** —— `delete_* / rollback_* / purge_* / clear_* / undo_* …` 必须标成
//!      `Sensitive`，以后想给它们加「原生二次确认」时，名单是现成的。
//!
//! 维护方式：新增命令 → 在 `COMMANDS` 里补一行 → 测试会告诉你漏没漏。

/// 命令的「能力域」。分类只为管理，不改变行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// 只读（list_/get_/preview/scan/status…）
    Read,
    /// 写（create_/update_/set_/apply_/import_/bind_…）
    Write,
    /// **会丢数据**（delete_/rollback_/purge_/clear_/undo_/reject_…）—— 将来要加原生确认的就是这批
    Sensitive,
    /// 凭据相关（keychain_/api_key）
    Credential,
    /// 开发者工具专用（dev::*）
    Dev,
}

pub struct Cmd {
    pub name: &'static str,
    pub domain: Domain,
}

/// 全部已注册命令。**必须与 `lib.rs` 的 `generate_handler!` 一一对应**（见文件末尾的测试）。
pub const COMMANDS: &[Cmd] = &[
    Cmd { name: "dev::dev_export_text", domain: Domain::Dev },
    Cmd { name: "dev::dev_export_diagnostics", domain: Domain::Dev },
    Cmd { name: "dev::dev_seed_sample", domain: Domain::Dev },
    Cmd { name: "dev::dev_health_check", domain: Domain::Dev },
    Cmd { name: "dev::dev_inject_summary", domain: Domain::Dev },
    Cmd { name: "dev::dev_inject_proposal", domain: Domain::Dev },
    Cmd { name: "dev::dev_rollback", domain: Domain::Dev },
    Cmd { name: "dev::dev_snapshots", domain: Domain::Dev },
    Cmd { name: "dev::dev_status", domain: Domain::Dev },
    Cmd { name: "dev::dev_create_test_story", domain: Domain::Dev },
    Cmd { name: "update_story_prologue", domain: Domain::Write },
    Cmd { name: "get_config", domain: Domain::Read },
    Cmd { name: "set_config", domain: Domain::Write },
    Cmd { name: "keychain::set_api_key", domain: Domain::Credential },
    Cmd { name: "keychain::delete_api_key", domain: Domain::Credential },
    Cmd { name: "main_model_key_status", domain: Domain::Credential },
    Cmd { name: "list_stories", domain: Domain::Read },
    Cmd { name: "create_story", domain: Domain::Write },
    Cmd { name: "delete_story", domain: Domain::Sensitive },
    Cmd { name: "list_characters", domain: Domain::Read },
    Cmd { name: "create_character", domain: Domain::Write },
    Cmd { name: "update_character", domain: Domain::Write },
    Cmd { name: "delete_character", domain: Domain::Sensitive },
    Cmd { name: "character_usage", domain: Domain::Write },
    Cmd { name: "list_world_settings", domain: Domain::Read },
    Cmd { name: "create_world_setting", domain: Domain::Write },
    Cmd { name: "update_world_setting", domain: Domain::Write },
    Cmd { name: "delete_world_setting", domain: Domain::Sensitive },
    Cmd { name: "detect_world_conflicts", domain: Domain::Write },
    Cmd { name: "list_relations", domain: Domain::Read },
    Cmd { name: "create_relation", domain: Domain::Write },
    Cmd { name: "update_relation", domain: Domain::Write },
    Cmd { name: "delete_relation", domain: Domain::Sensitive },
    Cmd { name: "test_api_connection", domain: Domain::Write },
    Cmd { name: "ensure_first_node", domain: Domain::Write },
    Cmd { name: "list_messages", domain: Domain::Read },
    Cmd { name: "create_message", domain: Domain::Write },
    Cmd { name: "create_node_at_day", domain: Domain::Write },
    Cmd { name: "list_timeline_nodes", domain: Domain::Read },
    Cmd { name: "create_next_day_node", domain: Domain::Write },
    Cmd { name: "update_message", domain: Domain::Write },
    Cmd { name: "delete_message", domain: Domain::Sensitive },
    Cmd { name: "create_interlude_node", domain: Domain::Write },
    Cmd { name: "delete_timeline_node", domain: Domain::Sensitive },
    Cmd { name: "generate_day_summary", domain: Domain::Write },
    Cmd { name: "close_day", domain: Domain::Write },
    Cmd { name: "maybe_auto_day_summary", domain: Domain::Write },
    Cmd { name: "maybe_auto_phase_summary", domain: Domain::Write },
    Cmd { name: "run_phase_summary", domain: Domain::Write },
    Cmd { name: "get_phase_min_days", domain: Domain::Read },
    Cmd { name: "phase_plan_stats", domain: Domain::Read },
    Cmd { name: "list_phase_rows", domain: Domain::Read },
    Cmd { name: "delete_phase_row", domain: Domain::Sensitive },
    Cmd { name: "get_build_info", domain: Domain::Read },
    Cmd { name: "data_overview", domain: Domain::Write },
    Cmd { name: "recall_explain", domain: Domain::Write },
    Cmd { name: "recall_intent", domain: Domain::Write },
    Cmd { name: "recall_get_weights", domain: Domain::Write },
    Cmd { name: "recall_set_weights", domain: Domain::Write },
    Cmd { name: "data_debug_max", domain: Domain::Write },
    Cmd { name: "data_set_debug_max", domain: Domain::Write },
    Cmd { name: "data_clean_debug", domain: Domain::Write },
    Cmd { name: "data_clean_logs", domain: Domain::Write },
    Cmd { name: "data_clean_orphans", domain: Domain::Write },
    Cmd { name: "data_clean_backups", domain: Domain::Write },
    Cmd { name: "dedupe_phase_rows", domain: Domain::Write },
    Cmd { name: "backfill_day_summaries", domain: Domain::Write },
    Cmd { name: "check_board_struct", domain: Domain::Read },
    Cmd { name: "check_board_ai", domain: Domain::Read },
    Cmd { name: "apply_check_patch", domain: Domain::Write },
    Cmd { name: "auto_analyze_evolution", domain: Domain::Write },
    Cmd { name: "memory_list", domain: Domain::Read },
    Cmd { name: "list_director_costs", domain: Domain::Read },
    Cmd { name: "delete_director_cost", domain: Domain::Sensitive },
    Cmd { name: "list_prompt_snapshots", domain: Domain::Read },
    Cmd { name: "get_prompt_snapshot", domain: Domain::Read },
    Cmd { name: "ollama_get", domain: Domain::Write },
    Cmd { name: "ollama_set", domain: Domain::Write },
    Cmd { name: "ollama_probe", domain: Domain::Write },
    Cmd { name: "ollama_test_chat", domain: Domain::Write },
    Cmd { name: "scan_foreshadow_local", domain: Domain::Read },
    Cmd { name: "scan_foreshadow_cloud", domain: Domain::Read },
    Cmd { name: "request_pardon", domain: Domain::Write },
    Cmd { name: "apply_pardon", domain: Domain::Write },
    Cmd { name: "list_pardon_requests", domain: Domain::Read },
    Cmd { name: "list_relation_changes", domain: Domain::Read },
    Cmd { name: "undo_relation_change", domain: Domain::Sensitive },
    Cmd { name: "list_stat_changes", domain: Domain::Read },
    Cmd { name: "undo_stat_change", domain: Domain::Sensitive },
    Cmd { name: "list_chapters", domain: Domain::Read },
    Cmd { name: "create_chapter", domain: Domain::Write },
    Cmd { name: "update_chapter", domain: Domain::Write },
    Cmd { name: "set_chapter_nodes", domain: Domain::Write },
    Cmd { name: "delete_chapter", domain: Domain::Sensitive },
    Cmd { name: "list_unassigned_days", domain: Domain::Read },
    Cmd { name: "list_foreshadows", domain: Domain::Read },
    Cmd { name: "create_foreshadow", domain: Domain::Write },
    Cmd { name: "update_foreshadow", domain: Domain::Write },
    Cmd { name: "delete_foreshadow", domain: Domain::Sensitive },
    Cmd { name: "scan_foreshadow_proposals", domain: Domain::Read },
    Cmd { name: "export_chapters", domain: Domain::Read },
    Cmd { name: "memory_update", domain: Domain::Write },
    Cmd { name: "memory_promise_state", domain: Domain::Write },
    Cmd { name: "memory_delete", domain: Domain::Write },
    Cmd { name: "memory_export", domain: Domain::Write },
    Cmd { name: "perform_lexicon", domain: Domain::Write },
    Cmd { name: "perform_latest", domain: Domain::Read },
    Cmd { name: "generator_list", domain: Domain::Write },
    Cmd { name: "generator_probe", domain: Domain::Write },
    Cmd { name: "generator_generate", domain: Domain::Write },
    Cmd { name: "generator_cleanup_temp", domain: Domain::Write },
    Cmd { name: "generator_list_temp", domain: Domain::Write },
    Cmd { name: "generator_ingest", domain: Domain::Write },
    Cmd { name: "generator_save_cloud_config", domain: Domain::Write },
    Cmd { name: "generator_get_cloud_config", domain: Domain::Write },
    Cmd { name: "generator_clear_cloud_key", domain: Domain::Write },
    Cmd { name: "stamp_ai_label", domain: Domain::Write },
    Cmd { name: "generate_story_reply_stream", domain: Domain::Write },
    Cmd { name: "list_recall_days", domain: Domain::Read },
    Cmd { name: "list_debug_requests", domain: Domain::Read },
    Cmd { name: "clear_debug_requests", domain: Domain::Sensitive },
    Cmd { name: "analyze_evolution", domain: Domain::Write },
    Cmd { name: "list_proposals", domain: Domain::Read },
    Cmd { name: "confirm_proposal_group", domain: Domain::Write },
    Cmd { name: "confirm_proposals_batch", domain: Domain::Write },
    Cmd { name: "selftest_inject_proposal", domain: Domain::Read },
    Cmd { name: "reject_proposals_batch", domain: Domain::Sensitive },
    Cmd { name: "get_evolution_require_approval", domain: Domain::Read },
    Cmd { name: "set_evolution_require_approval", domain: Domain::Write },
    Cmd { name: "reject_proposal_group", domain: Domain::Sensitive },
    Cmd { name: "cancel_generation", domain: Domain::Write },
    Cmd { name: "create_branch", domain: Domain::Write },
    Cmd { name: "list_branches", domain: Domain::Read },
    Cmd { name: "delete_branch", domain: Domain::Sensitive },
    Cmd { name: "rename_branch", domain: Domain::Write },
    Cmd { name: "set_current_branch", domain: Domain::Write },
    Cmd { name: "get_current_branch", domain: Domain::Read },
    Cmd { name: "list_snapshots", domain: Domain::Read },
    Cmd { name: "rollback_snapshot", domain: Domain::Sensitive },
    Cmd { name: "preview_snapshot", domain: Domain::Read },
    Cmd { name: "make_edit_backup", domain: Domain::Write },
    Cmd { name: "keep_snapshot", domain: Domain::Write },
    Cmd { name: "purge_temporary_snapshots", domain: Domain::Sensitive },
    Cmd { name: "list_audit_logs", domain: Domain::Read },
    Cmd { name: "list_event_logs", domain: Domain::Read },
    Cmd { name: "export_story_txt", domain: Domain::Read },
    Cmd { name: "export_story_md", domain: Domain::Read },
    Cmd { name: "export_story_html", domain: Domain::Read },
    Cmd { name: "portraits::portrait_list", domain: Domain::Write },
    Cmd { name: "portraits::portrait_import", domain: Domain::Write },
    Cmd { name: "portraits::portrait_create", domain: Domain::Write },
    Cmd { name: "portraits::portrait_bind", domain: Domain::Write },
    Cmd { name: "portraits::portrait_group_set", domain: Domain::Write },
    Cmd { name: "portraits::portrait_group_remove", domain: Domain::Write },
    Cmd { name: "portraits::portrait_tag_batch", domain: Domain::Write },
    Cmd { name: "portraits::portrait_tag_set", domain: Domain::Write },
    Cmd { name: "portraits::portrait_export", domain: Domain::Write },
    Cmd { name: "portraits::portrait_delete", domain: Domain::Write },
    Cmd { name: "portraits::portrait_update_params", domain: Domain::Write },
];

pub fn domain_of(name: &str) -> Option<Domain> {
    COMMANDS.iter().find(|c| c.name == name).map(|c| c.domain)
}

pub fn is_sensitive(name: &str) -> bool {
    domain_of(name) == Some(Domain::Sensitive)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 从 `lib.rs` 里抠出 `generate_handler![...]` 的条目（不引第三方解析器，够用）。
    fn registered_names() -> Vec<String> {
        let src = include_str!("lib.rs");
        let start = src.find("tauri::generate_handler![").expect("找不到 handler 列表");
        let rest = &src[start + "tauri::generate_handler![".len()..];
        let end = rest.find("])").expect("handler 列表没有收尾");
        rest[..end]
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|t| !t.is_empty())
            .filter(|t| {
                let mut chars = t.chars();
                matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                    && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
            })
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn registry_matches_registered_commands() {
        let mut reg: Vec<String> = COMMANDS.iter().map(|c| c.name.to_string()).collect();
        let mut actual = registered_names();
        reg.sort();
        reg.dedup();
        actual.sort();
        let missing: Vec<&String> = actual.iter().filter(|n| !reg.contains(n)).collect();
        let extra: Vec<&String> = reg.iter().filter(|n| !actual.contains(n)).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "命令登记表和 handler 不一致——\n  漏登记：{missing:?}\n  登记了但没注册：{extra:?}"
        );
    }

    #[test]
    fn destructive_commands_are_sensitive() {
        let bad: Vec<&str> = COMMANDS
            .iter()
            .filter(|c| {
                let base = c.name.rsplit("::").next().unwrap_or(c.name);
                let looks_destructive = ["delete_", "remove_", "purge_", "clear_", "rollback_", "undo_"]
                    .iter()
                    .any(|p| base.starts_with(p));
                // 删 Key 也算敏感，但它归 Credential 域（删的是凭据，不是剧情数据）
                looks_destructive && !matches!(c.domain, Domain::Sensitive | Domain::Credential)
            })
            .map(|c| c.name)
            .collect();
        assert!(bad.is_empty(), "这些命令会丢数据却没标 Sensitive：{bad:?}");
    }

    #[test]
    fn credential_commands_are_marked() {
        for c in COMMANDS {
            let base = c.name.rsplit("::").next().unwrap_or(c.name);
            if base.contains("api_key") || base.starts_with("keychain") {
                assert_eq!(c.domain, Domain::Credential, "{} 应该标 Credential", c.name);
            }
        }
    }
    /// 2026-11-07（C 方案）：这四个**丢了就真没了**的命令必须弹**原生**确认。
    /// 网页里的确认挡不住注入的脚本（它能自己调一遍 confirm），系统对话框才行。
    #[test]
    fn catastrophic_commands_ask_natively() {
        let src = include_str!("lib.rs");
        for name in ["delete_story", "rollback_snapshot", "purge_temporary_snapshots", "delete_branch"] {
            let needle = format!("async fn {name}(");
            let i = src.find(&needle).unwrap_or_else(|| panic!("找不到命令 {name}"));
            let body = &src[i..(i + 900).min(src.len())];
            assert!(
                body.contains("confirm_destructive"),
                "{name} 没有弹原生确认——这条命令是不可逆的，删掉确认等于把 C 方案拆了"
            );
        }
    }

}
