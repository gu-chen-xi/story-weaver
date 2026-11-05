//! 安全配置的结构守卫（2026-11-05）。
//!
//! 和 `frontend_guard.rs` 一个路子：把**已经关闭的风险**钉成测试，防止哪天顺手加回来。
//! 现在守两条：
//!   ① `tauri.conf.json` 里不许再有 `assetProtocol`（立绘已改 `portrait://`）；
//!   ② `Cargo.toml` 的 tauri 依赖不许带 `protocol-asset` feature（带了 build script 会报错，
//!      但那时报的是"allowlist 不匹配"，看不出是安全问题 —— 这里直接说清楚）。

#[cfg(test)]
mod tests {
    const CONF: &str = include_str!("../tauri.conf.json");
    const CARGO: &str = include_str!("../Cargo.toml");

    #[test]
    fn asset_protocol_stays_removed() {
        assert!(
            !CONF.contains("\"assetProtocol\""),
            "assetProtocol 已经下线（立绘走 portrait://），不要再加回来：scope=[\"**\"] 等于放开整个磁盘"
        );
        assert!(
            !CONF.contains("asset: http://asset.localhost"),
            "CSP 里不该再出现 asset: —— img-src / media-src 都该是 portrait:"
        );
    }

    #[test]
    fn portrait_protocol_is_wired() {
        assert!(CONF.contains("portrait: http://portrait.localhost"), "CSP 必须放行 portrait 协议");
        assert!(CARGO.contains("protocol-asset") == false, "tauri 不该再带 protocol-asset feature");
        let lib = include_str!("lib.rs");
        assert!(lib.contains("register_uri_scheme_protocol(\"portrait\""), "协议注册不能丢");
    }
}
