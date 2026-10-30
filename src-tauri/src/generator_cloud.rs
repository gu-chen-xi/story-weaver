//! 云端适配器（ROADMAP 第四批 ⑥ · P3）
//!
//! 两种形态（用户 2026-10-11 定：**两个都做**）：
//!
//! | 适配器 | 适用 | 关键点 |
//! |---|---|---|
//! | `cloud_openai_image` | 兼容 OpenAI Images API 的云服务 | `POST {base}/images/generations`，`response_format=b64_json`，兼容 `data[].b64_json` 与 `data[].url` |
//! | `cloud_custom` | 各家不兼容 API | 用户填 **URL + 请求体模板 + 响应字段路径**；占位符 `{{prompt}}` 等由软件填充 |
//!
//! 本文件属于 `generator` 的**通道实现层**：只负责"把请求发出去、把图取回来、落到临时目录"。
//! 不碰数据库、不碰素材库（R1 由 `generator_ingest` 统一负责入库）。

use crate::generator::{base64_decode, temp_dir, GenRequest, GenResult};

/// URL 取图时的最大字节数（防止把超大文件拉进内存）
const MAX_DOWNLOAD: usize = 32 * 1024 * 1024;

/// 云端通用配置（由 `portrait.*` 配置键组装而来）
#[derive(Debug, Clone, Default)]
pub struct CloudConfig {
    /// 基础地址（`cloud_openai_image` 用；填到 `/v1` 为止）
    pub base_url: String,
    /// 完整请求地址（`cloud_custom` 用；可含占位符）
    pub url: String,
    pub api_key: String,
    /// "bearer" | "api-key" | "none"
    pub auth_style: String,
    /// 额外请求头，形如 `a=b;c=d`
    pub extra_headers: String,
    /// OpenAI 形态的模型名（可空）
    pub model: String,

    /// `cloud_custom`：请求体模板（JSON 文本，可含占位符）
    pub body_template: String,
    /// `cloud_custom`：从响应里取图片的路径
    ///   - 以 `b64:` 开头 → 该路径的字符串是 base64
    ///   - 以 `url:` 开头 → 该路径是图片 URL（下载后入库）
    ///   - 其它        → 该路径是一个字符串数组，元素自动识别（http 开头当 URL，否则当 base64）
    pub response_path: String,
    /// `cloud_openai_image` 的取图路径：**单独一份键**。
    ///
    /// 2026-10-25 拆开：两条通道的响应形状本来就不一样（OpenAI 官方是 `data[0].b64_json`，
    /// 硅基流动是 `images[0].url`）。共用一个键会串——在自定义通道填了 `url:images.0.url`，
    /// 切到 OpenAI 通道时就会拿这个路径去解析 `data` 数组，直接报"找不到路径"。
    pub openai_response_path: String,
    /// `cloud_custom`：成功时 HTTP 状态码（默认 200）
    pub ok_status: u16,
    /// `cloud_custom`：把响应 JSON 整体当作 base64（极少数服务如此）
    pub response_raw_b64: bool,
}

impl CloudConfig {
    /// 用"界面上当前填的值"覆盖配置（这样**没保存也能测试连接**；留空的字段保留原值）
    pub fn apply_overrides(&mut self, o: &CloudOverrides) {
        let set = |dst: &mut String, v: &Option<String>| {
            if let Some(s) = v {
                if !s.trim().is_empty() {
                    *dst = s.clone();
                }
            }
        };
        set(&mut self.base_url, &o.base_url);
        set(&mut self.url, &o.url);
        set(&mut self.api_key, &o.api_key);
        set(&mut self.auth_style, &o.auth_style);
        set(&mut self.extra_headers, &o.extra_headers);
        set(&mut self.model, &o.model);
        set(&mut self.body_template, &o.body_template);
        set(&mut self.response_path, &o.response_path);
        set(&mut self.openai_response_path, &o.openai_response_path);
    }

    /// 从配置项组装（键名见 db.rs 的 DEFAULT_CONFIGS）
    pub fn from_map(get: &dyn Fn(&str) -> String) -> Self {
        Self {
            base_url: get("portrait.cloud_url"),
            url: get("portrait.cloud_request_url"),
            api_key: get("portrait.cloud_api_key"),
            auth_style: {
                let v = get("portrait.cloud_auth_style");
                if v.is_empty() { "bearer".to_string() } else { v }
            },
            extra_headers: get("portrait.cloud_extra_headers"),
            model: get("portrait.cloud_model"),
            body_template: get("portrait.cloud_body_template"),
            response_path: {
                // 默认按 OpenAI 形态取图：`data` 数组，元素自动识别（b64_json / url / 纯字符串）。
                // 2026-10-25 从 `b64:data.0.b64_json` 改成 `auto:data` —— 前者只认 base64，
                // 而不少"OpenAI 兼容"服务实际返回的是 url（例如硅基流动返回 images[0].url）。
                let v = get("portrait.cloud_response_path");
                if v.is_empty() { "auto:data".to_string() } else { v }
            },
            openai_response_path: {
                let v = get("portrait.cloud_openai_response_path");
                if v.is_empty() { "auto:data".to_string() } else { v }
            },
            ok_status: {
                let v = get("portrait.cloud_ok_status");
                v.parse::<u16>().unwrap_or(200)
            },
            response_raw_b64: get("portrait.cloud_response_raw_b64") == "true",
        }
    }
}

/// 界面当前值（用于"没保存也能测试连接"；空字段表示"用已保存的配置"）
#[derive(serde::Deserialize, Debug, Clone, Default)]
pub struct CloudOverrides {
    pub base_url: Option<String>,
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub auth_style: Option<String>,
    pub extra_headers: Option<String>,
    pub model: Option<String>,
    pub body_template: Option<String>,
    pub response_path: Option<String>,
    /// OpenAI 通道自己的取图路径（与自定义通道分开，理由见 `CloudConfig::openai_response_path`）
    pub openai_response_path: Option<String>,
}

/// 把 `{{key}}` 占位符替换掉（用于 URL、请求头、请求体模板）
pub fn fill(tpl: &str, vars: &[(&str, String)]) -> String {
    let mut s = tpl.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("{{{{{k}}}}}"), v);
    }
    s
}

