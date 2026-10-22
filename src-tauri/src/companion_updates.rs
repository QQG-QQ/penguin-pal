//! Companion-only release checks. No private Codex runtime updates or automatic execution.
use reqwest::{redirect::Policy, Client};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
const REPO: &str = "https://github.com/QQG-QQ/penguin-pal";
const API: &str = "https://api.github.com/repos/QQG-QQ/penguin-pal/releases?per_page=30";
const MAX_PACKAGE: u64 = 256 * 1024 * 1024;

#[derive(Deserialize)]
pub struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
    size: u64,
}
#[derive(Deserialize)]
pub struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Clone, Debug)]
pub struct Package {
    pub version: String,
    pub name: String,
    pub url: String,
    pub digest: String,
    pub checksum_url: Option<String>,
    pub release_url: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub status: &'static str,
    pub current_version: &'static str,
    pub latest_version: Option<String>,
    pub asset_name: Option<String>,
    pub release_url: String,
    pub downloadable: bool,
    pub message: String,
}
fn version(s: &str) -> Option<[u32; 3]> {
    if s.len() > 32 {
        return None;
    }
    let parts: Vec<_> = s.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}
fn hash(s: &str) -> Option<String> {
    if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(s.to_ascii_lowercase())
    } else {
        None
    }
}
fn trusted_url(tag: &str, name: &str, url: &str) -> bool {
    url == format!("{REPO}/releases/download/{tag}/{name}")
}
fn client() -> Result<Client, String> {
    Client::builder()
        .user_agent(concat!("PenguinPal-Companion/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(120))
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() < 5
                && attempt.url().scheme() == "https"
                && matches!(
                    attempt.url().host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.follow()
            } else {
                attempt.error("untrusted update redirect")
            }
        }))
        .build()
        .map_err(|_| "无法初始化更新检查".to_string())
}
pub fn choose(releases: Vec<Release>, arch: &str) -> Option<Package> {
    let mut options = vec![];
    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        let Some(raw) = release.tag_name.strip_prefix("companion-v") else {
            continue;
        };
        let Some(parsed) = version(raw) else { continue };
        let setup = format!("PenguinPal-Companion-{raw}-windows-{arch}-setup.exe");
        let portable = format!("PenguinPal-Companion-{raw}-windows-{arch}.exe");
        let candidate = release
            .assets
            .iter()
            .find(|a| a.name == setup)
            .or_else(|| release.assets.iter().find(|a| a.name == portable));
        let Some(asset) = candidate else { continue };
        if asset.size == 0
            || asset.size > MAX_PACKAGE
            || !trusted_url(&release.tag_name, &asset.name, &asset.browser_download_url)
        {
            continue;
        }
        let digest = asset
            .digest
            .as_deref()
            .and_then(|v| v.strip_prefix("sha256:"))
            .and_then(hash)
            .unwrap_or_default();
        let checksum_url = release
            .assets
            .iter()
            .find(|a| {
                a.name == format!("{}.sha256", asset.name)
                    && a.size <= 4096
                    && trusted_url(&release.tag_name, &a.name, &a.browser_download_url)
            })
            .map(|a| a.browser_download_url.clone());
        options.push((
            parsed,
            Package {
                version: raw.into(),
                name: asset.name.clone(),
                url: asset.browser_download_url.clone(),
                digest,
                checksum_url,
                release_url: format!("{REPO}/releases/tag/{}", release.tag_name),
            },
        ));
    }
    options.into_iter().max_by_key(|(v, _)| *v).map(|(_, p)| p)
}
pub fn info(package: Option<&Package>) -> UpdateInfo {
    let current = env!("CARGO_PKG_VERSION");
    match package {
        None => UpdateInfo {
            status: "none",
            current_version: current,
            latest_version: None,
            asset_name: None,
            release_url: format!("{REPO}/releases"),
            downloadable: false,
            message: "未找到匹配的 Companion 正式发布包；没有选择旧助手安装包。".into(),
        },
        Some(p) => {
            let newer = version(&p.version) > version(current);
            let verified = hash(&p.digest).is_some();
            UpdateInfo {
                status: if newer { "available" } else { "current" },
                current_version: current,
                latest_version: Some(p.version.clone()),
                asset_name: Some(p.name.clone()),
                release_url: p.release_url.clone(),
                downloadable: newer && verified,
                message: if newer && !verified {
                    "发现 Companion 更新，但缺少有效 SHA-256，暂不允许下载。".into()
                } else if newer {
                    "发现 Companion 更新，可下载并校验；不会自动运行。".into()
                } else {
                    "当前版本不低于已发布的 Companion 版本。".into()
                },
            }
        }
    }
}
pub async fn check() -> Result<(UpdateInfo, Option<Package>), String> {
    let client = client()?;
    let response = client
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| "连接 GitHub 更新服务失败，请稍后重试")?;
    if !response.status().is_success() {
        return Err(format!(
            "更新服务返回 HTTP {}，未判断为最新版本",
            response.status().as_u16()
        ));
    }
    let mut response = response;
    let mut bytes = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|_| "读取发布信息失败")? {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("发布信息过大".into());
        }
        bytes.extend_from_slice(&chunk)
    }
    let releases: Vec<Release> = serde_json::from_slice(&bytes).map_err(|_| "发布信息格式无效")?;
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    let mut package = choose(releases, arch);
    if let Some(p) = package.as_mut() {
        if p.digest.is_empty() {
            if let Some(url) = &p.checksum_url {
                let mut response = client
                    .get(url)
                    .send()
                    .await
                    .map_err(|_| "读取更新校验文件失败")?
                    .error_for_status()
                    .map_err(|_| "更新校验文件不可用")?;
                let mut bytes = vec![];
                while let Some(chunk) = response.chunk().await.map_err(|_| "读取校验摘要失败")?
                {
                    if bytes.len() + chunk.len() > 4096 {
                        return Err("校验文件过大".into());
                    }
                    bytes.extend_from_slice(&chunk)
                }
                let text = String::from_utf8(bytes).map_err(|_| "校验文件编码无效")?;
                p.digest = hash(text.split_whitespace().next().unwrap_or_default())
                    .ok_or("更新 SHA-256 摘要无效")?;
            }
        }
    }
    Ok((info(package.as_ref()), package))
}
fn verify_file(path: &Path, expected: &str) -> Result<bool, String> {
    let mut file = fs::File::open(path).map_err(|_| "无法读取更新文件")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|_| "校验更新文件失败")?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()) == expected)
}
pub async fn download(root: &Path, package: &Package) -> Result<PathBuf, String> {
    if hash(&package.digest).is_none() || !info(Some(package)).downloadable {
        return Err("没有可校验的 Companion 更新".into());
    }
    let folder = root.join("updates");
    fs::create_dir_all(&folder).map_err(|_| "无法创建更新目录")?;
    let final_path = folder.join(&package.name);
    if final_path.exists() {
        return if verify_file(&final_path, &package.digest)? {
            Ok(final_path)
        } else {
            Err("同名更新文件校验失败，请在更新目录中检查该文件".into())
        };
    }
    let temporary = folder.join(format!("{}.part", package.name));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "已有下载或残留临时文件，请检查更新目录")?;
    let result: Result<(), String> = async {
        let mut response = client()?
            .get(&package.url)
            .send()
            .await
            .map_err(|_| "下载更新失败")?
            .error_for_status()
            .map_err(|_| "更新文件不可用")?;
        if response.content_length().is_some_and(|n| n > MAX_PACKAGE) {
            return Err("更新文件超过 256 MB".into());
        }
        let mut total = 0;
        while let Some(chunk) = response.chunk().await.map_err(|_| "更新下载中断")? {
            total += chunk.len() as u64;
            if total > MAX_PACKAGE {
                return Err("更新文件过大".into());
            }
            file.write_all(&chunk).map_err(|_| "写入更新失败")?;
        }
        file.sync_all().map_err(|_| "保存更新失败")?;
        Ok(())
    }
    .await;
    drop(file);
    if let Err(e) = result {
        let _ = fs::remove_file(&temporary);
        return Err(e);
    }
    if !verify_file(&temporary, &package.digest)? {
        let _ = fs::remove_file(&temporary);
        return Err("更新 SHA-256 校验不匹配，已丢弃下载文件".into());
    }
    fs::rename(&temporary, &final_path).map_err(|_| "保存已校验更新失败")?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Explicit online GitHub check; not part of offline validation"]
    fn live_release_check() {
        let (info, package) = tauri::async_runtime::block_on(check()).unwrap();
        println!("{}", serde_json::to_string(&info).unwrap());
        assert_eq!(info.current_version, env!("CARGO_PKG_VERSION"));
        if let Some(package) = package {
            assert!(package
                .release_url
                .starts_with(&format!("{REPO}/releases/tag/companion-v")));
        }
    }
    fn release(tag: &str, name: &str, url: &str, digest: Option<&str>) -> Release {
        serde_json::from_value(serde_json::json!({"tag_name":tag,"draft":false,"prerelease":false,"assets":[{"name":name,"browser_download_url":url,"digest":digest,"size":100}]})).unwrap()
    }
    #[test]
    fn old_editions_foreign_urls_and_missing_hash_cannot_update() {
        assert!(choose(
            vec![release(
                "v99.0.0",
                "PenguinPal-setup.exe",
                "https://example.com/x",
                None
            )],
            "x64"
        )
        .is_none());
        let name = "PenguinPal-Companion-99.0.0-windows-x64-setup.exe";
        assert!(choose(
            vec![release(
                "companion-v99.0.0",
                name,
                "https://example.com/x",
                None
            )],
            "x64"
        )
        .is_none());
        let url = format!("{REPO}/releases/download/companion-v99.0.0/{name}");
        let p = choose(vec![release("companion-v99.0.0", name, &url, None)], "x64").unwrap();
        assert!(!info(Some(&p)).downloadable);
        let digest = format!("sha256:{}", "a".repeat(64));
        let p = choose(
            vec![release("companion-v99.0.0", name, &url, Some(&digest))],
            "x64",
        )
        .unwrap();
        assert!(info(Some(&p)).downloadable);
    }
    #[test]
    fn corrupt_package_is_detected() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("test.exe");
        fs::write(&p, b"hello").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"hello"));
        assert!(verify_file(&p, &digest).unwrap());
        assert!(!verify_file(&p, &"0".repeat(64)).unwrap());
        assert!(version("1.0.0/../").is_none());
    }
}
