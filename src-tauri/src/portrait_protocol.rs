//! `portrait://` 协议（2026-11-05）：立绘不再走 `assetProtocol`。
//!
//! 为什么换：`assetProtocol.scope = ["**"]` 意味着**整个磁盘**都能通过 `asset://` 读文件，
//! 这是 SECURITY.md §3.2 里明确记过的已知风险（当时论证过"不能外发、不能提权"，
//! 但仍然比需要的宽）。把立绘改成专用协议之后，这个协议**只认素材库目录里的文件名**。
//!
//! 边界（三条，缺一不可）：
//!   ① 只接受**文件名**，不接受路径：出现 `/` `\\` `..` `:` 一律拒绝；
//!   ② 落点固定在 `portraits_dir()` 里拼，永远出不去；
//!   ③ 读不到就 404，不回落、不猜。

use std::borrow::Cow;

/// 协议名（前端拼 URL 用；Windows 上会被 WebView2 变成 `http://portrait.localhost`）
pub const SCHEME: &str = "portrait";

/// 从 URI 路径解析出**文件名**；非法（穿越 / 绝对路径 / 空）返回 `None`。
///
/// 入参是 percent-encoded 的路径（前端用一个 `encodeURIComponent` 生成），
/// 先去掉开头的 `/` 再解码 —— 顺序反了会把 `%2F` 解成 `/` 而绕过检查。
pub fn file_from_path(uri_path: &str) -> Option<String> {
    let trimmed = uri_path.trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(trimmed).decode_utf8().ok()?;
    let name = decoded.trim();
    if name.is_empty() || name.starts_with('.') {
        return None;
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') || name.contains(':') {
        return None;
    }
    Some(name.to_string())
}

/// 按扩展名给 Content-Type（认不出来就给 `application/octet-stream`，浏览器会拒绝显示，正好）。
pub fn content_type(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// 处理一次 `portrait://localhost/<文件名>` 请求（落点 = 素材库目录）。
pub fn serve(uri_path: &str) -> tauri::http::Response<Cow<'static, [u8]>> {
    serve_in(&crate::portraits::portraits_dir(), uri_path)
}

/// [`serve`] 的注入版（测试用临时目录，不碰真实素材库）。
pub fn serve_in(root: &std::path::Path, uri_path: &str) -> tauri::http::Response<Cow<'static, [u8]>> {
    let Some(name) = file_from_path(uri_path) else {
        return tauri::http::Response::builder()
            .status(tauri::http::StatusCode::FORBIDDEN)
            .body(Cow::Borrowed(&b"bad path"[..]))
            .unwrap();
    };
    match std::fs::read(root.join(&name)) {
        Ok(bytes) => tauri::http::Response::builder()
            .status(tauri::http::StatusCode::OK)
            .header(tauri::http::header::CONTENT_TYPE, content_type(&name))
            // 立绘本机只增不改（同名不会复用），可以放心长缓存
            .header(tauri::http::header::CACHE_CONTROL, "public, max-age=86400")
            .body(Cow::Owned(bytes))
            .unwrap(),
        Err(_) => tauri::http::Response::builder()
            .status(tauri::http::StatusCode::NOT_FOUND)
            .body(Cow::Borrowed(&b"not found"[..]))
            .unwrap(),
    }
}

#[cfg(test)]
mod serve_tests {
    use super::*;

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        // 不用 %TEMP%：这台机器的临时目录对测试进程不总是可写（实测 Access Denied）。
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.tmp/portrait-tests");
        let d = base.join(format!("{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn serves_file_from_root_with_content_type() {
        let root = tmp_root("ok");
        std::fs::write(root.join("a.png"), b"PNGDATA").unwrap();
        let r = serve_in(&root, "/a.png");
        assert_eq!(r.status(), tauri::http::StatusCode::OK);
        assert_eq!(r.headers().get(tauri::http::header::CONTENT_TYPE).unwrap(), "image/png");
        assert_eq!(r.body().as_ref(), b"PNGDATA");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn traversal_and_missing_are_rejected() {
        let root = tmp_root("bad");
        std::fs::write(root.join("a.png"), b"x").unwrap();
        // 同一目录下另建一个文件，用 ../ 指向它必须被拒
        std::fs::write(root.parent().unwrap().join("outside.png"), b"secret").unwrap();
        assert_eq!(serve_in(&root, "/../outside.png").status(), tauri::http::StatusCode::FORBIDDEN);
        assert_eq!(serve_in(&root, "/nope.png").status(), tauri::http::StatusCode::NOT_FOUND);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_and_encoded_names() {
        assert_eq!(file_from_path("/a.png").as_deref(), Some("a.png"));
        assert_eq!(file_from_path("/%E5%A4%8F.png").as_deref(), Some("夏.png"));
        assert_eq!(file_from_path("/with%20space.png").as_deref(), Some("with space.png"));
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        for bad in ["/../secret.png", "/a/../b.png", "/C:%5Cwindows.png", "//etc/passwd", "/", "", "/.hidden"] {
            assert!(file_from_path(bad).is_none(), "必须拒绝：{bad}");
        }
    }

    #[test]
    fn maps_content_types() {
        assert_eq!(content_type("a.PNG"), "image/png");
        assert_eq!(content_type("a.jpeg"), "image/jpeg");
        assert_eq!(content_type("a.webp"), "image/webp");
        assert_eq!(content_type("a.exe"), "application/octet-stream");
    }
}
