//! Codex 自动更新模块
//!
//! 从 npm registry 和 GitHub Releases 检查最新版本并更新本地 Codex 运行时。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const CODEX_PACKAGE_NAME: &str = "@openai/codex";
const NPM_REGISTRY_URL: &str = "https://registry.npmjs.org/@openai%2Fcodex";
const CURRENT_APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const BOOTSTRAP_RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/QQG-QQ/codex-embedded-bootstrap/releases/latest";
const BOOTSTRAP_RELEASES_PAGE_URL: &str =
    "https://github.com/QQG-QQ/codex-embedded-bootstrap/releases/latest";
const BOOTSTRAP_MARKER_FILE: &str = ".penguinpal-bootstrap-release.json";

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

#[derive(Debug, Clone, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct BootstrapReleaseMarker {
    tag_name: String,
    asset_name: String,
}

#[derive(Debug, Clone)]
struct BootstrapReleaseStatus {
    current_version: Option<String>,
    latest_version: Option<String>,
    update_available: bool,
    asset: Option<GithubAsset>,
    release_url: Option<String>,
}

#[cfg(target_os = "windows")]
fn platform_dir() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "windows-arm64"
    } else {
        "windows-x64"
    }
}

#[cfg(not(target_os = "windows"))]
fn platform_dir() -> &'static str {
    "unix"
}

/// 获取应用管理的 Codex 安装目录
pub fn managed_install_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("获取本地数据目录失败: {e}"))?;

    Ok(local_data.join("codex").join(platform_dir()))
}

fn bootstrap_install_dir(app: &AppHandle) -> Result<PathBuf, String> {
    managed_install_dir(app)
}

fn bootstrap_staging_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let local_data = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("获取本地数据目录失败: {e}"))?;

    Ok(local_data
        .join("codex")
        .join(format!("{}.bootstrap-staging", platform_dir())))
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
        let direct_candidate = ancestor.join("@openai").join("codex").join("package.json");
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

fn bootstrap_marker_path(install_dir: &Path) -> PathBuf {
    install_dir.join(BOOTSTRAP_MARKER_FILE)
}

fn runtime_command_path(install_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    let command = "codex.cmd";
    #[cfg(not(target_os = "windows"))]
    let command = "codex";

    install_dir.join("node_modules").join(".bin").join(command)
}

fn runtime_node_path(install_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    let command = "node.exe";
    #[cfg(not(target_os = "windows"))]
    let command = "node";

    install_dir.join("node_modules").join(".bin").join(command)
}

fn looks_like_runtime_root(path: &Path) -> bool {
    runtime_command_path(path).is_file() && runtime_node_path(path).is_file()
}

fn read_bootstrap_marker(install_dir: &Path) -> Option<BootstrapReleaseMarker> {
    let content = fs::read_to_string(bootstrap_marker_path(install_dir)).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_bootstrap_marker(
    install_dir: &Path,
    marker: &BootstrapReleaseMarker,
) -> Result<(), String> {
    let content = serde_json::to_string_pretty(marker).map_err(|e| e.to_string())?;
    fs::write(bootstrap_marker_path(install_dir), content).map_err(|e| e.to_string())
}

fn build_http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("PenguinPal Assistant/{CURRENT_APP_VERSION}"))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))
}

fn normalize_version(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

fn bootstrap_asset_score(asset: &GithubAsset) -> Option<i32> {
    let name = asset.name.to_ascii_lowercase();
    if !name.ends_with(".zip") {
        return None;
    }

    #[cfg(target_os = "windows")]
    let platform_match = if cfg!(target_arch = "aarch64") {
        name.contains("windows-arm64")
    } else {
        name.contains("windows-x64")
    };

    #[cfg(not(target_os = "windows"))]
    let platform_match = name.contains("unix");

    if !platform_match {
        return None;
    }

    let mut score = 0;
    if name.contains("codex") {
        score += 10;
    }
    if name.contains("bootstrap") {
        score += 20;
    }
    Some(score)
}

fn select_bootstrap_asset(assets: &[GithubAsset]) -> Option<GithubAsset> {
    assets
        .iter()
        .filter_map(|asset| bootstrap_asset_score(asset).map(|score| (score, asset.clone())))
        .max_by_key(|(score, _)| *score)
        .map(|(_, asset)| asset)
}

async fn fetch_latest_bootstrap_release() -> Result<GithubRelease, String> {
    let client = build_http_client()?;
    let response = client
        .get(BOOTSTRAP_RELEASES_LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("获取 bootstrap release 失败: {e}"))?;

    let response = response
        .error_for_status()
        .map_err(|e| format!("bootstrap release 接口异常: {e}"))?;

    response
        .json::<GithubRelease>()
        .await
        .map_err(|e| format!("解析 bootstrap release 失败: {e}"))
}

fn describe_bootstrap_status(
    app: &AppHandle,
    release: &GithubRelease,
) -> Result<BootstrapReleaseStatus, String> {
    let install_dir = bootstrap_install_dir(app)?;
    let marker = read_bootstrap_marker(&install_dir);
    let latest_version = normalize_version(&release.tag_name);
    let current_version = marker
        .as_ref()
        .map(|item| normalize_version(&item.tag_name))
        .filter(|value| !value.trim().is_empty());
    let asset = select_bootstrap_asset(&release.assets);
    let runtime_present = looks_like_runtime_root(&install_dir);
    let update_available =
        asset.is_some() && (!runtime_present || current_version.as_deref() != Some(latest_version.as_str()));

    Ok(BootstrapReleaseStatus {
        current_version,
        latest_version: Some(latest_version),
        update_available,
        asset,
        release_url: release
            .html_url
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| Some(BOOTSTRAP_RELEASES_PAGE_URL.to_string())),
    })
}

