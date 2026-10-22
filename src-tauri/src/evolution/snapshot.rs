// 从 `evolution.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `evolution.rs` 里的其余条目（子模块本来就能访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `use snapshot::*;` 取回，
// 外部 `crate::evolution::xxx` 的调用路径**全部不变**。

use super::*;

// ---------- 快照 ----------
/// 供其它模块（如②检查的一键替换）在执行改动前打快照的入口
pub async fn make_snapshot_for_story(story_id: &str) -> Result<i64, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    make_snapshot(&pool, false, None, None).await.map_err(|e| e.to_string())
}

/// 存一个设定版本。
///
/// - `temporary = true` → **会话内临时版本**（回滚前的自动备份）。程序启动时会清掉上一会话遗留的，
///   所以它"退出即失效"（用户 2026-10-12 定的语义）
/// - `note` → 界面上显示的一句说明（例如"回滚到 V15 前的自动备份"）
pub(super) async fn make_snapshot(
    pool: &SqlitePool,
    temporary: bool,
    note: Option<&str>,
    branch: Option<String>,
) -> Result<i64, sqlx::Error> {
    // 读取全量设定（直接查库）
    // **快照必须按分支隔离**：原来读全表，导致"主线快照里装着分支的角色"，
    // 回滚时删主线、插全量 → 和分支行撞主键（`UNIQUE constraint failed: characters.id`，实测踩到）。
    let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let mut chars = Vec::new();
    let c_sql = format!(
        "SELECT id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile,
                speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion,
                stat_screen, version, background_past, motivation, goal, dialogue_examples, behavior_rules
         FROM characters WHERE {clause} ORDER BY id"
    );
    let mut cq = sqlx::query(&c_sql);
    if let Some(b) = &branch {
        cq = cq.bind(b);
    }
    let crows = cq.fetch_all(pool).await?;
    for r in &crows {
        chars.push(Character {
            id: r.try_get(0)?,
            name: r.try_get(1)?,
            alias: r.try_get(2)?,
            role: r.try_get(3)?,
            faction: r.try_get(4)?,
            age: r.try_get(5)?,
            gender: r.try_get(6)?,
            first_appearance: r.try_get(7)?,
            one_liner: r.try_get(8)?,
            profile: r.try_get(9)?,
            speech_style: r.try_get(10)?,
            traits: r.try_get(11)?,
            current_status: r.try_get(12)?,
            stat_stamina: r.try_get(13)?,
            stat_sanity: r.try_get(14)?,
            stat_emotion: r.try_get(15)?,
            stat_screen: r.try_get(16)?,
            background_past: r.try_get(18)?,
            motivation: r.try_get(19)?,
            goal: r.try_get(20)?,
            dialogue_examples: r.try_get(21)?,
            behavior_rules: r.try_get(22)?,
            version: r.try_get(17)?,
        });
    }

    let mut rels = Vec::new();
    let r_sql = format!(
        "SELECT id, from_character_id, to_character_id, relation_type, direction, strength, state,
                description, evidence, version
         FROM relations WHERE {clause} ORDER BY id"
    );
    let mut rq = sqlx::query(&r_sql);
    if let Some(b) = &branch {
        rq = rq.bind(b);
    }
    let rrows = rq.fetch_all(pool).await?;
    for r in &rrows {
        rels.push(Relation {
            id: r.try_get(0)?,
            from_character_id: r.try_get(1)?,
            to_character_id: r.try_get(2)?,
            relation_type: r.try_get(3)?,
            direction: r.try_get(4)?,
            strength: r.try_get(5)?,
            state: r.try_get(6)?,
            description: r.try_get(7)?,
            evidence: r.try_get(8)?,
            version: r.try_get(9)?,
            split_records: Vec::new(),
            split_matched: false,
        });
    }

    let mut world = Vec::new();
    // 注意：必须带 id —— 差异预览要按 id 把"当前设定"与"快照"对齐；
    // 早期版本漏了 id，导致预览把全部世界观条目误报成"将删除"（真机测试发现）。
    // 索引随之整体后移一位（id 在 0 号位）。
    let w_sql = format!(
        "SELECT id, key, value, is_hard_rule, version, category, meta_json
         FROM world_settings WHERE {clause} ORDER BY id"
    );
    let mut wq = sqlx::query(&w_sql);
    if let Some(b) = &branch {
        wq = wq.bind(b);
    }
    let rows = wq.fetch_all(pool).await?;
    for r in &rows {
        world.push(serde_json::json!({
            "id": r.try_get::<i64, _>(0)?,
            "key": r.try_get::<String, _>(1)?,
            "value": r.try_get::<String, _>(2)?,
            "is_hard_rule": r.try_get::<i64, _>(3)?,
            "version": r.try_get::<i64, _>(4)?,
            "category": r.try_get::<String, _>(5)?,
            "meta_json": r.try_get::<String, _>(6)?,
        }));
    }

    let chars_json: Vec<serde_json::Value> = chars
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id, "name": c.name, "alias": c.alias, "role": c.role,
                "faction": c.faction, "age": c.age, "gender": c.gender,
                "first_appearance": c.first_appearance, "one_liner": c.one_liner,
                "profile": c.profile, "speech_style": c.speech_style, "traits": c.traits,
                "current_status": c.current_status,
                "stat_stamina": c.stat_stamina, "stat_sanity": c.stat_sanity,
                "stat_emotion": c.stat_emotion, "stat_screen": c.stat_screen,
                "background_past": c.background_past, "motivation": c.motivation,
                "goal": c.goal, "dialogue_examples": c.dialogue_examples,
                "behavior_rules": c.behavior_rules,
                "version": c.version,
            })
        })
        .collect();
    let rels_json: Vec<serde_json::Value> = rels
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id, "from_character_id": r.from_character_id,
                "to_character_id": r.to_character_id, "relation_type": r.relation_type,
                "direction": r.direction, "strength": r.strength, "state": r.state,
                "description": r.description, "evidence": r.evidence, "version": r.version,
            })
        })
        .collect();

    let payload = serde_json::json!({
        "characters": chars_json,
        "relations": rels_json,
        "world_settings": world,
    });

    // 版本号 = 最大 version + 1；无则 1
    let row = sqlx::query("SELECT COALESCE(MAX(version), 0) FROM setting_snapshots")
        .fetch_one(pool)
        .await?;
    let prev: i64 = row.try_get(0)?;
    let new_version = prev + 1;

    sqlx::query(
        "INSERT INTO setting_snapshots(version, payload_json, parent_version, temporary, note, branch_id)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(new_version)
    .bind(payload.to_string())
    .bind(if prev == 0 { None } else { Some(prev) })
    .bind(if temporary { 1i64 } else { 0i64 })
    .bind(note.unwrap_or(""))
    .bind(branch.clone())
    .execute(pool)
    .await?;

    Ok(new_version)
}

