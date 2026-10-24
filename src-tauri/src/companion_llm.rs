//! 气泡陪伴的模型来源配置（Codex CLI 或 DeepSeek API）。
//!
//! 配置只保存在本机 companion 目录的 llm.json，不进入同步文档，
//! 避免 API Key 被跨端同步。
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, path::Path};

const MAX_KEY: usize = 400;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmConfig {
    #[serde(default = "tool_provider")]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "tool_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
}

fn tool_provider() -> String {
    "codex".to_string()
}

fn tool_base_url() -> String {
    "https://api.deepseek.com/v1".to_string()
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: tool_provider(),
            model: String::new(),
            base_url: tool_base_url(),
            api_key: String::new(),
        }
    }
}

impl LlmConfig {
    pub fn normalized(mut self) -> Self {
        self.provider = if self.provider.trim().eq_ignore_ascii_case("deepseek") {
            "deepseek".to_string()
        } else {
            "codex".to_string()
        };
        self.model = self.model.trim().chars().take(100).collect();
        self.base_url = self
            .base_url
            .trim()
            .trim_end_matches('/')
            .chars()
            .take(300)
            .collect();
        self.api_key = self.api_key.trim().chars().take(MAX_KEY).collect();
        if self.base_url.is_empty() {
            self.base_url = tool_base_url();
        }
        if self.model.is_empty() && self.provider == "deepseek" {
            self.model = "deepseek-chat".to_string();
        }
        self
    }

    pub fn is_deepseek(&self) -> bool {
        self.provider == "deepseek"
    }
}

pub fn path(root: &Path) -> std::path::PathBuf {
    root.join("llm.json")
}

pub fn load(root: &Path) -> LlmConfig {
    let mut config = fs::read(path(root))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<LlmConfig>(&bytes).ok())
        .unwrap_or_default();
    config.api_key = unseal(&config.api_key);
    config.normalized()
}

pub fn save(root: &Path, config: &LlmConfig) -> Result<LlmConfig, String> {
    let normalized = config.clone().normalized();
    let mut stored = normalized.clone();
    stored.api_key = seal(&normalized.api_key);
    fs::create_dir_all(root).map_err(|_| "无法建立设置目录。")?;
    let bytes = serde_json::to_vec_pretty(&stored).map_err(|_| "设置无法编码。")?;
    let target = path(root);
    let temporary = root.join("llm.json.tmp");
    fs::write(&temporary, &bytes).map_err(|_| "无法保存设置。")?;
    fs::rename(&temporary, &target).map_err(|_| "无法保存设置。")?;
    restrict_to_user(&target);
    Ok(normalized)
}

/// 把旧的明文 Key 迁移成 DPAPI 封装；在 companion 启动时调用一次。
pub fn migrate(root: &Path) {
    let Ok(bytes) = fs::read(path(root)) else { return };
    let Ok(config) = serde_json::from_slice::<LlmConfig>(&bytes) else { return };
    if config.api_key.is_empty() || config.api_key.starts_with(SEALED) {
        return;
    }
    let _ = save(root, &config);
}
const SEALED: &str = "dpapi:";

/// 用当前 Windows 用户的 DPAPI 加密 API Key；失败时退回明文，保证功能可用。
fn seal(key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    match dpapi::protect(key.as_bytes()) {
        Some(bytes) => format!("{}{}", SEALED, base64::engine::general_purpose::STANDARD.encode(bytes)),
        None => key.to_string(),
    }
}

fn unseal(stored: &str) -> String {
    match stored.strip_prefix(SEALED) {
        Some(encoded) => base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .ok()
            .and_then(|bytes| dpapi::unprotect(&bytes))
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default(),
        None => stored.to_string(),
    }
}

#[cfg(windows)]
mod dpapi {
    use base64::Engine as _;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
    };

    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        }
    }

    fn take(output: CRYPT_INTEGER_BLOB) -> Option<Vec<u8>> {
        if output.pbData.is_null() || output.cbData == 0 {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe { LocalFree(Some(HLOCAL(output.pbData as *mut core::ffi::c_void))) };
        Some(bytes)
    }

    pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
        let input = blob(plain);
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).ok()?;
        }
        take(output)
    }

    pub fn unprotect(sealed: &[u8]) -> Option<Vec<u8>> {
        let input = blob(sealed);
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).ok()?;
        }
        take(output)
    }

    #[allow(dead_code)]
    fn _assert_base64() {
        let _ = base64::engine::general_purpose::STANDARD.encode(b"");
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn protect(_plain: &[u8]) -> Option<Vec<u8>> {
        None
    }
    pub fn unprotect(_sealed: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

/// 收紧配置文件的 ACL：去掉继承权限，只保留当前用户完全控制。
/// Key 仍是明文，但至少不再对同机其他账户可读；DPAPI 加密留待后续。
#[cfg(windows)]
fn restrict_to_user(path: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Some(user) = std::env::var_os("USERNAME") else {
        return;
    };
    let grant = format!("{}:F", user.to_string_lossy());
    let _ = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(grant)
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(not(windows))]
fn restrict_to_user(_path: &Path) {}

pub fn bridge_value(config: &LlmConfig) -> Value {
    json!({
        "kind": if config.is_deepseek() { "deepSeek" } else { "codex" },
        "model": config.model,
        "baseUrl": config.base_url,
        "apiKey": config.api_key,
    })
}

pub fn public_view(config: &LlmConfig) -> Value {
    json!({
        "provider": config.provider,
        "model": config.model,
        "baseUrl": config.base_url,
        "hasKey": !config.api_key.is_empty(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn dpapi_roundtrip() {
        let sealed = super::seal("sk-test-123");
        assert!(sealed.starts_with(super::SEALED), "should be sealed: {sealed}");
        assert_eq!(super::unseal(&sealed), "sk-test-123");
        assert_eq!(super::unseal("plain-key"), "plain-key");
        assert_eq!(super::seal(""), "");
    }
}