// 从 `check.rs` 拆出的子模块（第二波「按内聚拆业务」）。
//
// `use super::*;` 让本文件看得见 `check.rs` 里的其余条目（子模块可访问父模块私有项）。
// 搬过来的条目统一提到 `pub(super)`，父模块用 `pub use patch::*;` 取回，
// 外部 `crate::check::xxx` 的调用路径**全部不变**。

use super::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PatchItem {
    /// 板块：characters / world / relations / terms / factions
    pub board: String,
    /// 条目类型：character / world_entry / term / faction / faction_relation / relation
    pub target_kind: String,
    /// 条目定位名
    pub target_name: String,
    /// 字段（英文名，须在白名单内）
    pub field: String,
    /// AI 认为的"当前值"（用于核对，防止 AI 基于错误理解给补丁）
    #[serde(default)]
    pub before: String,
    /// 改好的完整值（必须是完整值，不是半个） 
    pub value: String,
}

/// 每个板块允许被 AI 修改的字段白名单（= 数据库真实列名）。
/// 这是**防止 `no such column` 的关键闸门**：错字段在这里就被拒绝并给出人话原因，
/// 绝不放行到 SQL；同时天然挡掉 id / created_at 等内部字段。
pub(crate) fn field_allowed(board: &str, field: &str) -> bool {
    let chars = ["name", "alias", "role", "gender", "age", "one_liner", "profile", "speech_style", "traits"];
    // 注意：**不含 direction** —— 关系方向不交给 AI 改（用户 2026-10-11 决定）
    let rels = ["relation_type", "strength", "state", "description"];
    let world = ["value", "category", "attitude", "scale", "leader", "control_range"];
    match board {
        "characters" => chars.contains(&field) || rels.contains(&field),
        "world" => world.contains(&field),
        _ => false,
    }
}

/// 上下文感知的字段归一化：把模型写的别名 / 中文 / 泛化名，按**目标类型**翻译成数据库真实列名。
///
/// 关键点：模型经常用泛化词 `value` / `内容` / `数值` 表示"我要改的那个值" ——
/// 它到底是 `strength`（关系数值）、`profile`（人物小传）还是 `value`（世界条目内容），
/// 取决于**改的是谁**，所以本函数先判目标（名字带箭头 = 关系），再挑列名。
pub(crate) fn normalize_field(board: &str, target_kind: &str, target_name: &str, field: &str) -> Option<&'static str> {
    let raw = field.trim();
    let f = raw.to_ascii_lowercase();
    let has_arrow = target_name.contains('→') || target_name.contains("->")
        || target_name.contains("—>") || target_name.contains("–>");
    let is_relation = target_kind == "relation" || has_arrow;

    // 显式英文列名（原样，但要在本板块允许）
    let explicit: &[&str] = &[
        "name", "alias", "role", "gender", "age", "one_liner", "profile", "speech_style", "traits",
        "relation_type", "strength", "state", "description",
        "value", "category", "attitude", "scale", "leader", "control_range",
    ];

    // 别名 → 泛化语义（再由上下文决定具体列名）
    let alias = |k: &str| -> Option<&'static str> {
        Some(match k {
            "内容" | "正文" | "值" | "value" => "GENERIC_VALUE",
            "数值" | "强度" | "关系数值" | "strength" => "strength",
            "关系类型" | "relation_type" | "类型" => "relation_type",
            "状态" | "关系状态" | "state" => "state",
            "说明" | "关系说明" | "description" => "description",
            "人物小传" | "小传" | "profile" | "简介" => "profile",
            "说话方式" | "口癖" | "speech_style" => "speech_style",
            "性格" | "traits" => "traits",
            "一句话身份" | "身份" | "one_liner" => "one_liner",
            "角色名" | "名字" | "名称" | "name" => "name",
            "别名" | "alias" => "alias",
            "定位" | "角色定位" | "role" => "role",
            "性别" | "gender" => "gender",
            "年龄" | "age" => "age",
            "分类" | "category" => "category",
            "对主角态度" | "态度" | "attitude" => "attitude",
            "规模" | "scale" => "scale",
            "头目" | "leader" => "leader",
            "控制范围" | "control_range" => "control_range",
            _ => return None,
        })
    };

    // 顺序很关键：先解析"显式列名 / 别名"成中间语义，**再按上下文落地**，最后才做白名单校验。
    // 因为 `value` 在角色板块不是合法列名，但它在关系语境下应被理解成 strength ——
    // 若先校验白名单，就会被提前拒掉。
    // 先查别名表（它会把 value / 内容 这类**泛化词**标成 GENERIC_VALUE），
    // 别名表没命中才当作"显式列名"；若显式列名在本板块不合法，再按上下文落地。
    let mut col: &str = match alias(raw).or_else(|| alias(f.as_str())) {
        Some(c) => c,
        None => {
            if explicit.contains(&f.as_str()) {
                f.as_str()
            } else {
                return None;
            }
        }
    };
    // 只有泛化词（value / 内容 / 正文）才按上下文兜底 —— 那是模型表达"改那个值"的方式；
    // 像 scale 这种明确属于别的板块的字段一律拒绝，不猜（避免把世界字段偷偷改成关系数值）
    if col == "GENERIC_VALUE" {
        col = if is_relation {
            "strength"
        } else if board == "world" {
            "value"
        } else {
            "profile"
        };
    }
    if !field_allowed(board, col) {
        return None;
    }
    Some(match col {
        "name" => "name", "alias" => "alias", "role" => "role", "gender" => "gender", "age" => "age",
        "one_liner" => "one_liner", "profile" => "profile", "speech_style" => "speech_style", "traits" => "traits",
        "relation_type" => "relation_type", "strength" => "strength", "state" => "state", "description" => "description",
        "value" => "value", "category" => "category", "attitude" => "attitude", "scale" => "scale",
        "leader" => "leader", "control_range" => "control_range", _ => "value",
    })
}

