//! 伏笔清单（2026-10-30 阶段③）："当时不知道"才算伏笔。
//!
//! 状态机：proposed(AI 提议，等人判) / planted(埋下) / payoff(已回收) / overdue(逾期) / dropped(废弃)。
//! - `overdue` **不落库**，按 `窗口章数` 与当前最大章序现算（改了窗口/加了新章就自动跟着变）；
//! - 拒绝 AI 提议 = `dropped`（保留行，避免重复提同一句）；
//! - 手动/自动都写 `source`，界面可区分。

use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Serialize)]
pub struct Foreshadow {
    pub id: i64,
    pub text: String,
    pub planted_chapter_id: Option<i64>,
    pub planted_order: i64,
    pub window_chapters: i64,
    pub payoff_chapter_id: Option<i64>,
    pub status: String,
    pub source: String,
    pub note: String,
    /// 现算：埋下后超过窗口章数还没回收
    pub overdue: bool,
}

pub async fn ensure_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS foreshadows (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            branch_id          TEXT,
            text               TEXT NOT NULL,
            planted_chapter_id INTEGER,
            planted_node_id    INTEGER,
            window_chapters    INTEGER NOT NULL DEFAULT 3,
            payoff_chapter_id  INTEGER,
            status             TEXT NOT NULL DEFAULT 'planted',
            source             TEXT NOT NULL DEFAULT 'manual',
            note               TEXT NOT NULL DEFAULT '',
            created_at         TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 逾期判定（纯函数，好测）：只有 planted 才可能逾期；回收了/废弃了/还是提议的不算。
///
/// 2026-10-30 修：**不知道埋在哪一章就不判逾期**（`planted_order <= 0`）。
/// 手标伏笔只填一句话、不选章，`planted_chapter_id` 是 NULL → 章序按 0 算，
/// 于是故事有 4 章以上时（窗口默认 3）刚写下的一条立刻标红「逾期」。宁可显示"未逾期"。
pub fn is_overdue(status: &str, planted_order: i64, window_chapters: i64, max_order: i64) -> bool {
    status == "planted" && planted_order > 0 && max_order - planted_order > window_chapters
}

pub async fn list_in(pool: &SqlitePool, branch: &Option<String>) -> Result<Vec<Foreshadow>, sqlx::Error> {
    ensure_table(pool).await?;
    let clause = crate::story_db::branch_clause("f.branch_id", branch, 1);
    let max_order: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(order_no),0) FROM chapters")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    let sql = format!(
        "SELECT f.id, f.text, f.planted_chapter_id, COALESCE(c.order_no,0), f.window_chapters,
                f.payoff_chapter_id, f.status, f.source, f.note
         FROM foreshadows f LEFT JOIN chapters c ON c.id = f.planted_chapter_id
         WHERE {clause} ORDER BY f.id DESC"
    );
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    if let Some(b) = branch { q = q.bind(b); }
    let rows = q.fetch_all(pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        let status: String = r.try_get(6).unwrap_or_else(|_| "planted".into());
        let planted_order: i64 = r.try_get(3).unwrap_or(0);
        let window: i64 = r.try_get(4).unwrap_or(3);
        out.push(Foreshadow {
            id: r.try_get(0).unwrap_or(0),
            text: r.try_get(1).unwrap_or_default(),
            planted_chapter_id: r.try_get(2).ok(),
            planted_order,
            window_chapters: window,
            payoff_chapter_id: r.try_get(5).ok(),
            overdue: is_overdue(&status, planted_order, window, max_order),
            status,
            source: r.try_get(7).unwrap_or_else(|_| "manual".into()),
            note: r.try_get(8).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// 新建（手动默认 planted；AI 提议传 status=proposed / source=ai）
pub async fn create_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    text: &str,
    planted_chapter_id: Option<i64>,
    planted_node_id: Option<i64>,
    window_chapters: i64,
    status: &str,
    source: &str,
) -> Result<i64, sqlx::Error> {
    ensure_table(pool).await?;
    let text = text.trim();
    if text.is_empty() {
        return Err(sqlx::Error::Protocol("伏笔内容不能为空".into()));
    }
    sqlx::query_scalar(
        "INSERT INTO foreshadows(branch_id, text, planted_chapter_id, planted_node_id, window_chapters, status, source)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(branch)
    .bind(text)
    .bind(planted_chapter_id)
    .bind(planted_node_id)
    .bind(window_chapters.max(1))
    .bind(status)
    .bind(source)
    .fetch_one(pool)
    .await
}

/// 改状态 / 文本 / 窗口 / 回收章
pub async fn update_in(
    pool: &SqlitePool,
    id: i64,
    status: Option<&str>,
    text: Option<&str>,
    window_chapters: Option<i64>,
    payoff_chapter_id: Option<i64>,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE foreshadows SET
            status = COALESCE(?2, status),
            text = COALESCE(?3, text),
            window_chapters = COALESCE(?4, window_chapters),
            payoff_chapter_id = COALESCE(?5, payoff_chapter_id)
         WHERE id = ?1",
    )
    .bind(id)
    .bind(status)
    .bind(text)
    .bind(window_chapters)
    .bind(payoff_chapter_id)
    .execute(pool)
    .await?
    .rows_affected()
        > 0)
}

/// 删（彻底删行；拒绝 AI 提议请用 update status=dropped，别删，免得重复提）
pub async fn delete_in(pool: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM foreshadows WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected()
        > 0)
}


