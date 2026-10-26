use sqlx::Row;
use sqlx::SqlitePool;
use std::collections::HashMap;



/// 第五批 ③：对外函数使用的"当前分支"（读 `stories.current_branch_id`，`None` = 主线）。
///
/// 规则：**对外函数按"当前分支"工作，`_in` 核心版只管"给定分支"**。
/// 这样界面切分支只需调一次 `set_current_branch`，其余命令自动沿用（不会漏传参数）。
async fn current_branch(story_id: &str) -> Option<String> {
    crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None)
}


#[derive(serde::Serialize)]
pub struct WorldSetting {
    pub id: i64,
    pub key: String,
    pub value: String,
    pub is_hard_rule: i64,
    pub version: i64,
    pub category: String,
    pub meta_json: String,
}

pub async fn list_world_settings(story_id: &str) -> Result<Vec<WorldSetting>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    list_world_settings_in(&pool, &b).await
}

/// 第五批 ③：分支感知的世界观列表（`None` = 主线）。
pub async fn list_world_settings_in(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<WorldSetting>, sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let sql = format!(
        "SELECT id, key, value, is_hard_rule, version, category, meta_json FROM world_settings WHERE {} ORDER BY id",
        clause
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(WorldSetting {
            id: r.try_get(0)?,
            key: r.try_get(1)?,
            value: r.try_get(2)?,
            is_hard_rule: r.try_get(3)?,
            version: r.try_get(4)?,
            category: r.try_get(5)?,
            meta_json: r.try_get(6)?,
        });
    }
    Ok(out)
}

/// 写入时补默认坐标：**地理**条目一律带上 `x` / `y`（缺省 `"50"`）。
///
/// 为什么在写入口补：读的一侧可以兜底（`meta_coord` 缺省 50），但数据本身不该出现
/// "库里没有坐标、地图上却画在 50%" 这种两面不一致 —— 那正是 10-26 那个"检测不出来"的成因。
/// 其它分类原样返回：别往势力 / 术语 / 物品里塞坐标字段。
pub fn normalize_meta(category: &str, meta_json: &str) -> String {
    if category.trim() != CATEGORY_GEO {
        return meta_json.to_string();
    }
    let mut obj = match serde_json::from_str::<serde_json::Value>(meta_json) {
        Ok(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    for k in ["x", "y"] {
        let missing = match obj.get(k) {
            Some(serde_json::Value::String(s)) => s.trim().is_empty(),
            Some(v) if v.is_number() => false,
            Some(_) => true,
            None => true,
        };
        if missing {
            obj.insert(k.to_string(), serde_json::Value::String("50".to_string()));
        }
    }
    serde_json::Value::Object(obj).to_string()
}

#[allow(clippy::too_many_arguments)]
pub async fn create_world_setting(
    story_id: &str,
    key: &str,
    value: &str,
    is_hard_rule: bool,
    category: &str,
    meta_json: &str,
) -> Result<i64, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    create_world_setting_in(&pool, &b, key, value, is_hard_rule, category, meta_json).await
}

/// 第五批 ③：新建世界观设定（写入带上分支）。
#[allow(clippy::too_many_arguments)]
pub async fn create_world_setting_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    key: &str,
    value: &str,
    is_hard_rule: bool,
    category: &str,
    meta_json: &str,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO world_settings(key, value, is_hard_rule, category, meta_json, branch_id) VALUES(?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
    )
    .bind(key)
    .bind(value)
    .bind(if is_hard_rule { 1 } else { 0 })
    .bind(category)
    .bind(normalize_meta(category, meta_json))
    .bind(branch.clone())
    .fetch_one(pool)
    .await?;
    Ok(row.try_get(0)?)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_world_setting(
    story_id: &str,
    id: i64,
    key: &str,
    value: &str,
    is_hard_rule: bool,
    category: &str,
    meta_json: &str,
) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    update_world_setting_in(&pool, &b, id, key, value, is_hard_rule, category, meta_json).await
}

