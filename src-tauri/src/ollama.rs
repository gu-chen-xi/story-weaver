//! Ollama 本地模型（2026-10-31，用户需求）。
//!
//! 定位与边界（用户原话）：
//! * **只支持 Windows 版 Ollama**；不负责下载、不负责启动 —— 用户自己开 Ollama、自己点插件开关；
//! * 我们只做两件事：**检查地址通不通**（`/api/version`）、把主模型请求转到本地；
//! * **同一时间只有一个主模型**：开着 Ollama 就用它，云端被替换；辅助项（伏笔扫描等）可以同时用云端；
//! * 适配目标：**Ollama 0.40.11（Windows 版）**；实测版本会显示在插件卡上。

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

pub const KEY_ENABLED: &str = "plugin.ollama.enabled";
pub const KEY_BASE_URL: &str = "ollama.base_url";
pub const KEY_MODEL: &str = "ollama.model";
pub const KEY_CTX: &str = "ollama.context_window";
pub const KEY_FORESHADOW: &str = "ollama.foreshadow_scan";

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";
/// 内置的上下文大小选项（2026-10-31 加 32k：与「生成参数」页的 32k 基准对齐）
pub const CTX_CHOICES: [i64; 5] = [32_768, 65_536, 131_072, 262_144, 1_048_576];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cfg {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub context_window: i64,
    pub foreshadow_scan: bool,
}

/// 归一化地址：去尾部斜杠；空 → 默认；非 http(s) 视为非法。
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let t = raw.trim().trim_end_matches('/');
    let t = if t.is_empty() { DEFAULT_BASE_URL } else { t };
    if !(t.starts_with("http://") || t.starts_with("https://")) {
        return Err("地址必须以 http:// 或 https:// 开头".into());
    }
    Ok(t.to_string())
}

/// 上下文窗口取值：只认内置五档；填了别的就落回 32k（宁可保守）。
pub fn normalize_ctx(v: i64) -> i64 {
    if CTX_CHOICES.contains(&v) { v } else { CTX_CHOICES[0] }
}

/// 拼接 OpenAI 兼容的对话地址（测试对话用，纯函数）。
pub fn chat_url(base_url: &str) -> Result<String, String> {
    Ok(format!("{}/v1/chat/completions", normalize_base_url(base_url)?))
}

