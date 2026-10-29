//! 三维数值变更（2026-10-29）：体力 / 理智 / 存在感从「只读装饰」变成「会变」。
//!
//! 设计见 docs/design/三维数值变更.md。三条硬规矩：
//! 1. **模型不碰数字**：它只写一行协议 `〔状态变化〕熬夜：理由`，数字由 STAT_KINDS 表决定；
//! 2. **默认关**：插件开关 `plugin.stat-growth.enabled` 不为 true 时，不注入提示词、不解析、不写表；
//! 3. **可撤销**：每次写入 stat_changes 一行（source/revoked），撤销 = 反向写一行 + 原行标记。
//!
//! 去重（都在软件侧，不写进提示词 —— 塘主定：给模型加结构约束它会乱输出）：
//! * 每角色每维度每轮最多 1 条；
//! * 同一故事日同一类型最多 1 条（睡好一天只算一次）；
//! * 存在感另外「当天累计净额夹 ±5」（慢变量，不是每次都要变）。

use serde::Serialize;
use sqlx::{Row, SqlitePool};

/// 协议标记（AI 回复末尾）。改这里要同步改 src/lib/parse.ts 的 STAT_TAG
pub const TAG: &str = "〔状态变化〕";
/// 插件开关（和前端 `plugin.<id>.enabled` 同一套键名）
pub const PLUGIN_KEY: &str = "plugin.stat-growth.enabled";
/// 理由最长多少字（和导演代价同一口径）
pub const MAX_REASON_CHARS: usize = 32;

/// 一个类型 → 它影响哪些维度、各多少（**数字只在这里，提示词里没有**）
pub struct KindSpec {
    pub name: &'static str,
    pub effects: &'static [(&'static str, i64)],
}

pub const STAT_KINDS: &[KindSpec] = &[
    KindSpec { name: "受伤", effects: &[("stamina", -15)] },
    KindSpec { name: "生病", effects: &[("stamina", -15)] },
    KindSpec { name: "力竭", effects: &[("stamina", -15)] },
    KindSpec { name: "被欺骗", effects: &[("sanity", -10)] },
    KindSpec { name: "撞见无法解释的事", effects: &[("sanity", -10)] },
    KindSpec { name: "崩溃", effects: &[("sanity", -10)] },
    KindSpec { name: "被安抚", effects: &[("sanity", 10)] },
    KindSpec { name: "想通", effects: &[("sanity", 10)] },
    KindSpec { name: "被无视", effects: &[("screen", -3)] },
    KindSpec { name: "被当众羞辱", effects: &[("screen", -5)] },
    KindSpec { name: "被看见", effects: &[("screen", 3)] },
    KindSpec { name: "被认可", effects: &[("screen", 5)] },
];


/// 作息（2026-10-29 定：**只在日结里判**，不进对话协议）。
///
/// 三态语义：睡好 → 回一点；熬夜 → 掉体力；**普通的一夜什么都不写**。
pub const SLEEP_KINDS: &[KindSpec] = &[
    KindSpec { name: "睡好", effects: &[("stamina", 15), ("sanity", 4)] },
    KindSpec { name: "熬夜", effects: &[("stamina", -10)] },
];

/// 把某个类型展开成计划（日结与对话共用；数字都在这里，表外类型返回空）。
pub fn plan_kind(kind: &str, reason: &str) -> Vec<Planned> {
    let spec = STAT_KINDS.iter().chain(SLEEP_KINDS.iter()).find(|k| k.name == kind);
    let Some(spec) = spec else { return Vec::new() };
    let mut out = Vec::new();
    for &(dim, delta) in spec.effects {
        let cap = dim_cap(dim);
        if cap <= 0 { continue; }
        out.push(Planned { kind: kind.to_string(), reason: reason.to_string(), dim, delta: delta.clamp(-cap, cap) });
    }
    out
}
/// 单条上限（只设单条，**不设每轮总量** —— 塘主定）
const DIM_CAP: [(&str, i64); 3] = [("stamina", 15), ("sanity", 10), ("screen", 5)];
/// 存在感当天累计净额的绝对值上限
const SCREEN_DAY_CAP: i64 = 5;

