//! 立绘生成通道（ROADMAP 第四批 ⑥ · P1 骨架与边界）
//!
//! **架构边界（本模块的铁律，见 docs/开发期的文档（主参考）/立绘系统设计.md）**
//!
//! ```text
//!   绘图系统（本模块，可缺席）
//!        │ 产出的图片
//!        ▼ 必须走同一条入口
//!   立绘素材库（portraits.rs，零依赖，永远可用）
//!        │ 按 character_id 绑定
//!        ▼
//!   角色管理的立绘展示
//! ```
//!
//! - **R1**：本模块**不得绕过 `portraits::portrait_import`** 直接写文件或插表（版本/宽高/绑定/分组才不会乱）
//! - **R2**：`portraits.rs` **不得** import 本模块（没有绘图服务时素材库照样工作）
//! - **R3**：本模块失败/未配置**不得**阻塞素材库与角色管理（前端只显示友好提示）
//!
//! 验收标准：把本模块整块删掉（去掉 `mod generator;` 与插件注册表那条），`cargo test` / `tsc` 全过，
//! 且素材库的导入 / 绑定 / 分组 / 标签 / 版本 / 导出 / 角色卡展示**全部照常**。
//!
//! P1 范围：通道注册表（如实报告可用性）+ 连接测试 + 生成入口（结果落临时目录）。
//! 图片入库由前端串既有命令 `portrait_import` 完成 —— 这样 R1 在**结构上**就无法违反。

use serde::{Deserialize, Serialize};

/// 通道能力（**能力即数据**：前端据此决定显示哪些参数控件）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GenSupports {
    pub negative: bool,
    pub size: bool,
    pub steps: bool,
    pub cfg: bool,
    pub seed: bool,
    pub batch: bool,
}

/// 通道描述（注册表里的一条）
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GeneratorDescriptor {
    pub id: String,
    pub name: String,
    /// "local" | "cloud"
    pub channel: String,
    pub needs_base_url: bool,
    pub needs_api_key: bool,
    pub supports: GenSupports,
    /// **如实报告**：P1 只实现了本地 SD WebUI，云端两种如实标 false
    pub implemented: bool,
    /// 未实现时给用户看的原因
    pub note: String,
}

/// 生成请求（所有通道共用；不支持的字段会被适配器忽略）
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GenRequest {
    pub prompt: String,
    #[serde(default)]
    pub negative: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub steps: Option<u32>,
    #[serde(default)]
    pub cfg: Option<f64>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub batch: u32,
    /// 有值 = "重出这张"（P4 用它走版本链，P1 先透传）
    #[serde(default)]
    pub base_portrait_id: Option<i64>,
}

/// 生成结果：图片已落到**临时目录**，等前端调 `portrait_import` 入库
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenResult {
    /// 临时文件绝对路径
    pub files: Vec<String>,
    pub generator_id: String,
    /// 实际使用的种子（随机时回填，便于"同种子微调"）
    pub seeds: Vec<i64>,
    /// 通道原始返回摘要（排错与复现用）
    pub meta: String,
}

/// 通道注册表：**加一个通道 = 往这里加一条 + 在 `generate` 里加一个分支**
pub fn registry() -> Vec<GeneratorDescriptor> {
    let all = GenSupports { negative: true, size: true, steps: true, cfg: true, seed: true, batch: true };
    vec![
        GeneratorDescriptor {
            id: "local_sdwebui".into(),
            name: "本地 SD WebUI".into(),
            channel: "local".into(),
            needs_base_url: true,
            needs_api_key: false,
            supports: all.clone(),
            implemented: true,
            note: "连接本机 Stable Diffusion WebUI（需带 --api 启动）".into(),
        },
        GeneratorDescriptor {
            id: "cloud_openai_image".into(),
            name: "云端 · 兼容 OpenAI Images API".into(),
            channel: "cloud".into(),
            needs_base_url: true,
            needs_api_key: true,
            supports: GenSupports { steps: false, cfg: false, batch: true, ..all.clone() },
            implemented: false,
            note: "计划在 P3 实装（通用 Images API 形态，覆盖大多数云服务）".into(),
        },
        GeneratorDescriptor {
            id: "cloud_custom".into(),
            name: "云端 · 自定义 URL + 请求模板".into(),
            channel: "cloud".into(),
            needs_base_url: true,
            needs_api_key: true,
            supports: all.clone(),
            implemented: false,
            note: "计划在 P3 实装（应对各家不兼容 API，由你填请求模板）".into(),
        },
    ]
}

/// 按 id 取通道描述
pub fn descriptor_of(id: &str) -> Option<GeneratorDescriptor> {
    registry().into_iter().find(|d| d.id == id)
}

/// 连接测试结果
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenProbe {
    pub ok: bool,
    /// 给用户看的一句话（成功也给，便于确认连到了什么）
    pub detail: String,
}

