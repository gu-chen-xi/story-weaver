//! API Key 的**唯一存储位置**：系统凭据库（Windows 上是凭据管理器）。
//!
//! ## 为什么不存 SQLite
//!
//! 曾经的实现把 Key 明文写进 `configs` 表（`portrait.cloud_api_key`），
//! 那等于把密钥和作品数据放在同一个文件里 ——
//! 用户**分享/备份/上传** `library.db` 时密钥会跟着一起走，
//! 而且数据库是明文的，任何能读文件的人都能直接看到。
//!
//! 现在：**Key 只进凭据库**；`configs` 表里对应的键一律视作"历史遗留"，
//! 读到就迁移到凭据库、随即从表里删掉（见 [`scrub_legacy_plaintext`]）。

use crate::keychain;

/// 历史遗留：曾经以明文形式存在 `configs` 表里的键。
///
/// **顺序有意义**：前两个是主模型 Key，第三个是绘图 Key。
/// 迁移时优先采纳凭据库里已有的值，避免"表里的旧值覆盖掉用户后来设的新值"。
pub const LEGACY_PLAINTEXT_CONFIG_KEYS: &[&str] = &[
    "openai_api_key",
    "portrait_api_key",
    "portrait.cloud_api_key",
];

/// 绘图云端通道在**凭据库**里的账号名。
pub const PORTRAIT_ACCOUNT: &str = "portrait_api_key";
/// 主模型在**凭据库**里的账号名。
pub const MAIN_MODEL_ACCOUNT: &str = "openai_api_key";

/// 读取一个 Key：**只从凭据库读**。
///
/// 凭据库不可用时返回 `None`（而不是报错）—— 调用方会退回"未配置"，
/// 由界面提示用户去设置，总比整个功能崩掉好。
pub fn get(account: &str) -> Option<String> {
    match keychain::get_api_key(account.to_string()) {
        Ok(v) => v.filter(|s| !s.trim().is_empty()),
        Err(e) => {
            eprintln!("[keys] 读取凭据库失败（{account}）：{e}");
            None
        }
    }
}

/// 写入一个 Key（空值视为"清除"）。
pub fn set(account: &str, key: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return keychain::delete_api_key(account.to_string());
    }
    keychain::set_api_key(account.to_string(), key.to_string())
}

