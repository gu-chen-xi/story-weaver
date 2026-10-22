// 从 `check.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `check::tests` —— 因为 `check.rs` + `check/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `check`，一行都不用改写。

use super::*;

#[test]
fn relation_locator_parsing() {
    tauri::async_runtime::block_on(async {
        assert_eq!(super::split_arrow("宸熙→夏婉琴").unwrap(), ("宸熙".into(), "夏婉琴".into()));
        assert_eq!(super::split_arrow("宸熙 -> 夏婉琴").unwrap(), ("宸熙".into(), "夏婉琴".into()));
        assert!(super::split_arrow("宸熙夏婉琴").is_err(), "没有箭头应报错");
        assert!(super::split_arrow("→夏婉琴").is_err(), "缺左侧应报错");

    });
}

#[test]
fn world_board_rejects_relation_like_patches() {
    tauri::async_runtime::block_on(async {
        let pool = mem_pool().await;
        // 模型把"角色关系"标成世界板块条目 → 必须被拒绝并提示去角色管理检查
        let bad = vec![PatchItem {
            board: "world".into(), target_kind: "world_entry".into(), target_name: "宸熙→院长".into(),
            field: "value".into(), before: "80".into(), value: "10".into(),
        }];
        // 现在不再拒绝，而是**自动改判成角色板块的关系补丁**（value 语义 = 关系数值）
        let (fixed, _) = validate_patch(&pool, &bad).await.expect("应自动改判板块而不是拒绝");
        assert_eq!(fixed[0].board, "characters", "应改判为 characters：{:?}", fixed[0]);
        assert_eq!(fixed[0].target_kind, "relation", "应改判为 relation");
        assert_eq!(fixed[0].field, "strength", "泛化字段应归一为 strength");
        // 正常的世界条目仍然通过
        let ok = vec![PatchItem {
            board: "world".into(), target_kind: "world_entry".into(), target_name: "修女宿舍".into(),
            field: "value".into(), before: "旧".into(), value: "新".into(),
        }];
        let pool = mem_pool().await;
        assert!(validate_patch(&pool, &ok).await.is_ok());

    });
}

