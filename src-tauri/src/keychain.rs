use keyring::Entry;

/// 系统钥匙串服务名。Windows 上落到凭据管理器（Credential Manager），
/// 绝不写入项目目录。每个账号（provider）一条。
const SERVICE: &str = "ai-story-studio";

// 2026-11-28：统一打一行 stderr 诊断（account 名可以打，**key 绝不打印**）。
// 为什么需要：dev 模式下界面只弹一句 toast，出错时看不出是"没保存"还是"读失败"；
// 这行日志会同时进控制台和日志文件（tauri-plugin-log 会接管 stderr），排查时不用猜。
fn note(action: &str, account: &str, ok: bool, detail: &str) {
    let mark = if ok { "OK" } else { "FAIL" };
    if detail.is_empty() {
        eprintln!("[keyring] {action} {mark} · account={account}");
    } else {
        eprintln!("[keyring] {action} {mark} · account={account} · {detail}");
    }
}

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| {
        note("entry", account, false, &e.to_string());
        e.to_string()
    })
}

/// 保存 API Key 到系统钥匙串。
#[tauri::command]
pub fn set_api_key(account: String, key: String) -> Result<(), String> {
    let e = entry(&account)?;
    match e.set_password(&key) {
        Ok(()) => {
            note("save", &account, true, "");
            Ok(())
        }
        Err(e) => {
            note("save", &account, false, &e.to_string());
            Err(e.to_string())
        }
    }
}

/// 读取 API Key；不存在返回 None。
#[tauri::command]
pub fn get_api_key(account: String) -> Result<Option<String>, String> {
    let e = entry(&account)?;
    match e.get_password() {
        Ok(p) => {
            note("load", &account, true, "");
            Ok(Some(p))
        }
        Err(keyring::Error::NoEntry) => {
            note("load", &account, true, "未保存（NoEntry）");
            Ok(None)
        }
        Err(e) => {
            note("load", &account, false, &e.to_string());
            Err(e.to_string())
        }
    }
}

/// 删除 API Key；不存在视为成功。
#[tauri::command]
pub fn delete_api_key(account: String) -> Result<(), String> {
    let e = entry(&account)?;
    match e.delete_credential() {
        Ok(_) => {
            note("delete", &account, true, "");
            Ok(())
        }
        Err(keyring::Error::NoEntry) => {
            note("delete", &account, true, "本来就没有（NoEntry）");
            Ok(())
        }
        Err(e) => {
            note("delete", &account, false, &e.to_string());
            Err(e.to_string())
        }
    }
}
