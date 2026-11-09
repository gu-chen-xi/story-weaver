//! SQLite「主库 + `-wal` 文件对」的运行期防线（RUSTSEC-2026-0459）。
//!
//! 为什么需要它：RustSec 2026-11-09 披露的 CVE-2026-119375 里，SQLite < **3.54.0**
//! 打开「主库 + 页大小与主库不一致的 `-wal`」这对文件时，会把**页缓存里的未初始化内存**
//! 当成数据返回（信息泄露，CVSS 6.8，本地攻击、需要受害者打开这对文件）。
//!
//! 我们能做的很有限 —— `libsqlite3-sys` 是由 **sqlx 0.9 固定的**（`libsqlite3-sys = 0.30`，
//! 自带 SQLite **3.46.0**），修复版 `0.39.x` 需要 sqlx 换代，**升不动**。所以：
//!   ① CI 里先 `ignore` 这条 advisory（理由写清、并注明 sqlx 升级后必须删，见 `deny.toml`）；
//!   ② 这里加一道**打开库之前的检查**：`-wal` 的页大小与主库不一致时拒绝打开并说清原因 ——
//!      正好卡住这个 CVE 的触发形态，而且纯读文件头、零额外依赖。
//!
//! 边界：这不是"修好了 SQLite"，只是把"明显畸形/被篡改的文件对"挡在门外。
//! 真正的修复要等 sqlx 把 `libsqlite3-sys` 升到 ≥ 0.39.0。

use std::path::Path;

/// 主库文件头里的页大小（偏移 16，big-endian u16；值 1 表示 65536）。
/// 文件不存在 / 读不到 / 太短 → `None`（交给 sqlx 自己报错，这里不抢它的活）。
pub fn db_page_size(db: &Path) -> Option<u32> {
    let head = read_head(db, 100)?;
    let raw = u16::from_be_bytes([head[16], head[17]]) as u32;
    Some(if raw == 1 { 65536 } else { raw })
}

/// `-wal` 文件头里的页大小（偏移 8，big-endian u32）。
/// 没有 wal / 读不到 / 短于 32 字节 → `None`（SQLite 会把过短的 wal 当空处理，不是本 CVE 的形态）。
pub fn wal_page_size(wal: &Path) -> Option<u32> {
    let head = read_head(wal, 32)?;
    Some(u32::from_be_bytes([head[8], head[9], head[10], head[11]]))
}

fn read_head(p: &Path, need: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).ok()?;
    let mut buf = vec![0u8; need];
    let mut got = 0usize;
    while got < need {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) => return None,
        }
    }
    if got < need {
        return None;
    }
    Some(buf)
}

/// 打开库之前的检查：`-wal` 存在且页大小与主库不一致 → `Err`（附人话说明）。
///
/// 只有"两边都读得出来"时才比较：读不出来说明文件本身就有问题，让 sqlx 去报它自己的错。
pub fn check_wal_matches_db(db: &Path) -> Result<(), String> {
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    let wal = std::path::PathBuf::from(wal);
    if !wal.exists() {
        return Ok(());
    }
    let (Some(db_ps), Some(wal_ps)) = (db_page_size(db), wal_page_size(&wal)) else {
        return Ok(());
    };
    if db_ps == wal_ps {
        return Ok(());
    }
    Err(format!(
        "拒绝打开：{} 旁边有一个页大小不一致的 -wal（主库 {} / 日志 {}）。\n\
         这种「主库 + 异页大小 -wal」的文件对是已知的信息泄露攻击形态（CVE-2026-119375），\
         内置的 SQLite 3.46.0 会中招。\n\
         处理：先备份整个目录，再删掉那个 -wal（和同名的 -shm）后重试；\
         若这些文件不是你自己产生的，说明目录被别的程序写过。",
        db.display(),
        db_ps,
        wal_ps
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(tag: &str) -> PathBuf {
        // 不用 %TEMP%：这台机器的临时目录对测试进程不总是可写（同 portrait_protocol 的处理）
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../.tmp/db-guard-tests")
            .join(format!("{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_db(p: &Path, page_size: u16) {
        let mut b = vec![0u8; 100];
        b[0..16].copy_from_slice(b"SQLite format 3\0");
        b[16..18].copy_from_slice(&page_size.to_be_bytes());
        std::fs::write(p, b).unwrap();
    }

    fn write_wal(p: &Path, page_size: u32) {
        let mut b = vec![0u8; 32];
        b[0..4].copy_from_slice(&0x377f_0682u32.to_be_bytes());
        b[8..12].copy_from_slice(&page_size.to_be_bytes());
        std::fs::write(p, b).unwrap();
    }

    #[test]
    fn matching_wal_is_fine() {
        let d = tmp("ok");
        let db = d.join("a.db");
        write_db(&db, 4096);
        write_wal(&d.join("a.db-wal"), 4096);
        assert!(check_wal_matches_db(&db).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn mismatched_wal_is_refused_with_reason() {
        let d = tmp("bad");
        let db = d.join("b.db");
        write_db(&db, 4096);
        write_wal(&d.join("b.db-wal"), 8192);
        let e = check_wal_matches_db(&db).unwrap_err();
        assert!(e.contains("页大小不一致"), "错误信息要点明原因：{e}");
        assert!(e.contains("CVE-2026-119375"), "错误信息要留下可查的编号：{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn no_wal_or_short_wal_is_not_blocked() {
        let d = tmp("none");
        let db = d.join("c.db");
        write_db(&db, 4096);
        assert!(check_wal_matches_db(&db).is_ok(), "没有 wal 不该拦");
        std::fs::write(d.join("c.db-wal"), b"short").unwrap();
        assert!(check_wal_matches_db(&db).is_ok(), "过短的 wal 交给 SQLite 自己处理");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn page_size_65536_is_normalized() {
        let d = tmp("big");
        let db = d.join("e.db");
        write_db(&db, 1); // 规范里 1 = 65536
        assert_eq!(db_page_size(&db), Some(65536));
        let _ = std::fs::remove_dir_all(&d);
    }
}