pub fn dim_cap(dim: &str) -> i64 {
    DIM_CAP.iter().find(|(d, _)| *d == dim).map(|(_, c)| *c).unwrap_or(0)
}

/// 维度 → characters 表的列名（白名单，避免拼 SQL 时被注入）
fn column_of(dim: &str) -> Option<&'static str> {
    match dim {
        "stamina" => Some("stat_stamina"),
        "sanity" => Some("stat_sanity"),
        "screen" => Some("stat_screen"),
        _ => None,
    }
}

/// 这一行是不是状态变化协议行（形状判断，解析另说）
pub fn is_stat_line(line: &str) -> bool {
    line.trim_start().starts_with(TAG)
}

/// 解析一行：`〔状态变化〕熬夜：她一夜没合眼` → (类型, 理由)
pub fn parse_line(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let rest = t.strip_prefix(TAG)?;
    let (kind, reason) = rest.split_once(|c| c == '：' || c == ':')?;
    let kind = kind.trim();
    if kind.is_empty() {
        return None;
    }
    let reason: String = reason.trim().chars().take(MAX_REASON_CHARS).collect();
    Some((kind.to_string(), reason))
}

/// 计划里的一条（还没落库）
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub kind: String,
    pub reason: String,
    pub dim: &'static str,
    pub delta: i64,
}

/// 从回复里定出一轮要改的东西：**只认协议区里的第一条**（同一轮最多一条）。
///
/// 表外类型 → 空（不记账，只剥行）。
pub fn settle(reply: &str) -> Vec<Planned> {
    let start = crate::memory::protocol_block_start(reply);
    for line in reply.lines().skip(start) {
        if !is_stat_line(line) {
            continue;
        }
        let Some((kind, reason)) = parse_line(line) else {
            continue;
        };
        let Some(spec) = STAT_KINDS.iter().find(|k| k.name == kind) else {
            continue;
        };
        let mut out = Vec::new();
        for &(dim, delta) in spec.effects {
            let cap = dim_cap(dim);
            if cap <= 0 {
                continue;
            }
            let delta = delta.clamp(-cap, cap);
            out.push(Planned { kind: kind.clone(), reason: reason.clone(), dim, delta });
        }
        return out;
    }
    Vec::new()
}

/// 把协议行从文本里剥掉（按形状：表外类型也删）。
///
/// 只删**末尾协议区**里的那行 —— 正文中间出现同名字样是剧情文本，删掉就是改用户的剧情。
pub fn strip_line(text: &str) -> String {
    let start = crate::memory::protocol_block_start(text);
    text.lines()
        .enumerate()
        .filter(|(i, l)| !(*i >= start && is_stat_line(l)))
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// 插件是否开启（读 library.db 的 configs；拿不到就当关 —— 默认关是安全侧）
pub async fn enabled(lib_pool: &SqlitePool) -> bool {
    crate::db::get_config(lib_pool, PLUGIN_KEY)
        .await
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false)
}

/// 注入尾部块的【状态变化】说明（**只在插件开启时**出现；只有类型，没有数字）
pub fn legend() -> String {
    let names: Vec<&'static str> = STAT_KINDS.iter().map(|k| k.name).collect();
    [
        "【状态变化】（给软件的对账行，不是剧情内容）".to_string(),
        format!("- 这一轮如果有人物状态发生明显变化，在回复末尾单独一行写：{}类型：理由", TAG),
        format!("- 类型只能写：{}", names.join(" / ")),
        format!("- 冒号前直接写类型名，不要写字段名；理由 ≤ {} 字", MAX_REASON_CHARS),
        "- 没有明显变化就不写这一行；同一轮最多写一条".to_string(),
        "- 不要在正文里报数字（不要写「体力下降了多少」），状态变化只写在这一行里".to_string(),
    ]
    .join("\n")
}

/// 一次落库后的结果（前端提示用）
#[derive(Debug, Clone, Serialize)]
pub struct Applied {
    pub character_id: i64,
    pub dim: String,
    pub old: i64,
    pub new: i64,
    pub delta: i64,
    pub kind: String,
    pub reason: String,
}

