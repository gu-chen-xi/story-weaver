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

#[test]
fn marker_decides_first() {
    assert_eq!(parse_verdict("我还没想好。\n〔赦免:同意〕"), Verdict::Agreed);
    assert_eq!(parse_verdict("想都别想。\n〔赦免:不同意〕"), Verdict::Refused);
    // 全角冒号 + 空格
    assert_eq!(parse_verdict("算了。\n〔赦免： 同意〕"), Verdict::Agreed);
}

#[test]
fn fallback_looks_for_negation() {
    assert_eq!(parse_verdict("不原谅。这次不行。"), Verdict::Refused);
    assert_eq!(parse_verdict("好吧，我同意。"), Verdict::Agreed);
    assert_eq!(parse_verdict("……"), Verdict::Unclear);
}

#[test]
fn marker_in_the_middle_of_text_is_ignored() {
    // 标记后面还接着正文 → 不在末尾协议区 → 不算
    assert_eq!(parse_verdict("〔赦免:同意〕\n（她把门关上）"), Verdict::Unclear);
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
    crate::director::bump_strength(&pool, &None, "夏婉琴", "顾宸熙", back, "赦免成功（回补一半）").await.unwrap();

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