/// 模板值是「整串就是一个占位符」、替换后变成空串 → 这一项整个丢掉。
///
/// 为什么要丢：`"num_inference_steps":"{{steps}}"` 在 steps 留空时会变成 `""`，
/// 多数服务直接 400（该字段必须是 number）。用户想要的语义是「没填就别发这个字段」。
/// 只对整串就是占位符的情况生效 —— `"pre{{steps}}"` 这种替换后还有内容的照常保留。
fn drops_empty_placeholder(orig: &serde_json::Value, filled: &serde_json::Value) -> bool {
    match (orig.as_str(), filled.as_str()) {
        (Some(s), Some(f)) => {
            let t = s.trim();
            f.is_empty() && t.starts_with("{{") && t.ends_with("}}") && t.matches("{{").count() == 1
        }
        _ => false,
    }
}

/// 递归替换 JSON 里的字符串占位符
pub fn fill_json(v: &serde_json::Value, vars: &[(&str, String)]) -> serde_json::Value {
    match v {
        serde_json::Value::String(s) => {
            let filled = fill(s, vars);
            // 纯数字型占位符（{{seed}} 之类）整串替换时还原成数字，避免服务端类型校验失败
            if let Some((_k, _)) = vars.iter().find(|(k, _)| s.trim() == format!("{{{{{k}}}}}")) {
                if let Ok(n) = filled.parse::<i64>() {
                    return serde_json::json!(n);
                }
            }
            serde_json::Value::String(filled)
        }
        serde_json::Value::Array(a) => serde_json::Value::Array(a.iter().map(|x| fill_json(x, vars)).collect()),
        serde_json::Value::Object(o) => {
            let mut m = serde_json::Map::new();
            for (k, val) in o {
                let filled_val = fill_json(val, vars);
                // 留空的参数「不发」，而不是发空串（2026-10-25 用户要求）
                if drops_empty_placeholder(val, &filled_val) {
                    continue;
                }
                m.insert(fill(k, vars), filled_val);
            }
            serde_json::Value::Object(m)
        }
        other => other.clone(),
    }
}

/// 组装请求头（认证风格 + 额外头）
fn headers(cfg: &CloudConfig) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![("Content-Type".into(), "application/json".into())];
    match cfg.auth_style.as_str() {
        "bearer" if !cfg.api_key.is_empty() => {
            out.push(("Authorization".into(), format!("Bearer {}", cfg.api_key)))
        }
        "api-key" if !cfg.api_key.is_empty() => out.push(("api-key".into(), cfg.api_key.clone())),
        _ => {}
    }
    for part in cfg.extra_headers.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((k, v)) = part.split_once('=') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

/// 解析响应里的图片条目
#[derive(Debug, Clone, PartialEq)]
pub enum ImageRef {
    B64(String),
    Url(String),
}

/// 从响应 JSON 里按 `response_path` 取图片引用（纯函数，便于单测）
pub fn extract_images(resp: &serde_json::Value, path_spec: &str, raw_b64: bool) -> Result<Vec<ImageRef>, String> {
    if raw_b64 {
        return match resp.as_str() {
            Some(s) if !s.is_empty() => Ok(vec![ImageRef::B64(s.to_string())]),
            _ => Err("响应不是非空字符串（response_raw_b64 期望纯 base64 文本）".into()),
        };
    }
    // 支持 "auto:data.0.b64_json" / "b64:..." / "url:..." 三种写法；
    // 没写前缀时整体当路径、按 auto 识别。
    let (mode, path) = match path_spec.split_once(':') {
        Some((m, p)) if m == "b64" || m == "url" || m == "auto" => (m, p),
        _ => ("auto", path_spec),
    };
    if path.is_empty() {
        return Err("响应字段路径为空".into());
    }
    let node = json_path(resp, path)
        .ok_or_else(|| format!("响应里找不到路径「{path}」（请检查响应字段路径配置）"))?;

    // 数组：逐个自动识别
    if let Some(arr) = node.as_array() {
        let mut out = Vec::new();
        for it in arr {
            if let Some(s) = it.as_str() {
                out.push(classify(s, mode));
            } else if let Some(s) = it.get("b64_json").and_then(|x| x.as_str()) {
                out.push(ImageRef::B64(s.to_string()));
            } else if let Some(s) = it.get("url").and_then(|x| x.as_str()) {
                out.push(ImageRef::Url(s.to_string()));
            } else if let Some(s) = it.get("b64").and_then(|x| x.as_str()) {
                out.push(ImageRef::B64(s.to_string()));
            }
        }
        if out.is_empty() {
            return Err(format!("路径「{path}」里没有可识别的图片字段（支持字符串 / b64_json / url / b64）"));
        }
        return Ok(out);
    }
    // 单对象 / 单字符串
    if let Some(s) = node.as_str() {
        return Ok(vec![classify(s, mode)]);
    }
    if let Some(s) = node.get("b64_json").and_then(|x| x.as_str()) {
        return Ok(vec![ImageRef::B64(s.to_string())]);
    }
    if let Some(s) = node.get("url").and_then(|x| x.as_str()) {
        return Ok(vec![ImageRef::Url(s.to_string())]);
    }
    Err(format!("路径「{path}」的类型不认识（期望字符串 / 字符串数组 / 含 b64_json 或 url 的对象）"))
}

fn classify(s: &str, mode: &str) -> ImageRef {
    match mode {
        "b64" => ImageRef::B64(s.to_string()),
        "url" => ImageRef::Url(s.to_string()),
        _ => {
            if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:") {
                if s.starts_with("data:") {
                    ImageRef::B64(s.split(',').last().unwrap_or(s).to_string())
                } else {
                    ImageRef::Url(s.to_string())
                }
            } else {
                ImageRef::B64(s.to_string())
            }
        }
    }
}

/// 极简点路径取值：`data.0.b64_json`（支持数字下标；键名里含 `.` 的用 `["a.b"]`）
pub fn json_path<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = v;
    for seg in path.split('.') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        if let Ok(idx) = seg.parse::<usize>() {
            cur = cur.get(idx)?;
        } else {
            cur = cur.get(seg)?;
        }
    }
    Some(cur)
}

/// 默认的 OpenAI 形态请求体
fn openai_body(req: &GenRequest, cfg: &CloudConfig) -> serde_json::Value {
    let n = req.batch.clamp(1, 8);
    let mut body = serde_json::json!({
        "prompt": if req.negative.trim().is_empty() {
            req.prompt.clone()
        } else {
            format!("{}\n\nNegative prompt: {}", req.prompt, req.negative)
        },
        "n": n,
        "size": format!("{}x{}", if req.width == 0 { 1024 } else { req.width }, if req.height == 0 { 1024 } else { req.height }),
        "response_format": "b64_json",
    });
    // 单次指定的模型优先于配置里的（硅基流动这类服务靠 model 区分模型，换模型不该先跑设置页）
    let model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(cfg.model.trim());
    if !model.is_empty() {
        body["model"] = serde_json::json!(model);
    }
    body
}