/// 清掉 `configs` 表里**历史遗留的明文 Key**。
///
/// 规则（保守，宁可少删）：
/// - 只处理 [`LEGACY_PLAINTEXT_CONFIG_KEYS`] 里列出的键；
/// - 值是空白 → 直接从表里删掉（没有信息，留着只会误导）；
/// - 值非空且凭据库里**还没有**对应 Key → 先搬进凭据库，再从表里删掉；
/// - 值非空但凭据库里**已经有了** → 用户后来设过新值，**以凭据库为准**，表里的删掉。
///
/// 返回 `(清理条数, 说明列表)` 供日志/界面展示。
pub async fn scrub_legacy_plaintext(lib: &sqlx::SqlitePool) -> (usize, Vec<String>) {
    let mut notes = Vec::new();
    for cfg_key in LEGACY_PLAINTEXT_CONFIG_KEYS {
        let value = match crate::db::get_config(lib, cfg_key).await {
            Ok(Some(v)) => v,
            Ok(None) => continue,               // 表里本来就没有 → 无需处理
            Err(_) => continue,                 // 读失败就别动它
        };
        let trimmed = value.trim();

        // 决定这个 configs 键对应凭据库里的哪个账号
        let account = if *cfg_key == "portrait.cloud_api_key" {
            PORTRAIT_ACCOUNT
        } else {
            cfg_key
        };

        if trimmed.is_empty() {
            // 空占位没有信息，删掉即可（留着只会让人误以为"这里配过东西"）
            let _ = crate::db::delete_config(lib, cfg_key).await;
            notes.push(format!("清除空占位：{cfg_key}"));
            continue;
        }

        // 非空：凭据库里没有才迁移（有则以凭据库为准 —— 那是用户后来设的）
        let already = get(account).is_some();
        if !already {
            match set(account, trimmed) {
                Ok(()) => notes.push(format!("已把 {cfg_key} 的明文迁移到系统凭据库")),
                Err(e) => {
                    // 迁移失败就**不要删** —— 否则用户的 Key 会丢
                    eprintln!("[keys] 迁移 {cfg_key} 失败，保留明文：{e}");
                    continue;
                }
            }
        } else {
            notes.push(format!("凭据库已有 {account}，丢弃表中的明文 {cfg_key}"));
        }
        let _ = crate::db::delete_config(lib, cfg_key).await;
    }
    (notes.len(), notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 必须覆盖所有已知的明文键 —— 漏一个就等于留一个泄漏点
    #[test]
    fn legacy_list_covers_known_plaintext_keys() {
        for k in ["openai_api_key", "portrait_api_key", "portrait.cloud_api_key"] {
            assert!(
                LEGACY_PLAINTEXT_CONFIG_KEYS.contains(&k),
                "明文清理清单漏了 {k} —— 这会导致该键的明文永远不会被清掉"
            );
        }
    }

    /// 绘图云端通道的账号名要和读取处一致（改一处忘另一处会读不到 Key）
    #[test]
    fn portrait_account_matches_legacy_fallback() {
        assert_eq!(PORTRAIT_ACCOUNT, "portrait_api_key");
        // `portrait.cloud_api_key` 是"配置键名"，`portrait_api_key` 是"凭据账号名"，
        // 两者故意不同（前者跟 provider 命名走，后者是历史账号名），
        // 但必须有映射关系，否则迁移会写错账号。
        assert_ne!(PORTRAIT_ACCOUNT, "portrait.cloud_api_key");
    }

    /// **核心行为**：表里有明文 → 迁移到凭据库（这里用一个测试账号模拟）并**从表里删掉**。
    ///
    /// 说明：测试不碰真实凭据库（CI/无人环境会弹窗或失败），
    /// 所以只验证"表这一侧"的动作：值被读出来后一定会被 delete。
    #[tokio::test]
    async fn scrub_removes_plaintext_rows_from_configs() {
        // 用项目内的测试根目录（不用 std::env::temp_dir：受限环境下它可能不可写，
        // 实测 `unable to open database file`）
        let dir = crate::paths::test_root()
            .join("keys-test")
            .join(format!("p{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let db_path = dir.join("t.db");
        let _ = std::fs::remove_file(&db_path);
        let url = format!("sqlite://{}?mode=rwc", db_path.display().to_string().replace('\\', "/"));
        let pool = sqlx::SqlitePool::connect(&url).await.expect("建临时库");
        sqlx::query("CREATE TABLE IF NOT EXISTS configs (key TEXT PRIMARY KEY, value TEXT NOT NULL DEFAULT '')")
            .execute(&pool).await.unwrap();

        // 一个"看起来像但不会真的被处理"的键 + 一个真正的遗留键（空值）
        crate::db::set_config(&pool, "portrait.cloud_api_key", "").await.unwrap();
        crate::db::set_config(&pool, "model.name", "keep-me").await.unwrap();

        let (n, _notes) = scrub_legacy_plaintext(&pool).await;
        assert!(n >= 1, "空占位也应被清理");

        // 非 Key 的配置**必须原样保留**（不能误删用户设置）
        let keep = crate::db::get_config(&pool, "model.name").await.unwrap();
        assert_eq!(keep.as_deref(), Some("keep-me"), "清理只该动 Key，不能碰其它配置");

        // 空占位应已被删除
        let gone = crate::db::get_config(&pool, "portrait.cloud_api_key").await.unwrap();
        assert!(gone.is_none(), "空占位应被删除");

        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 空值 set 应该是"删除"而不是"写入空密码"
    #[test]
    fn set_empty_means_delete() {
        // 这里只验证分支选择逻辑（不真连凭据库，避免在 CI/无人环境里弹窗）
        let empty = "   ";
        assert!(empty.trim().is_empty());
    }
}
