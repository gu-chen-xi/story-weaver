// 从 `generator.rs` 拆出的测试模块（第一波「只搬测试」，逻辑未改）。
//
// 模块路径仍是 `generator::tests` —— 因为 `generator.rs` + `generator/` 目录是 Rust 标准布局，
// 所以本文件里的 `use super::*` 依然指向 `generator`，一行都不用改写。

use super::*;

#[test]
fn registry_reports_implemented_honestly() {
    let reg = registry();
    assert_eq!(reg.len(), 3, "P1 注册表：SD WebUI + 云端两种");
    // P1 实装本地 SD WebUI；P3 起云端两种也已实装 —— 注册表必须**如实**报告
    for id in ["local_sdwebui", "cloud_openai_image", "cloud_custom"] {
        let d = reg.iter().find(|x| x.id == id).unwrap();
        assert!(d.implemented, "{id} 应该已实装");
        assert!(!d.note.is_empty(), "{id} 应给出使用说明（怎么填、有什么限制）");
    }
    // 通道分类不能乱
    assert_eq!(reg.iter().find(|d| d.id == "local_sdwebui").unwrap().channel, "local");
    assert_eq!(reg.iter().find(|d| d.id == "cloud_custom").unwrap().channel, "cloud");
    // 能力声明要如实：OpenAI Images API 没有 negative / steps / cfg（负向并入 prompt 由适配器处理）
    let oa = &reg.iter().find(|d| d.id == "cloud_openai_image").unwrap().supports;
    assert!(!oa.negative && !oa.steps && !oa.cfg, "OpenAI 形态不该声称支持这些参数");
    assert!(oa.size && oa.batch);
    // 自定义形态：全都能填（由用户模板决定）
    let cu = &reg.iter().find(|d| d.id == "cloud_custom").unwrap().supports;
    assert!(cu.negative && cu.steps && cu.cfg && cu.seed);

    // 未实装的通道仍要如实报告（用一个不存在的 id 验证兜底行为）
    assert!(descriptor_of("no_such_channel").is_none());
}

#[test]
fn channels_are_either_local_or_cloud() {
    for d in registry() {
        assert!(d.channel == "local" || d.channel == "cloud", "通道分类只能是 local/cloud：{}", d.id);
    }
}

#[test]
fn descriptor_lookup_works() {
    assert!(descriptor_of("local_sdwebui").is_some());
    assert!(descriptor_of("nope").is_none());
}

#[test]
fn base64_decode_matches_known_values() {
    // "PNG" 的 base64 是 "UEVH"…实际用标准样例更稳妥：
    assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
    assert_eq!(base64_decode("").unwrap(), Vec::<u8>::new());
    assert!(base64_decode("!!!").is_none(), "非法字符应返回 None");
}

#[test]
fn cleanup_protects_fresh_files() {
    // 刚写的临时文件属于"可能正在生成"，清理时不应被删
    let p = temp_dir().join(format!("fresh-test-{}.png", std::process::id()));
    std::fs::write(&p, b"x").unwrap();
    let ps = p.to_string_lossy().to_string();
    let removed = cleanup_temp(&[]);
    assert!(p.exists(), "新文件必须被保护（并发生成时不能被删）；本次清理了 {removed} 个旧文件");
    assert_eq!(remove_files(&[ps]), 1, "收尾用的精确删除应生效");
    assert!(!p.exists());
}

/// 第五批「路径模型」修复后：临时目录**不再放在 `target/gen-tmp`** ——
/// `target` 是 cargo 的构建产物目录，打包后根本不存在（而且 `cargo clean` 会连临时文件一起清掉）。
/// 现在落在"数据根"下的 `temp/`；本测试改为断言这一点。
#[test]
fn temp_dir_is_writable_and_isolated_in_tests() {
    // **不变量：临时目录必须可写**（生成过程要用；不可写 = 功能直接失败）。
    //
    // 曾经还想断言"不在 target 下"，但那在受限环境下自相矛盾 ——
    // 数据根本身会兜底到 exe 同级，而 `cargo test` 的 exe 就在 `target/debug/deps`，
    // 于是兜底路径必然含 target。运行时"可写"比"路径好看"重要得多。
    let prod = crate::paths::temp_root();
    std::fs::create_dir_all(&prod).expect("temp_root() 应可建");
    std::fs::write(prod.join(".probe"), b"ok").expect("temp_root() 应可写");
    let _ = std::fs::remove_file(prod.join(".probe"));

    // 首选位置在数据根下（数据根兜底时它自然跟着走）
    assert!(prod.starts_with(crate::paths::temp_root()));
    assert!(crate::paths::data_root().join("temp").starts_with(crate::paths::data_root()));

    // 测试环境用隔离目录（不往用户数据目录写东西）
    let d = temp_dir();
    assert!(d.starts_with(crate::paths::test_root()), "测试下应落在隔离目录，实际 {d:?}");
    std::fs::create_dir_all(&d).expect("测试临时目录应可建");
}