#[test]
fn apply_patch_end_to_end_on_temp_story() {
    tauri::async_runtime::block_on(async {
        // 造一个真实临时 story.db（与其它模块一致：相对 current_dir 的 ../library/...）
        let tag = "999001";
        let dir = crate::paths::test_root().join(tag).join("projects").join(tag);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("story.db");
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(&path).create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE characters (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, profile TEXT DEFAULT '', age TEXT DEFAULT '')").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE relations (id INTEGER PRIMARY KEY AUTOINCREMENT, from_character_id INTEGER, to_character_id INTEGER, relation_type TEXT DEFAULT '', direction TEXT DEFAULT '', strength INTEGER DEFAULT 0, state TEXT DEFAULT '', description TEXT DEFAULT '')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO characters(name, profile) VALUES('甲','小传甲')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO characters(name, profile) VALUES('乙','小传乙')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO relations(from_character_id, to_character_id, relation_type, direction, strength, state) VALUES(1,2,'陌生人','双向',80,'稳定')").execute(&pool).await.unwrap();

        // ① 读数值列：strength=80 必须按整数读出（旧实现按文本读会报 mismatched types）
        let probe = PatchItem {
            board: "characters".into(), target_kind: "relation".into(), target_name: "甲→乙".into(),
            field: "strength".into(), before: "80".into(), value: "10".into(),
        };
        let (fixed, _) = validate_patch(&pool, &[probe.clone()]).await.expect("应通过校验");
        assert_eq!(fixed[0].board, "characters");
        let cur = read_current(&pool, &fixed[0]).await.expect("应能读出关系数值");
        assert_eq!(cur, "80", "数值列应按整数读出并转成文本");

        // ② 校验 + 执行（走完整的 apply_check_patch 路径）
        let done = apply_check_patch_in(&pool, &[probe.clone()]).await.expect("应执行成功");
        assert_eq!(done.len(), 1, "应有一条执行结果：{done:?}");
        let after: i64 = sqlx::query_scalar("SELECT strength FROM relations WHERE id = 1").fetch_one(&pool).await.unwrap();
        assert_eq!(after, 10, "数值应被改成 10（整数写入）");

        // ③ before 与库中现值不符 → 整批不执行
        let stale = PatchItem { before: "80".into(), ..probe.clone() };
        let err = apply_check_patch_in(&pool, &[stale]).await.unwrap_err();
        assert!(err.contains("当前值是"), "应提示现值不符：{err}");
        let still: i64 = sqlx::query_scalar("SELECT strength FROM relations WHERE id = 1").fetch_one(&pool).await.unwrap();
        assert_eq!(still, 10, "校验失败时不应有任何改动");

        // ④ 文本列（角色小传）写入 + 中文别名归一
        let prof = PatchItem {
            board: "characters".into(), target_kind: "character".into(), target_name: "甲".into(),
            field: "人物小传".into(), before: "小传甲".into(), value: "改后的小传".into(),
        };
        let (f2, _) = validate_patch(&pool, &[prof.clone()]).await.expect("中文别名应归一并通过");
        assert_eq!(f2[0].field, "profile", "中文别名应归一为 profile");
        apply_check_patch_in(&pool, &[prof]).await.expect("应执行成功");
        let got: String = sqlx::query_scalar("SELECT profile FROM characters WHERE id = 1").fetch_one(&pool).await.unwrap();
        assert_eq!(got, "改后的小传");

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn numeric_columns_detected_and_compared() {
    tauri::async_runtime::block_on(async {
        assert!(is_numeric_field("strength"));
        assert!(!is_numeric_field("profile"));
        assert!(!is_numeric_field("state"));
        // 数值列：80 与 "80" / 80.0 视为相同
        assert!(values_equal("strength", "80", "80"));
        assert!(values_equal("strength", "80.0", "80"));
        assert!(!values_equal("strength", "83", "80"));
        // 文本列：忽略首尾空白
        assert!(values_equal("profile", " 小传 ", "小传"));
        assert!(!values_equal("profile", "小传A", "小传B"));

    });
}

#[test]
fn field_aliases_normalize_to_real_columns() {
    tauri::async_runtime::block_on(async {
        // 模型把关系数值写成 value / 内容 / 数值 → 都应归一成 strength
        // 关系语境：泛化词 value / 内容 / 数值 → strength
        assert_eq!(normalize_field("characters", "relation", "宸熙→院长", "value"), Some("strength"));
        assert_eq!(normalize_field("characters", "relation", "宸熙→院长", "内容"), Some("strength"));
        assert_eq!(normalize_field("characters", "relation", "宸熙→院长", "数值"), Some("strength"));
        assert_eq!(normalize_field("characters", "relation", "宸熙→院长", "强度"), Some("strength"));
        assert_eq!(normalize_field("characters", "relation", "宸熙→院长", "关系数值"), Some("strength"));
        // 角色语境：泛化词 → profile；中文别名映射到列名
        assert_eq!(normalize_field("characters", "character", "夏婉琴", "内容"), Some("profile"));
        assert_eq!(normalize_field("characters", "character", "夏婉琴", "人物小传"), Some("profile"));
        assert_eq!(normalize_field("characters", "character", "夏婉琴", "说话方式"), Some("speech_style"));
        // 世界语境：内容 → value
        assert_eq!(normalize_field("world", "world_entry", "修女宿舍", "内容"), Some("value"));
        // 内部字段永远归一不了
        assert_eq!(normalize_field("characters", "character", "夏婉琴", "id"), None);
        assert_eq!(normalize_field("characters", "character", "夏婉琴", "created_at"), None);

    });
}

#[test]
fn wrong_field_is_rejected_with_readable_reason() {
    tauri::async_runtime::block_on(async {
        let pool = mem_pool().await;
        // 模型把关系数值写成世界字段 scale —— 必须被拒绝，且原因里说清该板块允许什么
        let bad = vec![PatchItem {
            board: "characters".into(), target_kind: "relation".into(), target_name: "宸熙→院长".into(),
            field: "scale".into(), before: "80".into(), value: "10".into(),
        }];
        let err = validate_patch(&pool, &bad).await.unwrap_err();
        assert!(err.contains("不在"), "错误信息应说明字段不被允许：{err}");
        assert!(err.contains("strength"), "错误信息应列出该板块可用字段：{err}");
        // 世界板块不允许改 key（名称）
        assert!(!field_allowed("world", "key"));
        assert!(field_allowed("world", "value"));

    });
}

#[test]
fn direction_is_not_ai_editable() {
    tauri::async_runtime::block_on(async {
        assert!(!field_allowed("characters", "direction"), "关系方向不应交给 AI 改");
        assert!(field_allowed("characters", "strength"));

    });
}

#[test]
fn patch_must_carry_non_empty_value() {
    tauri::async_runtime::block_on(async {
        let empty = vec![PatchItem {
            board: "characters".into(), target_kind: "relation".into(), target_name: "宸熙→院长".into(),
            field: "strength".into(), before: "80".into(), value: "  ".into(),
        }];
        let pool = mem_pool().await;
        assert!(validate_patch(&pool, &empty).await.is_err(), "空值必须被拒绝");

    });
}

/// 单测用的空内存库（拿不到真实 story 库时，resolve_target 会查不到名字 → 保留原板块，
/// 因此纯逻辑类单测仍然有效）
async fn mem_pool() -> SqlitePool {
    let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(":memory:");
    let pool = SqlitePool::connect_with(opts).await.unwrap();
    sqlx::query("CREATE TABLE characters (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, profile TEXT DEFAULT '')").execute(&pool).await.unwrap();
    sqlx::query("CREATE TABLE relations (id INTEGER PRIMARY KEY AUTOINCREMENT, from_character_id INTEGER, to_character_id INTEGER, relation_type TEXT DEFAULT '', direction TEXT DEFAULT '', strength INTEGER DEFAULT 0, state TEXT DEFAULT '', description TEXT DEFAULT '')").execute(&pool).await.unwrap();
    sqlx::query("CREATE TABLE world_settings (id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT, value TEXT DEFAULT '', category TEXT DEFAULT '')").execute(&pool).await.unwrap();
    pool
}

#[test]
fn field_is_resolved_from_current_value() {
    tauri::async_runtime::block_on(async {
        let dir = crate::paths::test_root().join("resfield").join("projects").join("resfield");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(dir.join("story.db")).create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE characters (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, alias TEXT DEFAULT '', role TEXT DEFAULT '', gender TEXT DEFAULT '', age TEXT DEFAULT '', one_liner TEXT DEFAULT '', profile TEXT DEFAULT '', speech_style TEXT DEFAULT '', traits TEXT DEFAULT '')").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE relations (id INTEGER PRIMARY KEY AUTOINCREMENT, from_character_id INTEGER, to_character_id INTEGER, relation_type TEXT DEFAULT '', direction TEXT DEFAULT '', strength INTEGER DEFAULT 0, state TEXT DEFAULT '', description TEXT DEFAULT '')").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE world_settings (id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT, value TEXT DEFAULT '', category TEXT DEFAULT '')").execute(&pool).await.unwrap();
        // 一句话身份里有内容，人物小传是空的（正是线上遇到的场景）
        sqlx::query("INSERT INTO characters(name, one_liner, profile) VALUES('夏婉琴','女主，身材瘦小纤细，身高154cm','')").execute(&pool).await.unwrap();

        // AI 说"我要改 profile"，但它给的当前值其实来自 one_liner → 应被改判到 one_liner
        let real = resolve_field(&pool, "characters", "character", "夏婉琴", "女主，身材瘦小纤细，身高154cm").await;
        assert_eq!(real.as_deref(), Some("one_liner"), "应按当前值反查到 one_liner");

        // 走完整校验：字段会被改判，且不再因 before 对不上而失败
        let patch = vec![PatchItem {
            board: "characters".into(), target_kind: "character".into(), target_name: "夏婉琴".into(),
            field: "profile".into(), before: "女主，身材瘦小纤细，身高154cm".into(),
            value: "修女，身材瘦小纤细，身高154cm".into(),
        }];
        let (fixed, _) = validate_patch(&pool, &patch).await.expect("应改判字段后通过");
        assert_eq!(fixed[0].field, "one_liner", "字段应被改判为 one_liner：{:?}", fixed[0]);

        // 执行后：one_liner 被改，profile 仍为空（不会写错字段）
        apply_check_patch_in(&pool, &patch).await.expect("应执行成功");
        let (ol, pf): (String, String) = sqlx::query_as("SELECT one_liner, profile FROM characters WHERE name='夏婉琴'").fetch_one(&pool).await.unwrap();
        assert!(ol.starts_with("修女"), "一句话身份应被更新：{ol}");
        assert_eq!(pf, "", "人物小传不应被误写");

        // before 与任何字段都对不上 → 不改判（随后 before 核对会给出人话拒绝）
        let none = resolve_field(&pool, "characters", "character", "夏婉琴", "完全不存在的文字").await;
        assert_eq!(none, None);

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn character_mislabeled_as_world_entry_is_corrected() {
    tauri::async_runtime::block_on(async {
        // 必须用**文件**临时库：SqlitePool 的每个连接是独立的内存库，`:memory:` 会导致建表与查询不在同一个库
        let dir = crate::paths::test_root().join("resolve").join("projects").join("resolve");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let opts = sqlx::sqlite::SqliteConnectOptions::new().filename(dir.join("story.db")).create_if_missing(true);
        let pool = SqlitePool::connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE characters (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, profile TEXT DEFAULT '')").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE relations (id INTEGER PRIMARY KEY AUTOINCREMENT, from_character_id INTEGER, to_character_id INTEGER, relation_type TEXT DEFAULT '', direction TEXT DEFAULT '', strength INTEGER DEFAULT 0, state TEXT DEFAULT '', description TEXT DEFAULT '')").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE world_settings (id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT, value TEXT DEFAULT '', category TEXT DEFAULT '')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO characters(name, profile) VALUES('宸熙','旧小传')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO world_settings(key, value, category) VALUES('修女宿舍','地点','地理')").execute(&pool).await.unwrap();

        // 模型把角色「宸熙」标成了世界条目，字段写作泛化词「内容」
        let bad = vec![PatchItem {
            board: "world".into(), target_kind: "world_entry".into(), target_name: "宸熙".into(),
            field: "内容".into(), before: "旧小传".into(), value: "新小传".into(),
        }];
        let (fixed, _) = validate_patch(&pool, &bad).await.expect("应按库反查改判为角色补丁");
        assert_eq!(fixed[0].board, "characters", "应按库改判为 characters：{:?}", fixed[0]);
        assert_eq!(fixed[0].target_kind, "character", "应改判为 character");
        assert_eq!(fixed[0].field, "profile", "泛化词「内容」在角色语境应落地为 profile");

        // 执行后小传确实被改了（证明改判让写入落到了正确的表）
        let done = apply_check_patch_in(&pool, &bad).await.expect("应执行成功");
        assert_eq!(done.len(), 1);
        let got: String = sqlx::query_scalar("SELECT profile FROM characters WHERE name = '宸熙'").fetch_one(&pool).await.unwrap();
        assert_eq!(got, "新小传");

        // 真正的世界条目仍然走世界板块
        let good = vec![PatchItem {
            board: "world".into(), target_kind: "world_entry".into(), target_name: "修女宿舍".into(),
            field: "value".into(), before: "地点".into(), value: "地点（宿舍区）".into(),
        }];
        let (f2, _) = validate_patch(&pool, &good).await.expect("世界条目应通过");
        assert_eq!(f2[0].board, "world");
        assert_eq!(f2[0].field, "value");
        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn strength_validation_catches_bad_values() {
    tauri::async_runtime::block_on(async {
        assert_eq!(strength_problem("83"), None);
        assert_eq!(strength_problem("0"), None);
        assert_eq!(strength_problem("100"), None);
        assert!(strength_problem("120").unwrap().contains("超出"));
        assert!(strength_problem("abc").unwrap().contains("不是数字"));
        assert!(strength_problem("").unwrap().contains("为空"));

    });
}

#[test]
fn patch_whitelist_blocks_internal_fields() {
    tauri::async_runtime::block_on(async {
        assert!(field_allowed("characters", "profile"));
        assert!(field_allowed("characters", "strength"));
        assert!(field_allowed("world", "value"));
        // 内部字段一律禁止
        assert!(!field_allowed("characters", "id"));
        assert!(!field_allowed("characters", "created_at"));
        assert!(!field_allowed("world", "meta_json"));
        // 未知板块禁止
        assert!(!field_allowed("unknown", "value"));

    });
}

#[test]
fn validate_patch_rejects_unsafe_items() {
    tauri::async_runtime::block_on(async {
        let ok = vec![PatchItem {
            board: "characters".into(), target_kind: "character".into(), target_name: "夏婉琴".into(),
            field: "profile".into(), before: String::new(), value: "改好的文本".into(),
        }];
        let pool = mem_pool().await;
        assert!(validate_patch(&pool, &ok).await.is_ok());

        let bad_field = vec![PatchItem {
            board: "characters".into(), target_kind: "character".into(), target_name: "夏婉琴".into(),
            field: "id".into(), before: String::new(), value: "9".into(),
        }];
        assert!(validate_patch(&pool, &bad_field).await.is_err(), "改 id 必须被拒绝");

        // "认不出的类型"现在会被**归一化**（按板块 + 字段推断），不再直接拒绝整批
        let odd_kind = vec![PatchItem {
            board: "characters".into(), target_kind: "whatever".into(), target_name: "夏婉琴".into(),
            field: "profile".into(), before: String::new(), value: "x".into(),
        }];
        let (fixed, _) = validate_patch(&pool, &odd_kind).await.expect("类型应被归一化而不是拒绝");
        assert_eq!(fixed[0].target_kind, "character", "应归一化为 character");

        // 未知板块：字段必然不在白名单，仍应被拒绝
        let bad_board = vec![PatchItem {
            board: "unknown_board".into(), target_kind: "character".into(), target_name: "夏婉琴".into(),
            field: "profile".into(), before: String::new(), value: "x".into(),
        }];
        assert!(validate_patch(&pool, &bad_board).await.is_err(), "未知板块应被拒绝");

    });
}