/// 连接测试：**只读探测，不产图**。未实现的通道如实说明，不假装成功。
pub async fn probe(generator_id: &str, base_url: &str) -> Result<GenProbe, String> {
    let d = descriptor_of(generator_id).ok_or_else(|| format!("未知通道：{generator_id}"))?;
    if !d.implemented {
        return Ok(GenProbe { ok: false, detail: format!("该通道尚未实装：{}", d.note) });
    }
    if d.needs_base_url && base_url.trim().is_empty() {
        return Ok(GenProbe { ok: false, detail: "还没有填服务地址".into() });
    }
    match generator_id {
        // SD WebUI：GET /sdapi/v1/options 是最轻的只读探测
        "local_sdwebui" => {
            let url = format!("{}/sdapi/v1/options", base_url.trim_end_matches('/'));
            match reqwest::Client::new().get(&url).timeout(std::time::Duration::from_secs(8)).send().await {
                Ok(r) if r.status().is_success() => {
                    let txt = r.text().await.unwrap_or_default();
                    let model = serde_json::from_str::<serde_json::Value>(&txt).ok()
                        .and_then(|v| v["sd_model_checkpoint"].as_str().map(|s| s.to_string()))
                        .unwrap_or_default();
                    Ok(GenProbe {
                        ok: true,
                        detail: if model.is_empty() {
                            "已连上 SD WebUI".to_string()
                        } else {
                            format!("已连上 SD WebUI，当前模型：{model}")
                        },
                    })
                }
                Ok(r) => Ok(GenProbe { ok: false, detail: format!("服务有响应但返回 HTTP {}（确认是 WebUI 且带 --api 启动）", r.status().as_u16()) }),
                Err(e) => Ok(GenProbe { ok: false, detail: format!("连不上：{e}（确认 WebUI 已启动且地址端口正确）") }),
            }
        }
        other => Err(format!("通道 {other} 的探测尚未实装")),
    }
}

/// 生成立绘：结果写到临时目录，由前端调 `portrait_import` 入库（**严守 R1**）
pub async fn generate(
    generator_id: &str,
    base_url: &str,
    api_key: &str,
    req: &GenRequest,
) -> Result<GenResult, String> {
    let d = descriptor_of(generator_id).ok_or_else(|| format!("未知通道：{generator_id}"))?;
    if !d.implemented {
        return Err(format!("该通道尚未实装：{}", d.note));
    }
    if req.prompt.trim().is_empty() {
        return Err("提示词不能为空".into());
    }
    match generator_id {
        "local_sdwebui" => generate_sdwebui(base_url, req).await,
        other => {
            let _ = api_key;
            Err(format!("通道 {other} 的生成尚未实装"))
        }
    }
}

/// 临时目录（生成结果先落这里，入库后由前端决定是否清理）
pub fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("target")
        .join("gen-tmp");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 清理临时目录里的旧文件（生成是重活，失败/取消时不留半成品）
