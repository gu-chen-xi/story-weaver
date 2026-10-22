// 从 `evolution.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `evolution::diag_tests` —— 因为 `evolution.rs` + `evolution/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `evolution`，一行都不用改写。

use super::*;

#[tokio::test]
#[ignore = "诊断用：需要 DW_DIAG_DB 指向真实 story.db；会临时改 DREAMWEAVER_DATA_DIR"]
async fn diag_analyze_real_db() {
    let Ok(src) = std::env::var("DW_DIAG_DB") else {
        eprintln!("没设 DW_DIAG_DB，跳过");
        return;
    };
    // 造一个临时数据根，把副本放到"真 id"的位置，
    // 这样 open_story_pool(story_id) 会命中副本而不是用户的库。
    let root = crate::paths::test_root().join("diag-data");
    let _ = std::fs::remove_dir_all(&root);
    let proj = root.join("library").join("default").join("projects").join("7");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::copy(&src, proj.join("story.db")).expect("复制库失败");
    std::env::set_var("DREAMWEAVER_DATA_DIR", &root);

    let story_id = "7";
    let pool = crate::story_db::open_story_pool(story_id).await.unwrap();

    let nodes: Vec<(i64, String, i64)> = sqlx::query_as(
        "SELECT n.id, n.date_label, (SELECT COUNT(*) FROM messages m WHERE m.node_id = n.id)
         FROM timeline_nodes n ORDER BY n.id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    eprintln!("=== 节点清单（{} 个）===", nodes.len());
    for (id, label, n) in &nodes {
        eprintln!("  节点 #{id} {label}：{n} 条消息");
    }

    eprintln!("\n=== 逐节点分析（走完整公开 API）===");
    for (id, label, n) in &nodes {
        if *n == 0 {
            continue;
        }
        match analyze_evolution(story_id, *id).await {
            Ok(rep) => eprintln!(
                "  节点 #{id} {label}: 事件 {:?} | 待确认 {} | 已应用 {} | notes {:?}",
                rep.events.iter().map(|e| e.event_type.as_str()).collect::<Vec<_>>(),
                rep.proposals.iter().filter(|p| p.status == "pending").count(),
                rep.applied_groups.len(),
                rep.notes
            ),
            Err(e) => eprintln!("  节点 #{id} {label}: ❌ {e}"),
        }
    }

    let cnt: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM change_proposals")
        .fetch_one(&pool).await.unwrap();
    eprintln!("\n=== 分析后 change_proposals 共 {cnt} 条 ===");
    let rows: Vec<(i64, i64, String, String, Option<String>, Option<String>, String, Option<i64>)> =
        sqlx::query_as(
            "SELECT id, COALESCE(group_id, id), target_type, field, old_value, new_value, status, node_id
             FROM change_proposals ORDER BY id DESC LIMIT 20",
        )
        .fetch_all(&pool).await.unwrap();
    for r in &rows {
        eprintln!(
            "  组{}/条{} {} {} {} → {} [{}] node={:?}",
            r.1, r.0, r.2, r.3,
            r.4.clone().unwrap_or_default(), r.5.clone().unwrap_or_default(), r.6, r.7
        );
    }

    // ── 时间线：同一天不能重复创建（后端守卫）──
    eprintln!("\n=== 时间线：重复创建同一天 ===");
    match crate::timeline::create_node_at_day(story_id, 3).await {
        Ok(n) => eprintln!("  ❌ 竟然创建成功了：{}（应当被拒绝）", n.date_label),
        Err(e) => eprintln!("  ✅ 被拒绝：{e}"),
    }
    match crate::timeline::create_node_at_day(story_id, 99).await {
        Ok(n) => eprintln!("  ✅ 新的一天可以创建：{}", n.date_label),
        Err(e) => eprintln!("  ❌ 新的一天失败了：{e}"),
    }

    // ── RAG ①：回顾检索在真实数据上的效果 ──
    eprintln!("\n=== RAG 回顾检索（真实数据）===");
    let days = crate::recall::list_days(&pool, &None).await.unwrap_or_default();
    eprintln!("  可回顾的天：{} 个", days.len());
    for d in days.iter().take(6) {
        eprintln!("    {}（节点#{}）{}…", d.date_label, d.node_id, d.preview.chars().take(28).collect::<String>());
    }
    let samples = [
        "你还记得我们第一次牵手的时候吗",
        "第 8 天那次争吵是怎么回事",
        "当时在院子里你说过什么",
        "今天天气不错",                    // 反例：不该触发
    ];
    for s in samples {
        let intent = crate::recall::detect_intent(s);
        let mut hits = Vec::new();
        if intent.is_recall {
            let exclude = std::collections::HashSet::new();
            hits = crate::recall::search(&pool, &None, 0, &exclude, &intent, None, 3, 1600, &crate::recall::RecallWeights::default())
                .await.unwrap_or_default();
        }
        eprintln!(
            "  「{s}」→ 意图={} 线索={:?} 事件={:?} 命中 {} 条",
            intent.is_recall, intent.clues, intent.events, hits.len()
        );
        for h in &hits {
            eprintln!("      {} 分{} 因为{:?}", h.date_label, h.score, h.why);
        }
    }
    // 强制指定某一天
    if let Some(d) = days.first() {
        let exclude = std::collections::HashSet::new();
        let forced = crate::recall::search(&pool, &None, 0, &exclude, &crate::recall::Intent::default(), Some(d.node_id), 1, 1600, &crate::recall::RecallWeights::default()).await.unwrap_or_default();
        eprintln!("\n  强制回顾「{}」→ 命中 {} 条（含原文 {} 条消息）", d.date_label, forced.len(), forced.iter().map(|h| h.messages.len()).sum::<usize>());
        eprintln!("{}", crate::recall::render_block(&forced));
    }

    // ── 回滚到"老格式"快照（payload 里混着分支的行）──
    // 这正是用户点「回滚到此版本」时报 1555 的场景。
    eprintln!("\n=== 回滚到最新版本（老格式，含分支行）===");
    let latest: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(version) FROM setting_snapshots WHERE temporary = 0",
    ).fetch_one(&pool).await.unwrap();
    if let Some(v) = latest {
        let before_chars: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters")
            .fetch_one(&pool).await.unwrap();
        match rollback_snapshot(story_id, v).await {
            Ok(m) => eprintln!("  ✅ 回滚到 V{v}：{m}"),
            Err(e) => eprintln!("  ❌ 回滚到 V{v} 失败：{e}"),
        }
        let after_chars: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters")
            .fetch_one(&pool).await.unwrap();
        let main_c: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE branch_id IS NULL")
            .fetch_one(&pool).await.unwrap();
        let br_c: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM characters WHERE branch_id IS NOT NULL")
            .fetch_one(&pool).await.unwrap();
        eprintln!("  角色总数 {before_chars} → {after_chars}（主线 {main_c} + 分支 {br_c}）");
    }

    // ── 测试注入 → 批量落地 的确定性验证 ──
    eprintln!("\n=== 注入测试提案 ===");
    match inject_test_proposal(story_id, Some(1)).await {
        Ok(m) => eprintln!("  ✅ {m}"),
        Err(e) => eprintln!("  ❌ {e}"),
    }
    let pending: Vec<(i64, String, Option<i64>)> = sqlx::query_as(
        "SELECT COALESCE(group_id, id), status, node_id FROM change_proposals
         WHERE evidence LIKE '%测试注入%' ORDER BY id DESC",
    )
    .fetch_all(&pool).await.unwrap();
    eprintln!("  注入后（共 {} 条）：{:?}", pending.len(), pending);

    if let Some((gid, _, _)) = pending.first() {
        let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM setting_snapshots")
            .fetch_one(&pool).await.unwrap();
        match confirm_proposals_batch(story_id, vec![*gid]).await {
            Ok(m) => eprintln!("  ✅ 落地：{m}"),
            Err(e) => eprintln!("  ❌ 落地：{e}"),
        }
        let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM setting_snapshots")
            .fetch_one(&pool).await.unwrap();
        eprintln!("  快照数 {before} → {after}（应 +1）");
        let st: Vec<String> = sqlx::query_scalar(
            "SELECT status FROM change_proposals WHERE group_id = ?1",
        ).bind(gid).fetch_all(&pool).await.unwrap();
        eprintln!("  该组状态：{:?}", st);
    }

    let _ = std::fs::remove_dir_all(&root);
}