/// 第五批 ③：更新世界观设定（带分支条件，防止跨分支误改）。
#[allow(clippy::too_many_arguments)]
pub async fn update_world_setting_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    id: i64,
    key: &str,
    value: &str,
    is_hard_rule: bool,
    category: &str,
    meta_json: &str,
) -> Result<(), sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 7);
    let sql = format!(
        "UPDATE world_settings SET key=?1, value=?2, is_hard_rule=?3, category=?4, meta_json=?5, version=version+1 WHERE id=?6 AND {}",
        clause
    );
    let mut q = sqlx::query(&sql)
        .bind(key)
        .bind(value)
        .bind(if is_hard_rule { 1 } else { 0 })
        .bind(category)
        .bind(normalize_meta(category, meta_json))
        .bind(id);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.execute(pool).await?;
    Ok(())
}

pub async fn delete_world_setting(story_id: &str, id: i64) -> Result<(), sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    delete_world_setting_in(&pool, &b, id).await
}

/// 第五批 ③：删除世界观设定（带分支条件）。
pub async fn delete_world_setting_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    id: i64,
) -> Result<(), sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 2);
    let sql = format!("DELETE FROM world_settings WHERE id = ?1 AND {}", clause);
    let mut q = sqlx::query(&sql).bind(id);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.execute(pool).await?;
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct WorldConflict {
    pub title: String,
    pub level: String,
    pub left_id: i64,
    pub right_id: i64,
    pub left_label: String,
    pub left_text: String,
    pub left_source: String,
    pub right_label: String,
    pub right_text: String,
    pub right_source: String,
}

/// 「地理」分类名（地点）。集中一处，避免界面写"地理"、代码写"地点"。
pub const CATEGORY_GEO: &str = "地理";

/// 坐标取值：兼容字符串（界面写 `"40"`）与数字（导入/老数据可能是 `40`）。
///
/// **缺坐标按 "50" 算** —— 这不是"随便给个默认值"，而是**和界面画图钉的规则完全一致**：
/// 地图用 `Number(m.x || 50)` 定位（`WorldPage.tsx`），没填的地点就画在 50% 处。
/// 所以两个没填坐标的地点在地图上本来就叠在一起，检测必须照实报出来（2026-10-26 用户实测：
/// "两个地点都是 50.50，但检测没报"—— 原因就是上一版在这里"跳过没坐标的"，和界面口径不一致）。
pub fn meta_coord(meta: &serde_json::Value, key: &str) -> String {
    // 老数据 / 别的工具可能写成 map_x / map_y（检查功能的字段表里就有这两个名字），
    // 读的时候两种都认，写的时候统一成 x / y（见 `normalize_meta`）。
    let legacy = format!("map_{key}");
    for k in [key, legacy.as_str()] {
        let v = match meta.get(k) {
            Some(serde_json::Value::String(s)) => s.trim().to_string(),
            Some(v) if v.is_number() => v.to_string(),
            _ => String::new(),
        };
        if !v.is_empty() {
            return v;
        }
    }
    "50".to_string()
}

/// 冲突检测（2026-10-26 修）：只比**同一分支内、真的填了坐标**的地点。
///
/// 三条规则，每条都在修一个实测出来的假冲突：
///   ① 只比当前分支 —— 建分支会把 `world_settings` 整份复制（见 `branch.rs`），
///      跨分支比会把"同一个地点"报成两个地点抢坐标（用户实测：夏家老宅 #21/#26）；
///   ② 没填坐标按 **(50,50)** 算 —— 和地图画图钉的默认位置一致（不填的地方本来就画在那儿，
///      两个叠在一起是真重叠，不是假冲突）；
///   ③ 同一坐标 N 个地点 → **两两全报**（N·(N-1)/2 条），不再只报前两个。
///
/// 设定条目的重名 / 空值 / 术语重复**不在这里** —— 那是「🔍 检查」的本地规则
/// （`check/local_rules.rs`），本函数只管"地点坐标"这一件事。
pub fn detect_conflicts_in(rows: &[WorldSetting]) -> Vec<WorldConflict> {
    let mut groups: HashMap<String, Vec<&WorldSetting>> = HashMap::new();
    for w in rows.iter().filter(|w| w.category.trim() == CATEGORY_GEO) {
        let meta: serde_json::Value =
            serde_json::from_str(&w.meta_json).unwrap_or_else(|_| serde_json::json!({}));
        let x = meta_coord(&meta, "x");
        let y = meta_coord(&meta, "y");
        groups.entry(format!("{},{}", x, y)).or_default().push(w);
    }
    let mut out = Vec::new();
    for (coord, list) in groups {
        for i in 0..list.len() {
            for j in (i + 1)..list.len() {
                let (a, b) = (list[i], list[j]);
                out.push(WorldConflict {
                    title: format!("地点坐标冲突：「{}」与「{}」都在 ({})", a.key, b.key, coord),
                    level: "提示".to_string(),
                    left_id: a.id,
                    right_id: b.id,
                    left_label: format!("地图与地点 · {}", a.key),
                    left_text: a.value.clone(),
                    left_source: format!("world_settings #{} · 坐标 {}", a.id, coord),
                    right_label: format!("地图与地点 · {}", b.key),
                    right_text: b.value.clone(),
                    right_source: format!("world_settings #{} · 坐标 {}", b.id, coord),
                });
            }
        }
    }
    // 固定顺序：HashMap 的遍历顺序不稳定，测试与界面都希望可复现
    out.sort_by(|p, q| (p.left_id, p.right_id).cmp(&(q.left_id, q.right_id)));
    out
}