#[derive(serde::Serialize)]
pub struct Snapshot {
    pub id: i64,
    pub version: i64,
    pub parent_version: Option<i64>,
    pub created_at: String,
    /// 第五批 ②：会话内临时版本（回滚前的自动备份）。程序启动时会被清掉 → "退出即失效"
    #[serde(default)]
    pub temporary: i64,
    /// 一句话说明（例如"回滚到 V15 前的自动备份"）
    #[serde(default)]
    pub note: String,
}

pub async fn list_snapshots(story_id: &str) -> Result<Vec<Snapshot>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // 第五批 ③：只列**当前分支**的设定版本
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let sql = format!(
        "SELECT id, version, parent_version, created_at, temporary, note
         FROM setting_snapshots WHERE {} ORDER BY version DESC",
        clause
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = &branch {
        q = q.bind(b);
    }
    let rows = q
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push(Snapshot {
            id: r.try_get(0).map_err(|e| e.to_string())?,
            version: r.try_get(1).map_err(|e| e.to_string())?,
            parent_version: r.try_get(2).map_err(|e| e.to_string())?,
            created_at: r.try_get(3).map_err(|e| e.to_string())?,
            temporary: r.try_get::<i64, _>(4).unwrap_or(0),
            note: r.try_get::<String, _>(5).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// 把一个**临时版本转为永久**（"保存这个版本"）。
///
/// 用户场景（2026-10-12）：手改角色属性前系统自动存了一个 ⏳ 临时版本，
/// 改完发现"这个状态我想留住" —— 但临时版本退出程序就没了。
/// 这个命令把它变成普通版本：清理时不再删它，永久保留。
///
/// 只允许转 `temporary = 1` 的（永久版本调用它是无意义的空操作，直接返回 Err 说明清楚）。
pub async fn keep_snapshot(story_id: &str, version: i64) -> Result<String, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let temp: Option<i64> = sqlx::query_scalar("SELECT temporary FROM setting_snapshots WHERE version = ?1")
        .bind(version)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?;
    let Some(temp) = temp else {
        return Err(format!("设定版本 V{version} 不存在"));
    };
    if temp == 0 {
        return Err(format!("V{version} 已经是永久版本，无需保存"));
    }
    sqlx::query("UPDATE setting_snapshots SET temporary = 0, note = ?1 WHERE version = ?2")
        .bind(format!("用户保留的版本（原为编辑前自动备份）"))
        .bind(version)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(format!("已把 V{version} 保存为永久版本，不会再被自动清理"))
}

/// 第五批 ②（B）：**直接编辑设定之前**自动存一个临时版本，返回版本号。
///
/// 与 `make_rollback_backup` 的区别只是 note 文案：那个用于回滚前，这个用于"手改设定前"。
/// 两者都存**临时**版本（退出程序即失效），所以不会把版本历史堆满。
///
/// `what` 由界面传入（如"修改角色「夏婉琴」"），写进 note 便于回看"这份备份是为什么存的"。
pub async fn make_edit_backup(story_id: &str, what: &str) -> Result<i64, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // 备份落在**当前所在分支**上（在分支里改设定时，备份的应是该分支的状态）
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let note = format!("编辑前自动备份：{what}（临时，退出程序即清除）");
    make_snapshot(&pool, true, Some(&note), branch)
        .await
        .map_err(|e| e.to_string())
}

/// 清掉**上一次运行**遗留的临时版本（在程序启动时对每个故事调用一次）。
///
/// 这是"退出即失效"的实现方式：临时版本只在本会话内有意义，重启后就没有保留价值，
/// 留着只会把版本列表越堆越长、把真正的永久版本淹没。
/// 返回清掉的条数（供界面提示）。
pub async fn purge_temporary_snapshots(story_id: &str) -> Result<i64, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let n = sqlx::query("DELETE FROM setting_snapshots WHERE temporary = 1")
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    Ok(n as i64)
}