/// 判断一条补丁是否其实是"角色关系"（名字带箭头，或字段是关系字段）—— 用于把误标成 world 的补丁改判回 characters
pub(crate) fn looks_like_relation(p: &PatchItem) -> bool {
    p.target_name.contains('→') || p.target_name.contains("->") || p.target_name.contains("—>") || p.target_name.contains("–>")
        || matches!(p.field.trim(), "strength" | "relation_type" | "state" | "数值" | "强度" | "关系数值" | "关系类型" | "状态")
}

/// 给"字段不被允许"的情况生成一句能指导用户的说明（列出该板块可用字段）
pub(crate) fn field_reject_reason(board: &str, field: &str) -> String {
    let list = match board {
        "characters" => "name / alias / role / gender / age / one_liner / profile / speech_style / traits（角色）或 relation_type / strength / state / description（关系）",
        "world" => "value / category / attitude / scale / leader / control_range",
        _ => "（未知板块）",
    };
    format!("字段「{field}」不在「{board}」板块的可改范围内（会造成数据库列不存在）。该板块只允许：{list}")
}

/// 归一化条目类型：模型偶尔会串用别的板块的枚举值（例如在角色板块写 `faction_relation`），
/// 按"板块 + 是否含箭头 + 字段是否属于关系"推断出正确的类型，而不是直接拒绝整批。
pub(crate) fn normalize_kind(board: &str, kind: &str, target_name: &str, field: &str) -> String {
    let has_arrow = target_name.contains("→") || target_name.contains("->") || target_name.contains("—>") || target_name.contains("–>");
    let rel_field = matches!(field, "relation_type" | "strength" | "state" | "description" | "direction");
    match board {
        "characters" => {
            if kind == "relation" || kind == "faction_relation" || rel_field || has_arrow {
                "relation".to_string()
            } else {
                "character".to_string()
            }
        }
        "world" => {
            // 世界板块全是按 key 定位的条目，统一成 world_entry（执行 SQL 一致）
            "world_entry".to_string()
        }
        _ => kind.to_string(),
    }
}

