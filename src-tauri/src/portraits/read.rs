// 从 `portraits.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `portraits.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use read::*;` 取回，
// 外部 `crate::portraits::xxx` 的调用路径**全部不变**。

use super::*;

/// 全部素材（按最近更新倒序）。
#[tauri::command]
pub async fn portrait_list() -> Result<Vec<PortraitItem>, String> {
    let p = pool().await?;
    portrait_list_in(&p).await
}

/// `portrait_list` 的核心（接 pool，供测试直连）。
///
/// ## 为什么不再逐行补数据（原来这里是 N+1）
///
/// 旧实现是「查一次列表 → 每行调 `row_to_item_bound`」，而那个函数每行要：
/// 查故事标题(1) + **新开一个 story.db 连接池** + 查角色名(1) + 查版本历史(1)。
///
/// 也就是说素材数为 N、其中 M 张绑定了角色时，总开销是：
/// `1 + N 条查询 + M 次新建连接池`（连接池创建远贵于单条查询）。
///
/// 现在改成**批量**：
/// `1`（列表） + `1`（版本历史，一条 IN） + `1`（故事标题，一条 IN）
/// + `每个不同故事 1 条`（角色名，一条 IN）+ `每个不同故事 1 次连接`。
///
/// 素材越多、越集中在少数故事里，差距越大。
pub async fn portrait_list_in(p: &SqlitePool) -> Result<Vec<PortraitItem>, String> {
    portrait_list_in_with(p, &(default_story_db as fn(i64) -> std::path::PathBuf)).await
}

/// `portrait_list_in` 的可注入版本（换成别的故事库路径解析方式；测试用临时目录）。
pub(super) async fn portrait_list_in_with(
    p: &SqlitePool,
    story_db: StoryDbResolver<'_>,
) -> Result<Vec<PortraitItem>, String> {
    ensure_table(p).await?;
    let rows = sqlx::query(
        &format!("SELECT {} FROM portraits ORDER BY updated_at DESC, id DESC", COLS)
    )
    .fetch_all(p)
    .await
    .map_err(|e| e.to_string())?;

    // ① 先把纯字段解析完（这一步原本就不查库）
    let mut items: Vec<PortraitItem> = Vec::with_capacity(rows.len());
    for r in &rows {
        items.push(row_to_item(r, p)?);
    }
    if items.is_empty() {
        return Ok(items);
    }



    // ③ 绑定名称：一次查完（按故事分组）
    let pairs: Vec<(Option<i64>, Option<i64>)> =
        items.iter().map(|i| (i.story_id, i.character_id)).collect();
    let bounds = resolve_bound_batch_with(p, &pairs, story_db).await;

    // ④ 回填
    for it in items.iter_mut() {
        if let Some(sid) = it.story_id {
            it.bound_story = bounds.stories.get(&sid).cloned();
            if let Some(cid) = it.character_id {
                it.bound_character = bounds.characters.get(&(sid, cid)).cloned();
            }
        }
    }
    Ok(items)
}

/// **旧实现的逐行版本**，只用于测试做"对照答案"（oracle）。
///
/// 保留它是有意的：批量版改错了、顺序变了、少填了字段，都能靠"两者必须完全相等"抓出来。
/// 生产代码不应调用它（`#[cfg(test)]` 把它挡在正式构建之外）。
#[cfg(test)]
pub async fn portrait_list_rowbyrow_in(
    p: &SqlitePool,
    story_db: StoryDbResolver<'_>,
) -> Result<Vec<PortraitItem>, String> {
    ensure_table(p).await?;
    let rows = sqlx::query(
        &format!("SELECT {} FROM portraits ORDER BY updated_at DESC, id DESC", COLS)
    )
    .fetch_all(p)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        // 逐行的老逻辑（含新开连接池），但路径走注入的 resolver
        let mut item = row_to_item(r, p)?;
        let (bs, bc) = resolve_bound_with(p, item.story_id, item.character_id, story_db).await;
        item.bound_story = bs;
        item.bound_character = bc;
            out.push(item);
    }
    Ok(out)
}

/// 按 id 取一条素材（入库桥接回读用；不知道调用方是谁）
pub async fn portrait_get(id: i64) -> Result<PortraitItem, String> {
    let pool = pool().await?;
    // 用统一的 COLS（row_to_item 按下标读列，必须保证列序一致；SELECT * 会随表结构变化而错位）
    let rows = sqlx::query(&format!("SELECT {} FROM portraits WHERE id = ?1", COLS))
        .bind(id)
        .fetch_all(&pool)
        .await
        .map_err(|e| e.to_string())?;
    match rows.first() {
        Some(r) => row_to_item_bound(r, &pool).await,
        None => Err(format!("找不到素材 #{id}")),
    }
}
