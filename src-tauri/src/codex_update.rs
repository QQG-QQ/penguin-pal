//! Codex 自动更新模块
//!
//! 从 npm registry 检查最新版本并更新本地 Codex 运行时

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

    let latest_version = match fetch_latest_version().await {
        Ok(v) => Some(v),
        Err(e) => {
            return CodexUpdateStatus {
                current_version: current_version.clone(),
                latest_version: None,
                update_available: false,
                install_path: install_dir.map(|p| p.to_string_lossy().to_string()),
                message: format!("无法检查最新版本: {}", e),
            };
        }
    };

    let update_available = match (&current_version, &latest_version) {
        (Some(current), Some(latest)) => {
            // 简单版本比较（假设版本格式为 x.y.z）
            compare_versions(current, latest)
        }
        (None, Some(_)) => true, // 未安装，需要安装
        _ => false,
    };

    let message = if update_available {
        format!(
            "有新版本可用: {} -> {}",
            current_version.as_deref().unwrap_or("未安装"),
            latest_version.as_deref().unwrap_or("未知")
        )
    } else {
        "已是最新版本".to_string()
    };

    CodexUpdateStatus {
        current_version,
        latest_version,
        update_available,
        install_path: install_dir.map(|p| p.to_string_lossy().to_string()),
        message,
    }
}

/// 比较版本号，返回 true 如果 latest > current
fn compare_versions(current: &str, latest: &str) -> bool {
    // 移除前缀 v 或 V（如果有）
    let current = current.trim_start_matches(|c| c == 'v' || c == 'V');
    let latest = latest.trim_start_matches(|c| c == 'v' || c == 'V');

    // 提取版本号部分（处理 "OpenAI Codex (v0.113.0)" 格式）
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
fn extract_version_number(version: &str) -> &str {
    // 处理 "OpenAI Codex (v0.113.0)" 格式
    if let Some(start) = version.find('(') {
        if let Some(end) = version.find(')') {
            let inner = &version[start + 1..end];
            return inner.trim_start_matches(|c| c == 'v' || c == 'V');
        }
    }
    // 处理普通格式
    version.trim()
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
    progress_callback: impl Fn(&str),
) -> Result<String, String> {
    // 确保目录存在
    std::fs::create_dir_all(install_dir)
        .map_err(|e| format!("创建安装目录失败: {}", e))?;

    if !check_npm_available() {
        return Err("npm 不可用。请先安装 Node.js 和 npm。".to_string());
    }

    progress_callback("正在安装/更新 Codex...");

    // 使用 npm install 安装到指定目录
    #[cfg(target_os = "windows")]
    let output = {
        use std::os::windows::process::CommandExt;
        Command::new("cmd")
            .args([
                "/C", "npm", "install",
                CODEX_PACKAGE_NAME,
                "--prefix", &install_dir.to_string_lossy(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .current_dir(install_dir)
            .output()
    };

    #[cfg(not(target_os = "windows"))]
    let output = {
        Command::new("sh")
            .args([
                "-c",
                &format!(
                    "npm install {} --prefix '{}'",
                    CODEX_PACKAGE_NAME,
                    install_dir.to_string_lossy()
                ),
            ])
            .current_dir(install_dir)
            .output()
    };

    match output {
        Ok(out) => {
            if out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                progress_callback("Codex 更新完成");
                Ok(format!("更新成功: {}", stdout.trim()))
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Err(format!("npm install 失败: {}", stderr.trim()))
            }
        }
        Err(e) => Err(format!("执行 npm 命令失败: {}", e)),
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
    }
}