/// 校验补丁并返回 (归一化后的补丁, 人类可读的执行清单)；有问题则返回 Err（不执行任何修改）
pub(crate) async fn validate_patch(pool: &SqlitePool, items: &[PatchItem]) -> Result<(Vec<PatchItem>, Vec<String>), String> {
    let mut lines = Vec::new();
    let mut fixed: Vec<PatchItem> = Vec::new();
    for (i, p) in items.iter().enumerate() {
        let n = i + 1;
        if p.target_name.trim().is_empty() {
            return Err(format!("第 {n} 条补丁缺少目标名称，已中止"));
        }
        if p.value.trim().is_empty() {
            return Err(format!("第 {n} 条补丁的新值为空（{} · {}），已中止", p.target_name, p.field));
        }
        // ① 用**数据库**判定 target_name 到底是谁（角色名 / 世界条目 / A→B 关系），据此改判板块与类型。
        //    这比"猜字段像什么"可靠得多：模型把「宸熙」标成 world_entry 时也能纠正回来。
        let (mut board, mut kind) = resolve_target(pool, p).await;
        // ② 兜底：库里查不到时，仍按"名字带箭头 = 关系"处理
        if board == "world" && looks_like_relation(p) {
            board = "characters".to_string();
            kind = "relation".to_string();
        }
        // ③ 归一化条目类型（确保与板块匹配）
        let kind = normalize_kind(&board, &kind, &p.target_name, &p.field);
        // ③ 归一化字段名（别名 / 中文 / 泛化词 → 数据库列名）；归不了或本板块不允许 → 人话拒绝
        let mut col = match normalize_field(&board, &kind, &p.target_name, &p.field) {
            Some(c) => c.to_string(),
            None => return Err(format!("第 {n} 条补丁：{}", field_reject_reason(&board, &p.field))),
        };
        // ④ 用 AI 声明的"当前值"反查它**真正属于哪个字段**（模型不知道字段名，只有软件知道）。
        //    这是"以角色名定位、字段由软件判断"的落地：命中就改判到真实字段，避免因字段名写错而整批失败。
        if !p.before.trim().is_empty() {
            if let Some(real) = resolve_field(pool, &board, &kind, &p.target_name, &p.before).await {
                col = real;
            }
        }
        if !matches!(kind.as_str(), "character" | "relation" | "world_entry") {
            return Err(format!("第 {n} 条补丁的条目类型「{}」不认识，已中止", p.target_kind));
        }
        let mut q = p.clone();
        q.board = board.clone();
        q.target_kind = kind;
        q.field = col.clone();
        lines.push(format!("{} · {}（{}）→ {}", q.target_name, q.field, q.target_kind, q.value.trim()));
        fixed.push(q);
    }
    Ok((fixed, lines))
}

/// 比较"AI 声明的当前值"与"库中现值"：数值列按数字比（容忍 80 / "80" / 80.0 / 空白），文本列按 trim 后比。
pub(crate) fn values_equal(field: &str, a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if is_numeric_field(field) {
        if let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>()) {
            return (x - y).abs() < f64::EPSILON;
        }
    }
    a.trim() == b.trim()
}

/// 按"AI 看到的当前值"反查它**真正属于哪个字段**（用户 2026-10-11 定：以角色名定位，字段由软件判断）。
///
/// 背景：给 AI 的板块内容是各字段拼成的 JSON，模型分不清某段文字来自 `one_liner` 还是 `profile`，
/// 于是随口写一个字段名 → 与库中现值对不上 → 被 before 核对拒绝（实测报错：「夏婉琴」的 profile 当前值是「」，
/// 但 AI 以为它是「女主，身材瘦小纤细，身高154cm」——那段其实在 one_liner 里）。
///
/// 做法：拿 AI 声明的 `before` 去比对该角色**各个可改字段**的真实值 ——
/// 完全相等 / 互为包含都算命中；命中唯一字段就改判；命中多个就取"包含关系更具体"的那个；都没命中则不猜。
pub(crate) async fn resolve_field(
    pool: &SqlitePool,
    board: &str,
    target_kind: &str,
    target_name: &str,
    before: &str,
) -> Option<String> {
    let b = before.trim();
    if b.is_empty() {
        return None;
    }
    if target_kind == "relation" {
        // 关系：按表中现值反查（strength 是数字列，按 i64 读）
        let (from, to) = split_arrow(target_name).ok()?;
        let row = sqlx::query(
            "SELECT COALESCE(relation_type,''), COALESCE(strength,0), COALESCE(state,''), COALESCE(description,'')
             FROM relations WHERE from_character_id = (SELECT id FROM characters WHERE name = ?1)
               AND to_character_id = (SELECT id FROM characters WHERE name = ?2)"
        ).bind(&from).bind(&to).fetch_optional(pool).await.ok().flatten()?;
        let vals: [(&str, String); 4] = [
            ("relation_type", row.try_get(0).unwrap_or_default()),
            ("strength", row.try_get::<i64, _>(1).unwrap_or(0).to_string()),
            ("state", row.try_get(2).unwrap_or_default()),
            ("description", row.try_get(3).unwrap_or_default()),
        ];
        return pick_field(&vals, b);
    }
    if board == "world" {
        let row = sqlx::query("SELECT COALESCE(value,''), COALESCE(category,'') FROM world_settings WHERE key = ?1")
            .bind(target_name).fetch_optional(pool).await.ok().flatten()?;
        let vals: [(&str, String); 2] = [
            ("value", row.try_get(0).unwrap_or_default()),
            ("category", row.try_get(1).unwrap_or_default()),
        ];
        return pick_field(&vals, b);
    }
    // 角色：查全部可改文本字段
    let row = sqlx::query(
        "SELECT COALESCE(name,''), COALESCE(alias,''), COALESCE(role,''), COALESCE(gender,''), COALESCE(age,''),
                COALESCE(one_liner,''), COALESCE(profile,''), COALESCE(speech_style,''), COALESCE(traits,'')
         FROM characters WHERE name = ?1"
    ).bind(target_name).fetch_optional(pool).await.ok().flatten()?;
    let vals: [(&str, String); 9] = [
        ("name", row.try_get(0).unwrap_or_default()),
        ("alias", row.try_get(1).unwrap_or_default()),
        ("role", row.try_get(2).unwrap_or_default()),
        ("gender", row.try_get(3).unwrap_or_default()),
        ("age", row.try_get(4).unwrap_or_default()),
        ("one_liner", row.try_get(5).unwrap_or_default()),
        ("profile", row.try_get(6).unwrap_or_default()),
        ("speech_style", row.try_get(7).unwrap_or_default()),
        ("traits", row.try_get(8).unwrap_or_default()),
    ];
    pick_field(&vals, b)
}