// ---------- 规则扫描（2026-10-30，零成本、保守） ----------
//
// 只在**日摘要**里找两类强信号：① 约定/承诺（后文必须兑现）② 明确没说完/没答的事。
// 宁缺勿滥：一条摘要最多取 3 句，一次扫描最多 5 条；命中的句子原样入 proposed，等人判。

const PROMISE_WORDS: &[&str] = &["答应", "约定", "保证", "发誓", "说好", "下次", "等你", "承诺"];
const OPEN_WORDS: &[&str] = &["没说", "没答", "没讲", "没提", "没人", "未说", "未答", "未提", "不知道", "不明白", "还没", "是谁", "为什么", "留下"];

/// 从一段摘要里挑候选句（纯函数，可测）
pub fn candidate_sentences(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split(|c| c == '。' || c == '！' || c == '？' || c == '\n' || c == ';' || c == '；') {
        let s = raw.trim();
        if s.chars().count() < 6 || s.chars().count() > 80 {
            continue;
        }
        let hit_promise = PROMISE_WORDS.iter().any(|w| s.contains(w));
        let hit_open = OPEN_WORDS.iter().any(|w| s.contains(w));
        if !(hit_promise || hit_open) {
            continue;
        }
        let s: String = s.chars().take(60).collect();
        if !out.contains(&s) {
            out.push(s);
        }
        if out.len() >= 3 {
            break;
        }
    }
    out
}