/// 清掉**所有故事**里遗留的临时版本（程序启动时调用 —— 那时还不知道用户会打开哪个故事）。
///
/// 遍历 library.db 里的故事目录：每个 `library/<库>/projects/<id>/story.db` 一个。
/// 单个故事失败只跳过它，不影响其它故事，也绝不影响启动。
pub async fn purge_all_temporary_snapshots() -> Result<i64, String> {
    // 库根：与 db::library_db_path() / story_db_path() 同一套定位（paths::library_root）
    let root = crate::paths::library_root();
    let mut total = 0i64;
    let mut stack = vec![root.clone()];
    let mut dbs: Vec<std::path::PathBuf> = Vec::new();
    // 广度遍历找出所有 story.db（层级固定，但用遍历更稳，避免写死深度）
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().map(|n| n == "story.db").unwrap_or(false) {
                dbs.push(path);
            }
        }
    }
    for db in dbs {
        // 用文件路径直接连、不经过 story_id（启动时没有 story_id 上下文）
        let Ok(opts) = sqlx::sqlite::SqliteConnectOptions::from_str(&format!("sqlite:{}", db.display())) else {
            continue;
        };
        let Ok(pool) = SqlitePool::connect_with(opts).await else { continue };
        // 老库可能还没这一列：先确保存在（幂等）
        if crate::story_db::ensure_snapshot_temporary_column(&pool).await.is_err() {
            continue;
        }
        if let Ok(r) = sqlx::query("DELETE FROM setting_snapshots WHERE temporary = 1")
            .execute(&pool)
            .await
        {
            total += r.rows_affected() as i64;
        }
    }
    Ok(total)
}

/// 回滚**之前**自动存一份"当前状态"作为临时版本，返回它的版本号。
///
/// 为什么需要：回滚是破坏性的（整表替换）。有了这份备份，用户点错回滚还能再回滚回来。
/// 之所以做成**临时**（而不是普通版本）：用户不希望每次回滚都在版本列表里留下永久记录
/// （2026-10-12 定："做成临时的，一旦退出程序就失效"）。
pub async fn make_rollback_backup(story_id: &str, target_version: i64) -> Result<i64, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // 备份要落在**当前所在分支**上（否则分支里回滚时会把备份存到主线）
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let note = format!("回滚到 V{target_version} 前的自动备份（临时，退出程序即清除）");
    make_snapshot(&pool, true, Some(&note), branch)
        .await
        .map_err(|e| e.to_string())
}

