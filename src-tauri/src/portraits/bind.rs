// 从 `portraits.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `portraits.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use bind::*;` 取回，
// 外部 `crate::portraits::xxx` 的调用路径**全部不变**。

use super::*;

/// 批量解析绑定名称的结果。
#[derive(Default)]
pub(super) struct BoundNames {
    /// story_id -> 故事标题
    ///
    /// `pub(super)`：这个结构体搬到子模块后，父模块（portraits.rs）里的回填逻辑要直接读字段。
    /// 只对本模块族可见，不会漏到 crate 外。
    pub(super) stories: HashMap<i64, String>,
    /// (story_id, character_id) -> 角色名
    pub(super) characters: HashMap<(i64, i64), String>,
}

/// **批量**解析绑定名称（替代"每张素材都查一次、而且每个都新开一个故事库连接"的 N+1）。
///
/// 查询次数：`1`（故事标题，一条 IN） + `每个不同故事 1 条`（角色名，一条 IN）。
/// 原实现是：`每张素材 1~2 条 + 每张已绑定素材各新开一个 story.db 连接池`。
///
/// 语义同原版：查不到就是 `None`，**不报错**（绑定名字取不到不该让素材列表失败）。
/// 故事库路径的解析方式。默认走 `story_db_path`；
/// **测试里会注入一个指向临时目录的版本** —— 否则单元测试会去读写真实用户数据。
// 必须带 Send + Sync：这些 future 会被 tauri 的异步运行时跨线程调度，
// 裸 `dyn Fn` 不满足 Send，会让所有调用它的 command 报 "future cannot be sent between threads"。
pub(super) type StoryDbResolver<'a> = &'a (dyn Fn(i64) -> std::path::PathBuf + Send + Sync);

pub(super) fn default_story_db(sid: i64) -> std::path::PathBuf {
    crate::story_db::story_db_path(&sid.to_string())
}

// 保留未用：批量解析绑定（当前逐条调用）
#[allow(dead_code)]
pub(super) async fn resolve_bound_batch(
    pool: &SqlitePool,
    pairs: &[(Option<i64>, Option<i64>)],
) -> BoundNames {
    resolve_bound_batch_with(pool, pairs, &(default_story_db as fn(i64) -> std::path::PathBuf)).await
}

pub(super) async fn resolve_bound_batch_with(
    pool: &SqlitePool,
    pairs: &[(Option<i64>, Option<i64>)],
    story_db: StoryDbResolver<'_>,
) -> BoundNames {
    let mut out = BoundNames::default();

    // ---- 故事标题：一条 IN 查询 ----
    let mut story_ids: Vec<i64> = pairs.iter().filter_map(|(s, _)| *s).collect();
    story_ids.sort_unstable();
    story_ids.dedup();
    for chunk in story_ids.chunks(500) {
        let ph = (1..=chunk.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT id, title FROM stories WHERE id IN ({ph})");
        let mut q = sqlx::query(&sql);
        for id in chunk {
            q = q.bind(*id);
        }
        if let Ok(rows) = q.fetch_all(pool).await {
            for r in &rows {
                if let (Ok(id), Ok(title)) = (r.try_get::<i64, _>(0), r.try_get::<String, _>(1)) {
                    out.stories.insert(id, title);
                }
            }
        }
    }

    // ---- 角色名：按故事分组，每个故事**一条** IN 查询（每故事只开一次连接）----
    let mut by_story: HashMap<i64, Vec<i64>> = HashMap::new();
    for (s, c) in pairs {
        if let (Some(sid), Some(cid)) = (s, c) {
            by_story.entry(*sid).or_default().push(*cid);
        }
    }
    for (sid, mut cids) in by_story {
        cids.sort_unstable();
        cids.dedup();
        let path = story_db(sid);
        let opts = match SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display())) {
            Ok(o) => o,
            Err(_) => continue,
        };
        let sp = match SqlitePool::connect_with(opts).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        for chunk in cids.chunks(500) {
            let ph = (1..=chunk.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!("SELECT id, name FROM characters WHERE id IN ({ph})");
            let mut q = sqlx::query(&sql);
            for id in chunk {
                q = q.bind(*id);
            }
            if let Ok(rows) = q.fetch_all(&sp).await {
                for r in &rows {
                    if let (Ok(id), Ok(name)) =
                        (r.try_get::<i64, _>(0), r.try_get::<String, _>(1))
                    {
                        out.characters.insert((sid, id), name);
                    }
                }
            }
        }
        sp.close().await;
    }

    out
}

/// 解析绑定名称：故事标题来自 library.db，角色名来自该故事的 story.db。
pub(super) async fn resolve_bound(
    pool: &SqlitePool,
    story_id: Option<i64>,
    character_id: Option<i64>,
) -> (Option<String>, Option<String>) {
    resolve_bound_with(pool, story_id, character_id, &(default_story_db as fn(i64) -> std::path::PathBuf)).await
}

/// `resolve_bound` 的可注入版本（测试用临时目录，避免读写真实用户数据）。
pub(super) async fn resolve_bound_with(
    pool: &SqlitePool,
    story_id: Option<i64>,
    character_id: Option<i64>,
    story_db: StoryDbResolver<'_>,
) -> (Option<String>, Option<String>) {
    let mut bs: Option<String> = None;
    let mut bc: Option<String> = None;
    if let Some(sid) = story_id {
        if let Ok(r) = sqlx::query("SELECT title FROM stories WHERE id = ?1")
            .bind(sid)
            .fetch_one(pool)
            .await
        {
            if let Ok(t) = r.try_get::<String, _>(0) {
                bs = Some(t);
            }
        }
        if let Some(cid) = character_id {
            let p = story_db(sid);
            if let Ok(opts) = SqliteConnectOptions::from_str(&format!("sqlite:{}", p.display())) {
                if let Ok(sp) = SqlitePool::connect_with(opts).await {
                    if let Ok(r) = sqlx::query("SELECT name FROM characters WHERE id = ?1")
                        .bind(cid)
                        .fetch_one(&sp)
                        .await
                    {
                        if let Ok(nm) = r.try_get::<String, _>(0) {
                            bc = Some(nm);
                        }
                    }
                    sp.close().await;
                }
            }
        }
    }
    (bs, bc)
}