/// 落库：夹值 → 去重 → 改 characters → 记 stat_changes。
///
/// 任何一步失败都**不阻塞生成**（返回已成功那几条）。
pub async fn apply(
    pool: &SqlitePool,
    branch: &Option<String>,
    node_id: i64,
    message_id: i64,
    character_id: i64,
    plan: &[Planned],
    source: &str,
) -> Vec<Applied> {
    let mut out = Vec::new();
    // 去重要**整轮**判断：一个类型可能影响两个维度（睡好 = 体力 + 理智），
    // 若按每条计划各自查一次，第一条插入后会把第二条自己判成"同日重复"（2026-10-29 测试踩到）。
    let mut seen_kind: Option<(String, bool)> = None;
    for p in plan {
        let Some(col) = column_of(p.dim) else { continue };
        let dup = match &seen_kind {
            Some((k, v)) if *k == p.kind => *v,
            _ => {
                let v: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM stat_changes sc
                     JOIN timeline_nodes n ON n.id = sc.node_id
                     WHERE sc.character_id = ?1 AND sc.kind = ?2 AND sc.revoked = 0
                       AND n.date_label = (SELECT date_label FROM timeline_nodes WHERE id = ?3)",
                )
                .bind(character_id)
                .bind(&p.kind)
                .bind(node_id)
                .fetch_one(pool)
                .await
                .unwrap_or(0);
                seen_kind = Some((p.kind.clone(), v > 0));
                v > 0
            }
        };
        if dup {
            continue;
        }
        // 存在感：当天累计净额夹 ±5
        let mut delta = p.delta;
        if p.dim == "screen" {
            let today: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(sc.delta),0) FROM stat_changes sc
                 JOIN timeline_nodes n ON n.id = sc.node_id
                 WHERE sc.character_id = ?1 AND sc.dim = ?2 AND sc.revoked = 0
                   AND n.date_label = (SELECT date_label FROM timeline_nodes WHERE id = ?3)",
            )
            .bind(character_id)
            .bind(p.dim)
            .bind(node_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0);
            let target = (today + delta).clamp(-SCREEN_DAY_CAP, SCREEN_DAY_CAP);
            delta = target - today;
            if delta == 0 {
                continue;
            }
        }
        let sql = format!("SELECT {col} FROM characters WHERE id = ?1");
        let old: i64 = match sqlx::query_scalar::<_, i64>(&sql)
            .bind(character_id)
            .fetch_optional(pool)
            .await
        {
            Ok(Some(v)) => v,
            _ => continue,
        };
        let new = (old + delta).clamp(0, 100);
        let eff = new - old;
        if eff == 0 {
            continue;
        }
        let upd = format!("UPDATE characters SET {col} = ?1, version = version + 1 WHERE id = ?2");
        if sqlx::query(&upd).bind(new).bind(character_id).execute(pool).await.is_err() {
            continue;
        }
        let _ = sqlx::query(
            "INSERT INTO stat_changes(branch_id, node_id, message_id, character_id, dim, delta, kind, reason, source)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(branch)
        .bind(node_id)
        .bind(message_id)
        .bind(character_id)
        .bind(p.dim)
        .bind(eff)
        .bind(&p.kind)
        .bind(&p.reason)
        .bind(source)
        .execute(pool)
        .await;
        out.push(Applied {
            character_id,
            dim: p.dim.to_string(),
            old,
            new,
            delta: eff,
            kind: p.kind.clone(),
            reason: p.reason.clone(),
        });
    }
    out
}

/// 账本一行
#[derive(Debug, Clone, Serialize)]
pub struct ChangeRow {
    pub id: i64,
    pub character_id: i64,
    pub dim: String,
    pub delta: i64,
    pub kind: String,
    pub reason: String,
    pub source: String,
    pub revoked: i64,
    pub created_at: String,
}

/// 建表（幂等）
pub async fn ensure_table(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS stat_changes (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            branch_id    TEXT,
            node_id      INTEGER,
            message_id   INTEGER,
            character_id INTEGER NOT NULL,
            dim          TEXT NOT NULL,
            delta        INTEGER NOT NULL,
            kind         TEXT NOT NULL DEFAULT '',
            reason       TEXT NOT NULL DEFAULT '',
            source       TEXT NOT NULL DEFAULT 'model',
            revoked      INTEGER NOT NULL DEFAULT 0,
            created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
        )",
    )
    .execute(pool)
    .await?;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_statchg_char ON stat_changes(character_id)")
        .execute(pool)
        .await;
    Ok(())
}

