use std::{cmp::Ordering, time::Duration};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::security::oauth;

const CURRENT_APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/QQG-QQ/penguin-pal-releases/releases/latest";
const RELEASES_PAGE_URL: &str = "https://github.com/QQG-QQ/penguin-pal-releases/releases/latest";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateStatus {
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub release_url: Option<String>,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: Option<String>,
    #[allow(dead_code)]
    draft: bool,
    #[allow(dead_code)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn check_update_status() -> AppUpdateStatus {
    let mut status = AppUpdateStatus {
        current_version: Some(CURRENT_APP_VERSION.to_string()),
        latest_version: None,
        update_available: false,
        release_url: Some(RELEASES_PAGE_URL.to_string()),
        download_url: None,
        asset_name: None,
        message: "尚未检查软件更新。".to_string(),
    };

    let client = match Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(format!("PenguinPal Assistant/{CURRENT_APP_VERSION}"))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            status.message = format!("创建软件更新检查客户端失败：{error}");
            return status;
        }
    };

    let response = match client
        .get(RELEASES_LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            status.message = format!("检查软件更新失败：{error}");
            return status;
        }
    };

    let response = match response.error_for_status() {
        Ok(response) => response,
        Err(error) => {
            status.message = format!("软件更新接口返回异常：{error}");
            return status;
        }
    };

    let release = match response.json::<GithubRelease>().await {
        Ok(release) => release,
        Err(error) => {
            status.message = format!("解析软件更新信息失败：{error}");
            return status;
        }
    };

    let latest_version = normalize_version(&release.tag_name);
    let release_url = release
        .html_url
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| Some(RELEASES_PAGE_URL.to_string()));
    let asset = select_download_asset(&release.assets);

    status.latest_version = Some(latest_version.clone());
    status.release_url = release_url.clone();
    status.download_url = asset.as_ref().map(|item| item.browser_download_url.clone());
    status.asset_name = asset.as_ref().map(|item| item.name.clone());

    match compare_versions(CURRENT_APP_VERSION, &latest_version) {
        Ordering::Less => {
            status.update_available = true;
            status.message = if let Some(asset) = asset {
                format!(
                    "发现新版本 {latest_version}，推荐安装包：{}。",
                    asset.name
                )
            } else {
                format!("发现新版本 {latest_version}，请前往发布页下载。")
            };
        }
        Ordering::Equal | Ordering::Greater => {
            status.message = format!("当前已是最新版本：{CURRENT_APP_VERSION}。");
        }
    }

    status
}

pub async fn open_update_download() -> Result<AppUpdateStatus, String> {
    let status = check_update_status().await;
    let target_url = status
        .download_url
        .as_deref()
        .or(status.release_url.as_deref())
        .ok_or_else(|| "当前没有可用的软件更新下载地址。".to_string())?;

    oauth::open_authorization_in_browser(target_url)?;
    Ok(status)
}

fn normalize_version(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

fn compare_versions(current: &str, latest: &str) -> Ordering {
    let current_parts = version_parts(current);
    let latest_parts = version_parts(latest);
    let len = current_parts.len().max(latest_parts.len());

    for index in 0..len {
        let left = *current_parts.get(index).unwrap_or(&0);
        let right = *latest_parts.get(index).unwrap_or(&0);
        match left.cmp(&right) {
            Ordering::Equal => continue,
            non_equal => return non_equal,
        }
    }

    Ordering::Equal
}

fn version_parts(raw: &str) -> Vec<u64> {
    normalize_version(raw)
        .split(['.', '-'])
        .filter_map(|part| {
            let digits: String = part.chars().take_while(|char| char.is_ascii_digit()).collect();
            if digits.is_empty() {
                None
            } else {
                digits.parse::<u64>().ok()
            }
        })
        .collect()
}

fn select_download_asset(assets: &[GithubAsset]) -> Option<&GithubAsset> {
    assets
        .iter()
        .filter_map(|asset| asset_score(asset).map(|score| (score, asset)))
        .max_by_key(|(score, _)| *score)
        .map(|(_, asset)| asset)
}

fn asset_score(asset: &GithubAsset) -> Option<i32> {
    let name = asset.name.to_ascii_lowercase();

    #[cfg(target_os = "windows")]
    {
        if !(name.ends_with(".exe") || name.ends_with(".msi")) {
            return None;
        }

        let mut score = 0;
        if matches!(std::env::consts::ARCH, "x86_64") {
            if name.contains("x64") {
                score += 40;
            }
            if name.contains("arm64") {
                score -= 20;
            }
        }

        if name.ends_with("-setup.exe") || name.contains("setup.exe") {
            score += 30;
        } else if name.ends_with(".exe") {
            score += 20;
        } else if name.ends_with(".msi") {
            score += 10;
        }

        return Some(score);
    }

    #[cfg(target_os = "macos")]
    {
        if name.ends_with(".dmg") {
            return Some(30);
        }
        if name.ends_with(".app.tar.gz") || name.ends_with(".app.zip") {
            return Some(20);
        }
        return None;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if name.ends_with(".appimage") {
            return Some(30);
        }
        if name.ends_with(".deb") {
            return Some(20);
        }
        if name.ends_with(".rpm") {
            return Some(15);
        }
        return None;
    }

    #[allow(unreachable_code)]
    None
}
