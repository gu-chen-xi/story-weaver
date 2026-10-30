use super::*;
use sqlx::sqlite::SqlitePoolOptions;

async fn pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    for ddl in [
        "CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY, date_label TEXT, sort_order INTEGER, node_type TEXT DEFAULT 'day', branch_id TEXT)",
        "CREATE TABLE messages (id INTEGER PRIMARY KEY AUTOINCREMENT, node_id INTEGER, content TEXT, branch_id TEXT)",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    ensure_table(&pool).await.unwrap();
    sqlx::query("INSERT INTO timeline_nodes(id,date_label,sort_order) VALUES(1,'第 1 天',1),(2,'第 2 天',2),(3,'第 3 天',3)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO messages(node_id,content) VALUES(1,'她站在门口'),(2,'门开了'),(2,'他进去')").execute(&pool).await.unwrap();
    pool
}

#[tokio::test]
async fn create_list_and_count_words() {
    let pool = pool().await;
    let id = create_in(&pool, &None, "序雾", &[1, 2]).await.unwrap();
    let list = list_in(&pool, &None).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].node_ids, vec![1, 2]);
    assert_eq!(list[0].words, 5 + 3 + 3, "按字符数统计（她站在门口=5）");
    assert_eq!(list[0].order_no, 1);
}


/// 独占（2026-10-30 用户定）：一天只属于一章 —— 圈进新章时自动从旧章摘掉
#[tokio::test]
async fn a_day_belongs_to_exactly_one_chapter() {
    let pool = pool().await;
    let a = create_in(&pool, &None, "A", &[1, 2]).await.unwrap();
    let b = create_in(&pool, &None, "B", &[2, 3]).await.unwrap();
    let list = list_in(&pool, &None).await.unwrap();
    let ca = list.iter().find(|c| c.id == a).unwrap();
    let cb = list.iter().find(|c| c.id == b).unwrap();
    assert_eq!(ca.node_ids, vec![1], "第 2 天被 B 抢走了");
    assert_eq!(cb.node_ids, vec![2, 3]);
    assert_eq!(unassigned_days(&pool, &None).await.unwrap().len(), 0, "全部有主");
}

#[tokio::test]
async fn set_nodes_replaces_whole_set() {
    let pool = pool().await;
    let id = create_in(&pool, &None, "A", &[1]).await.unwrap();
    set_nodes_in(&pool, id, &[2, 3]).await.unwrap();
    let list = list_in(&pool, &None).await.unwrap();
    assert_eq!(list[0].node_ids, vec![2, 3]);
}

#[tokio::test]
async fn update_changes_meta_only() {
    let pool = pool().await;
    let id = create_in(&pool, &None, "A", &[1]).await.unwrap();
    assert!(update_in(&pool, id, Some("新标题"), Some("final"), None, Some("本章目标"), None, None, None).await.unwrap());
    let list = list_in(&pool, &None).await.unwrap();
    assert_eq!(list[0].title, "新标题");
    assert_eq!(list[0].status, "final");
    assert_eq!(list[0].goal, "本章目标");
}

#[tokio::test]
async fn delete_keeps_story_messages() {
    let pool = pool().await;
    let id = create_in(&pool, &None, "A", &[1]).await.unwrap();
    delete_in(&pool, id).await.unwrap();
    assert_eq!(list_in(&pool, &None).await.unwrap().len(), 0);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages").fetch_one(&pool).await.unwrap();
    assert_eq!(n, 3, "删章不能删剧情");
}

#[tokio::test]
async fn unassigned_days_lists_only_free_ones_with_messages() {
    let pool = pool().await;
    create_in(&pool, &None, "A", &[1]).await.unwrap();
    let free = unassigned_days(&pool, &None).await.unwrap();
    assert_eq!(free, vec![(2, "第 2 天".to_string())], "1 已归章、3 没消息");
}

#[tokio::test]
async fn branch_scope_is_respected() {
    let pool = pool().await;
    create_in(&pool, &Some("b1".to_string()), "分支章", &[2]).await.unwrap();
    assert_eq!(list_in(&pool, &None).await.unwrap().len(), 0, "主线看不到分支的章");
    assert_eq!(list_in(&pool, &Some("b1".to_string())).await.unwrap().len(), 1);
}

/// 章序按分支各自编号（2026-10-30 修）：分支建过章，主线不该跳号（原来会出现 01、03）
#[tokio::test]
async fn order_numbers_are_per_branch() {
    let pool = pool().await;
    let b1 = create_in(&pool, &Some("b1".into()), "分支首章", &[2]).await.unwrap();
    let m1 = create_in(&pool, &None, "主线首章", &[1]).await.unwrap();
    assert_eq!(list_in(&pool, &Some("b1".into())).await.unwrap()[0].order_no, 1);
    let main = list_in(&pool, &None).await.unwrap();
    assert_eq!(main.iter().find(|c| c.id == m1).unwrap().order_no, 1, "主线也从 1 开始");
    let _ = create_in(&pool, &None, "主线次章", &[3]).await.unwrap();
    assert_eq!(list_in(&pool, &None).await.unwrap().iter().filter(|c| c.id != b1).count(), 2);
}

/// 删掉中间一章后，章序归一化成 1..n（不留洞）
#[tokio::test]
async fn deleting_middle_chapter_closes_the_gap() {
    let pool = pool().await;
    create_in(&pool, &None, "A", &[1]).await.unwrap();
    let mid = create_in(&pool, &None, "B", &[2]).await.unwrap();
    create_in(&pool, &None, "C", &[3]).await.unwrap();
    delete_in(&pool, mid).await.unwrap();
    let orders: Vec<i64> = list_in(&pool, &None).await.unwrap().iter().map(|c| c.order_no).collect();
    assert_eq!(orders, vec![1, 2], "删中间那章后不能留 1、3");
}

