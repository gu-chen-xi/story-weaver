// 从 `recall.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `recall::tests` —— 因为 `recall.rs` + `recall/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `recall`，一行都不用改写。

/// 真实数据诊断（手动跑）：
///   DW_DIAG_STORY_DB=<story.db 路径> cargo test --lib diag_explain_alias -- --ignored --nocapture
///
/// 验证用户的原始症状：物品「rtx显卡」（别名「显卡」）在第 4 天的日摘要里出现过，
/// 所以 `explain("显卡")` 必须给「第 4 天」加分并在理由里体现。
#[tokio::test]
#[ignore]
async fn diag_explain_alias() {
    let db = match std::env::var("DW_DIAG_STORY_DB") {
        Ok(v) => v,
        Err(_) => return,
    };
    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&db)
        .create_if_missing(false);
    let story = sqlx::SqlitePool::connect_with(opts).await.unwrap();
    // lib_pool 在 explain 里只用于 `load_weights`（读不到配置就用默认值），
    // 这里直接复用故事库，免去再建一个临时库（在受限环境里建库可能失败）。
    let lib = story.clone();

    let text = std::env::var("DW_DIAG_TEXT").unwrap_or_else(|_| "显卡".to_string());
    println!("=== explain(\"{text}\") ===");
    let rows = explain(&story, &None, &lib, &text, 9999).await.unwrap();
    for r in &rows {
        println!(
            "  {:<8} score={:>5} picked={:<5} 理由={}",
            r.day_label,
            r.score,
            r.picked,
            r.why.join(" / ")
        );
    }
    story.close().await;
}
use super::split_aliases;

// ===== v3.4：别名（回顾试算检测不到物品的直接原因）=====

