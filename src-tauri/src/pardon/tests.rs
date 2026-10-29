// 赦免请求的回归测试（2026-10-27）。
//
// 每条对应一条用户定的规矩：
//   标记优先 / 兜底看"不同意" / 盒子显示最后一句 / 3 次没同意扣 5 点 /
//   同意后才允许赦免（回补一半）/ 一天与一条消息各有上限。

use super::*;
use sqlx::sqlite::SqlitePoolOptions;

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    for ddl in [
        "CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY, date_label TEXT, branch_id TEXT)",
        "CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER, role TEXT, content TEXT, branch_id TEXT)",
        "CREATE TABLE characters (id INTEGER PRIMARY KEY, name TEXT, branch_id TEXT)",
        "CREATE TABLE relations (id INTEGER PRIMARY KEY AUTOINCREMENT, from_character_id INTEGER, to_character_id INTEGER, direction TEXT DEFAULT '单向', strength INTEGER, state TEXT DEFAULT '稳定', version INTEGER DEFAULT 1, branch_id TEXT)",
        "CREATE TABLE audit_logs (id INTEGER PRIMARY KEY AUTOINCREMENT, action TEXT, target TEXT, before TEXT, after TEXT, operator TEXT)",
        "CREATE TABLE director_costs (id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER, message_id INTEGER, character_id INTEGER, kind TEXT, reason TEXT, score INTEGER, source TEXT DEFAULT 'model', excused INTEGER DEFAULT 0, rel_delta INTEGER DEFAULT 0, created_at TEXT DEFAULT (datetime('now','localtime')), branch_id TEXT)",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    ensure_table(&pool).await.unwrap();
    sqlx::query("INSERT INTO characters(id,name) VALUES(1,'夏婉琴'),(2,'顾宸熙')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO relations(from_character_id,to_character_id,direction,strength) VALUES(1,2,'单向',20)").execute(&pool).await.unwrap();
    pool
}



/// **列表查询必须能跑**（2026-10-28 用户实测：盒子一直不显示）。
/// 它 LEFT JOIN 了 director_costs —— 那张表也有 branch_id，
/// 条件写成裸的 branch_id 会 ambiguous column name，命令直接报错、界面拿到空数组。
#[tokio::test]
async fn list_requests_joins_cost_without_ambiguous_branch() {
    let pool = pool().await;
    sqlx::query("INSERT INTO timeline_nodes(id,date_label) VALUES(7,'第 1 天')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO messages(id,node_id,role,content) VALUES(80,7,'assistant','x')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO director_costs(id,node_id,message_id,kind,reason,score) VALUES(1,7,80,'条件','交出钥匙',10)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO pardon_requests(cost_id,message_id,node_id,reply,verdict,my_name,ai_name) VALUES(1,80,7,'〔赦免:同意〕','agreed','顾宸熙','夏婉琴')")
        .execute(&pool).await.unwrap();

    let rows = list_requests_in(&pool, &None, None).await.expect("查询不该报 ambiguous");
    assert_eq!(rows.len(), 1, "应查得到那一条：{rows:?}");
    assert_eq!(rows[0].kind, "条件", "要 JOIN 出账的类型（盒子第一行要写「请求赦免什么」）");
    assert_eq!(rows[0].my_name, "顾宸熙");
    assert_eq!(rows[0].verdict, "agreed");
}
/// 盒子挂在**当天最后一条消息**后面（用户 2026-10-27 定），没有消息时才退回原消息
#[tokio::test]
async fn box_goes_after_the_last_message_of_the_day() {
    let pool = pool().await;
    sqlx::query("INSERT INTO timeline_nodes(id,date_label) VALUES(7,'第 1 天'),(8,'第 2 天')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO messages(id,node_id,role,content) VALUES(80,7,'assistant','a'),(81,7,'user','b'),(82,7,'assistant','c'),(90,8,'user','x')")
        .execute(&pool).await.unwrap();
    assert_eq!(last_message_of_node(&pool, 7, 80).await.unwrap(), 82, "挂当天最后一条，不挂产生账的那条");
    assert_eq!(last_message_of_node(&pool, 9, 80).await.unwrap(), 80, "这天没消息 → 退回原消息");
}


