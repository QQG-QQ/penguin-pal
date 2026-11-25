use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use uuid::Uuid;
use windows::{
    core::{w, PWSTR},
    Win32::{
        Foundation::ERROR_NOT_FOUND,
        Security::Credentials::{
            CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
            CRED_TYPE_GENERIC,
        },
    },
};

use super::{label, process_names_running, storage, Account, Book, APP};

const KEYRING_LIMIT: usize = 2560;
const ACCOUNT_LIMIT: usize = 100;

fn token(value: &Value) -> Option<&str> {
    value
        .pointer("/token/refresh_token")
        .and_then(Value::as_str)
}

pub(super) fn claims(value: &Value) -> Option<Value> {
    let raw = value["id_token"].as_str()?.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .ok()?;
    let payload: Value = serde_json::from_slice(&bytes).ok()?;
    matches!(
        payload["iss"].as_str(),
        Some("accounts.google.com" | "https://accounts.google.com")
    )
    .then_some(payload)
}

pub(super) fn matches_credential(first: &Value, second: &Value) -> bool {
    token(first).is_some_and(|left| token(second) == Some(left))
}

fn validate_credential(value: Value) -> Result<Value, String> {
    if !token(&value).is_some_and(|value| !value.trim().is_empty() && value.len() < KEYRING_LIMIT)
        || !value
            .pointer("/token/access_token")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        || !matches!(
            value["auth_method"].as_str(),
            None | Some("consumer" | "oauth-personal")
        )
    {
        return Err("文件没有可用于 Antigravity 的 Google 个人账号凭据。".into());
    }
    Ok(value)
}

fn parse_credential(bytes: &[u8]) -> Result<Value, String> {
    let string = std::str::from_utf8(bytes)
        .map_err(|_| "凭据编码无效。")?
        .trim_matches('\0')
        .trim();
    let decoded;
    let content = if let Some(encoded) = string.strip_prefix("go-keyring-base64:") {
        decoded = STANDARD.decode(encoded).map_err(|_| "凭据编码无效。")?;
        decoded.as_slice()
    } else {
        string.as_bytes()
    };
    validate_credential(serde_json::from_slice(content).map_err(|_| "凭据 JSON 格式无效。")?)
}

fn desktop_fallback(user: &Path) -> PathBuf {
    user.join(".gemini/jetski-standalone-oauth-token")
}

fn native_credential() -> Result<Option<Value>, String> {
    let mut handle = std::ptr::null_mut();
    let result = unsafe {
        CredReadW(
            w!("gemini:antigravity"),
            CRED_TYPE_GENERIC,
            None,
            &mut handle,
        )
    };
    if let Err(error) = result {
        return if error.code() == ERROR_NOT_FOUND.to_hresult() {
            Ok(None)
        } else {
            Err("无法读取 Windows 中的 Antigravity 凭据。".into())
        };
    }
    if handle.is_null() {
        return Err("Windows 返回了无效凭据。".into());
    }
    let parsed = unsafe {
        let credential = &*handle;
        let size = credential.CredentialBlobSize as usize;
        let parsed = if size == 0 || size > KEYRING_LIMIT || credential.CredentialBlob.is_null() {
            Err("Windows 返回了无效的 Antigravity 凭据。".into())
        } else {
            parse_credential(std::slice::from_raw_parts(credential.CredentialBlob, size))
        };
        CredFree(handle.cast());
        parsed
    };
    parsed.map(Some)
}

pub(super) fn current_credential(user: &Path) -> Result<Option<Value>, String> {
    if let Some(credential) = native_credential()? {
        return Ok(Some(credential));
    }
    storage::read_config(&desktop_fallback(user))?
        .map(|bytes| parse_credential(&bytes))
        .transpose()
}

fn write_native(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > KEYRING_LIMIT {
        return Err("此账号的凭据超出 Windows 凭据存储容量，请通过官方 App 登录。".into());
    }
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(w!("gemini:antigravity").as_ptr().cast_mut()),
        UserName: PWSTR(w!("antigravity").as_ptr().cast_mut()),
        CredentialBlob: bytes.as_ptr().cast_mut(),
        CredentialBlobSize: bytes.len() as u32,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    unsafe { CredWriteW(&credential, 0) }
        .map_err(|_| "无法写入 Windows 中的 Antigravity 凭据。".into())
}

pub(super) fn save_account(
    book: &mut Book,
    credential: Value,
    label: String,
) -> Result<(), String> {
    if let Some(saved) = book
        .accounts
        .iter_mut()
        .find(|saved| matches_credential(&saved.credential, &credential))
    {
        saved.credential = credential;
        saved.label = label;
    } else {
        if book.accounts.len() >= ACCOUNT_LIMIT {
            return Err("已达到 100 个账号的保存上限。".into());
        }
        book.accounts.push(Account {
            id: Uuid::new_v4(),
            label,
            credential,
        });
    }
    Ok(())
}

