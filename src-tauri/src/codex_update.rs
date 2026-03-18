//! Codex 自动更新模块
//!
//! 从 npm registry 检查最新版本并更新本地 Codex 运行时

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const CODEX_PACKAGE_NAME: &str = "@openai/codex";
const NPM_REGISTRY_URL: &str = "https://registry.npmjs.org/@openai%2Fcodex";

/// Codex 更新状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexUpdateStatus {
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub install_path: Option<String>,
    pub message: String,
}

/// npm registry 响应（简化）
#[derive(Debug, Deserialize)]
struct NpmPackageInfo {
    #[serde(rename = "dist-tags")]
    dist_tags: DistTags,
}

#[derive(Debug, Deserialize)]
struct DistTags {
    latest: String,
}

#[derive(Debug, Deserialize)]
struct InstalledPackageInfo {
    version: String,
}

/// 获取本地 Codex 安装目录
fn get_local_install_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("获取本地数据目录失败: {}", e))?;

    #[cfg(target_os = "windows")]
    let platform_dir = if cfg!(target_arch = "aarch64") {
        "windows-arm64"
    } else {
        "windows-x64"
    };

    #[cfg(not(target_os = "windows"))]
    let platform_dir = "unix";

    Ok(local_data.join("codex").join(platform_dir))
}

pub fn get_installed_package_version(install_dir: &Path) -> Option<String> {
    let package_json = install_dir
        .join("node_modules")
        .join("@openai")
        .join("codex")
        .join("package.json");
    let content = fs::read_to_string(package_json).ok()?;
    let package: InstalledPackageInfo = serde_json::from_str(&content).ok()?;
    Some(package.version)
}

pub fn get_runtime_command_package_version(command_path: &Path) -> Option<String> {
    for ancestor in command_path.ancestors() {
        let direct_candidate = ancestor
            .join("@openai")
            .join("codex")
            .join("package.json");
        if let Ok(content) = fs::read_to_string(&direct_candidate) {
            if let Ok(package) = serde_json::from_str::<InstalledPackageInfo>(&content) {
                return Some(package.version);
            }
        }

        let nested_candidate = ancestor
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("package.json");
        if let Ok(content) = fs::read_to_string(&nested_candidate) {
            if let Ok(package) = serde_json::from_str::<InstalledPackageInfo>(&content) {
                return Some(package.version);
            }
        }
    }

    None
}

/// 从 npm registry 获取最新版本
pub async fn fetch_latest_version() -> Result<String, String> {
    let client = reqwest::Client::new();
    let response = client
        .get(NPM_REGISTRY_URL)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("获取 npm 包信息失败: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("npm registry 返回错误: {}", response.status()));
    }

    let info: NpmPackageInfo = response
        .json()
        .await
        .map_err(|e| format!("解析 npm 响应失败: {}", e))?;

    Ok(info.dist_tags.latest)
}

/// 检查更新状态
pub async fn check_update_status(app: &AppHandle, current_version: Option<String>) -> CodexUpdateStatus {
    let install_dir = get_local_install_dir(app).ok();
    let installed_private_version = install_dir
        .as_ref()
        .and_then(|path| get_installed_package_version(path));

    let latest_version = match fetch_latest_version().await {
        Ok(v) => Some(v),
        Err(e) => {
            return CodexUpdateStatus {
                current_version: installed_private_version.or(current_version.clone()),
                latest_version: None,
                update_available: false,
                install_path: install_dir.map(|p| p.to_string_lossy().to_string()),
                message: format!("无法检查最新版本: {}", e),
            };
        }
    };

    let effective_current_version = installed_private_version.clone().or(current_version.clone());
    let update_available = match (installed_private_version.as_deref(), latest_version.as_deref()) {
        (None, Some(_)) => true,
        (Some(current), Some(latest)) => {
            compare_versions(current, latest)
        }
        _ => false,
    };

    let message = if installed_private_version.is_none() {
        format!(
            "桌宠私有 Codex 运行时未安装，将安装 {}。",
            latest_version.as_deref().unwrap_or("最新版本")
        )
    } else if update_available {
        format!(
            "有新版本可用: {} -> {}",
            effective_current_version.as_deref().unwrap_or("未安装"),
            latest_version.as_deref().unwrap_or("未知")
        )
    } else {
        "桌宠私有 Codex 运行时已是最新版本".to_string()
    };

    CodexUpdateStatus {
        current_version: effective_current_version,
        latest_version,
        update_available,
        install_path: install_dir.map(|p| p.to_string_lossy().to_string()),
        message,
    }
}