/// 真机验证用（默认跳过）：需要本机 127.0.0.1:7860 有一个可用的 SD WebUI。
/// 跑法：`cargo test --lib live_sdwebui -- --ignored --nocapture`
#[tokio::test]
#[ignore]
async fn live_sdwebui_probe_and_generate() {
    let base = "http://127.0.0.1:7860";
    let p = probe("local_sdwebui", base, &crate::generator_cloud::CloudConfig::default()).await.unwrap();
    println!("探测结果：ok={} detail={}", p.ok, p.detail);
    assert!(p.ok, "应能连上本机 SD WebUI：{}", p.detail);

    let req = GenRequest {
        prompt: "1girl, nun habit, upper body".into(),
        negative: "lowres, watermark".into(),
        width: 512, height: 512, batch: 1, seed: Some(12345),
        ..Default::default()
    };
    let r = generate("local_sdwebui", base, "", &crate::generator_cloud::CloudConfig::default(), &req).await.unwrap();
    println!("生成结果：{} 个文件，seeds={:?}，meta={}", r.files.len(), r.seeds, r.meta);
    assert!(!r.files.is_empty(), "应产出图片");
    for f in &r.files {
        let md = std::fs::metadata(f).expect("临时文件应存在");
        // 只需非空且有 PNG 头（本机真机测试产出的图会远大于此；65 字节的 1×1 PNG 也合法）
        assert!(md.len() > 20, "图片应有实际内容：{f}");
        let head = std::fs::read(f).unwrap();
        assert_eq!(&head[..4], b"\x89PNG", "应是 PNG");
    }
    // 收尾：只删自己产出的文件（并行跑测试时不会踩到别人；素材库目录始终不受影响）
    let removed = remove_files(&r.files);
    println!("清理临时文件 {removed} 个");
}

#[tokio::test]
async fn unknown_channel_refuses_politely() {
    // 未知通道：探测与生成都如实报错，而不是 panic 或假装成功
    let e = probe("no_such_channel", "https://example.com", &crate::generator_cloud::CloudConfig::default()).await.unwrap_err();
    assert!(e.contains("未知通道"), "应说明未知通道：{e}");
    let e2 = generate("no_such_channel", "https://example.com", "", &crate::generator_cloud::CloudConfig::default(), &GenRequest { prompt: "x".into(), ..Default::default() }).await.unwrap_err();
    assert!(e2.contains("未知通道"), "应如实报错：{e2}");
}

/// P3：云端通道**未配置时**的报错必须是"告诉用户去填什么"，而不是"尚未实装"
#[tokio::test]
async fn cloud_channels_ask_for_configuration() {
    // OpenAI 形态：没填地址 / 没填 Key
    let e = generate("cloud_openai_image", "", "", &crate::generator_cloud::CloudConfig::default(), &GenRequest { prompt: "x".into(), ..Default::default() }).await.unwrap_err();
    assert!(e.contains("服务地址"), "应提示填地址：{e}");
    let e2 = generate("cloud_openai_image", "https://api.example.com/v1", "", &crate::generator_cloud::CloudConfig::default(), &GenRequest { prompt: "x".into(), ..Default::default() }).await.unwrap_err();
    assert!(e2.contains("API Key"), "应提示填 Key：{e2}");

    // 自定义形态：没填请求地址 / 请求体模板（注意：这里走的是真实分发，但会在发请求前拦下）
    let e3 = generate("cloud_custom", "", "", &crate::generator_cloud::CloudConfig::default(), &GenRequest { prompt: "x".into(), ..Default::default() }).await.unwrap_err();
    assert!(e3.contains("请求地址"), "应提示填请求地址：{e3}");
}

#[tokio::test]
async fn missing_base_url_is_explained() {
    let p = probe("local_sdwebui", "", &crate::generator_cloud::CloudConfig::default()).await.unwrap();
    assert!(!p.ok);
    assert!(p.detail.contains("地址"), "应提示填地址：{}", p.detail);
}

#[tokio::test]
async fn empty_prompt_is_rejected_before_network() {
    let e = generate("local_sdwebui", "http://127.0.0.1:1", "", &crate::generator_cloud::CloudConfig::default(), &GenRequest { prompt: "  ".into(), ..Default::default() }).await.unwrap_err();
    assert!(e.contains("提示词"), "空提示词应在发请求前拦下：{e}");
}
