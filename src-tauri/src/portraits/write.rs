// 从 `portraits.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `portraits.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use write::*;` 取回，
// 外部 `crate::portraits::xxx` 的调用路径**全部不变**。

use super::*;

/// 导入本地图片：复制到素材目录 + 登记元数据，返回新素材。
#[tauri::command]
pub async fn portrait_import(
    srcPath: String,
    name: String,
    role: String,
    ver: String,
    tags: String,
    group: String,
    storyId: Option<i64>,
    characterId: Option<i64>,
) -> Result<PortraitItem, String> {
    let src = Path::new(&srcPath);
    if !src.exists() {
        return Err(format!("文件不存在：{}", srcPath));
    }
    let meta = std::fs::metadata(src).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("所选路径不是文件".to_string());
    }
    let size_bytes = meta.len() as i64;
    let (w, h) = image_size(src);
    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let dir = portraits_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let fname = format!("p{}.{}", millis, ext);
    let dst = dir.join(&fname);
    std::fs::copy(src, &dst).map_err(|e| e.to_string())?;

    let ratio = if w > 0 && h > 0 {
        format!("{}×{}", w, h)
    } else {
        "未知".to_string()
    };
    let p = pool().await?;
    ensure_table(&p).await?;
    let row = sqlx::query(
        "INSERT INTO portraits(name,role,ver,ratio,width,height,size_bytes,path,tags,group_name,story_id,character_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         RETURNING id,created_at"
    )
    .bind(&name)
    .bind(&role)
    .bind(&ver)
    .bind(&ratio)
    .bind(w)
    .bind(h)
    .bind(size_bytes)
    .bind(&fname)
    .bind(&tags)
    .bind(&group.trim())
    .bind(storyId)
    .bind(characterId)
    .fetch_one(&p)
    .await
    .map_err(|e| e.to_string())?;
    let id: i64 = row.try_get(0).map_err(|e: sqlx::Error| e.to_string())?;
    let created: String = row.try_get(1).map_err(|e: sqlx::Error| e.to_string())?;
    // 来源标记：手工导入记为 manual（这样素材库能区分"手工导入"与"AI 生成"）
    let _ = sqlx::query("UPDATE portraits SET source = 'manual' WHERE id = ?1 AND COALESCE(source,'') = ''")
        .bind(id)
        .execute(&p)
        .await;
    let (bound_story, bound_character) = resolve_bound(&p, storyId, characterId).await;
    let dv: String = created.chars().skip(5).take(5).collect();
    Ok(PortraitItem {
        id,
        name,
        role,
        ver,
        ratio,
        width: w,
        height: h,
        size: fmt_size(size_bytes),
        size_bytes,
        date: dv.clone(),
        pinned: false,
        tags: split_tags(&tags),
        g: format!("g{}", (id % 6) + 1),
        path: Some(dst.display().to_string()),
        story_id: storyId,
        character_id: characterId,
        bound_story,
        bound_character,
        group: String::new(),
        prompt: String::new(),
        negative: String::new(),
        seed: String::new(),
        steps: String::new(),
        cfg: String::new(),
        model: String::new(),
        source: String::new(),      // 紧接上面已 UPDATE 落库，由调用方回读
        origin_tool: String::new(),
        derived_from: None,
    })
}