/// 比较版本号，返回 true 如果 latest > current
fn compare_versions(current: &str, latest: &str) -> bool {
    let current = extract_version_number(current);
    let latest = extract_version_number(latest);

    let current_parts: Vec<u32> = current
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let latest_parts: Vec<u32> = latest
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    for i in 0..3 {
        let c = current_parts.get(i).copied().unwrap_or(0);
        let l = latest_parts.get(i).copied().unwrap_or(0);
        if l > c {
            return true;
        }
        if l < c {
            return false;
        }
    }
    false
}

/// 从版本字符串提取版本号
fn extract_version_number(version: &str) -> String {
    let trimmed = version.trim();
    let mut extracted = String::new();
    let mut started = false;

    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            started = true;
            extracted.push(ch);
            continue;
        }

        if started && ch == '.' {
            extracted.push(ch);
            continue;
        }

        if started {
            break;
        }
    }

    let normalized = extracted.trim_matches('.').to_string();
    if !normalized.is_empty() {
        normalized
    } else {
        trimmed.trim_start_matches(|c| c == 'v' || c == 'V').to_string()
    }
}

/// 检查 npm 是否可用
fn check_npm_available() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        Command::new("cmd")
            .args(["/C", "npm", "--version"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh")
            .args(["-c", "npm --version"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// 执行 Codex 更新
pub fn install_or_update_codex(
    install_dir: &Path,
    target_version: Option<&str>,
    progress_callback: impl Fn(&str),
) -> Result<String, String> {
    std::fs::create_dir_all(install_dir)
        .map_err(|e| format!("创建安装目录失败: {}", e))?;

    if !check_npm_available() {
        return Err("npm 不可用。请先安装 Node.js 和 npm。".to_string());
    }

    let package_spec = build_package_spec(target_version);
    let progress_message = format!("正在安装/更新 Codex ({package_spec})...");
    progress_callback(&progress_message);

    #[cfg(target_os = "windows")]
    let output = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("cmd");
        command
            .arg("/C")
            .arg("npm")
            .arg("install")
            .arg(&package_spec)
            .arg("--prefix")
            .arg(install_dir)
            .arg("--save-exact")
            .arg("--no-fund")
            .arg("--no-audit")
            .creation_flags(CREATE_NO_WINDOW)
            .current_dir(install_dir);
        command.output()
    };

    #[cfg(not(target_os = "windows"))]
    let output = {
        let mut command = Command::new("npm");
        command
            .arg("install")
            .arg(&package_spec)
            .arg("--prefix")
            .arg(install_dir)
            .arg("--save-exact")
            .arg("--no-fund")
            .arg("--no-audit")
            .current_dir(install_dir);
        command.output()
    };

    match output {
        Ok(out) => {
            if out.status.success() {
                let installed_version = get_installed_package_version(install_dir)
                    .unwrap_or_else(|| "未知版本".to_string());
                let completion_message = format!("Codex 更新完成: {installed_version}");
                progress_callback(&completion_message);
                Ok(completion_message)
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Err(format!("npm install 失败: {}", stderr.trim()))
            }
        }
        Err(e) => Err(format!("执行 npm 命令失败: {}", e)),
    }
}

fn build_package_spec(target_version: Option<&str>) -> String {
    match target_version.map(str::trim).filter(|value| !value.is_empty()) {
        Some(version) => format!("{CODEX_PACKAGE_NAME}@{version}"),
        None => CODEX_PACKAGE_NAME.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_versions() {
        assert!(compare_versions("0.113.0", "0.114.0"));
        assert!(compare_versions("0.113.0", "1.0.0"));
        assert!(!compare_versions("0.114.0", "0.113.0"));
        assert!(!compare_versions("0.113.0", "0.113.0"));
    }

    #[test]
    fn test_extract_version_number() {
        assert_eq!(extract_version_number("0.113.0"), "0.113.0");
        assert_eq!(extract_version_number("v0.113.0"), "0.113.0");
        assert_eq!(extract_version_number("OpenAI Codex (v0.113.0)"), "0.113.0");
        assert_eq!(extract_version_number("OpenAI Codex 0.113.0"), "0.113.0");
        assert_eq!(extract_version_number("codex-cli version 1.2.3-dev"), "1.2.3");
    }

    #[test]
    fn test_build_package_spec() {
        assert_eq!(build_package_spec(None), "@openai/codex");
        assert_eq!(build_package_spec(Some("0.114.0")), "@openai/codex@0.114.0");
    }
}