/// 给定连接池 + 分支（`None` = 主线）：查该分支的条目再判冲突。
///
/// 抽成独立函数是为了**能测**：分支隔离这条规则靠它，测试用内存池直接喂两个分支。
pub async fn detect_conflicts_with(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Result<Vec<WorldConflict>, sqlx::Error> {
    let rows = list_world_settings_in(pool, branch).await?;
    Ok(detect_conflicts_in(&rows))
}

/// 对外命令：按故事的**当前分支**检测（分支由 `stories.current_branch_id` 决定，
/// 界面切分支不用重新传参，和其它世界观命令同一套约定）。
pub async fn detect_world_conflicts(story_id: &str) -> Result<Vec<WorldConflict>, sqlx::Error> {
    let pool = crate::story_db::open_story_pool(story_id).await?;
    let b = current_branch(story_id).await;
    detect_conflicts_with(&pool, &b).await
}


// ===== 物品库（v3.4）=====
//
// 用户要的"物品库"就放在「世界观设定」里，作为一种 category。
// 为什么不做独立表/独立子系统：物品和术语/地点是同一种东西（名称 + 说明 + 别名），
// 独立表会带来第二套 CRUD、第二套分支复制、第二套检查 —— 收益不抵成本。

/// 物品分类名。集中一处，避免界面上写"物品"、代码里写"道具"这种漂移。
pub const CATEGORY_ITEM: &str = "物品";

/// 物品在提示词里的一行：`- 物品「怀表」：B 送的，走时慢三分钟（别名：旧表）`
pub fn item_line(key: &str, value: &str, aliases: &str) -> String {
    let a = aliases.trim();
    let suffix = if a.is_empty() {
        String::new()
    } else {
        format!("（别名：{}）", a)
    };
    format!("- 物品「{}」：{}{}", key.trim(), value.trim(), suffix)
}

#[cfg(test)]
mod item_tests;

// ===== 世界书（v3.4，触发式注入）=====
//
// 与「物品库」的区别（这条边界要写清楚，否则两套会重叠）：
//   * 物品 / 术语 / 地点 / 势力：**全量**进 system 的世界观段 —— 少而稳，永远该在场；
//   * 世界书：**命中才进**尾部块 —— 多而散（人物前史、器物来历、某地习俗），
//     全塞进去会吃掉历史预算，所以按触发词召回。

/// 世界条目的分类名。
pub const CATEGORY_LORE: &str = "世界书";

/// 拆触发词：中英文逗号 / 顿号 / 分号都认，去空白、跳过空串。
pub fn split_triggers(raw: &str) -> Vec<String> {
    raw.split(|c| c == ',' || c == '，' || c == '、' || c == ';' || c == '；')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 这条世界书是否命中；命中返回**命中的那个词**（给界面解释"为什么进"）。
///
/// 常驻（`always`）条目不参与匹配，永远算命中（返回 `"（常驻）"`）。
pub fn lore_hit(triggers: &str, always: bool, text: &str) -> Option<String> {
    if always {
        return Some("（常驻）".to_string());
    }
    let hay = text.to_lowercase();
    for trig in split_triggers(triggers) {
        if hay.contains(&trig.to_lowercase()) {
            return Some(trig);
        }
    }
    None
}

/// 拼【世界书】块。
///
/// `lines` 已按优先级排好序；超过 `budget_chars` 的条目会被丢弃并计数，
/// 在块首注明"另有 N 条未注入"——否则用户会以为写进去的设定没生效。
pub fn lore_block(lines: &[String], skipped: usize) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let mut s = String::from(
        "\n【世界书】（本条消息命中的设定，只作背景参考；除非剧情需要，不要主动复述）",
    );
    for l in lines {
        s.push('\n');
        s.push_str(l);
    }
    if skipped > 0 {
        s.push_str(&format!(
            "\n（另有 {} 条命中，因篇幅未注入；需要时直接问「设定」即可）",
            skipped
        ));
    }
    Some(s)
}

/// 世界书一行的格式：`- 「怀表」：B 送的，走时慢三分钟`
pub fn lore_line(key: &str, value: &str) -> String {
    format!("- 「{}」：{}", key.trim(), value.trim())
}

#[cfg(test)]
mod lore_tests;

/// 这个分类是不是**只走尾部块**（不进 system 的【世界观】）。
///
/// 为什么需要它：world_settings 里混着两类东西 ——
///   * 少而稳的（地点/势力/术语/物品）：每轮都该在场 → 进 system；
///   * 多而散的（世界书）：只在触发词命中时才该出现 → 进尾部块。
/// 渲染循环如果只写"已知分类"的分支，新分类会**掉进默认分支被全量渲染**（实测踩到：
/// 世界书条目每轮都出现在【世界观】里，且无视触发词）。
pub fn is_tail_only(category: &str) -> bool {
    category.trim() == CATEGORY_LORE
}

#[cfg(test)]
mod tail_only_tests;

/// 取当前分支的全部世界书条目：`(key, value, meta_json)`。
///
/// ## 为什么单独抽出来
///
/// 原来这段 SQL 直接写在 `generate_stream` 里，占位符编号是 `category = ?2` +
/// `branch_clause(..., 1)`。**主线**时 clause 是 `branch_id IS NULL`（不含占位符），
/// 而 sqlx 的 `bind` 按调用顺序落到 1、2… 号位 —— 分类被绑到 1 号位、SQL 却要 2 号位，
/// 参数没设 → 查询失败被 `unwrap_or_default()` 吞掉 → **主线永远查不到世界书**（实测踩到）。
///
/// 现在：分类固定 `?1`，分支子句从 `?2` 起编号，bind 顺序与 SQL 永远一致。
pub async fn fetch_lore_entries(
    pool: &SqlitePool,
    branch: &Option<String>,
) -> Vec<(String, String, String)> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 2);
    let sql = format!(
        "SELECT key, value, meta_json FROM world_settings
         WHERE category = ?1 AND {}
         ORDER BY id",
        clause
    );
    let mut q = sqlx::query(&sql).bind(CATEGORY_LORE);
    if let Some(b) = branch {
        q = q.bind(b);
    }
    q.fetch_all(pool)
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| {
            (
                r.try_get::<String, _>(0).unwrap_or_default(),
                r.try_get::<String, _>(1).unwrap_or_default(),
                r.try_get::<String, _>(2).unwrap_or_default(),
            )
        })
        .collect()
}

#[cfg(test)]
mod lore_query_tests;

/// 带"命中"标注的世界书行。
///
/// 为什么不只显示条目名：用户搜的是**触发词**（"怀表"），而条目名可能是「大肥鱼送的」这类
/// 看不出内容的摘要 —— 只显示条目名会让人以为"没生效"。带上命中词，一眼能对上因果。
///
/// 常驻条目的 `hit` 是 `"（常驻）"`（自带括号），此时不再套一层「命中：」。
pub fn lore_line_tagged(key: &str, value: &str, hit: &str) -> String {
    let tag = if hit.starts_with('（') {
        hit.to_string()
    } else {
        format!("（命中：{}）", hit)
    };
    format!("- 「{}」{}：{}", key.trim(), tag, value.trim())
}

#[cfg(test)]
mod lore_tag_tests;
#[cfg(test)]
mod conflict_tests;


