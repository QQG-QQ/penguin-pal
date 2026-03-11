use crate::{
    ai::guardrails,
    codex_runtime::apply_private_env,
    app_state::{AuthMode, ChatMessage, DesktopAction, ProviderConfig, ProviderKind},
};
use reqwest::Client;
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use tauri::async_runtime;

pub async fn respond(
    provider: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    history: &[ChatMessage],
) -> Result<(String, String), String> {
    if matches!(provider.kind, ProviderKind::Mock) {
        return Ok((mock_reply(history), "Mock Assistant".to_string()));
    }

    if !provider.allow_network {
        return Ok((
            "当前处于离线安全模式，已阻止外网 AI 调用。若要连接真实模型，请在设置中显式开启网络访问。"
                .to_string(),
            "Offline Guard".to_string(),
        ));
    }

    if matches!(provider.kind, ProviderKind::CodexCli) {
        return call_codex_cli(
            provider,
            codex_command,
            codex_home,
            permission_level,
            allowed_actions,
            history,
        )
        .await;
    }

    match provider.kind {
        ProviderKind::OpenAi => {
            let credential = credential_for_openai(provider, api_key, oauth_access_token, "OpenAI")?;
            call_openai_like(
                provider,
                Some(credential.as_str()),
                permission_level,
                allowed_actions,
                history,
                "https://api.openai.com/v1",
                "OpenAI",
            )
            .await
        }
        ProviderKind::Anthropic => {
            if matches!(provider.auth_mode, AuthMode::OAuth) {
                return Err(
                    "Anthropic 当前未接入 OAuth bearer token，这个版本仅支持 API Key。"
                        .to_string(),
                );
            }
            let key = required_key(api_key, "Anthropic")?;
            call_anthropic(provider, &key, permission_level, allowed_actions, history).await
        }
        ProviderKind::OpenAiCompatible => {
            let base_url = provider
                .base_url
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "http://127.0.0.1:11434/v1".to_string());
            let credential = match provider.auth_mode {
                AuthMode::ApiKey => api_key
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_string),
                AuthMode::OAuth => Some(required_oauth_token(
                    oauth_access_token,
                    "OpenAI-Compatible",
                )?),
            };

            call_openai_like(
                provider,
                credential.as_deref(),
                permission_level,
                allowed_actions,
                history,
                &base_url,
                "OpenAI-Compatible",
            )
            .await
        }
        ProviderKind::Mock | ProviderKind::CodexCli => unreachable!(),
    }
}

pub fn fallback_reply(error: &str) -> String {
    format!(
        "外部 AI 调用失败：{}。\n我没有执行任何桌面动作，也不会绕过白名单。你可以检查当前 provider 的登录状态、API Key、模型地址或切回 Mock 模式。",
        error
    )
}

fn run_codex_exec(command: &str, home_root: &Path, prompt: &str) -> Result<String, String> {
    let mut child = {
        let mut cmd = Command::new(command);
        apply_private_env(&mut cmd, home_root);
        cmd
    }
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--sandbox")
        .arg("read-only")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("执行 codex exec 失败：{error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|error| format!("写入 Codex CLI 输入失败：{error}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("等待 codex exec 完成失败：{error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "codex exec 返回失败状态，但没有可读错误输出。".to_string()
        });
    }

    if !stdout.is_empty() {
        return Ok(stdout);
    }

    if !stderr.is_empty() {
        return Ok(stderr);
    }

    Err("codex exec 没有返回可用文本。".to_string())
}

async fn call_codex_cli(
    provider: &ProviderConfig,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    history: &[ChatMessage],
) -> Result<(String, String), String> {
    let user_prompt = history
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "当前没有可发送给 Codex CLI 的用户消息。".to_string())?;

    let system_prompt = guardrails::compose_system_prompt(provider, permission_level, allowed_actions);
    let prompt = format!(
        "{system_prompt}\n\n用户输入：\n{user_prompt}\n\n请直接输出最终答复，不要输出命令行日志。"
    );

    let command = codex_command
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未检测到桌宠内置 Codex 运行时。请先把 Codex 私有运行时打包进应用资源。".to_string())?;
    let home_root = codex_home
        .filter(|value| !value.trim().is_empty())
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "当前未初始化桌宠私有 Codex 凭据目录。".to_string())?;

    let reply = async_runtime::spawn_blocking(move || run_codex_exec(&command, &home_root, &prompt))
        .await
        .map_err(|error| format!("等待 Codex CLI 响应失败：{error}"))??;

    Ok((reply, "Codex CLI".to_string()))
}

fn credential_for_openai(
    provider: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    provider_name: &str,
) -> Result<String, String> {
    match provider.auth_mode {
        AuthMode::ApiKey => required_key(api_key, provider_name),
        AuthMode::OAuth => required_oauth_token(oauth_access_token, provider_name),
    }
}