pub fn cleanup_temp(keep: &[String]) -> usize {
    let dir = temp_dir();
    let mut removed = 0;
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            let ps = p.to_string_lossy().to_string();
            if keep.iter().any(|k| *k == ps) {
                continue;
            }
            if std::fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// SD WebUI 适配器：POST /sdapi/v1/txt2img，返回 base64 图片数组
async fn generate_sdwebui(base_url: &str, req: &GenRequest) -> Result<GenResult, String> {
    if base_url.trim().is_empty() {
        return Err("请先填写本地服务地址（如 http://127.0.0.1:7860）".into());
    }
    let width = if req.width == 0 { 768 } else { req.width };
    let height = if req.height == 0 { 1024 } else { req.height };
    let batch = req.batch.clamp(1, 8);

    // 种子：给 -1 让服务随机；随机时用返回的 info 回填实际种子
    let seed = req.seed.unwrap_or(-1);
    let mut body = serde_json::json!({
        "prompt": req.prompt,
        "negative_prompt": req.negative,
        "width": width,
        "height": height,
        "batch_size": batch,
        "seed": seed,
    });
    if let Some(st) = req.steps { body["steps"] = serde_json::json!(st); }
    if let Some(c) = req.cfg { body["cfg_scale"] = serde_json::json!(c); }

    let url = format!("{}/sdapi/v1/txt2img", base_url.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .timeout(std::time::Duration::from_secs(300))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求本地服务失败：{e}（确认 SD WebUI 已启动且带 --api）"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("本地服务返回 HTTP {}：{}", status.as_u16(), text.chars().take(200).collect::<String>()));
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("返回不是合法 JSON：{e}"))?;
    let imgs = v["images"].as_array().cloned().unwrap_or_default();
    if imgs.is_empty() {
        return Err("本地服务没有返回图片（检查模型是否加载、显存是否足够）".into());
    }

    // 写临时文件（**不写素材库目录** —— 入库由前端调 portrait_import 完成）
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = temp_dir();
    let mut files = Vec::new();
    for (i, im) in imgs.iter().enumerate() {
        let b64 = im.as_str().unwrap_or_default();
        let raw = b64.split(',').last().unwrap_or(b64);
        let bytes = base64_decode(raw).ok_or_else(|| "图片 base64 解码失败".to_string())?;
        let p = dir.join(format!("gen-{stamp}-{i}.png"));
        std::fs::write(&p, &bytes).map_err(|e| format!("写临时文件失败：{e}"))?;
        files.push(p.to_string_lossy().to_string());
    }

    // 回填实际种子（只解析到就用，解析不到留空）
    let seeds = parse_seeds(&v).unwrap_or_else(|| vec![seed]);
    Ok(GenResult {
        files,
        generator_id: "local_sdwebui".into(),
        seeds,
        meta: format!("SD WebUI · {}×{} · batch {} · seed {}", width, height, batch, seed),
    })
}

/// 从 SD WebUI 的 info 字段里取实际种子
fn parse_seeds(v: &serde_json::Value) -> Option<Vec<i64>> {
    let info = v["info"].as_str()?;
    let j: serde_json::Value = serde_json::from_str(info).ok()?;
    let s = j["seed"].as_i64()?;
    Some(vec![s])
}

/// 极简 base64 解码（避免为一个功能引入新依赖）
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut rev = [255u8; 256];
    for (i, c) in TBL.iter().enumerate() {
        rev[*c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for ch in s.bytes() {
        if ch == b'=' || ch == b'\n' || ch == b'\r' {
            continue;
        }
        let val = rev[ch as usize];
        if val == 255 {
            return None;
        }
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_reports_implemented_honestly() {
        let reg = registry();
        assert_eq!(reg.len(), 3, "P1 注册表：SD WebUI + 云端两种");
        let sd = reg.iter().find(|d| d.id == "local_sdwebui").unwrap();
        assert!(sd.implemented, "P1 已实装 SD WebUI");
        // 云端两种如实标注未实装（不假装可用）
        for id in ["cloud_openai_image", "cloud_custom"] {
            let d = reg.iter().find(|x| x.id == id).unwrap();
            assert!(!d.implemented, "{id} 在 P1 应如实标注未实装");
            assert!(!d.note.is_empty(), "{id} 应说明原因");
            assert_eq!(d.channel, "cloud");
        }
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
    fn temp_dir_is_isolated_under_target() {
        let d = temp_dir();
        assert!(d.to_string_lossy().contains("gen-tmp"), "临时目录应放在 target/gen-tmp，不污染素材库");
    }

/// 真机验证用（默认跳过）：需要本机 127.0.0.1:7860 有一个可用的 SD WebUI。
    /// 跑法：`cargo test --lib live_sdwebui -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn live_sdwebui_probe_and_generate() {
        let base = "http://127.0.0.1:7860";
        let p = probe("local_sdwebui", base).await.unwrap();
        println!("探测结果：ok={} detail={}", p.ok, p.detail);
        assert!(p.ok, "应能连上本机 SD WebUI：{}", p.detail);

        let req = GenRequest {
            prompt: "1girl, nun habit, upper body".into(),
            negative: "lowres, watermark".into(),
            width: 512, height: 512, batch: 1, seed: Some(12345),
            ..Default::default()
        };
        let r = generate("local_sdwebui", base, "", &req).await.unwrap();
        println!("生成结果：{} 个文件，seeds={:?}，meta={}", r.files.len(), r.seeds, r.meta);
        assert!(!r.files.is_empty(), "应产出图片");
        for f in &r.files {
            let md = std::fs::metadata(f).expect("临时文件应存在");
            // 只需非空且有 PNG 头（本机真机测试产出的图会远大于此；65 字节的 1×1 PNG 也合法）
            assert!(md.len() > 20, "图片应有实际内容：{f}");
            let head = std::fs::read(f).unwrap();
            assert_eq!(&head[..4], b"\x89PNG", "应是 PNG");
        }
        // 收尾：清掉刚生成的临时文件（素材库目录不受影响 —— 入库由前端调 portrait_import）
        let removed = cleanup_temp(&[]);
        println!("清理临时文件 {removed} 个");
    }

    #[tokio::test]
    async fn unimplemented_channel_refuses_politely() {
        // 未实装通道：探测与生成都如实报错，而不是 panic 或假装成功
        let p = probe("cloud_custom", "https://example.com").await.unwrap();
        assert!(!p.ok);
        assert!(p.detail.contains("尚未实装"), "应说明未实装：{}", p.detail);
        let e = generate("cloud_custom", "https://example.com", "", &GenRequest { prompt: "x".into(), ..Default::default() }).await.unwrap_err();
        assert!(e.contains("尚未实装"), "应如实报错：{e}");
    }

    #[tokio::test]
    async fn missing_base_url_is_explained() {
        let p = probe("local_sdwebui", "").await.unwrap();
        assert!(!p.ok);
        assert!(p.detail.contains("地址"), "应提示填地址：{}", p.detail);
    }

    #[tokio::test]
    async fn empty_prompt_is_rejected_before_network() {
        let e = generate("local_sdwebui", "http://127.0.0.1:1", "", &GenRequest { prompt: "  ".into(), ..Default::default() }).await.unwrap_err();
        assert!(e.contains("提示词"), "空提示词应在发请求前拦下：{e}");
    }
}
