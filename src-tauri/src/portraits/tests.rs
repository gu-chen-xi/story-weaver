// 从 `portraits.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `portraits::tests` —— 因为 `portraits.rs` + `portraits/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `portraits`，一行都不用改写。

use super::*;

/// 临时测试库（**绝不碰真实数据**）：
/// - library 侧：`<test_root>/portraits-test/<tag>/lib.db`
/// - 故事侧：`<test_root>/portraits-test/<tag>/story-<sid>.db`
struct Fixture {
    dir: std::path::PathBuf,
    pool: SqlitePool,
}

impl Fixture {
    async fn new(tag: &str) -> Fixture {
        let dir = crate::paths::test_root()
            .join("portraits-test")
            .join(format!("{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建测试目录");
        let db = dir.join("lib.db");
        let url = format!("sqlite://{}?mode=rwc", db.display().to_string().replace('\\', "/"));
        let pool = SqlitePool::connect(&url).await.expect("建临时库");
        // 故事列表（resolve_bound 要查标题）
        sqlx::query("CREATE TABLE IF NOT EXISTS stories (id INTEGER PRIMARY KEY, title TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        ensure_table(&pool).await.expect("建 portraits 表");
        Fixture { dir, pool }
    }

    fn story_db_path(&self, sid: i64) -> std::path::PathBuf {
        self.dir.join(format!("story-{sid}.db"))
    }

    /// 解析器：指向本 fixture 的临时故事库
    fn resolver(&self) -> impl Fn(i64) -> std::path::PathBuf + Send + Sync + '_ {
        move |sid: i64| self.story_db_path(sid)
    }

    async fn add_story(&self, sid: i64, title: &str) {
        sqlx::query("INSERT INTO stories(id, title) VALUES(?1, ?2)")
            .bind(sid)
            .bind(title)
            .execute(&self.pool)
            .await
            .unwrap();
    }

    async fn add_character(&self, sid: i64, cid: i64, name: &str) {
        let db = self.story_db_path(sid);
        let url = format!("sqlite://{}?mode=rwc", db.display().to_string().replace('\\', "/"));
        let sp = SqlitePool::connect(&url).await.expect("建临时故事库");
        sqlx::query("CREATE TABLE IF NOT EXISTS characters (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .execute(&sp)
            .await
            .unwrap();
        sqlx::query("INSERT INTO characters(id, name) VALUES(?1, ?2)")
            .bind(cid)
            .bind(name)
            .execute(&sp)
            .await
            .unwrap();
        sp.close().await;
    }

    /// 插入一张素材；`bound` = Some((story_id, character_id))
    async fn add_portrait(&self, name: &str, bound: Option<(i64, i64)>) -> i64 {
        let (sid, cid) = match bound {
            Some((s, c)) => (Some(s), Some(c)),
            None => (None, None),
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO portraits(name, story_id, character_id) VALUES(?1, ?2, ?3) RETURNING id",
        )
        .bind(name)
        .bind(sid)
        .bind(cid)
        .fetch_one(&self.pool)
        .await
        .unwrap();
        id

    }

    async fn cleanup(&self) {
        self.pool.close().await;
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// **核心正确性**：批量版必须和"逐行版（oracle）"完全相同 —— 内容与顺序都要一致。
///
/// 这条就是防"优化着优化着把字段漏填/顺序改错"。
#[tokio::test]
async fn batch_list_matches_rowbyrow_oracle() {
    let f = Fixture::new("batch-eq").await;
    f.add_story(7, "初次相见").await;
    f.add_story(9, "破碎的她").await;
    f.add_character(7, 2, "夏婉琴").await;
    f.add_character(7, 4, "院长").await;
    f.add_character(9, 1, "她").await;

    // 覆盖各种情况：有绑定 / 无绑定 / 只有 story 没 character / 多版本 / 无版本
    f.add_portrait("绑定-夏婉琴", Some((7, 2))).await;
    f.add_portrait("绑定-院长", Some((7, 4))).await;
    f.add_portrait("未绑定", None).await;
    f.add_portrait("绑定-她", Some((9, 1))).await;
    f.add_portrait("角色不存在", Some((7, 999))).await;
    f.add_portrait("故事不存在", Some((404, 1))).await;

    let resolver = f.resolver();
    let got = portrait_list_in_with(&f.pool, &resolver).await.unwrap();
    let want = portrait_list_rowbyrow_in(&f.pool, &resolver).await.unwrap();

    assert_eq!(got.len(), want.len(), "数量必须一致");
    for (i, (a, b)) in got.iter().zip(want.iter()).enumerate() {
        assert_eq!(a.id, b.id, "第 {i} 项顺序不一致（id）");
        assert_eq!(a.name, b.name, "第 {i} 项 name");
        assert_eq!(a.story_id, b.story_id, "第 {i} 项 story_id");
        assert_eq!(a.character_id, b.character_id, "第 {i} 项 character_id");
        assert_eq!(a.bound_story, b.bound_story, "第 {i} 项 bound_story");
        assert_eq!(a.bound_character, b.bound_character, "第 {i} 项 bound_character");

    }

    // 顺带把"绑定名字真的解析出来了"钉住（不然上面两者都为 None 也会相等）
    let xia = got.iter().find(|x| x.name == "绑定-夏婉琴").unwrap();
    assert_eq!(xia.bound_story.as_deref(), Some("初次相见"));
    assert_eq!(xia.bound_character.as_deref(), Some("夏婉琴"));

    let none = got.iter().find(|x| x.name == "未绑定").unwrap();
    assert!(none.bound_story.is_none() && none.bound_character.is_none());


    // 角色不存在 → 故事标题有、角色名为 None（与原逻辑一致）
    let ghost = got.iter().find(|x| x.name == "角色不存在").unwrap();
    assert_eq!(ghost.bound_story.as_deref(), Some("初次相见"));
    assert!(ghost.bound_character.is_none());

    f.cleanup().await;
}

/// 空表也要正常（不能因为"0 张素材"就越界/报错）。
#[tokio::test]
async fn batch_list_handles_empty_table() {
    let f = Fixture::new("batch-empty").await;
    let resolver = f.resolver();
    let got = portrait_list_in_with(&f.pool, &resolver).await.unwrap();
    assert!(got.is_empty());
    f.cleanup().await;
}

/// **结构守卫**：list 路径里不许再出现逐行补数据的调用。
///
/// 用 `include_str!` 读**被守护的那份源码** —— 这类"批量改回逐行"的退化最容易悄悄发生，
/// 而且它不会让测试失败（结果一样，只是变慢），所以需要一条专门的守卫。
///
/// 2026-10-22 拆分注意：`portrait_list_in` 与 oracle 一起搬到了 `portraits/read.rs`，
/// 所以这里读 `read.rs`（原来是 `../portraits.rs`）。**守卫要跟着被守护的代码走**。
#[test]
fn list_path_must_not_call_per_row_loader() {
    const SRC: &str = include_str!("read.rs");
    let start = SRC.find("pub async fn portrait_list_in(").expect("找不到 portrait_list_in");
    // 扫描区间 = 从批量入口起，到"逐行对照版（oracle）"的文档注释之前。
    // 用 oracle 的注释当结束标记（它一定在后面），避免把 oracle 自身也算进来。
    let end = SRC[start..]
        .find("/// **旧实现的逐行版本**")
        .map(|i| start + i)
        .unwrap_or_else(|| SRC[start..].find("#[cfg(test)]").map(|i| start + i).unwrap_or(SRC.len()));
    let body = &SRC[start..end];
    assert!(body.len() > 200, "扫描区间异常短（{0} 字节），结束标记可能没找到", body.len());
    assert!(
        !body.contains("row_to_item_bound"),
        "portrait_list_in 里又出现了逐行补数据的 row_to_item_bound —— 这会把 N+1 带回来.\n{body}"
    );
    assert!(
        body.contains("resolve_bound_batch"),
        "portrait_list_in 应该用批量版 resolve_bound_batch"
    );
    // 版本历史已在 2026-10-25 整体移除（用户：太吵、多余），所以这里不再要求 load_versions_batch。
    assert!(
        !body.contains("load_versions_batch"),
        "版本历史已移除，portrait_list_in 里不该再查版本历史"
    );
}

/// **性能对照**：素材多时批量版应明显更快。
///
/// 旧实现每张已绑定素材都要**新开一个 story.db 连接池**，那才是真正的大头。
/// 这里造 60 张（其中 40 张绑定到同一故事），比较两条路径耗时。
#[tokio::test]
async fn batch_is_much_faster_than_rowbyrow() {
    let f = Fixture::new("batch-perf").await;
    f.add_story(7, "初次相见").await;
    for cid in 1..=8 {
        f.add_character(7, cid, &format!("角色{cid}")).await;
    }
    for i in 0..60i64 {
        let bound = if i < 40 { Some((7, (i % 8) + 1)) } else { None };
        f.add_portrait(&format!("素材{i}"), bound).await;
    }

    let resolver = f.resolver();

    // 预热一次（避免首次建连/文件缓存影响）
    let _ = portrait_list_rowbyrow_in(&f.pool, &resolver).await.unwrap();
    let _ = portrait_list_in_with(&f.pool, &resolver).await.unwrap();

    let t0 = std::time::Instant::now();
    let slow = portrait_list_rowbyrow_in(&f.pool, &resolver).await.unwrap();
    let slow_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = std::time::Instant::now();
    let fast = portrait_list_in_with(&f.pool, &resolver).await.unwrap();
    let fast_ms = t1.elapsed().as_secs_f64() * 1000.0;

    println!("[perf] 逐行 {slow_ms:.1} ms  vs  批量 {fast_ms:.1} ms  （{:.1}x）", slow_ms / fast_ms.max(0.001));
    assert_eq!(slow.len(), fast.len());
    assert_eq!(slow.len(), 60);

    // 断言"不变慢"是硬性要求；"快多少"只打印（不同机器波动大，不做硬断言）
    assert!(
        fast_ms <= slow_ms * 1.5 + 5.0,
        "批量版不该比逐行版慢：逐行 {slow_ms:.1} ms / 批量 {fast_ms:.1} ms"
    );

    f.cleanup().await;
}

/// 素材库的查询**不许写 `SELECT *`**（2026-10-25 踩坑）。
///
/// `row_to_item` / `row_to_item_bound` 是**按下标读列**的，`*` 会随表结构变化而错位 ——
/// 实测报错：`error occurred while decoding column 11: Rust type String (TEXT) is not
/// compatible with SQL type INTEGER`。统一用 `store.rs` 的 `COLS`。
#[test]
fn portraits_queries_never_use_select_star() {
    let sources: [(&str, &str); 5] = [
        ("read.rs", include_str!("read.rs")),
        ("write.rs", include_str!("write.rs")),
        ("store.rs", include_str!("store.rs")),
        ("bind.rs", include_str!("bind.rs")),
        ("export.rs", include_str!("export.rs")),
    ];
    for (name, src) in sources {
        assert!(
            !src.contains("SELECT * FROM"),
            "portraits/{name} 里出现 `SELECT * FROM`：行映射按下标读列，必须用 COLS（见 read.rs 的注释）"
        );
    }
}