/// 别名要能被解析出来，且支持中英文逗号/顿号/分号
#[test]
fn split_aliases_handles_separators() {
    assert_eq!(split_aliases(r#"{"aliases":"显卡"}"#), vec!["显卡".to_string()]);
    // 分隔符四种都认；单字「它」应被过滤掉
    assert_eq!(
        split_aliases(r#"{"aliases":"旧表,那块表，怀表、它"}"#),
        vec!["旧表".to_string(), "那块表".to_string(), "怀表".to_string()]
    );
    // 分号/全角分号也是分隔符；单字符片段被过滤
    assert_eq!(
        split_aliases(r#"{"aliases":"旧表;那块表；怀表"}"#),
        vec!["旧表".to_string(), "那块表".to_string(), "怀表".to_string()]
    );
    assert!(
        split_aliases(r#"{"aliases":"A;B;C"}"#).is_empty(),
        "全是单字符 → 全被过滤，不能塞进词典（单字误命中率太高）"
    );
}

/// 没有 meta / 空别名 / 单字别名 → 不产生词（单字太容易误命中）
#[test]
fn split_aliases_skips_empty_and_single_char() {
    assert!(split_aliases("{}").is_empty());
    assert!(split_aliases("").is_empty());
    assert!(split_aliases(r#"{"aliases":""}"#).is_empty());
    assert!(split_aliases(r#"{"aliases":"表"}"#).is_empty(), "单字别名不该进词典");
    assert!(split_aliases(r#"{"aliases":" , ，、"}"#).is_empty());
}
use super::*;

/// 回归（用户 2026-10-18 实测发现的**真正**根因）：
/// 角色名被当成"标签词"计分，导致 **调 tag_word 连带改了角色名的分**。
///
/// 复现方式：用户把 tag_word 从 20 调到 50，分数涨了 **60** 而不是 30
/// —— 因为那句话命中 1 个标签词 + 1 个角色名，两者都吃 tag_word（2 × 30 = 60）。
#[test]
fn tag_word_does_not_leak_into_name_score() {
    let lex = vec![
        LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord },
        LexTerm { word: "夏婉琴".to_string(), kind: LexKind::Name },
    ];
    // 摘要里两个词都有 → 两类都会命中
    let summary = "夏婉琴在院子里牵手";
    let base_w = RecallWeights::default();
    let tuned_w = RecallWeights { tag_word: 50, ..Default::default() };

    let (base_score, _) = score_summary(summary, Some(3), None, &[], &[], &[], 0, &[], &lex, &base_w);
    let (tuned, _) = score_summary(summary, Some(3), None, &[], &[], &[], 0, &[], &lex, &tuned_w);

    assert_eq!(
        tuned - base_score, 30,
        "tag_word 20→50 只该涨 30（涨 60 = 角色名也被当成标签词了）"
    );

    // 反过来：调 name_hit 也不该影响标签词那部分
    let tuned_name = RecallWeights { name_hit: 40, ..Default::default() };
    let (by_name, _) = score_summary(summary, Some(3), None, &[], &[], &[], 0, &[], &lex, &tuned_name);
    assert_eq!(by_name - base_score, 20, "name_hit 20→40 只该涨 20");
}

/// 回归（用户 2026-10-18 发现）：**标签词只能计一次分**。
///
/// 生产路径同时会传 `intent.clues`（detect_intent 从输入里认出的标签词）
/// 和 `lex_hits`（C 段的词典命中）—— C3 早期版本两处都加了 `w.tag_word`，
/// 导致"把 tag_word 从 20 调到 50，分数涨 60 而不是 30"。
///
/// 之前的权重测试没抓到，是因为它只传了 lex_hits、没传 clues，走不到重复路径。
#[test]
fn tag_word_is_counted_exactly_once() {
    let w = RecallWeights::default();
    let clues = vec!["牵手".to_string()];                        // detect_intent 会产出这个
    let lex = vec![LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord }]; // C 段也会命中

    // 只传 clues（老路径）
    let (only_clues, _) = score_summary("两人牵手", Some(3), None, &clues, &[], &[], 0, &[], &[], &w);
    // 只传 lex_hits（新路径）
    let (only_lex, _) = score_summary("两人牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &w);
    // 两条都传（**生产实际就是这样**）
    let (both, why) = score_summary("两人牵手", Some(3), None, &clues, &[], &[], 0, &[], &lex, &w);

    assert_eq!(both, only_lex, "同时传 clues 与 lex_hits 时**不能比单传 lex 更高**：{why:?}");
    // 老的 clues 路径已经删除（它的职责被 lex_hits 接管），所以单传 clues 不再计分
    assert_eq!(only_clues, 0, "clues 不再单独计分（避免与 lex_hits 重复）");
    // 把 tag_word 提到 50：应当只涨 30，而不是 60
    let w2 = RecallWeights { tag_word: 50, ..Default::default() };
    let (both2, _) = score_summary("两人牵手", Some(3), None, &clues, &[], &[], 0, &[], &lex, &w2);
    assert_eq!(both2 - both, 30, "tag_word 20→50 只该涨 30（涨 60 说明算了两遍）");
}

// ── C3：权重可配置 ──
#[test]
fn weights_are_configurable_not_hardcoded() {
    let w0 = RecallWeights::default();
    assert_eq!((w0.event, w0.day_exact, w0.hand_tag, w0.tag_word, w0.entity, w0.min_score),
               (100, 80, 25, 20, 20, 20), "默认值就是文档里那套（v3.4：entity 12→20）");

    let lex = vec![LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord }];
    let (base_score, _) = score_summary("两人牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &w0);

    let w1 = RecallWeights { tag_word: 50, ..Default::default() };
    let (tuned, _) = score_summary("两人牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &w1);
    assert_eq!(tuned - base_score, 30, "tag_word 20→50 应 +30");
}

/// 门槛也来自权重：调高 min_score 时，同样的分从"能带出"变成"带不出"
#[test]
fn min_score_comes_from_weights() {
    let w = RecallWeights { min_score: 999, ..Default::default() };
    let lex = vec![LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord }];
    let (score, _) = score_summary("两人牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &w);
    assert!(score < w.min_score, "门槛 999 时这条不该被带出（score={score}）");
}

/// JSON 往返：配置能存能读（`recall.weights` 就存这个）
#[test]
fn weights_round_trip_json() {
    let w = RecallWeights { tag_word: 33, min_score: 25, ..Default::default() };
    let json = serde_json::to_string(&w).unwrap();
    let back: RecallWeights = serde_json::from_str(&json).unwrap();
    assert_eq!(w, back);
    // JSON 坏了要回默认，而不是让回顾失效
    assert_eq!(serde_json::from_str::<RecallWeights>("{oops}").unwrap_or_default(), RecallWeights::default());
}

// ── A：手打标签参与检索（用户 2026-10-18 定）──
#[test]
fn hand_tag_hit_boosts_that_day() {
    // 用户把某天标了「约会」，之后说"那次约会"
    let (s, why) = score_summary("两人在院子里散步", Some(5), None, &[], &[], &[], 0, &["约会".to_string()], &[], &RecallWeights::default());
    assert!(s >= 25, "手打标签命中应给 25 分，实际 {s}");
    assert!(why.iter().any(|w| w.contains("#约会")), "应说明命中了手打标签：{why:?}");

    // 标签没被提到 → 不加分（不然每天都因标签拿到分）
    let (s2, _) = score_summary("两人在院子里散步", Some(5), None, &[], &[], &[], 0, &[], &[], &RecallWeights::default());
    assert!(s2 < 25);
}

// ── C：词典命中（标签词 20 / 命名实体 20 —— v3.4 起同权重）──
#[test]
fn lexicon_terms_score_by_weight() {
    let lex = vec![
        LexTerm { word: "牵手".into(), kind: LexKind::TagWord },
        LexTerm { word: "修女院".into(), kind: LexKind::Entity },
    ];
    let (s, why) = score_summary("两人在修女院里牵手", Some(3), None, &[], &[], &[], 0, &[], &lex, &RecallWeights::default());
    assert_eq!(s, 40, "20 + 20 = 40（v3.4 起 entity 与标签词同权重），实际 {s}");
    assert_eq!(why.len(), 2, "两条说明：{why:?}");
}


/// 回归锁：**只命中一个命名实体**就该够 min_score，能被真正带出来。
///
/// 这是用户实测暴露的问题：物品「显卡」写进第 4 天摘要，回顾试算显示命中 14 分、
/// 但 picked=false（entity=12 < min_score=20）→ "检测得到却带不出来"。
#[test]
fn single_entity_hit_reaches_min_score() {
    let w = RecallWeights::default();
    let lex = vec![LexTerm { word: "显卡".into(), kind: LexKind::Entity }];
    let (s, why) = score_summary("我还记得那块显卡", Some(4), None, &[], &[], &[], 0, &[], &lex, &w);
    assert!(s >= w.min_score, "实体单命中应过门槛：score={s} min={}", w.min_score);
    assert!(why.iter().any(|x| x.contains("显卡")), "理由里应提到命中词：{why:?}");
}
/// 词典词只在**用户输入里出现**才算数（纯函数 match_lexicon 负责）
#[test]
fn match_lexicon_only_returns_words_in_input() {
    let lex = vec![
        LexTerm { word: "牵手".to_string(), kind: LexKind::TagWord },
        LexTerm { word: "修女院".to_string(), kind: LexKind::Entity },
        LexTerm { word: "吵架".to_string(), kind: LexKind::TagWord },
        LexTerm { word: "单".to_string(), kind: LexKind::TagWord }, // 单字应被过滤
    ];
    let got = match_lexicon("还记得在修女院那次吗", &lex);
    assert_eq!(got.len(), 1, "只有「修女院」出现在输入里：{got:?}");
    assert_eq!(got[0].word, "修女院");
    assert_eq!(got[0].kind, LexKind::Entity);
}

/// 派生标签：手打 + 标签词 + 角色名（去重、零成本"补录"）
#[test]
fn derive_day_tags_merges_hand_vocab_and_names() {
    let hand = vec!["约会".to_string()];
    let names = vec!["夏婉琴".to_string(), "宸熙".to_string()];
    let tags = derive_day_tags("夏婉琴在院子里与宸熙牵手", &hand, &names);
    assert!(tags.contains(&"约会".to_string()), "手打标签应保留");
    assert!(tags.contains(&"牵手".to_string()), "标签词命中");
    assert!(tags.contains(&"院子".to_string()), "标签词命中");
    assert!(tags.contains(&"夏婉琴".to_string()), "角色名命中");
    assert_eq!(tags.iter().filter(|x| *x == "夏婉琴").count(), 1, "不能重复");
    // 摘要里没提的人不进来
    assert!(!tags.contains(&"院长".to_string()));
}

#[test]
fn parses_day_with_digits_and_chinese() {
    assert_eq!(parse_day("还记得第 5 天吗"), Some(5));
    assert_eq!(parse_day("第5天发生了什么"), Some(5));
    assert_eq!(parse_day("第三天那次"), Some(3));
    assert_eq!(parse_day("第十天"), Some(10));
    assert_eq!(parse_day("第十一天"), Some(11));
    assert_eq!(parse_day("第二十三天"), Some(23));
    assert_eq!(parse_day("第  7  天"), Some(7));
    assert_eq!(parse_day("没有天数"), None);
    assert_eq!(parse_day("第 5 章"), None);
}

#[test]
fn strong_cues_trigger_recall() {
    for s in ["你还记得吗", "那次在院子", "当时我说话", "我们第一次牵手的时候", "回顾一下第 2 天"] {
        assert!(detect_intent(s).is_recall, "应识别为回忆意图：{s}");
    }
}

#[test]
fn plain_scene_description_is_not_recall() {
    for s in ["我们走进院子", "她有点害羞", "外面在下雨"] {
        assert!(!detect_intent(s).is_recall, "不该判为回忆意图：{s}");
    }
}

#[test]
fn first_event_words_are_recognised() {
    let i = detect_intent("我们首次牵手的时候");
    assert!(i.is_recall);
    assert!(i.events.iter().any(|e| e.contains("牵手")), "应识别出首次牵手：{i:?}");

    let i2 = detect_intent("第一次告白那天");
    assert!(i2.is_recall);
}

#[test]
fn day_mention_is_a_clue() {
    let i = detect_intent("第 8 天的时候你在做什么");
    assert_eq!(i.day, Some(8));
    assert!(i.clues.iter().any(|c| c.contains("8")));
    assert!(i.is_recall);
}

/// 用户问了"第一次牵手"时，含该事件的摘要应显著高于只含标签词的摘要。
#[test]
fn scoring_ranks_first_event_highest() {
    let events = vec!["第一次牵手".to_string()];
    let (s_event, why) = score_summary("两人第一次牵手", Some(5), None, &[], &events, &[], 0, &[], &[], &RecallWeights::default());
    let (s_tag, _) = score_summary("在院子里散步", Some(5), None, &["院子".to_string()], &events, &[], 0, &[], &[], &RecallWeights::default());
    assert!(s_event > s_tag, "用户问到的事件命中应高于普通标签：{why:?}");
}

/// **用户没问事件时，摘要里出现"第一次X"不该给满分**（否则某一天会霸榜）。
#[test]
fn summary_event_without_user_ask_is_not_bonus() {
    let (s, why) = score_summary("这是第一次亲吻", Some(3), None, &[], &[], &[], 0, &[], &[], &RecallWeights::default());
    assert_eq!(s, 0, "用户没问首次事件时，摘要里的事件词不应计分：{why:?}");
}

/// 用户问"第 8 天"，不该被别的天靠"表白"二字抢走（实测踩到的霸榜）。
#[test]
fn day_match_beats_unrelated_event_word_in_summary() {
    let day_hit = score_summary("随便一句", Some(8), Some(8), &[], &[], &[], 0, &[], &[], &RecallWeights::default());
    let other = score_summary("那天他表白了", Some(12), Some(8), &[], &[], &[], 9, &[], &[], &RecallWeights::default());
    assert!(day_hit.0 > other.0, "指定的那天必须排最前：{} vs {}", day_hit.0, other.0);
}

#[test]
fn scoring_uses_requested_day() {
    let (s_hit, why) = score_summary("随便一句话", Some(5), Some(5), &[], &[], &[], 0, &[], &[], &RecallWeights::default());
    let (s_miss, _) = score_summary("随便一句话", Some(6), Some(5), &[], &[], &[], 0, &[], &[], &RecallWeights::default());
    assert!(s_hit > s_miss);
    assert!(why.iter().any(|w| w.contains("第 5 天")), "{why:?}");
}

/// **只有角色名命中的摘要必须判 0** —— 否则"当时在院子里"会稳定带回同样的第 1/2/3 天。
#[test]
fn name_only_match_scores_zero() {
    let names = vec!["夏婉琴".to_string(), "宸熙".to_string()];
    let (s, why) = score_summary("夏婉琴和宸熙在吃饭", Some(2), None, &[], &[], &names, 5, &[], &[], &RecallWeights::default());
    assert_eq!(s, 0, "只有角色名不该算命中：{why:?}");
    assert!(why.is_empty());
}

/// 渲染的是**原始对话**，不是摘要（用户实测后要求）。
#[test]
fn render_uses_original_messages_not_summary() {
    assert!(render_block(&[]).is_empty());
    let hit = RecallHit {
        node_id: 1,
        date_label: "第 1 天".into(),
        day: Some(1),
        summary: "（这是摘要，不该出现在提示词里）".into(),
        messages: vec![
            ("user".into(), "宝宝 你还记得第一天的时候吗".into()),
            ("assistant".into(), "那天在下雨。".into()),
        ],
        omitted: 0,
        score: 100,
        why: vec![],
    };
    let block = render_block(&[hit]);
    assert!(block.contains("【相关旧日记忆】"));
    assert!(block.contains("原始对话"), "标题应说明是原始对话：{block}");
    assert!(block.contains("[用户] 宝宝 你还记得第一天的时候吗"), "{block}");
    assert!(block.contains("[AI] 那天在下雨。"), "{block}");
    assert!(!block.contains("这是摘要"), "摘要绝不能被渲染进去：{block}");
}

#[test]
fn render_marks_omitted_count() {
    let hit = RecallHit {
        node_id: 1, date_label: "第 1 天".into(), day: Some(1), summary: String::new(),
        messages: vec![("user".into(), "甲".into())], omitted: 7, score: 10, why: vec![],
    };
    let block = render_block(&[hit]);
    assert!(block.contains("另有 7 条未列出"), "{block}");
}

#[test]
fn trim_day_keeps_head_and_counts_omitted() {
    let msgs: Vec<(String, String)> = (0..5)
        .map(|i| ("user".to_string(), format!("消息{}", i)))
        .collect();
    let (kept, omitted) = trim_day(msgs, 20);
    assert!(!kept.is_empty());
    assert_eq!(kept[0].1, "消息0", "应从最早开始保留");
    assert_eq!(kept.len() + omitted, 5);
}

#[test]
fn fit_hits_drops_tail_when_over_budget() {
    let mk = |id: i64, text: &str| RecallHit {
        node_id: id, date_label: format!("第 {id} 天"), day: Some(id), summary: String::new(),
        messages: vec![("user".into(), text.into())], omitted: 0, score: 10, why: vec![],
    };
    let hits = vec![mk(1, "aaaa"), mk(2, "bbbb"), mk(3, "cccc")];
    // 成本按渲染后正文估算；预算给得紧，只装得下两条
    let (kept, dropped) = fit_hits(hits, 30, |s| s.chars().count());
    assert!(kept.len() < 3, "预算紧时应丢弃末尾：kept={}", kept.len());
    assert_eq!(kept.len() + dropped, 3);
}

/// 真实库副本：打印每天的"可检索标签"（手打 + 派生），并模拟几个查询看命中。
#[tokio::test]
#[ignore]
async fn diag_recall_tags_on_real_db() {
    use sqlx::sqlite::SqliteConnectOptions;
    let Ok(dir) = std::env::var("DW_DIAG_DB") else { eprintln!("未设置 DW_DIAG_DB"); return };
    // 注意：副本必须写**系统临时目录**，不能写真实数据目录 ——
    // 测试中断会留下几 MB 的垃圾文件在用户的 projects/ 里（2026-10-18 实测踩到）。
    let tmp = std::env::temp_dir().join("dw_diag_tags.db");
    let _ = std::fs::copy(&dir, &tmp);
    let opts = SqliteConnectOptions::new().filename(&tmp);
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    for sql in ["ALTER TABLE story_summaries ADD COLUMN branch_id TEXT"] {
        let _ = sqlx::query(sql).execute(&pool).await;
    }

    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM characters ORDER BY id")
        .fetch_all(&pool).await.unwrap_or_default();
    let keys: Vec<String> = sqlx::query_scalar("SELECT key FROM world_settings WHERE category IN ('地理','术语','势力','物品') ORDER BY id")
        .fetch_all(&pool).await.unwrap_or_default();
    eprintln!("\n角色：{:?}\n世界观键：{:?}", names, keys);

    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT date_label, COALESCE(day_summary,''), COALESCE(tags,'') FROM timeline_nodes
         WHERE node_type IN ('day','main') AND is_flashback=0 ORDER BY sort_order, id")
        .fetch_all(&pool).await.unwrap();

    eprintln!("\n=== 每天的标签（手打 + 从摘要派生）===");
    for (label, summary, hand_raw) in &rows {
        let hand: Vec<String> = hand_raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        let tags = derive_day_tags(summary, &hand, &names);
        eprintln!("  {label:<10} 手打={hand:?}  全部={tags:?}");
    }

    let mut lexicon: Vec<LexTerm> = TAG_WORDS.iter().map(|w| LexTerm { word: (*w).to_string(), kind: LexKind::TagWord }).collect();
    for k in &keys {
        let k = k.trim().to_string();
        if k.chars().count() >= 2 && !lexicon.iter().any(|x| x.word == k) { lexicon.push(LexTerm { word: k, kind: LexKind::Entity }); }
    }
    // 与生产一致：角色名也进词典（权重 20）
    for n in &names {
        let n = n.trim().to_string();
        if n.chars().count() >= 2 && !lexicon.iter().any(|x| x.word == n) { lexicon.push(LexTerm { word: n, kind: LexKind::Name }); }
    }

    for q in ["还记得那次牵手吗", "修女院那次", "当时院长说什么了"] {
        let hits = match_lexicon(q, &lexicon);
        eprintln!("\n查询「{q}」→ 词典命中 {:?}", hits.iter().map(|h| (h.word.clone(), h.kind)).collect::<Vec<_>>());
        // 逐天打分（不带意图事件，只看词典 + 手打标签）
        for (label, summary, hand_raw) in &rows {
            let hand: Vec<String> = hand_raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let hand_hits: Vec<String> = hand.iter().filter(|t| q.contains(t.as_str())).cloned().collect();
            let (sc, why) = score_summary(summary, None, None, &[], &[], &names, 0, &hand_hits, &hits, &RecallWeights::default());
            if sc > 0 { eprintln!("     {label} → {sc} 分  {why:?}"); }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
    let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
}

/// 真实库副本：跑一次 `explain`，看会带哪几天、为什么（C3 的"试算"就是它）
#[tokio::test]
#[ignore]
async fn diag_explain_on_real_db() {
    use sqlx::sqlite::SqliteConnectOptions;
    let Ok(dir) = std::env::var("DW_DIAG_DB") else { eprintln!("未设置 DW_DIAG_DB"); return };
    // 只读打开真实库：explain 只读不写，不需要复制
    // （复制会被正在运行的应用锁住 → "拒绝访问"，实测踩到）
    let opts = SqliteConnectOptions::new().filename(&dir).read_only(true);
    let pool = match SqlitePool::connect_with(opts).await {
        Ok(p) => p,
        Err(e) => { eprintln!("打开库失败：{e}"); return }
    };

    let lib = lib_pool_beside_story(&dir).await;
    for q in ["还记得那次牵手吗", "当时院长说什么了", "第 9 天发生什么"] {
        eprintln!("\n查询「{q}」");
        match explain(&pool, &None, &lib, q, 0).await {
            Ok(rows) => {
                for r in rows.iter().filter(|r| r.score > 0).take(6) {
                    eprintln!(
                        "  {:<10} {:>4} 分 {:<6} {}",
                        r.day_label, r.score,
                        if r.picked { "[会带出]" } else { "[未达门槛]" },
                        r.why.join(" · ")
                    );
                }
            }
            Err(e) => eprintln!("  ERR {e}"),
        }
    }
}

/// 诊断用：从 story.db 的路径推断 library.db（`<lib>/projects/<id>/story.db` → `<lib>/library.db`）
///
/// 不能用 `paths::library_db()` —— 那个依赖运行时的数据根目录（start.cmd 用 env 指到项目内
/// 的 data/library），而测试进程里没有那个 env，会指到 %APPDATA% 下的另一份。
async fn lib_pool_beside_story(story_db: &str) -> SqlitePool {
    // <root>/default/projects/<id>/story.db → 上三级是 default，library.db 就在那儿
    let lib = std::path::Path::new(story_db)
        .parent().and_then(|p| p.parent()).and_then(|p| p.parent())
        .map(|p| p.join("library.db"))
        .expect("推断 library.db 失败");
    let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&lib).read_only(true);
    SqlitePool::connect_with(opts).await.unwrap_or_else(|e| panic!("连 library.db 失败（{lib:?}）：{e}"))
}