/// `cloud_openai_image`：兼容 OpenAI Images API
pub async fn generate_openai(cfg: &CloudConfig, req: &GenRequest) -> Result<GenResult, String> {
    if cfg.base_url.trim().is_empty() {
        return Err("还没有填云端服务地址（填到 /v1 为止，例如 https://api.example.com/v1）".into());
    }
    if cfg.api_key.trim().is_empty() {
        return Err("还没有填云端 API Key".into());
    }
    let url = format!("{}/images/generations", cfg.base_url.trim_end_matches('/'));
    let body = openai_body(req, cfg);
    let resp = send_json(&url, cfg, &body).await?;
    // 取图路径：**OpenAI 通道用自己那份键**（portrait.cloud_openai_response_path）——
    // 和自定义通道的 response_path 分开存，否则一边填了另一边就跟着串（2026-10-25 修）。
    let path = if cfg.openai_response_path.trim().is_empty() { "auto:data" } else { cfg.openai_response_path.trim() };
    let imgs = extract_images(&resp, path, cfg.response_raw_b64)?;

    let mut files = Vec::new();
    for (i, im) in imgs.iter().enumerate() {
        let bytes = match im {
            ImageRef::B64(s) => base64_decode(s.split(',').last().unwrap_or(s))
                .ok_or_else(|| "返回的 base64 解码失败".to_string())?,
            ImageRef::Url(u) => download(u, cfg).await?,
        };
        files.push(write_temp(&bytes, i)?);
    }
    Ok(GenResult {
        files,
        generator_id: "cloud_openai_image".into(),
        seeds: req.seed.map(|s| vec![s]).unwrap_or_default(),
        meta: format!(
            "云端 Images API · {}×{} · n={} · model={}",
            if req.width == 0 { 1024 } else { req.width },
            if req.height == 0 { 1024 } else { req.height },
            req.batch.clamp(1, 8),
            if cfg.model.is_empty() { "(服务默认)" } else { &cfg.model }
        ),
    })
}

/// 这次实际用的种子。
///
/// **不能沿用 SD WebUI 的 -1 约定**：云端服务会直接拒绝 ——
/// 硅基流动实测返回 `400 {"code":20015,"message":"seed: Must be greater than or equal to 0"}`。
/// 所以"没填 / 填了负数"就自己摇一个非负的；摇出来的值会回填到素材记录里，照样能复现。
pub fn resolved_seed(req: &GenRequest) -> i64 {
    if let Some(s) = req.seed {
        if s >= 0 {
            return s;
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(123_456_789);
    // 落在 0 ..= 2_000_000_000：各家服务的上界都容得下（硅基流动是 0~9999999999）
    (nanos % 2_000_000_000) as i64
}

/// `cloud_custom`：用户填 URL + 请求体模板 + 响应路径
/// 模板占位符的取值表（`cloud_custom` 用；抽成函数是为了能单测"模型名取谁"）
fn template_vars(req: &GenRequest, cfg: &CloudConfig) -> Vec<(&'static str, String)> {
    let seed = resolved_seed(req);
    // 模型名：本次指定的优先，其次用「云端配置」里存的那个
    let model = req
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| cfg.model.clone());
    vec![
        ("prompt", req.prompt.clone()),
        ("negative", req.negative.clone()),
        ("negative_prompt", req.negative.clone()),
        ("width", (if req.width == 0 { 1024 } else { req.width }).to_string()),
        ("height", (if req.height == 0 { 1024 } else { req.height }).to_string()),
        ("batch", req.batch.clamp(1, 8).to_string()),
        ("seed", seed.to_string()),
        ("steps", req.steps.map(|v| v.to_string()).unwrap_or_default()),
        ("cfg", req.cfg.map(|v| v.to_string()).unwrap_or_default()),
        ("model", model),
        ("api_key", cfg.api_key.clone()),
    ]
}

pub async fn generate_custom(cfg: &CloudConfig, req: &GenRequest) -> Result<GenResult, String> {
    if cfg.url.trim().is_empty() {
        return Err("还没有填请求地址".into());
    }
    if cfg.body_template.trim().is_empty() {
        return Err("还没有填请求体模板".into());
    }
    let tpl: serde_json::Value = serde_json::from_str(&cfg.body_template)
        .map_err(|e| format!("请求体模板不是合法 JSON：{e}"))?;

    let seed = resolved_seed(req);
    let vars = template_vars(req, cfg);
    let url = fill(&cfg.url, &vars);
    let body = fill_json(&tpl, &vars);
    let resp = send_json(&url, cfg, &body).await?;

    // 成功状态码校验（有些服务用 201 等）
    let imgs = extract_images(&resp, &cfg.response_path, cfg.response_raw_b64)?;
    let mut files = Vec::new();
    for (i, im) in imgs.iter().enumerate() {
        let bytes = match im {
            ImageRef::B64(s) => base64_decode(s.split(',').last().unwrap_or(s))
                .ok_or_else(|| "响应里的 base64 解码失败".to_string())?,
            ImageRef::Url(u) => download(u, cfg).await?,
        };
        files.push(write_temp(&bytes, i)?);
    }
    Ok(GenResult {
        files,
        generator_id: "cloud_custom".into(),
        seeds: vec![seed],
        meta: format!("云端自定义 · {}×{} · n={}", req.width, req.height, req.batch.clamp(1, 8)),
    })
}

/// 发 JSON 请求并解析响应
async fn send_json(url: &str, cfg: &CloudConfig, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let mut rb = reqwest::Client::new()
        .post(url)
        .timeout(std::time::Duration::from_secs(180))
        .json(body);
    for (k, v) in headers(cfg) {
        rb = rb.header(k, v);
    }
    let resp = rb.send().await.map_err(|e| format!("请求失败：{e}"))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let ok = status.is_success() || status.as_u16() == cfg.ok_status;
    if !ok {
        return Err(format!(
            "服务返回 HTTP {}：{}",
            status.as_u16(),
            text.chars().take(300).collect::<String>()
        ));
    }
    serde_json::from_str(&text).map_err(|e| {
        format!(
            "响应不是合法 JSON：{e}；原始内容开头：{}",
            text.chars().take(160).collect::<String>()
        )
    })
}

/// 图片 URL 的可信级别（决定允不允许下、带不带鉴权）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadTrust {
    /// 与用户配置的 `base_url` / 自定义请求地址**同源**：放行，并带鉴权头
    /// （有些服务的图床确实要鉴权）
    SameOrigin,
    /// 第三方公网 https：放行，但**不带任何凭证**
    PublicHttps,
}