/// 新建素材（无图占位，后续可导入/绑定图片）。
#[tauri::command]
pub async fn portrait_create(
    name: String,
    role: String,
    ver: String,
    ratio: String,
    tags: String,
    storyId: Option<i64>,
    characterId: Option<i64>,
) -> Result<PortraitItem, String> {
    let ratio = if ratio.trim().is_empty() {
        "待补充".to_string()
    } else {
        ratio.trim().to_string()
    };
    let p = pool().await?;
    ensure_table(&p).await?;
    let row = sqlx::query(
        "INSERT INTO portraits(name,role,ver,ratio,tags,story_id,character_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7) RETURNING id,created_at"
    )
    .bind(&name)
    .bind(&role)
    .bind(&ver)
    .bind(&ratio)
    .bind(&tags)
    .bind(storyId)
    .bind(characterId)
    .fetch_one(&p)
    .await
    .map_err(|e| e.to_string())?;
    let id: i64 = row.try_get(0).map_err(|e: sqlx::Error| e.to_string())?;
    let created: String = row.try_get(1).map_err(|e: sqlx::Error| e.to_string())?;
    let _ = sqlx::query("UPDATE portraits SET source = 'placeholder' WHERE id = ?1 AND COALESCE(source,'') = ''")
        .bind(id)
        .execute(&p)
        .await;
    let (bound_story, bound_character) = resolve_bound(&p, storyId, characterId).await;
    let dv: String = created.chars().skip(5).take(5).collect();
    Ok(PortraitItem {
        id,
        name,
        role,
        ver,
        ratio,
        width: 0,
        height: 0,
        size: "—".to_string(),
        size_bytes: 0,
        date: dv.clone(),
        pinned: false,
        tags: split_tags(&tags),
        g: format!("g{}", (id % 6) + 1),
        path: None,
        story_id: storyId,
        character_id: characterId,
        bound_story,
        bound_character,
        group: String::new(),
        prompt: String::new(),
        negative: String::new(),
        seed: String::new(),
        steps: String::new(),
        cfg: String::new(),
        model: String::new(),
        source: String::new(),      // 紧接上面已 UPDATE 落库，由调用方回读
        origin_tool: String::new(),
        derived_from: None,
    })
}

/// 绑定素材到某故事的某个角色（传 null 解除绑定）。
#[tauri::command]
pub async fn portrait_bind(
    id: i64,
    storyId: Option<i64>,
    characterId: Option<i64>,
) -> Result<PortraitItem, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    sqlx::query(
        "UPDATE portraits SET story_id = ?1, character_id = ?2, updated_at = datetime('now','localtime')
         WHERE id = ?3"
    )
    .bind(storyId)
    .bind(characterId)
    .bind(id)
    .execute(&p)
    .await
    .map_err(|e| e.to_string())?;
    let row = sqlx::query(&format!("SELECT {} FROM portraits WHERE id = ?1", COLS))
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    row_to_item_bound(&row, &p).await
}