/// 列账（新→旧）
pub async fn list_in(
    pool: &SqlitePool,
    branch: &Option<String>,
    character_id: Option<i64>,
    limit: i64,
) -> Result<Vec<ChangeRow>, sqlx::Error> {
    let clause = crate::story_db::branch_clause("branch_id", branch, 1);
    let mut idx = if branch.is_some() { 2 } else { 1 };
    let char_part = match character_id {
        Some(_) => {
            let p = format!(" AND character_id = ?{idx}");
            idx += 1;
            p
        }
        None => String::new(),
    };
    let sql = format!(
        "SELECT id, character_id, dim, delta, kind, reason, source, revoked, created_at
         FROM stat_changes WHERE {clause}{char_part} ORDER BY id DESC LIMIT ?{idx}"
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = branch { q = q.bind(b); }
    if let Some(cid) = character_id { q = q.bind(cid); }
    let rows = q.bind(limit.clamp(1, 200)).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| ChangeRow {
            id: r.try_get(0).unwrap_or(0),
            character_id: r.try_get(1).unwrap_or(0),
            dim: r.try_get(2).unwrap_or_default(),
            delta: r.try_get(3).unwrap_or(0),
            kind: r.try_get(4).unwrap_or_default(),
            reason: r.try_get(5).unwrap_or_default(),
            source: r.try_get(6).unwrap_or_default(),
            revoked: r.try_get(7).unwrap_or(0),
            created_at: r.try_get(8).unwrap_or_default(),
        })
        .collect())
}

/// 撤销结果
#[derive(Debug, Serialize)]
pub struct UndoResult {
    pub undo_id: i64,
    pub character_id: i64,
    pub dim: String,
    pub old: i64,
    pub new: i64,
}

/// 单条撤销：反向写一行 + 原行 revoked=1（夹 0~100；撤销行不能再撤）
pub async fn undo_in(pool: &SqlitePool, change_id: i64) -> Result<UndoResult, String> {
    let row = sqlx::query(
        "SELECT character_id, dim, delta, kind, source, revoked FROM stat_changes WHERE id = ?1",
    )
    .bind(change_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| format!("找不到这条变化（id = {change_id}）"))?;
    let character_id: i64 = row.try_get(0).map_err(|e| e.to_string())?;
    let dim: String = row.try_get(1).unwrap_or_default();
    let delta: i64 = row.try_get(2).unwrap_or(0);
    let kind: String = row.try_get(3).unwrap_or_default();
    let source: String = row.try_get(4).unwrap_or_default();
    let revoked: i64 = row.try_get(5).unwrap_or(0);
    if revoked != 0 {
        return Err("这条已经撤销过了".to_string());
    }
    if source == "undo" {
        return Err("撤销行本身不能再撤销".to_string());
    }
    let col = column_of(&dim).ok_or_else(|| format!("未知维度：{dim}"))?;
    let sql = format!("SELECT {col} FROM characters WHERE id = ?1");
    let old: i64 = sqlx::query_scalar::<_, i64>(&sql)
        .bind(character_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("角色不存在（id = {character_id}）"))?;
    let new = (old - delta).clamp(0, 100);
    let eff = new - old;
    let upd = format!("UPDATE characters SET {col} = ?1, version = version + 1 WHERE id = ?2");
    sqlx::query(&upd).bind(new).bind(character_id).execute(pool).await.map_err(|e| e.to_string())?;
    let undo_id: i64 = sqlx::query_scalar(
        "INSERT INTO stat_changes(character_id, dim, delta, kind, reason, source) VALUES(?1, ?2, ?3, ?4, ?5, 'undo') RETURNING id",
    )
    .bind(character_id)
    .bind(&dim)
    .bind(eff)
    .bind(&kind)
    .bind(format!("撤销 #{change_id}"))
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("UPDATE stat_changes SET revoked = 1 WHERE id = ?1")
        .bind(change_id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(UndoResult { undo_id, character_id, dim, old, new })
}

#[cfg(test)]
mod tests;
