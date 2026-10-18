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
    println!("cargo:rerun-if-changed=build.rs");
}
