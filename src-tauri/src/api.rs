use serde_json::json;
use sqlx::SqlitePool;

use crate::db;
use crate::keychain;

#[derive(serde::Serialize)]
pub struct TestResult {
    pub ok: bool,
    pub detail: String,
    pub reply: Option<String>,
}

pub async fn test_connection(pool: &SqlitePool) -> Result<TestResult, String> {
    // 读配置
    let base_url = db::get_config(pool, "model.base_url")
        .await
        .map_err(|e| format!("读 base_url 失败: {e}"))?
        .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
    let model = db::get_config(pool, "model.name")
        .await
        .map_err(|e| format!("读 model.name 失败: {e}"))?
        .unwrap_or_else(|| "deepseek-chat".to_string());

    // 读 Key
    let api_key = keychain::get_api_key("openai_api_key".to_string())
        .map_err(|e| format!("读 API Key 失败: {e}"))?;
    let api_key = match api_key {
        Some(k) if !k.is_empty() => k,
        _ => {
            return Ok(TestResult {
                ok: false,
                detail: "尚未设置 API Key（请到设置里填）".to_string(),
                reply: None,
            });
        }
    };

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": "请只回复两个字：你好"}],
        "max_tokens": 20,
        "stream": false
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读响应失败: {e}"))?;

    if !status.is_success() {
        return Ok(TestResult {
            ok: false,
            detail: format!("HTTP {}: {}", status.as_u16(), &text.chars().take(300).collect::<String>()),
            reply: None,
        });
    }

    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("解析 JSON 失败: {e}; body={}", text.chars().take(300).collect::<String>()))?;
    let reply = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok(TestResult {
        ok: true,
        detail: format!("HTTP 200，model={model}"),
        reply: Some(reply),
    })
}
