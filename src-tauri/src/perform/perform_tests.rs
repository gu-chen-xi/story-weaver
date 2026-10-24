// 表演标签（动作/情绪）的规则测试（v3.5.1）。
//
// 为什么需要：
//   ① 词表是"数据"，退化方式很隐蔽 —— 有人删了一半词，界面只是少显示几个标签，没人会发现；
//   ② 提取规则错会让**误报**爆炸（比如把正文里所有"感动"都当表演），必须用测试锁边界；
//   ③ `performance` 记录写进 event_logs，格式错了会让导演监视器的「本场」永远空白。

use super::*;
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;
use std::str::FromStr;

async fn mem_pool() -> SqlitePool {
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::from_str("sqlite::memory:").unwrap().create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE event_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            node_id INTEGER,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            confidence REAL NOT NULL DEFAULT 0.0,
            source TEXT NOT NULL DEFAULT 'rule',
            created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

/// 词表规模：用户要求"这种标签多一点"，少于阈值就说明被削了
#[test]
fn lexicon_is_large_enough() {
    assert!(EMOTIONS.len() >= 40, "情绪词表只有 {} 个（至少 40）", EMOTIONS.len());
    assert!(ACTIONS.len() >= 55, "动作词表只有 {} 个（至少 55）", ACTIONS.len());
    // 6 组情绪 / 6 组动作都要有词，且每组 >= 3
    for group in ["暖", "冷", "紧", "散", "炸", "弱"] {
        let n = EMOTIONS.iter().filter(|(_, g, _, _)| *g == group).count();
        assert!(n >= 3, "情绪组「{group}」只有 {n} 个词");
    }
    for group in ["靠近", "退开", "定格", "手部", "视线", "声音"] {
        let n = ACTIONS.iter().filter(|(_, g)| *g == group).count();
        assert!(n >= 3, "动作组「{group}」只有 {n} 个词");
    }
    // 不许有重复词（重复会让匹配出现同一标签两次）
    let mut seen = std::collections::HashSet::new();
    for (w, _, _, _) in EMOTIONS.iter() {
        assert!(seen.insert(*w), "情绪词重复：{w}");
    }
    let mut seen2 = std::collections::HashSet::new();
    for (w, _) in ACTIONS.iter() {
        assert!(seen2.insert(*w), "动作词重复：{w}");
    }
}

/// 原 8 个词必须还在（老数据/老界面依赖它们）
#[test]
fn legacy_emotions_survive() {
    for w in ["生气", "开心", "难过", "害羞", "惊讶", "害怕", "平静", "温柔"] {
        assert!(emotion_of(w).is_some(), "老词丢了：{w}");
    }
}

/// 只认括号段落：正文里的"感动"不算表演（否则误报爆炸）
#[test]
fn only_parenthesized_segments_count() {
    // 用户给的例子
    let p = from_reply("当然呀，我当然爱你呀（非常感动）");
    assert_eq!(p.emotions, vec!["感动".to_string()], "括号里的情绪要认出来");
    assert!(p.actions.is_empty());

    // 同一个词出现在正文、括号外 → 不算
    let p2 = from_reply("她非常感动，但什么也没说。");
    assert!(p2.emotions.is_empty(), "括号外的不该算表演：{p2:?}");

    // 括号里的动作也要认
    let p3 = from_reply("（转身）（低头，攥紧了拳头）");
    assert!(p3.actions.iter().any(|a| a == "转身"), "{p3:?}");
    assert!(p3.actions.iter().any(|a| a == "低头"), "{p3:?}");
    assert!(p3.actions.iter().any(|a| a == "攥紧"), "{p3:?}");
}

/// 显式标签（current_status 里的 `情绪：X` / `动作：X`）
#[test]
fn parse_status_reads_explicit_tags() {
    let (e, a) = parse_status("刚被拒绝｜情绪：崩溃｜动作：转过身");
    assert_eq!(e.as_deref(), Some("崩溃"));
    assert_eq!(a.as_deref(), Some("转过身"));

    // 只有情绪、没有动作
    let (e2, a2) = parse_status("情绪：麻木");
    assert_eq!(e2.as_deref(), Some("麻木"));
    assert!(a2.is_none());

    // 完全没有标签
    let (e3, a3) = parse_status("在等雨停");
    assert!(e3.is_none() && a3.is_none());

    // 半角冒号也认（用户手打时常混用）
    let (e4, _) = parse_status("情绪: 委屈");
    assert_eq!(e4.as_deref(), Some("委屈"));
}

/// 落库 + 读回（导演监视器「本场」靠这条链路）
#[tokio::test]
async fn performance_round_trips_through_event_logs() {
    let pool = mem_pool().await;
    let p = Performance {
        actions: vec!["转身".to_string(), "低头".to_string()],
        emotions: vec!["感动".to_string()],
    };
    log_performance(&pool, 7, &p).await.unwrap();

    let (back, _at) = latest(&pool, 7).await.unwrap().expect("应读到一条");
    assert_eq!(back.actions, p.actions);
    assert_eq!(back.emotions, p.emotions);
    assert_eq!(back.summary(), "动作：转身、低头 · 情绪：感动");

    // 别的节点读不到（按 node_id 隔离）
    assert!(latest(&pool, 8).await.unwrap().is_none());
    // 空标签不写库
    log_performance(&pool, 9, &Performance::default()).await.unwrap();
    assert!(latest(&pool, 9).await.unwrap().is_none());
}

/// 括号提取的边界：未闭合 / 超长 / 嵌套都不该 panic
#[test]
fn parenthesized_is_robust() {
    assert!(parenthesized("（没有闭合").is_empty());
    assert!(parenthesized("正常（转身）继续").len() == 1);
    let long = "（".to_string() + &"很长的描写".repeat(20) + "）";
    assert!(parenthesized(&long).is_empty(), "超长括号不当动作（避免抓到大段旁白）");
}