/// 是否本机 / 内网 / 链路本地地址（防止响应里的 url 被用来打内网）。
fn is_local_or_private_host(host: &str) -> bool {
    let h = host.trim().trim_matches(['[', ']']).to_ascii_lowercase();
    if h.is_empty() {
        return true;
    }
    if h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") || h == "0.0.0.0" {
        return true;
    }
    match h.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        Ok(std::net::IpAddr::V6(v6)) => v6.is_loopback() || v6.is_unspecified(),
        Err(_) => false,
    }
}

fn same_origin(a: &reqwest::Url, b: &reqwest::Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// **下载前的准入判定**（纯函数，便于单测）。
///
/// 背景（安全审计 2026-10-30 · F-01）：`url` 来自服务端响应体，原实现"给什么打什么"，
/// 于是恶意/被篡改的响应能把请求带去 `127.0.0.1` / 内网，并把 `Authorization` 头一起送过去。
///
/// 规则：
/// 1. 只允许 http/https；
/// 2. 与用户配置同源（`base_url` 或自定义请求地址）→ 放行，**带**鉴权头；
/// 3. 其它地址只允许 **https + 公网**，且**不带**任何凭证；
/// 4. 本机 / 内网 / 链路本地一律拒绝（同源除外 —— 本地 SD 服务本来就配在 127.0.0.1）。
fn classify_download_url(raw: &str, cfg: &CloudConfig) -> Result<DownloadTrust, String> {
    let u = reqwest::Url::parse(raw.trim()).map_err(|e| format!("图片地址不合法（{raw}）：{e}"))?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err(format!("图片地址只支持 http/https：{raw}"));
    }
    let trusted: Vec<reqwest::Url> = [cfg.base_url.trim(), cfg.url.trim()]
        .iter()
        .filter(|s| !s.is_empty())
        .filter_map(|s| reqwest::Url::parse(s).ok())
        .filter(|b| matches!(b.scheme(), "http" | "https"))
        .collect();
    if trusted.iter().any(|b| same_origin(b, &u)) {
        return Ok(DownloadTrust::SameOrigin);
    }
    if u.scheme() != "https" {
        return Err(format!("图片地址指向非 https 的第三方主机，已拒绝：{raw}"));
    }
    if is_local_or_private_host(u.host_str().unwrap_or("")) {
        return Err(format!("图片地址指向本机/内网，已拒绝：{raw}"));
    }
    Ok(DownloadTrust::PublicHttps)
}

