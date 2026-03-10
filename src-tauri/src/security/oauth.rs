use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use reqwest::{Client, Url};
use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::app_state::{now_millis, PendingOAuthState, ProviderConfig, ProviderKind};

const OAUTH_LOGIN_TTL_MS: u64 = 5 * 60 * 1000;

pub struct OAuthExchange {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub account_hint: Option<String>,
}

pub fn prepare_authorization(provider: &ProviderConfig) -> Result<PendingOAuthState, String> {
    ensure_supported_provider(provider)?;

    let authorize_url = required_field(provider.oauth.authorize_url.as_ref(), "Authorize URL")?;
    let client_id = required_field(provider.oauth.client_id.as_ref(), "OAuth Client ID")?;
    let redirect_url = required_field(provider.oauth.redirect_url.as_ref(), "Redirect URL")?;

    let state = random_urlsafe(24);
    let verifier = random_urlsafe(64);
    let challenge = pkce_challenge(&verifier);
    let created_at = now_millis();

    let mut url = Url::parse(authorize_url).map_err(|error| error.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", client_id);
        query.append_pair("redirect_uri", redirect_url);
        query.append_pair("state", &state);
        query.append_pair("code_challenge", &challenge);
        query.append_pair("code_challenge_method", "S256");
        if !provider.oauth.scopes.is_empty() {
            query.append_pair("scope", &provider.oauth.scopes.join(" "));
        }
    }

    Ok(PendingOAuthState {
        state,
        verifier,
        authorization_url: url.to_string(),
        created_at,
        expires_at: created_at + OAUTH_LOGIN_TTL_MS,
    })
}

pub fn parse_callback(callback_url: &str) -> Result<(String, String), String> {
    let url = Url::parse(callback_url.trim()).map_err(|error| error.to_string())?;
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut error_description = None;

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.to_string()),
            "state" => state = Some(value.to_string()),
            "error" => error = Some(value.to_string()),
            "error_description" => error_description = Some(value.to_string()),
            _ => {}
        }
    }

    if let Some(error) = error {
        let detail = error_description.unwrap_or_else(|| "授权服务返回了错误。".to_string());
        return Err(format!("OAuth 授权失败：{} ({})", detail, error));
    }

    let code = code.ok_or_else(|| "浏览器回调地址中缺少 code。".to_string())?;
    let state = state.ok_or_else(|| "浏览器回调地址中缺少 state。".to_string())?;

    Ok((code, state))
}

pub async fn exchange_code(
    provider: &ProviderConfig,
    pending: &PendingOAuthState,
    code: &str,
) -> Result<OAuthExchange, String> {
    ensure_supported_provider(provider)?;

    let token_url = required_field(provider.oauth.token_url.as_ref(), "Token URL")?;
    let client_id = required_field(provider.oauth.client_id.as_ref(), "OAuth Client ID")?;
    let redirect_url = required_field(provider.oauth.redirect_url.as_ref(), "Redirect URL")?;

    let response = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?
        .post(token_url)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("redirect_uri", redirect_url),
            ("code_verifier", pending.verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!("OAuth token exchange 失败({status}): {body}"));
    }

    let value: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .ok_or_else(|| "OAuth token exchange 返回中缺少 access_token。".to_string())?;
    let refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let expires_at = extract_expires_at(&value);
    let account_hint = extract_account_hint(&value);

    Ok(OAuthExchange {
        access_token,
        refresh_token,
        expires_at,
        account_hint,
    })
}

pub fn ensure_supported_provider(provider: &ProviderConfig) -> Result<(), String> {
    match provider.kind {
        ProviderKind::Mock => Err("Mock Provider 不支持 OAuth 登录。".to_string()),
        ProviderKind::CodexCli => Err(
            "Codex CLI Provider 不使用通用 OAuth 字段，请直接在设置中执行 codex login。"
                .to_string(),
        ),
        ProviderKind::Anthropic => Err(
            "Anthropic 当前未接入 OAuth bearer token，这个版本仅支持 API Key。"
                .to_string(),
        ),
        ProviderKind::OpenAi | ProviderKind::OpenAiCompatible => Ok(()),
    }
}

fn extract_expires_at(value: &Value) -> Option<u64> {
    value
        .get("expires_in")
        .and_then(|item| {
            item.as_u64().or_else(|| item.as_str().and_then(|raw| raw.parse::<u64>().ok()))
        })
        .map(|seconds| now_millis() + seconds.saturating_mul(1000))
}

fn extract_account_hint(value: &Value) -> Option<String> {
    ["email", "preferred_username", "username", "sub"]
        .into_iter()
        .find_map(|key| value.get(key).and_then(Value::as_str))
        .map(|value| value.to_string())
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

fn random_urlsafe(length: usize) -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}

fn required_field<'a>(value: Option<&'a String>, label: &str) -> Result<&'a str, String> {
    value
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("OAuth 配置不完整：缺少 {}。", label))
}

pub fn open_authorization_in_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(url)
            .spawn()
            .map_err(|error| format!("无法打开系统浏览器: {error}"))?;
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|error| format!("无法打开系统浏览器: {error}"))?;
        return Ok(());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|error| format!("无法打开系统浏览器: {error}"))?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("当前系统不支持自动拉起浏览器，请手动复制授权链接。".to_string())
}

pub fn wait_for_callback(redirect_url: &str, timeout: Duration) -> Result<String, String> {
    let parsed = Url::parse(redirect_url.trim()).map_err(|error| error.to_string())?;
    if parsed.scheme() != "http" {
        return Err("自动回调仅支持 http://127.0.0.1 或 http://localhost".to_string());
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "Redirect URL 缺少 host。".to_string())?;
    if host != "127.0.0.1" && host != "localhost" {
        return Err("自动回调仅允许本机回环地址（127.0.0.1 / localhost）。".to_string());
    }
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "Redirect URL 缺少端口。".to_string())?;
    let bind_addr = if host == "localhost" {
        format!("localhost:{port}")
    } else {
        format!("127.0.0.1:{port}")
    };
    let path = if parsed.path().is_empty() {
        "/"
    } else {
        parsed.path()
    };

    let listener =
        TcpListener::bind(&bind_addr).map_err(|error| format!("无法监听 OAuth 回调端口 {bind_addr}: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;

    let start = Instant::now();
    loop {
        if start.elapsed() > timeout {
            return Err("等待 OAuth 浏览器回调超时，请重试。".to_string());
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buffer = [0_u8; 8192];
                let size = stream.read(&mut buffer).map_err(|error| error.to_string())?;
                let request = String::from_utf8_lossy(&buffer[..size]);
                let request_line = request.lines().next().unwrap_or_default();
                let target = request_line
                    .split_whitespace()
                    .nth(1)
                    .ok_or_else(|| "OAuth 回调请求格式错误。".to_string())?;

                let (status, body, callback) = if target.starts_with(path) {
                    let callback_url = format!("{}://{}:{}{}", parsed.scheme(), host, port, target);
                    (
                        "200 OK",
                        "<html><body><h3>OAuth 登录成功</h3><p>可以关闭这个页面并返回企鹅桌宠。</p></body></html>",
                        Some(callback_url),
                    )
                } else {
                    (
                        "404 Not Found",
                        "<html><body><h3>Not Found</h3></body></html>",
                        None,
                    )
                };

                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();

                if let Some(callback_url) = callback {
                    return Ok(callback_url);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(120));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}
