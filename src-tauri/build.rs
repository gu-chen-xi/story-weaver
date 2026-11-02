fn main() {
    tauri_build::build();

    // 把**构建时间**编进二进制：界面上显示，用于回答"我现在跑的是哪一版"。
    // —— 起因（2026-10-18）：用户以为在用新构建，其实窗口还是旧的（start.cmd 复制失败/没重启），
    //    排查只能看文件时间戳，很不直观。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=DW_BUILD_TS={secs}");
    // **不要**写 cargo:rerun-if-changed=build.rs（2026-11-02 用户截图暴露）：
    // 一旦写了这条，构建脚本就**只在 build.rs 自己变化时**才重跑 → DW_BUILD_TS 停在
    // 上次改 build.rs 的那一刻，界面上的"构建时间"永远是旧值（截图里显示 11-01 22:58）。
    // 什么都不写才是想要的行为：包里任何文件变了都会重跑，时间戳跟着更新。
}