/// 在"字段 → 当前值"里挑出与 `before` 匹配的那个：
/// 相等优先；否则取"库里值包含 before"或"before 包含库里值"的字段；多个命中取**更长的一方**（更具体）。
pub(super) fn pick_field(vals: &[(&str, String)], before: &str) -> Option<String> {
    for (f, v) in vals {
        if v.trim() == before && !before.is_empty() {
            return Some((*f).to_string());
        }
    }
    let mut best: Option<(&str, usize)> = None;
    for (f, v) in vals {
        let vt = v.trim();
        if vt.is_empty() {
            continue;
        }
        let related = vt.contains(before) || before.contains(vt);
        if related {
            let score = vt.chars().count();
            if best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some((f, score));
            }
        }
    }
    best.map(|(f, _)| f.to_string())
}

/// 用数据库反查"这个 target_name 到底是谁"，据此**改判板块与条目类型**。
/// 模型经常把角色（如「宸熙」）标成 `world_entry`、把角色关系标成 `world` —— 名字是唯一可靠的线索，
/// 所以这里直接查库：先看是不是角色名、再看是不是世界条目的 key、最后看是不是「A→B」关系。
/// 返回 `(board, target_kind)`；查不到就保留原样（后续白名单/定位会给出人话拒绝）。
pub(crate) async fn resolve_target(pool: &SqlitePool, p: &PatchItem) -> (String, String) {
    let name = p.target_name.trim();
    let is_arrow = p.target_name.contains('→') || p.target_name.contains("->")
        || p.target_name.contains("—>") || p.target_name.contains("–>");
    if is_arrow {
        return ("characters".to_string(), "relation".to_string());
    }
    // 角色名？
    let is_char: Option<i64> = sqlx::query_scalar("SELECT id FROM characters WHERE name = ?1")
        .bind(name).fetch_optional(pool).await.ok().flatten();
    if is_char.is_some() {
        return ("characters".to_string(), "character".to_string());
    }
    // 世界条目？
    let is_world: Option<String> = sqlx::query_scalar("SELECT key FROM world_settings WHERE key = ?1")
        .bind(name).fetch_optional(pool).await.ok().flatten();
    if is_world.is_some() {
        return ("world".to_string(), "world_entry".to_string());
    }
    (p.board.clone(), p.target_kind.clone())
}

/// 数值型列（SQLite 里存 INTEGER）：这些列必须按整数读取，否则 sqlx 会报
/// `mismatched types ... TEXT is not compatible with SQL type INTEGER`。
pub(crate) fn is_numeric_field(col: &str) -> bool {
    matches!(col, "strength")
}