/// 扫描某一章（`chapter_id=None` 时扫全故事且只扫**已归章**的天），把候选写成 proposed。
/// 返回新增条数；已存在同文本的（含被拒绝成 dropped 的）跳过。
pub async fn scan_proposals_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    chapter_id: Option<i64>,
) -> Result<i64, sqlx::Error> {
    ensure_table(pool).await?;
    let mut sql = String::from(
        "SELECT n.id, COALESCE(n.day_summary,''), c.id FROM timeline_nodes n
         JOIN chapter_nodes cn ON cn.node_id = n.id
         JOIN chapters c ON c.id = cn.chapter_id
         WHERE 1=1"
    );
    if chapter_id.is_some() {
        sql.push_str(" AND c.id = ?1");
    }
    sql.push_str(" ORDER BY c.order_no ASC, n.sort_order ASC");
    let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()));
    if let Some(cid) = chapter_id {
        q = q.bind(cid);
    }
    let _ = branch; // 章本身已按分支隔离，节点取自章
    let rows = q.fetch_all(pool).await?;
    let mut added = 0i64;
    for r in &rows {
        if added >= 5 {
            break;
        }
        let summary: String = r.try_get(1).unwrap_or_default();
        let cid: i64 = r.try_get(2).unwrap_or(0);
        for cand in candidate_sentences(&summary) {
            if added >= 5 {
                break;
            }
            let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM foreshadows WHERE text = ?1")
                .bind(&cand)
                .fetch_one(pool)
                .await
                .unwrap_or(0);
            if exists > 0 {
                continue;
            }
            create_in(pool, branch, &cand, Some(cid), None, 3, "proposed", "ai").await?;
            added += 1;
        }
    }
    Ok(added)
}
/// 解析本地模型返回的候选行（2026-10-31，严格行协议，复用同一前缀）。
///
/// 兼容两种写法：`〔伏笔〕一句话` / `〔伏笔〕第 3 章：一句话`（后者把章号带回来）。
/// 模型回「无」或什么都没有 → 空列表（不许为了凑数硬找）。
pub fn parse_local_lines(reply: &str) -> Vec<(Option<i64>, String)> {
    let mut out: Vec<(Option<i64>, String)> = Vec::new();
    for line in reply.lines() {
        // 小模型常加列表符号/编号，先剥掉（- * • 1. 1、 ① 等），再认前缀；
        // 括号也宽容一点：【伏笔】［伏笔］[伏笔] 都当同一种。
        let t = line
            .trim()
            .trim_start_matches(['-', '*', '•', '·', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '.', '、', ')', '）', '▶', '·'])
            .trim();
        // 2026-11-04 实测：9b 有时只写「〔未说明来源的奖学金项目信息〕」，漏掉「伏笔」二字。
        // 先记住"这行本来有方括号"，再剥括号；剥完不是「伏笔」也不是别的协议行，就当正文收下。
        let bracketed = t.starts_with('【') || t.starts_with('［') || t.starts_with('[') || t.starts_with('〔');
        let t = t.trim_start_matches(['【', '［', '[', '〔']).trim();
        let rest_owned;
        let rest = match t.strip_prefix("伏笔") {
            Some(r) => r.trim_start_matches([':', '：', '〕', '】', '］', ']']).trim(),
            None => {
                if !bracketed {
                    continue;
                }
                let inner = t.trim_end_matches(['〕', '】', '］', ']']).trim();
                let is_protocol = ["状态变化", "作息", "告白", "导演", "态度", "承诺", "伏笔"]
                    .iter()
                    .any(|k| inner.starts_with(k));
                if is_protocol || inner.is_empty() {
                    continue;
                }
                rest_owned = inner.to_string();
                rest_owned.as_str()
            }
        };
        let mut ch = None;
        let mut body = rest;
        if let Some(idx) = rest.find('：').or_else(|| rest.find(':')) {
            let head = rest[..idx].trim();
            if head.starts_with('第') && head.contains('章') {
                let n: String = head.chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(v) = n.parse::<i64>() {
                    ch = Some(v);
                    let cut = idx + rest[idx..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
                    body = rest[cut..].trim();
                }
            }
        }
        // 去掉包裹的星号/引号（模型爱把心声、台词原样抄回来）
        let raw_text = body.trim().trim_matches(['*', '＊', '"', '“', '”', '「', '」', '『', '』', ' ', '\t']);
        // 2026-11-04 用户要求：输出里不许出现「用户」二字（模型会写「用户提前准备好的申请材料」）。
        // 提示词里已经写了一遍，但 7B/9B 会当没看见 —— 这里机械地剥掉，保证落库文本干净。
        let raw_text = scrub_actor_words(raw_text);
        let text: String = raw_text.chars().take(60).collect();
        // 心声/台词原文不算伏笔（2026-11-01 用户实测：库里进了一堆「*门是我锁的。他配的。*」）
        if body.contains('*') || body.contains('＊') {
            continue;
        }
        // 测试数据（【样例】开头）不当真
        if text.contains("【样例】") || text.contains("样例") {
            continue;
        }
        // 只收“东西/话题”，不收台词（2026-11-01 用户实测：模型把心声去掉星号后照抄成句子）。
        // 含第一/第三人称代词、问号、省略号、引号 → 判定为台词/心声，丢弃。
        // 只拦第一/第二人称与引号、问号、省略号：内心独白和台词几乎都带「我/你」，
        // 而「她说了一半」这类第三人称概括是合法候选，不能一起误杀。
        if text.contains('我') || text.contains('你')
            || text.contains('？') || text.contains('?') || text.contains('…') || text.contains('「')
        {
            continue;
        }
        // 去掉句尾句号，统一成“话题”形态
        let text = text.trim_end_matches(['。', '，', '；', '、', ' ']).to_string();
        if text.is_empty() {
            continue;
        }
        if text.is_empty() || text == "无" {
            continue;
        }
        // 模型会**照抄格式模板**（2026-11-01 实测：真候选后面跟一行「〔伏笔〕第 1 章：30 字以内的一句话」），
        // 这种不含信息的行必须丢掉，否则会当成真伏笔入库。
        if text.contains("30 字") || text.contains("30字") || text == "一句话" || text.contains("第 N 章") {
            continue;
        }
        if !out.iter().any(|(_, x)| x == &text) {
            out.push((ch, text));
        }
    }
    out
}


#[cfg(test)]
mod local_parse_tests {
    use super::*;

    #[test]
    fn tolerates_list_markers_and_brackets() {
        let r = "1. 〔伏笔〕第 3 章：那把钥匙\n- 【伏笔】她说了一半\n[伏笔] 门口的脚步\n无";
        let got = parse_local_lines(r);
        assert_eq!(got.len(), 3, "三种写法都要认：{got:?}");
        assert_eq!(got[0].0, Some(3));
        assert_eq!(got[1].1, "她说了一半");
    }


    #[test]
    fn rejects_heart_voice_and_sample_pollution() {
        let r = "〔伏笔〕第 1 章：*门是我锁的。他配的。*\n〔伏笔〕第 1 章：把话咽了回去。\n〔伏笔〕第 1 章：【样例】第 7 天的夜里，把话咽了回去。";
        let got = parse_local_lines(r);
        assert_eq!(got.len(), 1, "心声原文与样例数据都不算：{got:?}");
        assert_eq!(got[0].1, "把话咽了回去");
    }
    #[test]
    fn drops_template_echo_lines() {
        let r = "〔伏笔〕第 1 章：用户在提到怀表时被打断了。\n〔伏笔〕第 1 章：30 字以内的一句话\n无";
        let got = parse_local_lines(r);
        assert_eq!(got.len(), 1, "模板行不能入库：{got:?}");
        // 2026-11-04：落库文本里不许留「用户」（scrub_actor_words 会剥掉）
        assert_eq!(got[0].1, "在提到怀表时被打断了");
    }
}


    /// 2026-11-04：输出里不许出现「用户」「AI」；删完留下的「的的」也要收拾。
    #[test]
    fn scrub_removes_actor_words() {
        assert_eq!(scrub_actor_words("用户提前准备好的申请材料"), "提前准备好的申请材料");
        assert_eq!(scrub_actor_words("用户声称认识很久的时长"), "声称认识很久的时长");
        assert_eq!(scrub_actor_words("AI 和用户之间的约定"), "和之间的约定");
        assert_eq!(scrub_actor_words("的门闩"), "门闩");
    }

    /// 2026-11-04：模型漏写「伏笔」二字、只给「〔东西〕」时也要收（但不能吞别的协议行）。
    #[test]
    fn accepts_bracketed_lines_without_prefix() {
        let r = "〔未说明来源的奖学金项目信息〕\n〔状态变化〕受伤：被推下台阶\n〔伏笔〕抽屉里的两把钥匙";
        let got = parse_local_lines(r);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].1.contains("奖学金"));
        assert!(got[1].1.contains("两把钥匙"));
    }

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> SqlitePool {
        let p = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::query("CREATE TABLE chapters (id INTEGER PRIMARY KEY, order_no INTEGER)")
            .execute(&p).await.unwrap();
        sqlx::query("INSERT INTO chapters VALUES(1,1),(2,2),(3,3),(4,4),(5,5)")
            .execute(&p).await.unwrap();
        ensure_table(&p).await.unwrap();
        p
    }

    #[test]
    fn overdue_only_for_planted() {
        assert!(is_overdue("planted", 1, 3, 5), "1 → 5 超过 3 章窗口");
        assert!(!is_overdue("planted", 4, 3, 5), "才过 1 章");
        assert!(!is_overdue("payoff", 1, 3, 5), "已回收不算逾期");
        assert!(!is_overdue("dropped", 1, 3, 5));
        assert!(!is_overdue("proposed", 1, 3, 5), "提议还没埋，不算");
        // 2026-10-30 修：手标伏笔不选章（章序 0）时不该立刻判逾期
        assert!(!is_overdue("planted", 0, 3, 9), "不知道埋在哪章 ≠ 逾期");
    }

    #[tokio::test]
    async fn create_list_and_refuse_proposal() {
        let pool = pool().await;
        let id = create_in(&pool, &None, "那把钥匙的齿形", Some(1), Some(7), 3, "planted", "manual").await.unwrap();
        let ai = create_in(&pool, &None, "她没说完的半句", Some(2), None, 3, "proposed", "ai").await.unwrap();
        let list = list_in(&pool, &None).await.unwrap();
        assert_eq!(list.len(), 2);
        let mine = list.iter().find(|f| f.id == id).unwrap();
        assert_eq!(mine.status, "planted");
        assert_eq!(mine.source, "manual");
        assert!(mine.overdue, "第 1 章埋、现在到第 5 章 → 逾期");
        let prop = list.iter().find(|f| f.id == ai).unwrap();
        assert_eq!(prop.status, "proposed");
        assert!(!prop.overdue, "提议不算逾期");
        update_in(&pool, ai, Some("dropped"), None, None, None).await.unwrap();
        assert_eq!(list_in(&pool, &None).await.unwrap().iter().find(|f| f.id == ai).unwrap().status, "dropped");
    }

    #[tokio::test]
    async fn empty_text_is_rejected() {
        let pool = pool().await;
        assert!(create_in(&pool, &None, "   ", None, None, 3, "planted", "manual").await.is_err());
    }


    #[test]
    fn candidates_pick_promises_and_open_questions_only() {
        let text = "她在门口站了一会儿。她答应过下次会说实话。关于那把钥匙的齿形，没人解释。";
        let got = candidate_sentences(text);
        assert!(got.iter().any(|s| s.contains("答应过下次")), "{got:?}");
        assert!(got.iter().any(|s| s.contains("没人解释")), "{got:?}");
        assert!(!got.iter().any(|s| s.contains("站了一会儿")), "普通叙述不该进来");
    }

    #[test]
    fn candidates_ignore_short_and_long() {
        assert!(candidate_sentences("答应").is_empty(), "太短");
        let long = "答应".to_string() + &"啊".repeat(200);
        assert!(candidate_sentences(&long).is_empty(), "太长");
    }

    #[test]
    fn candidates_dedupe_identical_sentences() {
        let text = "她答应过下次会说实话。她答应过下次会说实话。";
        assert_eq!(candidate_sentences(text).len(), 1);
    }

    /// 扫描：只扫有日摘要的章；同文本不重复提（含被拒绝的）
    #[tokio::test]
    async fn scan_writes_proposals_and_skips_duplicates() {
        let pool = pool().await;
        sqlx::query("CREATE TABLE timeline_nodes (id INTEGER PRIMARY KEY, day_summary TEXT, sort_order INTEGER)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE chapter_nodes (chapter_id INTEGER, node_id INTEGER, PRIMARY KEY(chapter_id,node_id))")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO timeline_nodes VALUES(7,'她答应过下次会说实话。关于那把钥匙，没人解释。',1),(8,'她走开了。',2)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO chapter_nodes VALUES(1,7),(1,8)").execute(&pool).await.unwrap();
        let n = scan_proposals_in(&pool, &None, Some(1)).await.unwrap();
        assert_eq!(n, 2, "两句强信号：答应 / 没人解释");
        let again = scan_proposals_in(&pool, &None, Some(1)).await.unwrap();
        assert_eq!(again, 0, "重复扫描不再新增");
        let list = list_in(&pool, &None).await.unwrap();
        assert!(list.iter().all(|f| f.status == "proposed" && f.source == "ai"));
    }

    #[tokio::test]
    async fn branch_scope_is_respected() {
        let pool = pool().await;
        create_in(&pool, &Some("b1".into()), "分支伏笔", None, None, 3, "planted", "manual").await.unwrap();
        assert_eq!(list_in(&pool, &None).await.unwrap().len(), 0);
        assert_eq!(list_in(&pool, &Some("b1".into())).await.unwrap().len(), 1);
    }
}

