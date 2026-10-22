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
    add_version(&p, id, "导入图片素材").await?;
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
        versions: vec![VersionItem { version: 1, desc: "导入图片素材".to_string(), date: dv }],
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
    add_version(&p, id, "创建占位素材").await?;
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
        versions: vec![VersionItem { version: 1, desc: "创建占位素材".to_string(), date: dv }],
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

/// 批量设置分组（group_name 字段）。返回更新条数。
#[tauri::command]
pub async fn portrait_group_set(ids: Vec<i64>, group: String) -> Result<usize, String> {
    let p = pool().await?;
    ensure_table(&p).await?;
    let g = group.trim().to_string();
    let mut n = 0usize;
    for id in &ids {
        sqlx::query(
            "UPDATE portraits SET group_name = ?1, updated_at = datetime('now','localtime') WHERE id = ?2"
        )
        .bind(&g)
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
    let joined = |t: (String, String, String, String, String, String)| -> String {
        format!("{}\x01{}\x01{}\x01{}\x01{}\x01{}", t.0, t.1, t.2, t.3, t.4, t.5)
    };
    if joined(old) != joined((
        n_prompt.clone(),
        n_neg.clone(),
        n_seed.clone(),
        n_steps.clone(),
        n_cfg.clone(),
        n_model.clone(),
    )) {
        add_version(&p, id, "修改生成参数").await?;
    }
    let row = sqlx::query(&format!("SELECT {} FROM portraits WHERE id = ?1", COLS))
        .bind(id)
        .fetch_one(&p)
        .await
        .map_err(|e| e.to_string())?;
    row_to_item_bound(&row, &p).await
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
    if old != new {
        let desc = if old.is_empty() {
            format!("设置角色：{}", new)
        } else if new.is_empty() {
            format!("清空角色：{} → 未设置", old)
        } else {
            format!("修改角色：{} → {}", old, new)
        };
        add_version(&p, id, &desc).await?;
    }
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
