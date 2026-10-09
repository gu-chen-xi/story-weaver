use keyring::Entry;

/// 系统钥匙串服务名。Windows 上落到凭据管理器（Credential Manager），
/// 绝不写入项目目录。每个账号（provider）一条。
const SERVICE: &str = "ai-story-studio";

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| e.to_string())
}

/// 保存 API Key 到系统钥匙串。
#[tauri::command]
pub fn set_api_key(account: String, key: String) -> Result<(), String> {
    let e = entry(&account)?;
    e.set_password(&key).map_err(|e| e.to_string())
}

/// 读取 API Key；不存在返回 None。
#[tauri::command]
pub fn get_api_key(account: String) -> Result<Option<String>, String> {
    let e = entry(&account)?;
    match e.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// 删除 API Key；不存在视为成功。
#[tauri::command]
pub fn delete_api_key(account: String) -> Result<(), String> {
    let e = entry(&account)?;
    match e.delete_password() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