/// 伏笔扫描 v2 ①（2026-11-03）：**不再喂全部日摘要**，只给本批覆盖的那几天 + 往前 `back` 天。
///
/// 为什么：伏笔是局部现象（"这句话没说出口"根本不需要前 15K 剧情）。把整本历史塞进去，
/// 模型要一边维持阅读连贯一边分类，注意力被故事流吃光，「无」就成了最省力的全局答案 ——
/// 用户 2026-11-02 的判断，这里先做最省事也最见效的一步。
///
/// `days` 已按时间线排序；返回窗口内**索引**（含端点）。本批今天数落在窗口外 → 返回空。
pub(crate) fn context_window(days: &[(i64, String, String)], batch_ids: &std::collections::HashSet<i64>, back: usize) -> Vec<usize> {
    let first = days.iter().position(|(id, _, _)| batch_ids.contains(id));
    let last = days.iter().rposition(|(id, _, _)| batch_ids.contains(id));
    match (first, last) {
        (Some(f), Some(l)) => {
            let start = f.saturating_sub(back);
            (start..=l).collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod context_window_tests {
    use super::context_window;

/// 2026-11-03（伏笔扫描 v2 ①）：前情窗口 = 本批覆盖的那几天 + 往前 3 天。
#[test]
fn context_window_is_local_not_global() {
    // 10 天（id 1..10）
    let days: Vec<(i64, String, String)> = (1..=10)
        .map(|d| (d, format!("第 {d} 天"), format!("摘要{d}")))
        .collect();
    // 本批 = 第 7、8 天
    let batch: std::collections::HashSet<i64> = [7i64, 8].into_iter().collect();
    let w = context_window(&days, &batch, 3);
    assert_eq!(w, vec![3, 4, 5, 6, 7], "本批 7、8 天 + 往前 3 天 = 第 4~8 天（索引 3..=7）");
    // 窗口长度固定上限：本批 2 天 + 前情 3 天
    assert!(w.len() <= 2 + 3, "窗口不该超过 本批 + 前情：{w:?}");
    // 第一页（本批是开头几天）→ 前面没有前情，只给本批
    let head: std::collections::HashSet<i64> = [1i64, 2].into_iter().collect();
    assert_eq!(context_window(&days, &head, 3), vec![0, 1]);
    // 本批今天数对不上 → 空窗口（调用方给"直接看原文"）
    let none: std::collections::HashSet<i64> = [99].into_iter().collect();
    assert!(context_window(&days, &none, 3).is_empty());
}
}


/// 取第一步里判「→ 否」的**细节原文**（2026-11-03）。
///
/// 用途：模型常「标了 12 条否却只写 1 条伏笔」（用户实测 #119），叫它补它干脆不回（#120）。
/// 这些「→ 否」条目是它自己判过的结论，缺的直接拿来兜底，不再问第二遍。
pub fn no_lines(reply: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in reply.lines() {
        let t = line.trim();
        let Some(pos) = t.find("→") else { continue };
        if !t[pos + "→".len()..].trim_start().starts_with('否') {
            continue;
        }
        let head = t[..pos].trim();
        let head = head.trim_start_matches(|c: char| c.is_ascii_digit());
        let head = head.trim_start_matches(['.', '、', ')', '）']).trim();
        let mut head = head.to_string();
        if let Some(open) = head.rfind('（') {
            if head.ends_with('）') && open > 0 {
                head.truncate(open);
            }
        }
        let head = head.trim().to_string();
        if (4..=40).contains(&head.chars().count()) {
            out.push(head);
        }
    }
    out
}

/// 第一步完整性自检（2026-11-03）：数「带编号的条目」和「标了 →是 / →否 的条目」。
pub fn checklist_balance(reply: &str) -> (usize, usize) {
    let mut items = 0usize;
    let mut marked = 0usize;
    for line in reply.lines() {
        let t = line.trim();
        let starts_digit = matches!(t.chars().next(), Some(c) if c.is_ascii_digit());
        let sep: String = t.chars().skip_while(|c| c.is_ascii_digit()).take(1).collect();
        if !(starts_digit && matches!(sep.as_str(), "." | "、" | ")" | "）")) {
            continue;
        }
        items += 1;
        if t.contains("→ 是") || t.contains("→ 否") || t.contains("→是") || t.contains("→否") {
            marked += 1;
        }
    }
    (items, marked)
}

#[cfg(test)]
mod scan_helper_tests {
    use super::{checklist_balance, no_lines};

/// 2026-11-03：第一步里判「→ 否」的条目要能取出来兜底（模型常只写 1 条伏笔）。
#[test]
fn no_lines_extract_details_without_markers() {
    let reply = [
        "1. 夏婉琴没抬眼（没解释的失态）→ 否",
        "2. 指节慢慢收紧 → 是",
        "3. 今天怀表的事没细说 → 否",
        "4. 再数下去，我就不知道自己会说什么了 →否",
    ]
    .join("\n");
    let got = no_lines(&reply);
    assert_eq!(got.len(), 3, "只取 →否 的三条：{got:?}");
    assert_eq!(got[0], "夏婉琴没抬眼", "编号和括注都剥掉");
    assert_eq!(got[1], "今天怀表的事没细说");
    assert!(got[2].contains("再数下去"));
}

/// 2026-11-03：第一步丢行自检（用户实测：列 6 条、只判 5 条）。
#[test]
fn checklist_balance_counts_items_and_marks() {
    assert_eq!(checklist_balance("1. 甲 → 否\n2. 乙 → 是"), (2, 2));
    assert_eq!(checklist_balance("1. 甲\n2. 乙 → 是\n3. 丙 → 否"), (3, 2), "第 1 条没标判断要被数出来");
    assert_eq!(checklist_balance("1. 甲→否\n2. 乙→是"), (2, 2), "半角箭头也要认");
    assert_eq!(checklist_balance("〔伏笔〕没说完的话"), (0, 0));
}
}

/// 去掉「用户」「AI」这类**出戏的人称**（2026-11-04 用户要求）。
///
/// 模型会把叙述视角词抄进伏笔里（「用户提前准备好的申请材料」），提示词说一遍不够，
/// 这里机械剥掉；删除后留下的「的的」「的：」也一并收拾。
pub fn scrub_actor_words(s: &str) -> String {
    let mut out = s.replace("用户", "").replace("AI", "").replace("ai", "");
    while out.contains("的的") {
        out = out.replace("的的", "的");
    }
    out.trim().trim_start_matches(['的', '：', '，', '、', ' ']).trim().to_string()
}