/// 下载 URL 图片。
///
/// 安全审计 2026-10-30：F-01 加了来源准入（见 [`classify_download_url`]）；
/// F-02 改成**流式限长** —— 不再先把整个响应读进内存才检查大小。
async fn download(url: &str, cfg: &CloudConfig) -> Result<Vec<u8>, String> {
    let trust = classify_download_url(url, cfg)?;
    let mut rb = reqwest::Client::new().get(url).timeout(std::time::Duration::from_secs(120));
    if trust == DownloadTrust::SameOrigin {
        // 只有同一个服务才带鉴权；第三方图床不带，避免把 Key 送出去
        for (k, v) in headers(cfg) {
            if k.eq_ignore_ascii_case("content-type") {
                continue;
            }
            rb = rb.header(k, v);
        }
    }
    let mut resp = rb.send().await.map_err(|e| format!("下载图片失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载图片返回 HTTP {}", resp.status().as_u16()));
    }
    // ① 声明长度超限：连下载都别开始
    if let Some(len) = resp.content_length() {
        if len > MAX_DOWNLOAD as u64 {
            return Err(format!("图片过大（Content-Length {len} 字节，上限 {MAX_DOWNLOAD}）"));
        }
    }
    // ② 流式累计：对方不给 Content-Length（chunked）时也拦得住
    let mut out: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("下载图片中断：{e}"))? {
        if out.len() + chunk.len() > MAX_DOWNLOAD {
            return Err(format!("图片过大（超过 {MAX_DOWNLOAD} 字节，已中止下载）"));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// 落临时目录（**不写素材库** —— 入库由 `generator_ingest` 统一走 portrait_import）
fn write_temp(bytes: &[u8], idx: usize) -> Result<String, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let p = temp_dir().join(format!("cloud-{stamp}-{idx}.png"));
    std::fs::write(&p, bytes).map_err(|e| format!("写临时文件失败：{e}"))?;
    Ok(p.to_string_lossy().to_string())
}

/// 探测：只读、尽量省额度
pub async fn probe_openai(cfg: &CloudConfig) -> String {
    if cfg.base_url.trim().is_empty() {
        return "还没有填云端服务地址（形如 https://api.example.com/v1）".into();
    }
    if cfg.api_key.trim().is_empty() {
        return "还没有填云端 API Key".into();
    }
    // 先试 GET /models（最省额度；不是所有服务都实现）
    let url = format!("{}/models", cfg.base_url.trim_end_matches('/'));
    let mut rb = reqwest::Client::new().get(&url).timeout(std::time::Duration::from_secs(10));
    for (k, v) in headers(cfg) {
        rb = rb.header(k, v);
    }
    if let Ok(r) = rb.send().await {
        if r.status().is_success() {
            let t = r.text().await.unwrap_or_default();
            let names: Vec<String> = serde_json::from_str::<serde_json::Value>(&t).ok()
                .and_then(|v| v["data"].as_array().map(|a| {
                    a.iter().filter_map(|x| x["id"].as_str().map(|s| s.to_string())).take(4).collect()
                }))
                .unwrap_or_default();
            return if names.is_empty() {
                "地址与鉴权均可用（/models 有响应）".to_string()
            } else {
                format!("地址与鉴权均可用，可选模型：{}", names.join("、"))
            };
        }
        if r.status().as_u16() == 401 || r.status().as_u16() == 403 {
            return format!("鉴权失败（HTTP {}）：请检查 API Key 与鉴权方式", r.status().as_u16());
        }
    }
    // /models 不可用：如实说明，建议用一次小尺寸生成来验证
    "没有 /models 端点（这在很多服务上是正常的）。建议直接填小尺寸（如 512×512）生成一张来验证；生成若报错会显示服务端原文".to_string()
}

/// 探测：自定义形态只做配置完整性检查（不替用户发请求，避免误触计费）
pub fn probe_custom(cfg: &CloudConfig) -> String {
    let mut miss = Vec::new();
    if cfg.url.trim().is_empty() { miss.push("请求地址"); }
    if cfg.body_template.trim().is_empty() { miss.push("请求体模板"); }
    if cfg.response_path.trim().is_empty() { miss.push("响应字段路径"); }
    if !miss.is_empty() {
        return format!("还缺：{}", miss.join(" / "));
    }
    // 模板必须是合法 JSON
    if let Err(e) = serde_json::from_str::<serde_json::Value>(&cfg.body_template) {
        return format!("请求体模板不是合法 JSON：{e}");
    }
    "配置齐了（自定义形态只在真正生成时才会请求服务，避免误触计费）".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---------- 占位符填充 ----------

    #[test]
    fn fill_replaces_placeholders() {
        assert_eq!(fill("a/{{x}}/b", &[("x", "1".into())]), "a/1/b");
        assert_eq!(fill("{{a}}{{b}}", &[("a", "1".into()), ("b", "2".into())]), "12");
        // 未提供的占位符原样保留（便于用户发现拼错）
        assert_eq!(fill("{{nope}}", &[("x", "1".into())]), "{{nope}}");
    }

    #[test]
    fn fill_json_walks_nested_and_keeps_types() {
        let tpl = json!({
            "prompt": "{{prompt}}",
            "n": "{{batch}}",
            "meta": { "w": "{{width}}", "list": ["{{height}}", "fixed"] }
        });
        let out = fill_json(&tpl, &[
            ("prompt", "1girl".into()), ("batch", "2".into()),
            ("width", "768".into()), ("height", "1024".into()),
        ]);
        assert_eq!(out["prompt"], "1girl");
        assert_eq!(out["n"], 2, "纯数字占位符应还原成数字类型");
        assert_eq!(out["meta"]["w"], 768);
        assert_eq!(out["meta"]["list"][0], 1024);
        assert_eq!(out["meta"]["list"][1], "fixed");
    }

    /// 留空的参数**不发**（2026-10-25）：整串就是一个占位符、替换后为空 → 丢掉这一项。
    /// 典型场景：模板里写了 "num_inference_steps":"{{steps}}"，但用户没填步数 ——
    /// 发空串会被服务端判成类型错误（硅基流动就是这么 400 的）。
    #[test]
    fn fill_json_drops_blank_placeholders() {
        let tpl: serde_json::Value = serde_json::from_str(
            r#"{"steps":"{{steps}}","cfg":"{{cfg}}","keep":"x","mixed":"pre{{cfg}}","seed":"{{seed}}"}"#,
        )
        .unwrap();
        let vars: Vec<(&str, String)> = vec![
            ("steps", "".into()), ("cfg", "".into()), ("seed", "42".into()),
        ];
        let out = fill_json(&tpl, &vars);
        assert!(out.get("steps").is_none(), "空占位符整项应被丢掉：{out}");
        assert!(out.get("cfg").is_none(), "空占位符整项应被丢掉：{out}");
        assert_eq!(out["keep"], "x", "固定值不受影响");
        assert_eq!(out["mixed"], "pre", "替换后还有内容的照常保留");
        assert_eq!(out["seed"], 42, "数字占位符仍还原成数字");
    }
    // ---------- 点路径 ----------

    #[test]
    fn json_path_handles_index_and_nesting() {
        let v = json!({ "data": [ { "b64_json": "AAA" } ], "outer": { "inner": 5 } });
        assert_eq!(json_path(&v, "data.0.b64_json").unwrap(), "AAA");
        assert_eq!(json_path(&v, "outer.inner").unwrap(), 5);
        assert!(json_path(&v, "data.9.b64_json").is_none());
        assert!(json_path(&v, "nope").is_none());
    }

    // ---------- 响应解析（各家形态） ----------

    #[test]
    fn extract_handles_openai_shape() {
        let r = json!({ "data": [ { "b64_json": "AAA" }, { "b64_json": "BBB" } ] });
        assert_eq!(
            extract_images(&r, "auto:data", false).unwrap(),
            vec![ImageRef::B64("AAA".into()), ImageRef::B64("BBB".into())]
        );
    }

    #[test]
    fn extract_handles_url_list_and_data_uri() {
        let r = json!({ "output": ["https://x/a.png", "data:image/png;base64,ZZZ"] });
        assert_eq!(
            extract_images(&r, "auto:output", false).unwrap(),
            vec![ImageRef::Url("https://x/a.png".into()), ImageRef::B64("ZZZ".into())]
        );
    }

    #[test]
    fn extract_respects_explicit_mode() {
        // 显式 url: 模式：即使值看起来像 base64 也当 URL
        let r = json!({ "img": "QQQ" });
        assert_eq!(extract_images(&r, "url:img", false).unwrap(), vec![ImageRef::Url("QQQ".into())]);
        let r2 = json!({ "img": "https://x/a.png" });
        assert_eq!(extract_images(&r2, "b64:img", false).unwrap(), vec![ImageRef::B64("https://x/a.png".into())]);
    }

    #[test]
    fn extract_handles_nested_url_object() {
        let r = json!({ "result": { "images": [ { "url": "https://x/a.png" } ] } });
        assert_eq!(
            extract_images(&r, "auto:result.images", false).unwrap(),
            vec![ImageRef::Url("https://x/a.png".into())]
        );
    }

    #[test]
    fn extract_reports_bad_path_helpfully() {
        let r = json!({ "data": [] });
        let e = extract_images(&r, "auto:data", false).unwrap_err();
        assert!(e.contains("可识别"), "空数组应给出可操作的提示：{e}");
        let e2 = extract_images(&r, "auto:missing.path", false).unwrap_err();
        assert!(e2.contains("找不到路径"), "路径错应说清：{e2}");
    }

    #[test]
    fn extract_raw_b64_mode() {
        let r = json!("QUJD");
        assert_eq!(extract_images(&r, "", true).unwrap(), vec![ImageRef::B64("QUJD".into())]);
        assert!(extract_images(&json!({ "a": 1 }), "", true).is_err());
    }

    // ---------- 鉴权与请求头 ----------

    #[test]
    fn headers_follow_auth_style() {
        let mut c = CloudConfig { api_key: "K".into(), auth_style: "bearer".into(), ..Default::default() };
        let h = headers(&c);
        assert!(h.iter().any(|(k, v)| k == "Authorization" && v == "Bearer K"));

        c.auth_style = "api-key".into();
        let h2 = headers(&c);
        assert!(h2.iter().any(|(k, v)| k == "api-key" && v == "K"));
        assert!(!h2.iter().any(|(k, _)| k == "Authorization"), "api-key 风格不该带 Bearer");

        // 额外头（分号分隔）
        c.extra_headers = "X-Org=abc; X-Trace = t1".into();
        let h3 = headers(&c);
        assert!(h3.iter().any(|(k, v)| k == "X-Org" && v == "abc"));
        assert!(h3.iter().any(|(k, v)| k == "X-Trace" && v == "t1"));
    }

    #[test]
    fn headers_omit_auth_without_key() {
        let c = CloudConfig { auth_style: "bearer".into(), ..Default::default() };
        let h = headers(&c);
        assert!(!h.iter().any(|(k, _)| k == "Authorization" || k == "api-key"));
    }

    // ---------- 配置组装与探测 ----------

    #[test]
    fn config_from_map_has_sane_defaults() {
        let empty = |_: &str| String::new();
        let c = CloudConfig::from_map(&empty);
        assert_eq!(c.auth_style, "bearer");
        assert_eq!(c.response_path, "auto:data", "默认按 OpenAI 形态取图：data 数组、元素自动识别");
        assert_eq!(c.openai_response_path, "auto:data", "OpenAI 通道那份默认也是 auto:data");
        assert_eq!(c.ok_status, 200);
        assert!(!c.response_raw_b64);
    }

    /// **两条通道的取图路径必须各存各的**（2026-10-25）：
    /// 共用一个键时，在自定义通道填了 `url:images.0.url`，切到 OpenAI 通道会拿它去解析
    /// `data` 数组 → 直接报"找不到路径"。这条锁住"不会互相串"。
    #[test]
    fn response_path_is_per_channel() {
        let get = |k: &str| match k {
            "portrait.cloud_response_path" => "url:images.0.url".to_string(),
            _ => String::new(),
        };
        let c = CloudConfig::from_map(&get);
        assert_eq!(c.response_path, "url:images.0.url", "自定义通道读自己的键");
        assert_eq!(c.openai_response_path, "auto:data", "OpenAI 通道不该被自定义那份污染");

        let get2 = |k: &str| match k {
            "portrait.cloud_openai_response_path" => "url:data.0.url".to_string(),
            _ => String::new(),
        };
        let c2 = CloudConfig::from_map(&get2);
        assert_eq!(c2.openai_response_path, "url:data.0.url");
        assert_eq!(c2.response_path, "auto:data", "反过来也不能串");
    }

    #[test]
    fn probe_custom_checks_config_completeness() {
        let c = CloudConfig::default();
        let msg = probe_custom(&c);
        assert!(msg.contains("请求地址") && msg.contains("请求体模板") && msg.contains("响应字段路径"), "应列出缺什么：{msg}");

        let c2 = CloudConfig {
            url: "https://x/api".into(),
            body_template: "{不是 JSON".into(),
            response_path: "b64:data".into(),
            ..Default::default()
        };
        assert!(probe_custom(&c2).contains("不是合法 JSON"));

        let c3 = CloudConfig {
            url: "https://x/api".into(),
            body_template: r#"{"prompt":"{{prompt}}"}"#.into(),
            response_path: "b64:data".into(),
            ..Default::default()
        };
        assert!(probe_custom(&c3).contains("配置齐了"));
    }

    #[test]
    fn openai_probe_asks_for_missing_fields_first() {
        tauri::async_runtime::block_on(async {
            let empty = CloudConfig::default();
            assert!(probe_openai(&empty).await.contains("服务地址"));
            let no_key = CloudConfig { base_url: "https://x/v1".into(), ..Default::default() };
            assert!(probe_openai(&no_key).await.contains("API Key"));
        });
    }

    // ---------- OpenAI 请求体形状 ----------

    #[test]
    fn openai_body_merges_negative_into_prompt() {
        let c = CloudConfig { model: "img-1".into(), ..Default::default() };
        let req = GenRequest { prompt: "1girl".into(), negative: "lowres".into(), width: 768, height: 1024, batch: 3, ..Default::default() };
        let b = openai_body(&req, &c);
        assert_eq!(b["n"], 3);
        assert_eq!(b["size"], "768x1024");
        assert_eq!(b["model"], "img-1");
        assert_eq!(b["response_format"], "b64_json", "优先要 base64，避免再下一跳下载");
        let p = b["prompt"].as_str().unwrap();
        assert!(p.contains("1girl") && p.contains("Negative prompt: lowres"), "负向提示词应并入：{p}");

        // 没填负向时不出现多余的段落
        let req2 = GenRequest { prompt: "1girl".into(), ..Default::default() };
        let b2 = openai_body(&req2, &c);
        assert_eq!(b2["prompt"], "1girl");
        assert_eq!(b2["size"], "1024x1024", "宽高为 0 时用 1024 兜底");
    }

    /// **合规相关**：验证图片字节流直通、不重编码、不剥离元数据。
    ///
    /// 背景：《人工智能生成合成内容标识办法》（2025-09-01 施行）要求 AI 生成合成内容带
    /// 显式标识（可见水印）与隐式标识（文件元数据），并禁止任何组织和个人恶意删除、篡改、隐匿标识。
    /// 因此本软件对生成结果的搬运必须是**纯字节拷贝**，绝不能解码后重编码（那会丢掉元数据）。
    ///
    /// 本测试构造一个带自定义 PNG tEXt 块（模拟服务写入的 AI 生成元数据）的图片，
    /// 走完整的 base64 → 解码 → 落盘链路，断言元数据块原样还在。
    #[test]
    fn image_bytes_pass_through_without_stripping_metadata() {
        let marker: &[u8] = b"AI-Generated\x00";
        let png: Vec<u8> = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89,
            0x00, 0x00, 0x00, 0x12, 0x74, 0x45, 0x58, 0x74,
            b'A', b'I', b'-', b'G', b'e', b'n', b'e', b'r', b'a', b't', b'e', b'd', 0x00,
            b'A', b'I', b' ', b'G', b'E', b'N',
            0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let has = |b: &[u8]| b.windows(marker.len()).any(|w| w == marker);
        assert!(has(&png), "构造的测试图应含元数据块");

        let b64 = base64_encode(&png);
        let decoded = crate::generator::base64_decode(&b64).expect("应能解码");
        assert_eq!(decoded, png, "base64 往返必须逐字节一致");
        assert!(has(&decoded), "元数据块必须保留");

        // 落盘验证：**用测试私有目录**，不碰共享的 temp/。
        // 原来调 write_temp() 写共享临时目录，全量并行跑时会被别的用例的清理/删除撞上，
        // 偶发 `std::fs::read` 报 NotFound（E-BUILD-06）。write_temp 本身就是一次纯字节写，
        // 这里保持"解码后原样落盘、再读回来逐字节比对"这个合规断言不变。
        let dir = crate::paths::test_root().join(format!("cloud_meta_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("img.png");
        std::fs::write(&path, &decoded).unwrap();
        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(on_disk, png, "落盘必须是纯字节拷贝，不得重编码（否则会丢元数据标识）");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 测试用：最简 base64 编码
    fn base64_encode(data: &[u8]) -> String {
        const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(TBL[((n >> 18) & 63) as usize] as char);
            out.push(TBL[((n >> 12) & 63) as usize] as char);
            out.push(if chunk.len() > 1 { TBL[((n >> 6) & 63) as usize] as char } else { '=' });
            out.push(if chunk.len() > 2 { TBL[(n & 63) as usize] as char } else { '=' });
        }
        out
    }


// ================= 真机级验证（默认跳过） =================
    //
    // 需要本机 127.0.0.1:7891 有一个"假云端"：
    //   GET  /v1/models                → {"data":[{"id":"mock-image-1"}]}
    //   POST /v1/images/generations    → {"data":[{"b64_json":"<1x1 PNG>"} ...]}
    //   POST /custom/generate          → {"result":{"images":[{"b64":"<1x1 PNG>"}]}}
    // 跑法：`cargo test --lib live_cloud -- --ignored --nocapture`
    //
    // 这样能在**没有任何真实云服务**的前提下，验证两个云端适配器的完整链路：
    // 探测 → 请求（含鉴权头）→ 解析 → base64 解码 → 落临时目录 → PNG 头正确。

    const MOCK: &str = "http://127.0.0.1:7891";

    #[tokio::test]
    #[ignore]
    async fn live_cloud_openai_probe_and_generate() {
        let cfg = CloudConfig {
            base_url: format!("{MOCK}/v1"),
            api_key: "test-key".into(),
            model: "mock-image-1".into(),
            ..Default::default()
        };
        let detail = probe_openai(&cfg).await;
        println!("OpenAI 形态探测：{detail}");
        assert!(detail.contains("可用") || detail.contains("模型"), "探测应能连通：{detail}");

        let req = GenRequest { prompt: "1girl".into(), negative: "lowres".into(), width: 768, height: 1024, batch: 2, ..Default::default() };
        let r = generate_openai(&cfg, &req).await.unwrap();
        println!("OpenAI 形态生成：{} 张，meta={}", r.files.len(), r.meta);
        assert_eq!(r.files.len(), 2, "n=2 应产出两张");
        for f in &r.files {
            let bytes = std::fs::read(f).unwrap();
            assert_eq!(&bytes[..4], b"\x89PNG", "应是 PNG");
        }
        crate::generator::remove_files(&r.files);   // 只删自己的，避免并行互踩
    }

    #[tokio::test]
    #[ignore]
    async fn live_cloud_custom_probe_and_generate() {
        let cfg = CloudConfig {
            url: format!("{MOCK}/custom/generate"),
            api_key: "test-key".into(),
            body_template: r#"{"prompt":"{{prompt}}","w":"{{width}}","h":"{{height}}","n":"{{batch}}","seed":"{{seed}}"}"#.into(),
            response_path: "b64:result.images.0.b64".into(),
            ..Default::default()
        };
        println!("自定义形态探测：{}", probe_custom(&cfg));

        let req = GenRequest { prompt: "1girl, nun".into(), width: 512, height: 768, batch: 1, seed: Some(42), ..Default::default() };
        let r = generate_custom(&cfg, &req).await.unwrap();
        println!("自定义形态生成：{} 张，seeds={:?}，meta={}", r.files.len(), r.seeds, r.meta);
        assert_eq!(r.files.len(), 1);
        let bytes = std::fs::read(&r.files[0]).unwrap();
        assert_eq!(&bytes[..4], b"\x89PNG", "应是 PNG");
        assert_eq!(r.seeds, vec![42], "自定义形态应回填所用种子");
        crate::generator::remove_files(&r.files);   // 只删自己的，避免并行互踩
    }

    #[test]
    fn openai_body_omits_model_when_blank() {
        let c = CloudConfig::default();
        let b = openai_body(&GenRequest { prompt: "x".into(), ..Default::default() }, &c);
        assert!(b.get("model").is_none(), "没填模型就不带 model 字段");
    }

    /// v3.7：界面上的 model 输入框（单次指定）优先于「云端配置」里存的模型 ——
    /// 硅基流动这类服务靠 model 区分模型，换模型不该强制先去设置页改一次。
    #[test]
    fn openai_body_takes_request_model_first() {
        let cfg = CloudConfig { model: "cfg-model".into(), ..Default::default() };
        let with = |m: Option<&str>| {
            openai_body(&GenRequest { prompt: "x".into(), model: m.map(str::to_string), ..Default::default() }, &cfg)
        };
        assert_eq!(with(None)["model"], "cfg-model", "没传就用配置里的");
        assert_eq!(with(Some("Kwai-Kolors/Kolors"))["model"], "Kwai-Kolors/Kolors", "传了就用传的");
        assert_eq!(with(Some("   "))["model"], "cfg-model", "传了空白等于没传");
    }

    /// 自定义模板（`{{model}}`）取模型名的规则必须和 OpenAI 形态一致，
    /// 否则同一个服务换通道就行为不同 —— 这条锁住"两处同源"。
    #[test]
    fn template_vars_prefer_request_model() {
        let cfg = CloudConfig { model: "cfg-model".into(), ..Default::default() };
        let get = |v: &[(&'static str, String)], k: &str| {
            v.iter().find(|(n, _)| *n == k).map(|(_, s)| s.clone()).unwrap_or_default()
        };
        let req = GenRequest { prompt: "p".into(), model: Some("req-model".into()), seed: Some(7), ..Default::default() };
        let v = template_vars(&req, &cfg);
        assert_eq!(get(&v, "model"), "req-model", "本次指定的模型优先");
        assert_eq!(get(&v, "seed"), "7");
        assert_eq!(get(&v, "prompt"), "p");

        let v = template_vars(&GenRequest { prompt: "p".into(), ..Default::default() }, &cfg);
        assert_eq!(get(&v, "model"), "cfg-model", "没传就用配置里的");
        let v = template_vars(&GenRequest { prompt: "p".into(), model: Some(" ".into()), ..Default::default() }, &cfg);
        assert_eq!(get(&v, "model"), "cfg-model", "传了空白等于没传");
    }

    /// 云端服务不接受负数种子（硅基流动实测：`seed: Must be greater than or equal to 0`）。
    /// 所以"没填"必须是**摇一个非负的**，而不是沿用 SD WebUI 的 -1 约定。
    #[test]
    fn resolved_seed_never_goes_negative() {
        let absent = resolved_seed(&GenRequest { prompt: "p".into(), ..Default::default() });
        assert!((0..=2_000_000_000).contains(&absent), "没填种子应摇出非负值，实际 {absent}");
        let negative = resolved_seed(&GenRequest { prompt: "p".into(), seed: Some(-1), ..Default::default() });
        assert!(negative >= 0, "填了 -1 也要换成非负值，实际 {negative}");
        assert_eq!(
            resolved_seed(&GenRequest { prompt: "p".into(), seed: Some(42), ..Default::default() }),
            42,
            "填了非负值就照用"
        );
    }

    // ---------- 下载准入与限长（安全审计 2026-10-30 · F-01 / F-02） ----------

    fn cfg_with_base(base: &str) -> CloudConfig {
        CloudConfig {
            base_url: base.into(),
            api_key: "K".into(),
            auth_style: "bearer".into(),
            ..Default::default()
        }
    }

    /// 起一个只接一次的假服务端，返回（端口，记录到的请求头行）。
    fn tiny_server(response: Vec<u8>) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 8192];
                let n = s.read(&mut buf).unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                if let Some(head) = text.split("\r\n\r\n").next() {
                    seen2.lock().unwrap().extend(head.lines().map(|l| l.trim().to_string()));
                }
                let _ = s.write_all(&response);
                let _ = s.flush();
            }
        });
        (port, seen)
    }

    /// F-01 的核心规则：跨源只准公网 https；本机/内网/降级一律拒绝；
    /// 同源（含本地 SD）照常放行。
    #[test]
    fn download_policy_rejects_private_and_downgrade_targets() {
        let cfg = cfg_with_base("https://api.example.com/v1");
        assert_eq!(
            classify_download_url("https://api.example.com/a.png", &cfg).unwrap(),
            DownloadTrust::SameOrigin
        );
        assert_eq!(
            classify_download_url("https://cdn.other.com/a.png", &cfg).unwrap(),
            DownloadTrust::PublicHttps,
            "第三方公网 https 应放行（但不带凭证）"
        );
        for bad in [
            "http://127.0.0.1:9/x",
            "http://192.168.1.1/x",
            "http://10.0.0.5/x",
            "https://169.254.169.254/latest/meta-data/",
            "http://localhost/x",
            "http://[::1]/x",
            "file:///C:/Windows/win.ini",
            "ftp://x/y",
        ] {
            assert!(classify_download_url(bad, &cfg).is_err(), "应拒绝：{bad}");
        }

        // 本地 SD 场景：base_url 自己就是 127.0.0.1 → 同源放行，不误伤
        let local = cfg_with_base("http://127.0.0.1:7860");
        assert_eq!(
            classify_download_url("http://127.0.0.1:7860/file=/a.png", &local).unwrap(),
            DownloadTrust::SameOrigin
        );

        // 自定义通道只填了请求地址：同源也要认（`base_url` 为空时）
        let custom = CloudConfig { url: "https://api.example.com/gen".into(), ..Default::default() };
        assert_eq!(
            classify_download_url("https://api.example.com/a.png", &custom).unwrap(),
            DownloadTrust::SameOrigin
        );
        assert!(
            classify_download_url("http://127.0.0.1/x", &custom).is_err(),
            "只填了请求地址时，也不能回头打内网"
        );
    }

    /// 同源下载仍然要带鉴权头（有些图床确实要），别把正常用法定死。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn download_same_origin_still_sends_auth() {
        let (port, seen) = tiny_server(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\npwn!".to_vec(),
        );
        let cfg = cfg_with_base(&format!("http://127.0.0.1:{port}"));
        let out = download(&format!("http://127.0.0.1:{port}/a.png"), &cfg).await.unwrap();
        assert_eq!(out, b"pwn!");
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .any(|h| h.to_ascii_lowercase().starts_with("authorization:")),
            "同源下载应带鉴权头"
        );
    }

    /// F-01 的回归：跨源 + 本机地址在**发请求之前**就被拒。
    #[tokio::test]
    async fn download_refuses_cross_origin_loopback_before_request() {
        let cfg = cfg_with_base("https://api.example.com/v1");
        let err = download("http://127.0.0.1:59999/a.png", &cfg).await.unwrap_err();
        assert!(err.contains("已拒绝"), "{err}");
    }

    /// F-02 的回归：声明长度超限时立即拒绝，不开始下载。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn download_rejects_declared_huge_body() {
        const BIG: usize = 40 * 1024 * 1024;
        let (port, _seen) = tiny_server(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {BIG}\r\nConnection: close\r\n\r\n"
            )
            .into_bytes(),
        );
        let cfg = cfg_with_base(&format!("http://127.0.0.1:{port}"));
        let err = download(&format!("http://127.0.0.1:{port}/big.png"), &cfg).await.unwrap_err();
        assert!(err.contains("图片过大"), "{err}");
        assert!(err.contains("Content-Length"), "应先按声明长度拒绝：{err}");
    }

    /// F-02 的回归：不给 Content-Length（chunked）时，流式累计到上限就中止，
    /// 服务端写不完 40MB。
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn download_stops_chunked_stream_at_limit() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let written = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let w = written.clone();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 8192];
                let _ = s.read(&mut buf);
                if s.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .is_err()
                {
                    return;
                }
                let chunk = vec![0u8; 64 * 1024];
                let mut total = 0usize;
                while total < 40 * 1024 * 1024 {
                    let head = format!("{:X}\r\n", chunk.len());
                    if s.write_all(head.as_bytes()).is_err()
                        || s.write_all(&chunk).is_err()
                        || s.write_all(b"\r\n").is_err()
                    {
                        break;
                    }
                    total += chunk.len();
                    // 逐块累加：客户端一到上限就会断开，服务端可能还卡在半路
                    w.store(total, std::sync::atomic::Ordering::SeqCst);
                }
            }
        });
        let cfg = cfg_with_base(&format!("http://127.0.0.1:{port}"));
        let err = download(&format!("http://127.0.0.1:{port}/big.png"), &cfg).await.unwrap_err();
        assert!(err.contains("图片过大"), "{err}");
        // 等写端把进度落下来（最多 3 秒）
        let mut n = 0usize;
        for _ in 0..60 {
            n = written.load(std::sync::atomic::Ordering::SeqCst);
            if n >= 30 * 1024 * 1024 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(n < 40 * 1024 * 1024, "应在上限处中止，服务端不该写完 40MB；实际 {n}");
        assert!(n > 30 * 1024 * 1024, "中止前应确实读到了约 32MB；实际 {n}");
    }
}