/// 读取当前 relations 摘要（id + 两端角色名 + 类型/方向/强度），用于回滚前后对比。
pub(super) async fn relation_summary(pool: &SqlitePool) -> Result<Vec<(i64, String, String, String, String, i64)>, String> {
    let rows = sqlx::query(
        "SELECT r.id, c1.name, c2.name, r.relation_type, r.direction, r.strength
         FROM relations r
         LEFT JOIN characters c1 ON c1.id = r.from_character_id
         LEFT JOIN characters c2 ON c2.id = r.to_character_id
         ORDER BY r.id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in &rows {
        out.push((
            r.try_get::<i64, _>(0).map_err(|e| e.to_string())?,
            r.try_get::<String, _>(1).unwrap_or_default(),
            r.try_get::<String, _>(2).unwrap_or_default(),
            r.try_get::<String, _>(3).unwrap_or_default(),
            r.try_get::<String, _>(4).unwrap_or_default(),
            r.try_get::<i64, _>(5).map_err(|e| e.to_string())?,
        ));
    }
    Ok(out)
}

/// 单个字段的差异
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct FieldChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

/// 一个条目的差异（新增 / 删除 / 修改）
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct ItemChange {
    /// "added" | "removed" | "modified"
    pub kind: String,
    /// 人类可读的条目名（角色名 / "A 是 B 的『X』" / 世界观条目的 key）
    pub label: String,
    /// 仅 modified 时非空
    pub fields: Vec<FieldChange>,
}

/// 一份快照与"当前设定"的差异
#[derive(serde::Serialize, Clone, Debug, Default)]
pub struct SnapshotDiff {
    pub characters: Vec<ItemChange>,
    pub relations: Vec<ItemChange>,
    pub world_settings: Vec<ItemChange>,
}

impl SnapshotDiff {
    // 保留未用：提案统计（界面将来用）
    #[allow(dead_code)]
    pub fn total(&self) -> usize {
        self.characters.len() + self.relations.len() + self.world_settings.len()
    }
    /// 差异摘要文案（"新增 2、修改 3、删除 1"），供界面直接显示
    // 保留未用：提案统计（界面将来用）
    #[allow(dead_code)]
    pub fn summary(&self) -> String {
        let mut a = 0;
        let mut m = 0;
        let mut r = 0;
        for it in self.characters.iter().chain(self.relations.iter()).chain(self.world_settings.iter()) {
            match it.kind.as_str() {
                "added" => a += 1,
                "modified" => m += 1,
                _ => r += 1,
            }
        }
        format!("新增 {a} · 修改 {m} · 删除 {r}")
    }
}

pub(super) fn jstr(v: &serde_json::Value, k: &str) -> String {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// 角色卡被比对的字段（**刻意不含 version** —— 版本号是内部计数，用户看不懂也不该关心）
pub(super) const CHAR_FIELDS: &[(&str, &str)] = &[
    ("name", "姓名"),
    ("alias", "别名"),
    ("role", "定位"),
    ("faction", "势力"),
    ("age", "年龄"),
    ("gender", "性别"),
    ("one_liner", "一句话"),
    ("profile", "小传"),
    ("speech_style", "说话方式"),
    ("traits", "性格"),
    ("current_status", "当前状态"),
    ("stat_stamina", "体力"),
    ("stat_sanity", "理智"),
    ("stat_emotion", "情绪"),
    ("stat_screen", "存在感"),
    // v3.4「角色卡 v2」
    ("background_past", "过去"),
    ("motivation", "动机"),
    ("goal", "目的"),
    ("dialogue_examples", "对话示例"),
    ("behavior_rules", "行为准则"),
];

pub(super) const REL_FIELDS: &[(&str, &str)] = &[
    ("relation_type", "关系"),
    ("direction", "方向"),
    ("strength", "强度"),
    ("state", "状态"),
    ("description", "说明"),
    ("evidence", "证据"),
];

pub(super) const WORLD_FIELDS: &[(&str, &str)] = &[
    ("key", "条目"),
    ("value", "内容"),
    ("is_hard_rule", "硬规则"),
    ("category", "分类"),
];

/// 比对两组条目：`before`（当前）→ `after`（快照）。
///
/// **对齐规则**：优先按 `id`；条目没有 `id` 时退回按 `key`（世界观条目就是这种 ——
/// 它的主键是 id，但 payload 早期版本没写 id，只存了 key）。
/// 若两边都没有 id/key（理论上不该发生），则退化为"按数组下标"对齐，宁可报得少，
/// 也不要造出一堆假的"将新增 / 将删除"。
///
/// 纯函数：只吃 JSON 数组，不碰数据库 —— 便于单测，也便于以后做"任意两个版本对比"。
pub(super) fn diff_items(
    before: &[serde_json::Value],
    after: &[serde_json::Value],
    fields: &[(&str, &str)],
    label_of: &dyn Fn(&serde_json::Value) -> String,
    // 按哪个字段把两边对齐。**必须由调用方指定**，不能让函数"聪明地"自己挑：
    // 若一边有 id（当前库）而另一边没有（老快照 payload），自动挑就会挑出不同的键，
    // 于是同一条目一边算"新增"、一边算"删除"（真机测试踩到过这个坑）。
    match_on: &str,
) -> Vec<ItemChange> {
    use std::collections::BTreeMap;
    let key_of = |v: &serde_json::Value, fallback_idx: usize| -> String {
        match v.get(match_on) {
            Some(serde_json::Value::String(s)) => format!("s:{s}"),
            Some(serde_json::Value::Number(n)) => format!("n:{n}"),
            _ => format!("x:{fallback_idx}"),
        }
    };
    let idx = |arr: &[serde_json::Value]| -> BTreeMap<String, serde_json::Value> {
        arr.iter()
            .enumerate()
            .map(|(i, v)| (key_of(v, i), v.clone()))
            .collect()
    };
    let (bm, am) = (idx(before), idx(after));
    let mut out = Vec::new();
    // 新增（快照里有、当前没有）
    for (k, v) in &am {
        if !bm.contains_key(k) {
            out.push(ItemChange { kind: "added".into(), label: label_of(v), fields: vec![] });
        }
    }
    // 删除（当前有、快照没有）
    for (k, v) in &bm {
        if !am.contains_key(k) {
            out.push(ItemChange { kind: "removed".into(), label: label_of(v), fields: vec![] });
        }
    }
    // 修改（两边都有但字段不同）
    for (k, b) in &bm {
        if let Some(a) = am.get(k) {
            let mut fs = Vec::new();
            for (k, cn) in fields {
                let (bv, av) = (jstr(b, k), jstr(a, k));
                if bv != av {
                    fs.push(FieldChange { field: (*cn).to_string(), before: bv, after: av });
                }
            }
            if !fs.is_empty() {
                out.push(ItemChange { kind: "modified".into(), label: label_of(a), fields: fs });
            }
        }
    }
    out
}

/// 对比"某快照"与"当前设定"。返回的每一项都写明：当前是什么、回滚后会变成什么。
///
/// 用途：回滚是**破坏性操作**（现存设定会被整表替换）。原实现只有一句"确定回滚吗"，
/// 用户根本不知道会失去什么 —— 这里先给出可读的差异清单，让回滚变成有依据的决定。
pub async fn preview_snapshot(story_id: &str, version: i64) -> Result<SnapshotDiff, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    let row = sqlx::query("SELECT payload_json FROM setting_snapshots WHERE version = ?1")
        .bind(version)
        .fetch_optional(&pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("快照 V{version} 不存在"))?;
    let payload: serde_json::Value =
        serde_json::from_str(&row.try_get::<String, _>(0).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;

    // 当前设定（读成同样的 JSON 形状，直接复用 diff_items）
    //
    // **必须与 make_snapshot 用同一套分支过滤**：否则快照里只有本分支的行、
    // 而"当前"读的是全表 → 别的分支的行会被误报成"回滚将删除"。
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);
    let clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let chars_sql = format!(
        "SELECT id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile,
                speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion,
                stat_screen
         FROM characters WHERE {clause} ORDER BY id"
    );
    let mut chq = sqlx::query(&chars_sql);
    if let Some(b) = &branch {
        chq = chq.bind(b);
    }
    let cur_chars: Vec<serde_json::Value> = chq
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>(0).unwrap_or(0),
            "name": r.try_get::<String, _>(1).unwrap_or_default(),
            "alias": r.try_get::<String, _>(2).unwrap_or_default(),
            "role": r.try_get::<String, _>(3).unwrap_or_default(),
            "faction": r.try_get::<String, _>(4).unwrap_or_default(),
            "age": r.try_get::<String, _>(5).unwrap_or_default(),
            "gender": r.try_get::<String, _>(6).unwrap_or_default(),
            "first_appearance": r.try_get::<String, _>(7).unwrap_or_default(),
            "one_liner": r.try_get::<String, _>(8).unwrap_or_default(),
            "profile": r.try_get::<String, _>(9).unwrap_or_default(),
            "speech_style": r.try_get::<String, _>(10).unwrap_or_default(),
            "traits": r.try_get::<String, _>(11).unwrap_or_default(),
            "current_status": r.try_get::<String, _>(12).unwrap_or_default(),
            "stat_stamina": r.try_get::<i64, _>(13).unwrap_or(50),
            "stat_sanity": r.try_get::<i64, _>(14).unwrap_or(50),
            "stat_emotion": r.try_get::<i64, _>(15).unwrap_or(50),
            "stat_screen": r.try_get::<i64, _>(16).unwrap_or(50),
        })
    })
    .collect();

    let rels_sql = format!(
        "SELECT r.id, c1.name, c2.name, r.relation_type, r.direction, r.strength, r.state,
                r.description, r.evidence
         FROM relations r
         LEFT JOIN characters c1 ON c1.id = r.from_character_id
         LEFT JOIN characters c2 ON c2.id = r.to_character_id
         WHERE {clause_r} ORDER BY r.id",
        clause_r = crate::story_db::branch_clause("r.branch_id", &branch, 1)
    );
    let mut rq = sqlx::query(&rels_sql);
    if let Some(b) = &branch {
        rq = rq.bind(b);
    }
    let cur_rels: Vec<serde_json::Value> = rq
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>(0).unwrap_or(0),
            "from_name": r.try_get::<String, _>(1).unwrap_or_default(),
            "to_name": r.try_get::<String, _>(2).unwrap_or_default(),
            "relation_type": r.try_get::<String, _>(3).unwrap_or_default(),
            "direction": r.try_get::<String, _>(4).unwrap_or_default(),
            "strength": r.try_get::<i64, _>(5).unwrap_or(50),
            "state": r.try_get::<String, _>(6).unwrap_or_default(),
            "description": r.try_get::<String, _>(7).unwrap_or_default(),
            "evidence": r.try_get::<String, _>(8).unwrap_or_default(),
        })
    })
    .collect();

    let world_sql = format!(
        "SELECT id, key, value, is_hard_rule, category FROM world_settings
         WHERE {clause} ORDER BY id"
    );
    let mut wq = sqlx::query(&world_sql);
    if let Some(b) = &branch {
        wq = wq.bind(b);
    }
    let cur_world: Vec<serde_json::Value> = wq
    .fetch_all(&pool)
    .await
    .map_err(|e| e.to_string())?
    .iter()
    .map(|r| {
        serde_json::json!({
            "id": r.try_get::<i64, _>(0).unwrap_or(0),
            "key": r.try_get::<String, _>(1).unwrap_or_default(),
            "value": r.try_get::<String, _>(2).unwrap_or_default(),
            "is_hard_rule": r.try_get::<i64, _>(3).unwrap_or(0),
            "category": r.try_get::<String, _>(4).unwrap_or_default(),
        })
    })
    .collect();

    let arr = |v: &serde_json::Value, k: &str| -> Vec<serde_json::Value> {
        v.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default()
    };

    // 关系的条目名："A → B（类型）"；快照 payload 里 relations 只存 id，
    // 名字要从**当前**关系表或快照里的角色名单反查（两边都试，保证标签可读）
    let name_of = |v: &serde_json::Value, ids: &std::collections::HashMap<i64, String>| -> String {
        let f = v.get("from_character_id").and_then(|x| x.as_i64()).unwrap_or(0);
        let t = v.get("to_character_id").and_then(|x| x.as_i64()).unwrap_or(0);
        let fname = ids.get(&f).cloned().unwrap_or_else(|| format!("#{f}"));
        let tname = ids.get(&t).cloned().unwrap_or_else(|| format!("#{t}"));
        format!("{fname} → {tname}")
    };
    let cur_names: std::collections::HashMap<i64, String> = cur_chars
        .iter()
        .filter_map(|c| Some((c.get("id")?.as_i64()?, c.get("name")?.as_str()?.to_string())))
        .collect();
    let snap_names: std::collections::HashMap<i64, String> = arr(&payload, "characters")
        .iter()
        .filter_map(|c| Some((c.get("id")?.as_i64()?, c.get("name")?.as_str()?.to_string())))
        .collect();
    // 合并两份名单：快照里删掉的角色也要能显示出名字
    let mut all_names = snap_names.clone();
    for (k, v) in &cur_names {
        all_names.entry(*k).or_insert_with(|| v.clone());
    }

    let diff = SnapshotDiff {
        characters: diff_items(
            &cur_chars,
            &arr(&payload, "characters"),
            CHAR_FIELDS,
            &|v| jstr(v, "name"),
            "id",
        ),
        relations: diff_items(
            &cur_rels,
            &arr(&payload, "relations"),
            REL_FIELDS,
            &|v| name_of(v, &all_names),
            "id",
        ),
        world_settings: diff_items(
            &cur_world,
            &arr(&payload, "world_settings"),
            WORLD_FIELDS,
            &|v| jstr(v, "key"),
            // 世界观条目按 **key** 对齐而不是 id：老快照的 payload 里没有 id，
            // 按 id 对齐会让同一条目同时出现在"新增"与"删除"里（真机测试发现）
            "key",
        ),
    };
    Ok(diff)
}

