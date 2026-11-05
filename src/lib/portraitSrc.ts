// 立绘地址（2026-11-05）：`asset://` → `portrait://`。
//
// 为什么换：原来靠 `assetProtocol.scope = ["**"]`，等于整个磁盘都能通过协议读文件，
// 是 SECURITY.md §3.2 里明确记过的已知风险。现在改成专用协议，后端只认素材库目录里的文件名。
//
// 平台差异（Tauri 文档写明）：Windows / Android 上自定义协议会被 WebView2 暴露成
// `http://<scheme>.localhost/<path>`，其它平台是 `<scheme>://localhost/<path>`。

export function portraitSrc(absPath: string, ua: string = typeof navigator === "undefined" ? "" : navigator.userAgent): string {
  const name = absPath.split(/[\\/]/).pop() ?? "";
  const encoded = encodeURIComponent(name);
  return /Windows|Android/i.test(ua)
    ? `http://portrait.localhost/${encoded}`
    : `portrait://localhost/${encoded}`;
}