/// 把素材**加入**分组（2026-10-25 改语义）。
///
/// 原来是一对一"设置"：`group_name` 只能有一个值，换组就顶掉旧的。
/// 现在一个素材可以**同时属于多个组**（角色组是派生的，自定义组存这里，逗号分隔）——
/// 用户要的是"同一张图既在夏婉琴组、又在女生组"，所以这里是**追加 + 去重**，不是覆盖。
/// 要移出请用 `portrait_group_remove`。
#[tauri::command]
pub async fn portrait_group_set(ids: Vec<i64>, group: String) -> Result<usize, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let g = group.trim().to_string();
    if g.is_empty() {
        return Ok(0);
    }
    let mut n = 0usize;
    for id in &ids {
        let old: String = sqlx::query_scalar("SELECT group_name FROM portraits WHERE id = ?1")
            .bind(id)
            .fetch_one(&p)
            .await
            .map_err(|e| e.to_string())?;
        let mut set = split_groups(&old);
        if !set.iter().any(|x| x == &g) {
            set.push(g.clone());
        }
        sqlx::query(
            "UPDATE portraits SET group_name = ?1, updated_at = datetime('now','localtime') WHERE id = ?2"
        )
        .bind(set.join(","))
        .bind(id)
        .execute(&p)
        .await
        .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// 把素材**移出**某个分组（只动自定义组；角色组由"绑定关系"派生，解绑才消失）。
#[tauri::command]
pub async fn portrait_group_remove(ids: Vec<i64>, group: String) -> Result<usize, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let g = group.trim().to_string();
    if g.is_empty() {
        return Ok(0);
    }
    let mut n = 0usize;
    for id in &ids {
        let old: String = sqlx::query_scalar("SELECT group_name FROM portraits WHERE id = ?1")
            .bind(id)
            .fetch_one(&p)
            .await
            .map_err(|e| e.to_string())?;
        let set: Vec<String> = split_groups(&old).into_iter().filter(|x| x != &g).collect();
        sqlx::query(
            "UPDATE portraits SET group_name = ?1, updated_at = datetime('now','localtime') WHERE id = ?2"
        )
        .bind(set.join(","))
        .bind(id)
        .execute(&p)
        .await
        .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// 批量追加标签（去重合并）。返回更新条数。
#[tauri::command]
pub async fn portrait_tag_batch(ids: Vec<i64>, tags: String) -> Result<usize, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let mut n = 0usize;
    for id in &ids {
        let old: String = sqlx::query_scalar("SELECT tags FROM portraits WHERE id = ?1")
            .bind(id)
            .fetch_one(&p)
            .await
            .map_err(|e| e.to_string())?;
        let mut set: Vec<String> = split_tags(&old);
        for t in split_tags(&tags) {
            if !set.contains(&t) {
                set.push(t);
            }
        }
        sqlx::query(
            "UPDATE portraits SET tags = ?1, updated_at = datetime('now','localtime') WHERE id = ?2"
        )
        .bind(set.join(","))
        .bind(id)
        .execute(&p)
        .await
        .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// 批量删除素材（删记录 + 删图片文件）。返回删除条数。
#[tauri::command]
pub async fn portrait_delete(ids: Vec<i64>) -> Result<usize, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let mut n = 0usize;
    for id in &ids {
        let fname: Option<String> = sqlx::query_scalar("SELECT path FROM portraits WHERE id = ?1")
            .bind(id)
            .fetch_optional(&p)
            .await
            .map_err(|e| e.to_string())?
            .flatten();
        if let Some(f) = fname {
            let f = portraits_dir().join(&f);
            let _ = std::fs::remove_file(&f);
        }
        sqlx::query("DELETE FROM portraits WHERE id = ?1")
            .bind(id)
            .execute(&p)
            .await
            .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// 更新素材生成参数（提示词 / 负向提示词 / seed / steps / cfg / 模型）。内容变化时记录版本。
#[tauri::command]
pub async fn portrait_update_params(
    id: i64,
    prompt: String,
    negative: String,
    seed: String,
    steps: String,
    cfg: String,
    model: String,
) -> Result<PortraitItem, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let old: (String, String, String, String, String, String) = sqlx::query_as(
        "SELECT prompt, negative, seed, steps, cfg, model FROM portraits WHERE id = ?1"
    )
    .bind(id)
    .fetch_one(&p)
    .await
    .map_err(|e| e.to_string())?;
    let n_prompt = prompt.trim().to_string();
    let n_neg = negative.trim().to_string();
    let n_seed = seed.trim().to_string();
    let n_steps = steps.trim().to_string();
    let n_cfg = cfg.trim().to_string();
    let n_model = model.trim().to_string();
    sqlx::query(
        "UPDATE portraits SET prompt = ?1, negative = ?2, seed = ?3, steps = ?4, cfg = ?5, model = ?6, updated_at = datetime('now','localtime') WHERE id = ?7"
    )
    .bind(&n_prompt)
    .bind(&n_neg)
    .bind(&n_seed)
    .bind(&n_steps)
    .bind(&n_cfg)
    .bind(&n_model)
    .bind(id)
    .execute(&p)
    .await
    .map_err(|e| e.to_string())?;
    // 改生成参数**不记版本**（2026-10-25 用户定：版本历史只由「重新生成 / 删除」驱动，
    // 改参数、改标签、改角色这类编辑不产生 v2/v3…，否则历史里全是噪音）。
    let _ = &old;
    let row = sqlx::query(&format!("SELECT {} FROM portraits WHERE id = ?1", COLS))
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    row_to_item_bound(&row, &p).await
}

/// **整体替换**标签（2026-10-25）。
///
/// 为什么要单独一个命令：`portrait_tag_batch` 是"只追加、不删除"的，
/// 而「表情标签」要求**每张立绘只有一个表情名**（换表情时得把旧的摘掉）——
/// 追加语义做不到，只能整体设置。
#[tauri::command]
pub async fn portrait_tag_set(id: i64, tags: String) -> Result<PortraitItem, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let old: String = sqlx::query_scalar("SELECT tags FROM portraits WHERE id = ?1")
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    // 归一：去空、去重、保序
    let mut set: Vec<String> = Vec::new();
    for t in split_tags(&tags) {
        if !set.contains(&t) {
            set.push(t);
        }
    }
    let joined = set.join(",");
    sqlx::query(
        "UPDATE portraits SET tags = ?1, updated_at = datetime('now','localtime') WHERE id = ?2"
    )
    .bind(&joined)
    .bind(id)
    .execute(&p)
    .await
    .map_err(|e| e.to_string())?;
    // 改标签**不记版本**（同上：只有「重新生成 / 删除」会动版本历史）
    let _ = &old;
    // 回读统一走 COLS（**不能 `SELECT *`**：row_to_item_bound 按下标读列，
    // `*` 会随表结构变化而错位 —— 实测报 "decoding column 11 ... TEXT vs INTEGER"）
    super::portrait_get(id).await
}

/// 更新素材字段（当前支持修改角色 role）。返回更新后的素材。
#[tauri::command]
pub async fn portrait_update(id: i64, role: String) -> Result<PortraitItem, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let old: String = sqlx::query_scalar("SELECT role FROM portraits WHERE id = ?1")
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    let new = role.trim().to_string();
    sqlx::query(
        "UPDATE portraits SET role = ?1, updated_at = datetime('now','localtime') WHERE id = ?2"
    )
    .bind(&new)
    .bind(id)
    .execute(&p)
    .await
    .map_err(|e| e.to_string())?;
    // 改角色**不记版本**（同上；角色归属现在由「绑定角色」决定，这个字段只是旧数据兼容）
    let _ = &old;
    let row = sqlx::query(&format!("SELECT {} FROM portraits WHERE id = ?1", COLS))
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    row_to_item_bound(&row, &p).await
}

/// 通用列写入：把 `cols` 里的 (列名, 值) 写进一条素材。
/// **素材库不理解这些列的语义** —— 它只做"把调用方给的字符串写进去"，
/// 因此调用方想记什么（生成参数、来源标记…）都不需要素材库知道。
/// 列名会做白名单校验，避免拼出非法 SQL。
pub async fn portrait_set_columns(id: i64, cols: &[(&str, String)]) -> Result<(), String> {
    const ALLOWED: &[&str] = &[
        "name", "role", "ver", "ratio", "tags", "group_name", "pinned",
        "story_id", "character_id",
        // 来源与参数快照（对素材库而言只是普通文本列）
        "source", "origin_tool", "derived_from",
        "prompt", "negative", "seed", "steps", "cfg", "model",
    ];
    if cols.is_empty() {
        return Ok(());
    }
    let mut sets = Vec::new();
    for (c, _) in cols {
        if !ALLOWED.contains(c) {
            return Err(format!("不允许写入的列：{c}"));
        }
        sets.push(format!("{c} = ?"));
    }
    let sql = format!(
        "UPDATE portraits SET {}, updated_at = datetime('now','localtime') WHERE id = ?",
        sets.join(", ")
    );
    let pool = pool().await?;
    let mut q = sqlx::query(&sql);
    for (_, v) in cols {
        q = q.bind(v);
    }
    q = q.bind(id);
    q.execute(&pool).await.map_err(|e| e.to_string())?;
    Ok(())
}