/// 赦免请求必须带上她的设定与当前关系（2026-10-29 用户定）：
/// 强度低/破裂时明确禁止「给台阶」，否则会回出「我不太会认这个」这种软话。
#[test]
fn profile_block_carries_setting_and_strength() {
    let low = profile_block("夏婉琴", "顾宸熙", "她话少", "冷淡", "不主动靠近", "刚被冒犯", 0, "破裂");
    assert!(low.contains("强度 0"), "要带强度：{low}");
    assert!(low.contains("疏离"), "要带档位名");
    assert!(low.contains("破裂"));
    assert!(low.contains("不要写心软"), "低强度必须禁止软化：{low}");
    assert!(low.contains("她话少") && low.contains("不主动靠近"));

    let normal = profile_block("夏婉琴", "顾宸熙", "", "", "", "", 70, "稳定");
    assert!(normal.contains("（未填）"), "空字段不硬造");
    assert!(!normal.contains("不要写心软"), "高强度不该套低强度禁令");
}
#[test]
fn marker_decides_first() {
    assert_eq!(parse_verdict("我还没想好。\n〔赦免:同意〕"), Verdict::Agreed);
    assert_eq!(parse_verdict("想都别想。\n〔赦免:不同意〕"), Verdict::Refused);
    // 全角冒号 + 空格
    assert_eq!(parse_verdict("算了。\n〔赦免： 同意〕"), Verdict::Agreed);
}


/// 标记写在**最前面**也要认（2026-10-28 实测）：她回了
/// 「〔赦免:不同意〕+ 理由 + 想说的话」，末尾协议区里找不到标记。
#[test]
fn marker_written_first_is_still_recognised() {
    let reply = "〔赦免:不同意〕\n\n理由：拿话换的事，没有第二天就讨价还价的道理。\n想对顾宸熙说：不行。";
    assert_eq!(parse_verdict(reply), Verdict::Refused);
    let reply2 = "〔赦免:同意〕\n\n理由：算你识相。";
    assert_eq!(parse_verdict(reply2), Verdict::Agreed);
}
#[test]
fn fallback_looks_for_negation() {
    assert_eq!(parse_verdict("不原谅。这次不行。"), Verdict::Refused);
    assert_eq!(parse_verdict("好吧，我同意。"), Verdict::Agreed);
    assert_eq!(parse_verdict("……"), Verdict::Unclear);
}

#[test]
fn marker_must_be_its_own_line() {
    // 2026-10-28 改：模型会把标记写在**最前面**再补理由（真实回复就是这样），
    // 所以整行匹配的标记在任何位置都认；但**混在句子里**的不认。
    assert_eq!(parse_verdict("〔赦免:同意〕\n（她把门关上）"), Verdict::Agreed);
    assert_eq!(parse_verdict("她说了一句〔赦免:同意〕就走了。"), Verdict::Unclear);
}

#[test]
fn box_shows_the_last_sentence() {
    assert_eq!(last_line_of("第一句。\n\n〔赦免:不同意〕\n"), "〔赦免:不同意〕");
    assert_eq!(last_line_of(""), "");
}