/// 读取补丁目标字段的**当前值**（用于核对 `before` 与前端展示 diff）——
/// **按列类型分别读取**：数值列走 i64，其余走 String（并统一转成字符串返回，便于与 AI 的文本比对）。
/// 找不到条目时返回 Err（不静默）。
pub(crate) async fn read_current(pool: &SqlitePool, p: &PatchItem) -> Result<String, String> {
    let name = p.target_name.trim();
    let col = p.field.as_str();
    let numeric = is_numeric_field(col);

    // 每类目标一条 SQL；数值列与文本列分开处理
    let (sql_num, sql_txt): (String, String) = match (p.board.as_str(), p.target_kind.as_str()) {
        ("characters", "character") => (
            format!("SELECT COALESCE({col}, 0) FROM characters WHERE name = ?1"),
            format!("SELECT COALESCE({col}, '') FROM characters WHERE name = ?1"),
        ),
        ("characters", "relation") => {
            let (_from, _to) = split_arrow(name)?;
            (
                format!("SELECT COALESCE({col}, 0) FROM relations WHERE from_character_id = (SELECT id FROM characters WHERE name = ?1) AND to_character_id = (SELECT id FROM characters WHERE name = ?2)"),
                format!("SELECT COALESCE({col}, '') FROM relations WHERE from_character_id = (SELECT id FROM characters WHERE name = ?1) AND to_character_id = (SELECT id FROM characters WHERE name = ?2)"),
            )
        }
        ("world", _) => (
            format!("SELECT COALESCE({col}, 0) FROM world_settings WHERE key = ?1"),
            format!("SELECT COALESCE({col}, '') FROM world_settings WHERE key = ?1"),
        ),
        (b, k) => return Err(format!("不支持的板块/条目类型组合：{b} / {k}")),
    };

    // 关系类目标多一个绑定参数
    let is_relation = p.target_kind == "relation";
    let (found, text): (bool, String) = if numeric {
        let row: Option<i64> = if is_relation {
            let (from, to) = split_arrow(name)?;
            sqlx::query_scalar(&sql_num).bind(&from).bind(&to).fetch_optional(pool).await.map_err(|e| e.to_string())?
        } else {
            sqlx::query_scalar(&sql_num).bind(name).fetch_optional(pool).await.map_err(|e| e.to_string())?
        };
        match row {
            Some(v) => (true, v.to_string()),
            None => (false, String::new()),
        }
    } else {
        let row: Option<String> = if is_relation {
            let (from, to) = split_arrow(name)?;
            sqlx::query_scalar(&sql_txt).bind(&from).bind(&to).fetch_optional(pool).await.map_err(|e| e.to_string())?
        } else {
            sqlx::query_scalar(&sql_txt).bind(name).fetch_optional(pool).await.map_err(|e| e.to_string())?
        };
        match row {
            Some(v) => (true, v),
            None => (false, String::new()),
        }
    };

    if !found {
        return Err(format!("找不到条目「{name}」（{}）", p.target_kind));
    }
    Ok(text)
}

/// 执行一条补丁：按 `target_kind` 定位条目、按 `field` 更新该字段。
/// 只在 `validate_patch` 通过后才会被调用；失败返回人话原因（不 panic）。
pub(super) async fn apply_one(pool: &SqlitePool, p: &PatchItem) -> Result<String, String> {
    let name = p.target_name.trim();
    let value = p.value.clone();
    // 防伪核对：AI 给的 before 必须与库中现值一致，否则说明它基于错误理解生成补丁 —— 拒绝，不猜
    let current = read_current(pool, p).await?;
    if !p.before.trim().is_empty() && p.before.trim() != current.trim() {
        return Err(format!(
            "「{name}」的 {} 当前值是「{}」，但 AI 以为它是「{}」——为避免误改，本条已跳过",
            p.field, current.trim(), p.before.trim()
        ));
    }
    match (p.board.as_str(), p.target_kind.as_str()) {
        // 角色字段（数值列按整数写，文本列按文本写 —— 否则 SQLite 会类型不匹配）
        ("characters", "character") => {
            let col = p.field.as_str();
            let sql = format!("UPDATE characters SET {col} = ?1 WHERE name = ?2");
            let n = if is_numeric_field(col) {
                let v: i64 = value.trim().parse().map_err(|_| format!("「{value}」不是有效数字，无法写入 {col}"))?;
                sqlx::query(&sql).bind(v).bind(name).execute(pool).await.map_err(|e| e.to_string())?.rows_affected()
            } else {
                sqlx::query(&sql).bind(&value).bind(name).execute(pool).await.map_err(|e| e.to_string())?.rows_affected()
            };
            if n == 0 { return Err(format!("找不到角色「{name}」")); }
            Ok(format!("角色「{name}」的 {} 已更新", p.field))
        }
        // 关系字段：target_name 形如 "宸熙→夏婉琴"（也兼容 "宸熙 → 夏婉琴"）
        ("characters", "relation") => {
            let (from, to) = split_arrow(name)?;
            let col = p.field.as_str();
            let sql = format!(
                "UPDATE relations SET {col} = ?1 WHERE from_character_id = (SELECT id FROM characters WHERE name = ?2)
                   AND to_character_id = (SELECT id FROM characters WHERE name = ?3)"
            );
            let n = if is_numeric_field(col) {
                let v: i64 = value.trim().parse().map_err(|_| format!("「{value}」不是有效数字，无法写入 {col}"))?;
                sqlx::query(&sql).bind(v).bind(&from).bind(&to).execute(pool).await.map_err(|e| e.to_string())?.rows_affected()
            } else {
                sqlx::query(&sql).bind(&value).bind(&from).bind(&to).execute(pool).await.map_err(|e| e.to_string())?.rows_affected()
            };
            if n == 0 { return Err(format!("找不到关系「{name}」")); }
            Ok(format!("关系「{name}」的 {} 已更新", p.field))
        }
        // 世界设定：设定条目 / 势力 / 术语都按 key 更新
        ("world", _) => {
            let col = p.field.as_str();
            let sql = format!("UPDATE world_settings SET {col} = ?1 WHERE key = ?2");
            let n = sqlx::query(&sql).bind(&value).bind(name)
                .execute(pool).await.map_err(|e| e.to_string())?.rows_affected();
            if n == 0 { return Err(format!("找不到条目「{name}」")); }
            Ok(format!("条目「{name}」的 {} 已更新", p.field))
        }
        (b, k) => Err(format!("不支持的板块/条目类型组合：{b} / {k}")),
    }
}