/// 回滚到指定快照版本：以该版本 payload 覆盖 characters / relations / world_settings，并写审计。
/// 回滚辅助：`id` 在该表里**是否已被"别的分支"占着**。
///
/// 为什么需要：早先的 `make_snapshot` 读全表（不分分支），
/// 所以**老快照的 payload 里混着别的分支的行**。回滚时若照原 id 插回去，
/// 会与那些仍存在的分支行撞主键 —— 就是 `UNIQUE constraint failed: characters.id`。
/// 这类行应该**跳过**：它们不属于当前分支，本来就不该被这次回滚改动。
pub(super) async fn id_taken_by_other_branch(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
    id: i64,
    branch: &Option<String>,
) -> Result<bool, String> {
    let sql = format!("SELECT branch_id FROM {table} WHERE id = ?1");
    let existing: Option<Option<String>> = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    Ok(matches!(existing, Some(b) if &b != branch))
}

/// 回滚辅助：`id` 在该表里是否还存在（同分支残留的异常情况）。
pub(super) async fn id_exists(conn: &mut sqlx::SqliteConnection, table: &str, id: i64) -> Result<bool, String> {
    let sql = format!("SELECT 1 FROM {table} WHERE id = ?1");
    let hit: Option<i64> = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;
    Ok(hit.is_some())
}