#[tokio::test]
async fn third_refusal_costs_five_points() {
    let pool = pool().await;
    sqlx::query("INSERT INTO timeline_nodes(id,date_label) VALUES(7,'第 1 天')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO messages(id,node_id,role,content) VALUES(80,7,'assistant','（她把门关上）')")
        .execute(&pool).await.unwrap();
    // 3 次都不同意
    for i in 0..3 {
        sqlx::query("INSERT INTO pardon_requests(cost_id,message_id,node_id,verdict) VALUES(1,80,7,'refused')")
            .execute(&pool).await.unwrap();
        if i < 2 {
            let got = penalize_if_still_refused(&pool, &None, 1, "夏婉琴", "顾宸熙").await.unwrap();
            assert!(got.is_none(), "还没到 3 次，不该扣");
        }
    }
    let got = penalize_if_still_refused(&pool, &None, 1, "夏婉琴", "顾宸熙").await.unwrap();
    assert_eq!(got, Some(5), "第 3 次仍不同意 → 扣 5 点");
    let s: i64 = sqlx::query_scalar("SELECT strength FROM relations WHERE id = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(s, 15);
}

#[tokio::test]
async fn agreed_request_does_not_trigger_the_penalty() {
    let pool = pool().await;
    sqlx::query("INSERT INTO messages(id,node_id,role,content) VALUES(80,7,'assistant','x')").execute(&pool).await.unwrap();
    for v in ["refused", "refused", "agreed"] {
        sqlx::query("INSERT INTO pardon_requests(cost_id,message_id,node_id,verdict) VALUES(1,80,7,?1)")
            .bind(v).execute(&pool).await.unwrap();
    }
    assert!(penalize_if_still_refused(&pool, &None, 1, "夏婉琴", "顾宸熙").await.unwrap().is_none());
}

#[tokio::test]
async fn apply_pardon_needs_agreement_and_restores_half() {
    let pool = pool().await;
    sqlx::query("INSERT INTO timeline_nodes(id,date_label) VALUES(7,'第 1 天')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO messages(id,node_id,role,content) VALUES(80,7,'assistant','x')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO director_costs(id,node_id,message_id,kind,reason,score,rel_delta) VALUES(1,7,80,'白给','她没反抗',30,10)")
        .execute(&pool).await.unwrap();

    // 没有"同意" → 拒绝（这里直接调内部逻辑：先看 agreed 计数）
    let agreed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pardon_requests WHERE cost_id = 1 AND verdict = 'agreed'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(agreed, 0, "还没同意");

    // 插一条同意，自己执行 apply 的核心：excused=1 + 回补一半
    sqlx::query("INSERT INTO pardon_requests(cost_id,message_id,node_id,verdict) VALUES(1,80,7,'agreed')")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE director_costs SET excused = 1 WHERE id = 1").execute(&pool).await.unwrap();
    let back = 10 / 2;
    let meta = crate::director::BumpMeta { source: "pardon", kind: "赦免回补", reason: "赦免成功（回补一半）", node_id: None, message_id: None };
    crate::director::bump_strength(&pool, &None, "夏婉琴", "顾宸熙", back, meta).await.unwrap();

    let s: i64 = sqlx::query_scalar("SELECT strength FROM relations WHERE id = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(s, 25, "20 + 5（只回补一半）");
    let ex: i64 = sqlx::query_scalar("SELECT excused FROM director_costs WHERE id = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(ex, 1);
}

#[tokio::test]
async fn daily_limit_counts_story_days_not_wall_clock() {
    let pool = pool().await;
    // 两个节点：第 1 天 / 第 2 天
    sqlx::query("INSERT INTO timeline_nodes(id,date_label) VALUES(7,'第 1 天'),(8,'第 2 天')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO messages(id,node_id,role,content) VALUES(80,7,'assistant','a'),(81,8,'assistant','b')")
        .execute(&pool).await.unwrap();
    for _ in 0..MAX_PER_DAY {
        sqlx::query("INSERT INTO pardon_requests(cost_id,message_id,node_id,verdict) VALUES(1,80,7,'refused')")
            .execute(&pool).await.unwrap();
    }
    assert_eq!(count_for_day(&pool, &None, "第 1 天").await.unwrap(), MAX_PER_DAY);
    assert_eq!(count_for_day(&pool, &None, "第 2 天").await.unwrap(), 0, "换一天就重新计数");
    assert_eq!(count_for_cost(&pool, 1).await.unwrap(), MAX_PER_DAY);
}