/// 解析关系定位名（"A→B" / "A → B" / "A->B"）
pub(crate) fn split_arrow(s: &str) -> Result<(String, String), String> {
    for sep in ["→", "->", "—>", "–>"] {
        if let Some((a, b)) = s.split_once(sep) {
            let (a, b) = (a.trim().to_string(), b.trim().to_string());
            if a.is_empty() || b.is_empty() {
                return Err(format!("关系定位「{s}」格式不对，应形如「A→B」"));
            }
            return Ok((a, b));
        }
    }
    Err(format!("关系定位「{s}」缺少箭头，应形如「A→B」"))
}

/// 用给定的池执行补丁（供测试使用；生产路径走 `apply_check_patch`，它会按 story_id 打开池并打快照）
#[cfg(test)]
pub(crate) async fn apply_check_patch_in(pool: &SqlitePool, items: &[PatchItem]) -> Result<Vec<String>, String> {
    let (fixed, _) = validate_patch(pool, items).await?;
    for p in &fixed {
        let current = read_current(pool, p).await?;
        if !p.before.trim().is_empty() && !values_equal(p.field.as_str(), p.before.trim(), current.trim()) {
            return Err(format!(
                "「{}」的 {} 当前值是「{}」，但 AI 以为它是「{}」——为避免误改，本次未做任何修改（请重新检查后再试）",
                p.target_name, p.field, current.trim(), p.before.trim()
            ));
        }
    }
    let mut out = Vec::new();
    for p in &fixed {
        out.push(apply_one(pool, p).await?);
    }
    Ok(out)
}

/// 执行一组补丁（先整体校验，校验不过则一条都不执行）；返回每条的执行结果
pub async fn apply_check_patch(story_id: &str, items: &[PatchItem]) -> Result<Vec<String>, String> {
    let pool = crate::story_db::open_story_pool(story_id).await.map_err(|e| e.to_string())?;
    // ① 白名单 + 归一化（任一条不过就整体中止）
    let (fixed, _lines) = validate_patch(&pool, items).await?;
    // ② 执行前打快照（失败不阻断）
    let _ = crate::evolution::make_snapshot_for_story(story_id).await;
    let mut out = Vec::new();
    for p in &fixed {
        // ③ 逐条执行；**先把全部条目的 before 核对一遍**再动手，避免改一半失败
        let current = read_current(&pool, p).await?;
        if !p.before.trim().is_empty() && !values_equal(p.field.as_str(), p.before.trim(), current.trim()) {
            return Err(format!(
                "「{}」的 {} 当前值是「{}」，但 AI 以为它是「{}」——为避免误改，本次未做任何修改（请重新检查后再试）",
                p.target_name, p.field, current.trim(), p.before.trim()
            ));
        }
    }
    for p in &fixed {
        let msg = apply_one(&pool, p).await?;
        out.push(msg);
    }
    Ok(out)
}