fn required_key(api_key: Option<String>, provider: &str) -> Result<String, String> {
    api_key
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{provider} 尚未配置 API Key"))
}

fn required_oauth_token(token: Option<String>, provider: &str) -> Result<String, String> {
    token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{provider} 尚未完成 OAuth 登录或访问令牌已失效"))
}

fn mock_reply(history: &[ChatMessage]) -> String {
    let latest = history
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.as_str())
        .unwrap_or_default();

    if latest.contains("什么模型") || latest.contains("你是谁") || latest.contains("怎么运行") {
        return "我现在以 PenguinPal 桌宠助手身份运行。当前如果选中 Mock，就说明还没切到真实模型；切到 Codex CLI 或其他 Provider 后，我会按对应模型工作。"
            .to_string();
    }

    if latest.contains("安全") || latest.contains("权限") {
        return "当前桌宠运行在严格白名单模式。AI 只能提出建议，真正的系统动作只能通过动作面板，并且高风险操作必须逐项确认。"
            .to_string();
    }

    if latest.contains("OAuth") || latest.contains("登录") {
        return "现在已经支持 OAuth 准备流和 API Key 双模式。是否真能用 OAuth 调模型，取决于你的上游模型网关是否支持 OAuth bearer token。"
            .to_string();
    }

    if latest.contains("记事本")
        || latest.contains("计算器")
        || latest.contains("控制电脑")
        || latest.contains("打开")
    {
        return "桌面控制已经被收口到白名单动作层，目前高风险动作必须先申请一次性授权票据，再勾选确认项并输入确认短语。"
            .to_string();
    }

    if latest.contains("语音") {
        return "检测到麦克风后会自动进入语音监听，识别到内容后会直接转写并发送。回复完成后，如果开启了语音回复，会使用系统 TTS 播报。"
            .to_string();
    }

    "桌宠 UI、对话壳、OAuth 准备流和更严格的确认网关已经连通。你现在可以继续微调人设、模型和动作白名单。".to_string()
}

async fn call_openai_like(
    provider: &ProviderConfig,
    credential: Option<&str>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    history: &[ChatMessage],
    base_url: &str,
    label: &str,
) -> Result<(String, String), String> {
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let client = Client::new();
    let system_prompt = guardrails::compose_system_prompt(provider, permission_level, allowed_actions);
    let payload = json!({
        "model": provider.model,
        "temperature": 0.4,
        "messages": build_openai_messages(&system_prompt, history),
    });

    let mut request = client.post(endpoint).json(&payload);
    if let Some(token) = credential {
        request = request.bearer_auth(token);
    }

    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;

    if !status.is_success() {
        return Err(format!("{label} 请求失败({status}): {body}"));
    }

    let value: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    let reply = extract_openai_content(&value)
        .ok_or_else(|| format!("{label} 返回内容为空或格式不兼容"))?;

    Ok((reply, label.to_string()))
}

async fn call_anthropic(
    provider: &ProviderConfig,
    api_key: &str,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    history: &[ChatMessage],
) -> Result<(String, String), String> {
    let endpoint = provider
        .base_url
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "https://api.anthropic.com/v1/messages".to_string());
    let client = Client::new();
    let system_prompt = guardrails::compose_system_prompt(provider, permission_level, allowed_actions);
    let payload = json!({
        "model": provider.model,
        "system": system_prompt,
        "max_tokens": 1024,
        "messages": build_anthropic_messages(history),
    });

    let response = client
        .post(endpoint)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&payload)
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;

    if !status.is_success() {
        return Err(format!("Anthropic 请求失败({status}): {body}"));
    }

    let value: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    let reply = value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .map(|text| text.to_string())
        .ok_or_else(|| "Anthropic 返回内容为空或格式不兼容".to_string())?;

    Ok((reply, "Anthropic".to_string()))
}

fn build_openai_messages(system_prompt: &str, history: &[ChatMessage]) -> Vec<Value> {
    let mut messages = Vec::new();

    if !system_prompt.trim().is_empty() {
        messages.push(json!({
            "role": "system",
            "content": system_prompt,
        }));
    }

    messages.extend(history.iter().map(|message| {
        json!({
            "role": message.role,
            "content": message.content,
        })
    }));

    messages
}

fn build_anthropic_messages(history: &[ChatMessage]) -> Vec<Value> {
    history
        .iter()
        .filter(|message| message.role == "user" || message.role == "assistant")
        .map(|message| {
            json!({
                "role": message.role,
                "content": message.content,
            })
        })
        .collect()
}

fn extract_openai_content(value: &Value) -> Option<String> {
    let content = value
        .get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?;

    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    content
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .filter(|text| !text.is_empty())
}