fn remove_dir_if_exists(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|e| format!("删除目录失败 {path:?}: {e}"))?;
    }
    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|e| format!("创建目录失败 {destination:?}: {e}"))?;

    for entry in fs::read_dir(source).map_err(|e| format!("读取目录失败 {source:?}: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(&source_path, &destination_path)
                .map_err(|e| format!("复制文件失败 {source_path:?}: {e}"))?;
        }
    }

    Ok(())
}

fn find_runtime_root(base: &Path, depth: usize) -> Option<PathBuf> {
    if looks_like_runtime_root(base) {
        return Some(base.to_path_buf());
    }

    if depth == 0 {
        return None;
    }

    let entries = fs::read_dir(base).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_runtime_root(&path, depth - 1) {
                return Some(found);
            }
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn escape_powershell_literal(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

#[cfg(target_os = "windows")]
fn expand_bootstrap_archive(zip_path: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    let command = format!(
        "$ErrorActionPreference='Stop'; Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
        escape_powershell_literal(zip_path),
        escape_powershell_literal(destination)
    );
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &command,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("执行 Expand-Archive 失败: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        Err(format!("Expand-Archive 失败: {detail}"))
    }
}

#[cfg(not(target_os = "windows"))]
fn expand_bootstrap_archive(_zip_path: &Path, _destination: &Path) -> Result<(), String> {
    Err("Bootstrap 自动更新目前只支持 Windows。".to_string())
}

pub async fn ensure_bootstrap_runtime(
    app: &AppHandle,
    force: bool,
    progress_callback: impl Fn(&str),
) -> Result<Option<String>, String> {
    let release = fetch_latest_bootstrap_release().await?;
    let status = describe_bootstrap_status(app, &release)?;

    if !force && !status.update_available {
        return Ok(None);
    }

    let asset = status.asset.ok_or_else(|| {
        format!(
            "未找到适用于当前平台的 bootstrap release 安装包。{:?}",
            status.release_url
        )
    })?;

    let install_dir = bootstrap_install_dir(app)?;
    let staging_dir = bootstrap_staging_dir(app)?;
    let archive_path = staging_dir.join("bootstrap.zip");
    let extract_dir = staging_dir.join("extract");

    remove_dir_if_exists(&staging_dir)?;
    fs::create_dir_all(&staging_dir).map_err(|e| e.to_string())?;

    progress_callback(&format!("正在下载 Codex bootstrap ({})...", asset.name));
    let client = build_http_client()?;
    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|e| format!("下载 bootstrap 安装包失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("bootstrap 安装包下载接口异常: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("读取 bootstrap 安装包失败: {e}"))?;
    fs::write(&archive_path, &bytes).map_err(|e| format!("写入 bootstrap 安装包失败: {e}"))?;

    progress_callback("正在解压 Codex bootstrap...");
    expand_bootstrap_archive(&archive_path, &extract_dir)?;

    let runtime_root = find_runtime_root(&extract_dir, 4).ok_or_else(|| {
        "解压后没有找到可用的 Codex runtime 目录，发布包需要包含 node_modules/.bin/codex.cmd 和 node_modules/.bin/node.exe。"
            .to_string()
    })?;

    remove_dir_if_exists(&install_dir)?;
    progress_callback("正在安装 Codex bootstrap...");
    copy_dir_recursive(&runtime_root, &install_dir)?;
    write_bootstrap_marker(
        &install_dir,
        &BootstrapReleaseMarker {
            tag_name: normalize_version(&release.tag_name),
            asset_name: asset.name.clone(),
        },
    )?;
    let _ = remove_dir_if_exists(&staging_dir);

    Ok(Some(format!(
        "Codex bootstrap 已更新到 {} ({})",
        normalize_version(&release.tag_name),
        asset.name
    )))
}

/// 从 npm registry 获取最新版本
pub async fn fetch_latest_version() -> Result<String, String> {
    let client = build_http_client()?;
    let response = client
        .get(NPM_REGISTRY_URL)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("获取 npm 包信息失败: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("npm registry 返回错误: {}", response.status()));
    }

    let info: NpmPackageInfo = response
        .json()
        .await
        .map_err(|e| format!("解析 npm 响应失败: {e}"))?;

    Ok(info.dist_tags.latest)
}

/// 检查更新状态
pub async fn check_update_status(
    app: &AppHandle,
    current_version: Option<String>,
) -> CodexUpdateStatus {
    let install_dir = managed_install_dir(app).ok();
    let installed_private_version = install_dir
        .as_ref()
        .and_then(|path| get_installed_package_version(path));

    let latest_version_result = fetch_latest_version().await;
    let latest_version = latest_version_result.as_ref().ok().cloned();

    let effective_current_version = installed_private_version.clone().or(current_version);
    let npm_update_available = match (effective_current_version.as_deref(), latest_version.as_deref()) {
        (None, Some(_)) => true,
        (Some(current), Some(latest)) => compare_versions(current, latest),
        _ => false,
    };

    let bootstrap_status = match fetch_latest_bootstrap_release().await {
        Ok(release) => describe_bootstrap_status(app, &release).ok(),
        Err(_) => None,
    };
    let bootstrap_update_available = bootstrap_status
        .as_ref()
        .map(|status| status.update_available)
        .unwrap_or(false);

    let mut message_parts = Vec::new();

    if installed_private_version.is_none() {
        let target_version = latest_version
            .as_deref()
            .or_else(|| {
                bootstrap_status
                    .as_ref()
                    .and_then(|status| status.latest_version.as_deref())
            })
            .unwrap_or("latest");
        message_parts.push(format!(
            "桌宠私有 Codex 运行时未安装，将安装 {target_version}。"
        ));
    } else if npm_update_available {
        message_parts.push(format!(
            "有新版本可用: {} -> {}",
            effective_current_version.as_deref().unwrap_or("未安装"),
            latest_version.as_deref().unwrap_or("未知")
        ));
    } else if !bootstrap_update_available {
        message_parts.push("桌宠私有 Codex 运行时已是最新版本。".to_string());
    }

    if let Err(error) = latest_version_result {
        message_parts.push(format!("npm 版本检查失败: {error}"));
    }

    if bootstrap_update_available {
        if let Some(status) = &bootstrap_status {
            message_parts.push(format!(
                "Bootstrap 有新版本: {} -> {}",
                status.current_version.as_deref().unwrap_or("未安装"),
                status.latest_version.as_deref().unwrap_or("latest")
            ));
        }
    }

    if message_parts.is_empty() {
        message_parts.push("桌宠私有 Codex 运行时状态已检查。".to_string());
    }

    CodexUpdateStatus {
        current_version: effective_current_version,
        latest_version,
        update_available: npm_update_available || bootstrap_update_available,
        install_path: install_dir.map(|p| p.to_string_lossy().to_string()),
        message: message_parts.join(" "),
    }
}

/// 比较版本号，返回 true 如果 latest > current
fn compare_versions(current: &str, latest: &str) -> bool {
    let current = extract_version_number(current);
    let latest = extract_version_number(latest);

    let current_parts: Vec<u32> = current.split('.').filter_map(|s| s.parse().ok()).collect();
    let latest_parts: Vec<u32> = latest.split('.').filter_map(|s| s.parse().ok()).collect();

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
pub fn npm_available() -> bool {
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
    fs::create_dir_all(install_dir).map_err(|e| format!("创建安装目录失败: {e}"))?;

    if !npm_available() {
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
                let installed_version =
                    get_installed_package_version(install_dir).unwrap_or_else(|| "未知版本".to_string());
                let completion_message = format!("Codex 更新完成: {installed_version}");
                progress_callback(&completion_message);
                Ok(completion_message)
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Err(format!("npm install 失败: {}", stderr.trim()))
            }
        }
        Err(e) => Err(format!("执行 npm 命令失败: {e}")),
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

    #[test]
    fn test_select_bootstrap_asset_prefers_platform_zip() {
        #[cfg(target_os = "windows")]
        let expected_name = if cfg!(target_arch = "aarch64") {
            "codex-embedded-bootstrap-windows-arm64.zip"
        } else {
            "codex-embedded-bootstrap-windows-x64.zip"
        };

        #[cfg(not(target_os = "windows"))]
        let expected_name = "codex-embedded-bootstrap-unix.zip";

        let assets = vec![
            GithubAsset {
                name: "runtime-fallback.zip".to_string(),
                browser_download_url: "https://example.com/runtime-fallback.zip".to_string(),
            },
            GithubAsset {
                name: expected_name.to_string(),
                browser_download_url: "https://example.com/platform.zip".to_string(),
            },
            GithubAsset {
                name: expected_name.replace(".zip", ".tar.gz"),
                browser_download_url: "https://example.com/platform.tar.gz".to_string(),
            },
        ];

        let selected = select_bootstrap_asset(&assets).expect("expected matching bootstrap asset");
        assert_eq!(selected.name, expected_name);
    }
}