async fn get(lib_pool: &SqlitePool, key: &str) -> String {
    crate::db::get_config(lib_pool, key)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

pub async fn load(lib_pool: &SqlitePool) -> Cfg {
    let base = normalize_base_url(&get(lib_pool, KEY_BASE_URL).await)
        .unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    let ctx = get(lib_pool, KEY_CTX).await.trim().parse::<i64>().unwrap_or(CTX_CHOICES[0]);
    Cfg {
        enabled: get(lib_pool, KEY_ENABLED).await.trim() == "true",
        base_url: base,
        model: get(lib_pool, KEY_MODEL).await.trim().to_string(),
        context_window: normalize_ctx(ctx),
        foreshadow_scan: get(lib_pool, KEY_FORESHADOW).await.trim() == "true",
    }
}

/// 只检查“服务在不在”，**不启动、不下载**。返回 `/api/version` 里的版本号。
pub async fn probe(base_url: &str) -> Result<String, String> {
    let base = normalize_base_url(base_url)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/api/version", base);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("连不上 {url}：{e}（Ollama 要先手动启动）"))?;
    if !resp.status().is_success() {
        return Err(format!("{url} 返回 HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("返回的不是合法 JSON：{e}"))?;
    Ok(v.get("version").and_then(|x| x.as_str()).unwrap_or("未知").to_string())
}

/// 主模型是否改走本地。开着但连不上/没填模型 → 报错（不静默回落云端，免得用户以为在用本地）。
pub async fn chat_override(lib_pool: &SqlitePool) -> Result<Option<Cfg>, String> {
    let cfg = load(lib_pool).await;
    if !cfg.enabled {
        return Ok(None);
    }
    if cfg.model.trim().is_empty() {
        return Err("Ollama 已开启，但还没填「模型名」（ollama list 里能看到的那个）".into());
    }
    probe(&cfg.base_url).await?;
    Ok(Some(cfg))
}

/// 「测试对话」：用**插件里填的地址和模型**发一句最小请求，验证本地服务可用。
/// 与云端完全隔离（不读 model.* 配置、不读 keychain）。
pub async fn test_chat(cfg: &Cfg) -> Result<String, String> {
    if cfg.model.trim().is_empty() {
        return Err("先填模型名（ollama list 里那个）".into());
    }
    let url = chat_url(&cfg.base_url)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "model": cfg.model,
        "messages": [{ "role": "user", "content": "请只回复两个字：你好" }],
        "max_tokens": 32,
        "stream": false
    });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("连不上 {url}：{e}（Ollama 要先手动启动）"))?;
    if !resp.status().is_success() {
        return Err(format!("{url} 返回 HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("返回的不是合法 JSON：{e}"))?;
    let text = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if text.trim().is_empty() {
        return Err("模型返回了空内容（模型名对不对？）".into());
    }
    Ok(text.trim().to_string())
}

/// 通用的一次性对话（非流式），给伏笔扫描这类短任务用。
///
/// **必须走原生 `/api/chat`**（2026-11-01 实测）：OpenAI 兼容端点不收 `num_ctx`，
/// Ollama 默认上下文只有 2048 —— 长输入被截断，甚至直接回空。
/// 原生接口能用 `options.num_ctx`，和界面上选的档位对上。
pub async fn chat_once(cfg: &Cfg, system: &str, user: &str, max_tokens: u32) -> Result<String, String> {
    if cfg.model.trim().is_empty() {
        return Err("Ollama 还没填模型名".into());
    }
    let url = format!("{}/api/chat", normalize_base_url(&cfg.base_url)?);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "model": cfg.model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "stream": false,
        // 2026-11-03：思考型模型（Qwen3 系）会把输出全塞进 message.thinking，
        // 正文因此为空。这个字段对不支持的模型是空操作。
        "think": false,
        "options": { "num_ctx": cfg.context_window, "num_predict": max_tokens },
    });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("连不上 {url}：{e}（Ollama 要先手动启动）"))?;
    if !resp.status().is_success() {
        return Err(format!("{url} 返回 HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("返回的不是合法 JSON：{e}"))?;
    let content = v
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if content.is_empty() {
        let thinking = v.get("message").and_then(|m| m.get("thinking")).and_then(|x| x.as_str()).unwrap_or("");
        let reason = v.get("done_reason").and_then(|x| x.as_str()).unwrap_or("");
        return Err(describe_empty_reply(reason, thinking, max_tokens));
    }
    Ok(content)
}

/// 空回复的诊断文案（2026-11-03 用户实测）。
///
/// 场景：装了一个**没有对话模板**的 GGUF（`TEMPLATE {{ .Prompt }}`），模型不知道
/// 什么时候该停，把输出全写进思考流 → `content` 永远为空、`done_reason=length`。
/// 必须说清楚，否则界面只显示"没有输出"，用户会以为软件坏了。
pub fn describe_empty_reply(reason: &str, thinking: &str, max_tokens: u32) -> String {
    if !thinking.trim().is_empty() || reason == "length" {
        format!(
            "模型只写进了思考过程，正文是空的（done_reason={}，输出上限 {} 被思考吃光）。\n多半是这个模型没有对话模板，或属于必须关掉思考的思考型模型。\n建议：① 换官方带模板的模型（qwen2.5:7b / qwen3:8b）；② 或提高输出上限。",
            if reason.is_empty() { "?" } else { reason },
            max_tokens
        )
    } else {
        "模型返回了空内容（可能上下文被截断，或模型不适应当前提示格式）".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
/// 2026-11-03：思考型/无模板模型只会往 thinking 里写，正文空 —— 要说清原因。
    #[test]
    fn empty_reply_explains_thinking_model() {
        let msg = describe_empty_reply("length", "Thinking Process: ...", 1200);
        assert!(msg.contains("思考"), "{msg}");
        assert!(msg.contains("1200"), "要带上输出上限：{msg}");
        assert!(msg.contains("对话模板"), "{msg}");
        // 普通空回复（没思考、没超长）用另一句
        let plain = describe_empty_reply("stop", "", 300);
        assert!(plain.contains("空内容"), "{plain}");
        assert!(!plain.contains("思考"), "{plain}");
    }

    fn base_url_is_normalized() {
        assert_eq!(normalize_base_url("").unwrap(), DEFAULT_BASE_URL);
        assert_eq!(normalize_base_url("  http://192.168.1.9:11434/ ").unwrap(), "http://192.168.1.9:11434");
        assert_eq!(normalize_base_url("http://127.0.0.1:11434///").unwrap(), "http://127.0.0.1:11434");
        assert!(normalize_base_url("127.0.0.1:11434").is_err(), "缺协议要报错");
    }

    #[test]
    fn chat_url_appends_openai_path() {
        assert_eq!(chat_url("http://127.0.0.1:11434").unwrap(), "http://127.0.0.1:11434/v1/chat/completions");
    }

    #[test]
    fn ctx_only_accepts_builtin_choices() {
        assert_eq!(normalize_ctx(131_072), 131_072);
        assert_eq!(normalize_ctx(1_048_576), 1_048_576);
        assert_eq!(normalize_ctx(32_768), 32_768, "32k 是合法档位");
        assert_eq!(normalize_ctx(999), 32_768, "非法值落回 32k");
    }
}