pub(super) fn read_import(value: Value) -> Result<Vec<(Value, String)>, String> {
    let values = match value {
        Value::Array(values) => values,
        value => vec![value],
    };
    if values.is_empty() || values.len() > ACCOUNT_LIMIT {
        return Err("一次可导入 1 至 100 个账号。".into());
    }
    values
        .into_iter()
        .enumerate()
        .map(|(index, mut value)| {
            let name = value["email"].as_str().and_then(|name| label(name).ok());
            if let Some(number) = value
                .pointer("/token/expiry_timestamp")
                .and_then(Value::as_i64)
            {
                let expiry =
                    DateTime::<Utc>::from_timestamp(number, 0).ok_or("凭据到期时间无效。")?;
                value = json!({
                    "token": {
                        "access_token": value["token"]["access_token"],
                        "refresh_token": value["token"]["refresh_token"],
                        "token_type": "Bearer", "expiry": expiry.to_rfc3339(),
                    },
                    "auth_method": "consumer", "id_token": value["token"]["id_token"],
                });
            }
            let value = validate_credential(value)?;
            let email = claims(&value)
                .and_then(|value| value["email"].as_str().and_then(|email| label(email).ok()));
            Ok((
                value,
                name.or(email)
                    .unwrap_or_else(|| format!("导入账号 {}", index + 1)),
            ))
        })
        .collect()
}

fn write_credential(
    fallback: PathBuf,
    target: &Value,
    write_system: impl FnOnce(&[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(target).map_err(|_| "无法编码账号凭据。")?;
    if bytes.len() > KEYRING_LIMIT {
        return Err("此账号的凭据超出 Windows 凭据存储容量，请通过官方 App 登录。".into());
    }
    let Some(before) = storage::read_config(&fallback)? else {
        return write_system(&bytes);
    };
    parse_credential(&before)?;
    let change = storage::Change::new(fallback, Some(before), bytes.clone());
    change.apply()?;
    if let Err(error) = write_system(&bytes) {
        change
            .reversed()
            .apply()
            .map_err(|_| "写入系统凭据失败，桌面凭据也未能还原。请在 App 重新登录。")?;
        return Err(error);
    }
    Ok(())
}

pub(super) fn ensure_shared_login_stopped() -> Result<(), String> {
    if process_names_running(&[APP, "Antigravity IDE.exe", "agy.exe", "agy_acp_server.exe"])? {
        return Err("请退出所有 Antigravity App、IDE 和 agy 窗口后再切换共享登录。".into());
    }
    Ok(())
}

pub(super) fn switch_account(
    user: &Path,
    expected: Option<&Value>,
    target: &Value,
) -> Result<(), String> {
    ensure_shared_login_stopped()?;
    if current_credential(user)?.as_ref() != expected {
        return Err("Antigravity 登录已发生变化，请刷新后重试。".into());
    }
    write_credential(desktop_fallback(user), target, write_native)?;
    let current = native_credential()?.ok_or("系统凭据写入后无法读回。")?;
    if !matches_credential(&current, target) {
        return Err("系统凭据已被其他程序修改，请刷新后检查。".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn credential(refresh: &str) -> Value {
        json!({
            "auth_method": "consumer",
            "token": {"access_token": "synthetic", "refresh_token": refresh, "token_type": "Bearer"},
        })
    }

    #[test]
    fn accepts_antigravity_and_exported_accounts() {
        let direct = credential("first");
        let imported = read_import(json!([
            direct,
            {"email": "example@example.com", "token": {
                "access_token": "synthetic", "refresh_token": "second", "expiry_timestamp": 1_900_000_000,
            }},
        ])).unwrap();
        assert_eq!(imported.len(), 2);
        assert_eq!(imported[1].1, "example@example.com");
        assert_eq!(imported[1].0["auth_method"], "consumer");
        assert_eq!(
            imported[1].0["token"]["expiry"],
            "2030-03-17T17:46:40+00:00"
        );
        assert!(read_import(json!([credential("first"), {"token": {}}])).is_err());
    }

    #[test]
    fn credential_switch_restores_existing_fallback_on_failed_keyring_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fallback");
        let original = serde_json::to_vec(&credential("old")).unwrap();
        fs::write(&path, &original).unwrap();
        let target = credential("new");
        let result = write_credential(path.clone(), &target, |bytes| {
            assert_eq!(parse_credential(bytes).unwrap(), target);
            assert_eq!(fs::read(&path).unwrap(), bytes);
            Err("synthetic Windows error".into())
        });
        assert_eq!(result.unwrap_err(), "synthetic Windows error");
        assert_eq!(fs::read(&path).unwrap(), original);
        write_credential(path.clone(), &target, |_| Ok(())).unwrap();
        assert_eq!(parse_credential(&fs::read(path).unwrap()).unwrap(), target);
    }

    #[test]
    fn missing_fallback_stays_absent_and_invalid_fallback_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fallback");
        write_credential(path.clone(), &credential("new"), |_| Ok(())).unwrap();
        assert!(!path.exists());
        fs::write(&path, b"unexpected data").unwrap();
        assert!(write_credential(path.clone(), &credential("new"), |_| {
            panic!("must validate the old store before writing Windows credentials");
        })
        .is_err());
        assert_eq!(fs::read(path).unwrap(), b"unexpected data");
    }
}