/// 把一份快照 payload **写回**当前分支（回滚的后半段）。
///
/// 抽成独立函数的原因：这段逻辑踩过主键冲突（`UNIQUE constraint failed: characters.id`），
/// 需要能被单测直接覆盖 —— 而 `rollback_snapshot` 会自己按 story_id 开池，测不进去。
pub(super) async fn restore_settings_from_payload(
    conn: &mut sqlx::SqliteConnection,
    branch: &Option<String>,
    payload: &serde_json::Value,
) -> Result<(), String> {
    // 角色 id 映射（旧 id → 新 id），关系要用它重写两端。
    // 主线：沿用快照里的 id（原行为不变）；分支：**不沿用** ——
    // 否则分支会与源分支的主线行撞主键（分支里已经有同 id 的复制行）。
    let mut char_id_map: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    if let Some(chars) = payload.get("characters").and_then(|v| v.as_array()) {
        for c in chars {
            let old_id = c.get("id").and_then(|v| v.as_i64());
            let mut use_explicit_id = if branch.is_some() { None } else { old_id };
            if let Some(oid) = old_id {
                if id_taken_by_other_branch(&mut *conn, "characters", oid, &branch).await? {
                    // 老快照混进来的别的分支的行 → 跳过（不改别的分支、也不撞主键）
                    continue;
                }
                if id_exists(&mut *conn, "characters", oid).await? {
                    // 同分支却还在（删除没覆盖到）→ 交给自增，宁可换 id 也别让回滚整体失败
                    use_explicit_id = None;
                }
            }
            let inserted = sqlx::query(
                "INSERT INTO characters(id, name, alias, role, faction, age, gender, first_appearance, one_liner, profile, speech_style, traits, current_status, stat_stamina, stat_sanity, stat_emotion, stat_screen, background_past, motivation, goal, dialogue_examples, behavior_rules, version, branch_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24) RETURNING id",
            )
            // 老快照 payload 里可能没有 id；**不能**退回 0（会主键冲突 1555），传 None 让数据库自增
            .bind(use_explicit_id)
            .bind(c.get("name").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("alias").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("role").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("faction").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("age").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("gender").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("first_appearance").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("one_liner").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("profile").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("speech_style").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("traits").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("current_status").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("stat_stamina").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("stat_sanity").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("stat_emotion").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(c.get("stat_screen").and_then(|v| v.as_i64()).unwrap_or(50))
            // v3.4：老快照没有这些 key → 空串（前向兼容：新字段回滚后为空，不报错）
            .bind(c.get("background_past").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("motivation").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("goal").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("dialogue_examples").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("behavior_rules").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(c.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .bind(branch.clone())
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;
            if let Some(oid) = old_id {
                char_id_map.insert(oid, inserted.try_get::<i64, _>(0).unwrap_or(oid));
            }
        }
    }
    if let Some(rels) = payload.get("relations").and_then(|v| v.as_array()) {
        for r in rels {
            let mut rel_id = if branch.is_some() { None } else { r.get("id").and_then(|v| v.as_i64()) };
            if let Some(oid) = rel_id {
                if id_taken_by_other_branch(&mut *conn, "relations", oid, &branch).await? {
                    continue;
                }
                if id_exists(&mut *conn, "relations", oid).await? {
                    rel_id = None;
                }
            }
            sqlx::query(
                "INSERT INTO relations(id, from_character_id, to_character_id, relation_type, direction, strength, state, description, evidence, version, branch_id)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )
            // 同上：缺失 id 时传 None；分支下不沿用 id；被别的分支占着则已 continue
            .bind(rel_id)
            // 两端按映射重写（分支里角色是新 id，不重写会指向源分支的角色行）
            .bind({
                let old = r.get("from_character_id").and_then(|v| v.as_i64()).unwrap_or(0);
                *char_id_map.get(&old).unwrap_or(&old)
            })
            .bind({
                let old = r.get("to_character_id").and_then(|v| v.as_i64()).unwrap_or(0);
                *char_id_map.get(&old).unwrap_or(&old)
            })
            .bind(r.get("relation_type").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(r.get("direction").and_then(|v| v.as_str()).unwrap_or("单向"))
            .bind(r.get("strength").and_then(|v| v.as_i64()).unwrap_or(50))
            .bind(r.get("state").and_then(|v| v.as_str()).unwrap_or("稳定"))
            .bind(r.get("description").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(r.get("evidence").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(r.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .bind(branch.clone())
            .execute(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;
        }
    }
    if let Some(worlds) = payload.get("world_settings").and_then(|v| v.as_array()) {
        for w in worlds {
            let mut wid = if branch.is_some() { None } else { w.get("id").and_then(|v| v.as_i64()) };
            if let Some(oid) = wid {
                if id_taken_by_other_branch(&mut *conn, "world_settings", oid, &branch).await? {
                    continue;
                }
                if id_exists(&mut *conn, "world_settings", oid).await? {
                    wid = None;
                }
            }
            sqlx::query(
                "INSERT INTO world_settings(id, key, value, is_hard_rule, version, category, meta_json, branch_id) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            // 同上：缺失 id 时传 None；分支下不沿用 id；被别的分支占着则已 continue
            .bind(wid)
            .bind(w.get("key").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("value").and_then(|v| v.as_str()).unwrap_or(""))
            .bind(w.get("is_hard_rule").and_then(|v| v.as_i64()).unwrap_or(0))
            .bind(w.get("version").and_then(|v| v.as_i64()).unwrap_or(1))
            .bind(w.get("category").and_then(|v| v.as_str()).unwrap_or("设定"))
            .bind(w.get("meta_json").and_then(|v| v.as_str()).unwrap_or("{}"))
            .bind(branch.clone())
            .execute(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

pub async fn rollback_snapshot(story_id: &str, version: i64) -> Result<String, String> {
    // 第五批 ③：**先解析出"当前在哪条分支"** —— 备份与回滚都必须落在同一条分支上。
    // （曾经把备份写在解析之前 → 分支里回滚时，备份会存到主线，备份的不是被改的那份状态。）
    let branch = crate::branch::resolve_branch(story_id, &None).await.unwrap_or(None);

    // 第五批 ②：**先自动存一份当前状态**（临时版本），这样点错回滚还能退回来。
    // 备份失败**不阻断回滚**（否则"备份不了就永远回滚不了"），但要把情况告诉用户。
    let backup_note = match make_rollback_backup(story_id, version).await {
        Ok(v) => format!("已自动备份当前状态为临时版本 V{v}（退出程序即清除；想长期留住可用列表里的「保存为永久」）。"),
        Err(e) => format!("⚠ 自动备份失败（{e}）；本次回滚后无法退回。"),
    };
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    rollback_snapshot_in(&pool, version, branch, backup_note).await
}

/// `rollback_snapshot` 的实现（**pool / 分支 / 备份说明可注入**，测试直接传临时库）。
///
/// 拆出来是为了能测住"回滚失败必须整体回退"——之前这段逻辑自己按 story_id 开池，单测进不去，
/// 于是"删了一半就报错"的事故没有被任何测试拦住。
pub(super) async fn rollback_snapshot_in(
    pool: &SqlitePool,
    version: i64,
    branch: Option<String>,
    backup_note: String,
) -> Result<String, String> {
    let before = relation_summary(pool).await?;
    let row = sqlx::query("SELECT version, payload_json FROM setting_snapshots WHERE version = ?1")
        .bind(version)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("快照 V{} 不存在", version))?;
    let payload: serde_json::Value = serde_json::from_str(&row.try_get::<String, _>(1).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    // 清空并恢复三张设定表。
    // 第五批 ③：**只清目标分支的行**（`branch_id IS NULL` = 主线）——
    // 原实现是整表 DELETE，分支下回滚会把别的分支的数据一起清掉。
    // **必须在一个事务里**：原来是"删三张表 → 逐条插回"，中途失败就留下删了一半的库。
    // 实测事故：插角色时撞主键（1555）报错，主线的关系与世界观已经被删掉、再也没插回来。
    let del_clause = crate::story_db::branch_clause("branch_id", &branch, 1);
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    for table in ["characters", "relations", "world_settings"] {
        let sql = format!("DELETE FROM {table} WHERE {}", del_clause);
        let mut q = sqlx::query(&sql);
        if let Some(b) = &branch {
            q = q.bind(b);
        }
        q.execute(&mut *tx).await.map_err(|e| e.to_string())?;
    }

    restore_settings_from_payload(&mut tx, &branch, &payload).await?;
    // 全部成功才提交；任一步失败 → 整个回滚，库保持回滚前的样子
    tx.commit().await.map_err(|e| e.to_string())?;

    audit(pool, "rollback", &format!("snapshot#{}", version), "", &payload.to_string())
        .await
        .map_err(|e| e.to_string())?;

    // 回滚前后对比，生成可见的变更摘要
    let after = relation_summary(pool).await?;
    let mut diffs = Vec::new();
    for (id, n1, n2, typ, dir, strength) in &after {
        if let Some((_, _b1, _b2, btyp, bdir, bstr)) = before.iter().find(|b| b.0 == *id) {
            let label = format!("{}→{}", n1, n2);
            if btyp != typ {
                diffs.push(format!("{} 关系类型 {}→{}", label, btyp, typ));
            }
            if bdir != dir {
                diffs.push(format!("{} 方向 {}→{}", label, bdir, dir));
            }
            if bstr != strength {
                diffs.push(format!("{} 强度 {}→{}", label, bstr, strength));
            }
        } else {
            diffs.push(format!("{}→{}（新增关系）", n1, n2));
        }
    }
    let summary = if diffs.is_empty() {
        "各关系无变化".to_string()
    } else {
        diffs.join("；")
    };
    // 把"已自动备份为临时版本 Vx"告诉用户 —— 否则他不知道还能退回来
    if backup_note.trim().is_empty() {
        Ok(format!("已回滚到快照 V{version}：{summary}"))
    } else {
        Ok(format!("已回滚到快照 V{version}：{summary}\n{backup_note}"))
    }
}
