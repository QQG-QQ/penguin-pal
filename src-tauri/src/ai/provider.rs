#![allow(dead_code)]

use crate::{
    agent::vision_types::{VisionProviderStatus, VisionProviderStatusKind},
    ai::guardrails,
    app_state::{
        AuthMode, ChatMessage, DesktopAction, ProviderConfig, ProviderKind, VisionChannelConfig,
        VisionChannelKind,
    },
    codex_config::CodexConfig,
    codex_runtime::apply_private_env,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::Client;
use serde_json::{json, Value};
use std::{
    env,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::async_runtime;

#[derive(Debug, Clone)]
struct CodexExecResult {
    text: String,
    thread_id: Option<String>,
}

pub async fn respond(
    provider: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
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
            codex_thread_id,
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

pub async fn plan_control_request(
    provider: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    planner_prompt: &str,
    user_input: &str,
) -> Result<String, String> {
    if matches!(provider.kind, ProviderKind::Mock) {
        return Err("Mock provider 不支持自然语言桌面代理规划。".to_string());
    }

    if !provider.allow_network {
        return Err("当前处于离线安全模式，已阻止外部规划模型调用。".to_string());
    }

    if matches!(provider.kind, ProviderKind::CodexCli) {
        let command = codex_command
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "未检测到桌宠内置 Codex 运行时。".to_string())?;
        let home_root = codex_home
            .filter(|value| !value.trim().is_empty())
            .map(std::path::PathBuf::from)
            .ok_or_else(|| "当前未初始化桌宠私有 Codex 凭据目录。".to_string())?;
        let prompt = format!(
            "{planner_prompt}\n\n当前权限等级：L{permission_level}\n当前白名单动作数：{}\n用户输入：\n{}\n\n只输出 JSON。",
            allowed_actions.len(),
            user_input.trim()
        );

        let existing_thread_id = codex_thread_id.clone();
        let result = async_runtime::spawn_blocking(move || {
            run_codex_exec(&command, &home_root, &prompt, &prompt, existing_thread_id)
        })
            .await
            .map_err(|error| format!("等待 Codex CLI 规划结果失败：{error}"))??;
        if result.thread_id.is_some() {
            *codex_thread_id = result.thread_id.clone();
        }
        return Ok(result.text);
    }

    match provider.kind {
        ProviderKind::OpenAi => {
            let credential =
                credential_for_openai(provider, api_key, oauth_access_token, "OpenAI")?;
            call_openai_like_prompt(
                provider,
                Some(credential.as_str()),
                planner_prompt,
                user_input,
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
            call_anthropic_prompt(provider, &key, planner_prompt, user_input).await
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

            call_openai_like_prompt(
                provider,
                credential.as_deref(),
                planner_prompt,
                user_input,
                &base_url,
                "OpenAI-Compatible",
            )
            .await
        }
        ProviderKind::Mock | ProviderKind::CodexCli => unreachable!(),
    }
}

pub fn vision_support_status(
    vision_channel: &VisionChannelConfig,
    api_key: Option<&str>,
) -> VisionProviderStatus {
    if !vision_channel.enabled || matches!(vision_channel.kind, VisionChannelKind::Disabled) {
        return VisionProviderStatus {
            kind: VisionProviderStatusKind::Unsupported,
            message: "视觉副通道未启用。".to_string(),
        };
    }

    if !vision_channel.allow_network {
        return VisionProviderStatus {
            kind: VisionProviderStatusKind::DisabledOffline,
            message: "当前处于离线安全模式，已阻止视觉分析。".to_string(),
        };
    }

    match vision_channel.kind {
        VisionChannelKind::Disabled => VisionProviderStatus {
            kind: VisionProviderStatusKind::Unsupported,
            message: "视觉副通道未启用。".to_string(),
        },
        VisionChannelKind::OpenAi => {
            if api_key.map(|value| value.trim().is_empty()).unwrap_or(true) {
                return VisionProviderStatus {
                    kind: VisionProviderStatusKind::Unsupported,
                    message: "视觉副通道缺少 OpenAI API Key。".to_string(),
                };
            }

            VisionProviderStatus {
                kind: VisionProviderStatusKind::Supported,
                message: "视觉副通道已启用 OpenAI 图像分析。".to_string(),
            }
        }
        VisionChannelKind::OpenAiCompatible => VisionProviderStatus {
            kind: VisionProviderStatusKind::Unknown,
            message: "OpenAI-Compatible 视觉副通道是否支持图像输入取决于具体上游实现，将按最佳努力尝试。".to_string(),
        },
    }
}

pub async fn analyze_window_image(
    vision_channel: &VisionChannelConfig,
    api_key: Option<String>,
    image_path: &Path,
    vision_prompt: &str,
) -> Result<String, String> {
    let support = vision_support_status(vision_channel, api_key.as_deref());
    if !matches!(
        support.kind,
        VisionProviderStatusKind::Supported | VisionProviderStatusKind::Unknown
    ) {
        return Err(support.message);
    }

    match vision_channel.kind {
        VisionChannelKind::OpenAi => {
            let credential = required_key(api_key, "视觉副通道 OpenAI")?;
            call_openai_like_vision(
                vision_channel,
                Some(credential.as_str()),
                vision_prompt,
                image_path,
                "https://api.openai.com/v1",
                "Vision(OpenAI)",
            )
            .await
        }
        VisionChannelKind::OpenAiCompatible => {
            let base_url = vision_channel
                .base_url
                .clone()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "http://127.0.0.1:11434/v1".to_string());

            call_openai_like_vision(
                vision_channel,
                api_key.as_deref(),
                vision_prompt,
                image_path,
                &base_url,
                "Vision(OpenAI-Compatible)",
            )
            .await
        }
        VisionChannelKind::Disabled => Err(support.message),
    }
}

pub fn fallback_reply(error: &str) -> String {
    format!(
        "外部 AI 调用失败：{}。\n我没有执行任何桌面动作，也不会绕过白名单。你可以检查当前 provider 的登录状态、API Key、模型地址或切回 Mock 模式。",
        error
    )
}

fn is_codex_banner_line(line: &str) -> bool {
    let lowered = line.trim().to_lowercase();
    lowered.starts_with("openai codex")
        || lowered == "--------"
        || lowered.starts_with("workdir:")
        || lowered.starts_with("model:")
        || lowered.starts_with("provider:")
        || lowered.starts_with("approval:")
        || lowered.starts_with("sandbox:")
        || lowered.starts_with("reasoning effort:")
        || lowered.starts_with("reasoning summaries:")
        || lowered.starts_with("session id:")
}

fn strip_codex_banner(raw: &str) -> String {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || is_codex_banner_line(trimmed) {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_codex_json_error_messages(raw: &str) -> Vec<String> {
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || !trimmed.starts_with('{') {
                return None;
            }

            let value = serde_json::from_str::<Value>(trimmed).ok()?;
            if value.get("type").and_then(Value::as_str) == Some("error") {
                return value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
            }

            value
                .get("item")
                .and_then(|item| item.get("message"))
                .and_then(Value::as_str)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .collect()
}

fn normalize_codex_failure(stdout: &str, stderr: &str) -> String {
    let primary = if !stderr.is_empty() { stderr } else { stdout };
    let cleaned = strip_codex_banner(primary);
    if !cleaned.is_empty() {
        return cleaned;
    }

    let json_errors = extract_codex_json_error_messages(primary);
    if !json_errors.is_empty() {
        return json_errors.join("\n");
    }

    "Codex CLI 调用失败，但只返回了启动元信息。通常是私有配置、登录状态或运行时参数不兼容。请先检查 Codex CLI 登录状态，并避免让设置页直接覆写 CLI 私有配置。".to_string()
}

fn extract_codex_thread_id(stdout: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            return None;
        }
        let value = serde_json::from_str::<Value>(trimmed).ok()?;
        if value.get("type").and_then(Value::as_str) == Some("thread.started") {
            value
                .get("thread_id")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        } else {
            None
        }
    })
}

fn build_codex_output_file() -> std::path::PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    env::temp_dir().join(format!(
        "penguin-pal-codex-last-message-{}-{}.txt",
        std::process::id(),
        timestamp
    ))
}

fn run_codex_exec_once(
    command: &str,
    home_root: &Path,
    prompt: &str,
    thread_id: Option<String>,
) -> Result<CodexExecResult, String> {
    let output_file = build_codex_output_file();
    let mut child = {
        let mut cmd = Command::new(command);
        apply_private_env(&mut cmd, home_root);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    };

    if let Some(thread_id) = thread_id.as_ref().filter(|value| !value.trim().is_empty()) {
        child
            .arg("exec")
            .arg("resume")
            .arg(thread_id)
            .arg("--json")
            .arg("--skip-git-repo-check")
            .arg("--output-last-message")
            .arg(&output_file)
            .arg("-");
    } else {
        child
            .arg("exec")
            .arg("--json")
            .arg("--skip-git-repo-check")
            .arg("--sandbox")
            .arg("read-only")
            .arg("--output-last-message")
            .arg(&output_file)
            .arg("-");
    }

    let mut child = child
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
    let discovered_thread_id =
        extract_codex_thread_id(&stdout).or_else(|| thread_id.clone());

    if !output.status.success() {
        let _ = fs::remove_file(&output_file);
        return Err(if !stderr.is_empty() || !stdout.is_empty() {
            normalize_codex_failure(&stdout, &stderr)
        } else {
            "codex exec 返回失败状态，但没有可读错误输出。".to_string()
        });
    }

    let reply = fs::read_to_string(&output_file)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let _ = fs::remove_file(&output_file);

    if let Some(text) = reply {
        return Ok(CodexExecResult {
            text,
            thread_id: discovered_thread_id,
        });
    }

    if !stderr.is_empty() {
        let cleaned = strip_codex_banner(&stderr);
        if !cleaned.is_empty() {
            return Ok(CodexExecResult {
                text: cleaned,
                thread_id: discovered_thread_id,
            });
        }
    }

    if !stdout.is_empty() {
        let cleaned = strip_codex_banner(&stdout);
        if !cleaned.is_empty() {
            return Ok(CodexExecResult {
                text: cleaned,
                thread_id: discovered_thread_id,
            });
        }
    }

    Err("codex exec 没有返回可用文本。".to_string())
}

fn run_codex_exec(
    command: &str,
    home_root: &Path,
    resume_prompt: &str,
    restart_prompt: &str,
    thread_id: Option<String>,
) -> Result<CodexExecResult, String> {
    let existing_thread_id = thread_id.clone();
    match run_codex_exec_once(command, home_root, resume_prompt, thread_id) {
        Ok(result) => Ok(result),
        Err(error) if existing_thread_id.is_some() => {
            run_codex_exec_once(command, home_root, restart_prompt, None)
                .map_err(|fallback_error| {
                    format!(
                        "{error}\n\n另外，尝试启动一个新的 Codex 线程也失败了：{fallback_error}"
                    )
                })
        }
        Err(error) => Err(error),
    }
}

async fn call_codex_cli(
    _provider: &ProviderConfig,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    _permission_level: u8,
    _allowed_actions: &[DesktopAction],
    history: &[ChatMessage],
) -> Result<(String, String), String> {
    // 检查是否有用户消息
    if !history.iter().any(|m| m.role == "user" && !m.content.trim().is_empty()) {
        return Err("当前没有可发送给 Codex CLI 的用户消息。".to_string());
    }

    let command = codex_command
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未检测到桌宠内置 Codex 运行时。请先把 Codex 私有运行时打包进应用资源。".to_string())?;
    let home_root = codex_home
        .filter(|value| !value.trim().is_empty())
        .map(std::path::PathBuf::from)
        .ok_or_else(|| "当前未初始化桌宠私有 Codex 凭据目录。".to_string())?;

    // 加载 Codex 配置
    let codex_home_dir = home_root.join(".codex");
    let config = CodexConfig::load_from_home(&codex_home_dir).unwrap_or_default();

    // 检查 history 中是否有 system 消息（Shell Agent 模式）
    let system_from_history = history
        .iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.clone());

    let is_shell_agent_mode = system_from_history.is_some();

    // 如果有 system 消息，使用它；否则使用简单的默认提示
    let unified_system = system_from_history.unwrap_or_else(|| {
        "你是一个智能助手，可以回答问题和帮助用户完成任务。".to_string()
    });

    // 添加推理强度提示
    let reasoning_hint = match config.model_reasoning_effort.as_str() {
        "xhigh" => "\n\n请进行深度思考和详细分析。",
        "high" => "\n\n请认真思考后给出详细回答。",
        "low" => "\n\n请简洁快速地回答。",
        _ => "",
    };

    // 构建包含完整对话历史的 prompt
    let conversation = build_codex_conversation(history);

    // Shell Agent 模式下使用简化提示
    let final_instruction = if is_shell_agent_mode {
        "请根据上述内容决定下一步操作。"
    } else {
        "请基于上述对话历史，直接输出对最后一条用户消息的答复。"
    };

    let resume_prompt = if codex_thread_id.is_some() {
        let latest_user_message = history
            .iter()
            .rev()
            .find(|message| message.role == "user" && !message.content.trim().is_empty())
            .map(|message| message.content.trim().to_string())
            .unwrap_or_default();

        format!(
            "{unified_system}{reasoning_hint}\n\n## 当前回合用户消息\n{latest_user_message}\n\n{final_instruction}"
        )
    } else {
        format!(
            "{unified_system}{reasoning_hint}\n\n## 对话历史\n{conversation}\n\n{final_instruction}"
        )
    };
    let restart_prompt = format!(
        "{unified_system}{reasoning_hint}\n\n## 对话历史\n{conversation}\n\n{final_instruction}"
    );

    let existing_thread_id = codex_thread_id.clone();
    let result = async_runtime::spawn_blocking(move || {
        run_codex_exec(
            &command,
            &home_root,
            &resume_prompt,
            &restart_prompt,
            existing_thread_id,
        )
    })
        .await
        .map_err(|error| format!("等待 Codex CLI 响应失败：{error}"))??;
    if result.thread_id.is_some() {
        *codex_thread_id = result.thread_id.clone();
    }

    Ok((result.text, "Codex CLI".to_string()))
}

pub async fn probe_codex_cli_runtime(
    codex_command: String,
    codex_home: String,
) -> Result<(), String> {
    let home_root = PathBuf::from(codex_home);
    async_runtime::spawn_blocking(move || {
        run_codex_exec_once(
            &codex_command,
            &home_root,
            "只回复 OK，不要附加解释。",
            None,
        )
        .map(|_| ())
    })
    .await
    .map_err(|error| format!("等待 Codex CLI 状态探测失败：{error}"))?
}

/// 构建 Codex CLI 的对话历史文本
fn build_codex_conversation(history: &[ChatMessage]) -> String {
    history
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .map(|m| {
            let role_label = if m.role == "user" { "用户" } else { "助手" };
            format!("{role_label}：{}", m.content.trim())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
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
    // Shell Agent 模式：使用 history 中的 system 消息
    let system_prompt = history
        .iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.clone())
        .unwrap_or_else(|| guardrails::compose_system_prompt(provider, permission_level, allowed_actions));
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

async fn call_openai_like_prompt(
    provider: &ProviderConfig,
    credential: Option<&str>,
    planner_prompt: &str,
    user_input: &str,
    base_url: &str,
    label: &str,
) -> Result<String, String> {
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let client = Client::new();
    let payload = json!({
        "model": provider.model,
        "temperature": 0.0,
        "messages": build_openai_messages_from_texts(planner_prompt, user_input),
    });

    let mut request = client.post(endpoint).json(&payload);
    if let Some(token) = credential {
        request = request.bearer_auth(token);
    }

    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;

    if !status.is_success() {
        return Err(format!("{label} 规划请求失败({status}): {body}"));
    }

    let value: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    extract_openai_content(&value).ok_or_else(|| format!("{label} 规划返回内容为空或格式不兼容"))
}

async fn call_openai_like_vision(
    vision_channel: &VisionChannelConfig,
    credential: Option<&str>,
    vision_prompt: &str,
    image_path: &Path,
    base_url: &str,
    label: &str,
) -> Result<String, String> {
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let client = Client::builder()
        .timeout(std::time::Duration::from_millis(
            vision_channel.timeout_ms.max(1000),
        ))
        .build()
        .map_err(|error| error.to_string())?;
    let image_url = build_image_data_url(image_path)?;
    let payload = json!({
        "model": vision_channel.model,
        "temperature": 0.0,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": vision_prompt },
                { "type": "image_url", "image_url": { "url": image_url, "detail": "low" } }
            ]
        }],
    });

    let mut request = client.post(endpoint).json(&payload);
    if let Some(token) = credential {
        request = request.bearer_auth(token);
    }

    let response = request
        .send()
        .await
        .map_err(|error| normalize_vision_request_error(error, label))?;
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;

    if !status.is_success() {
        return Err(format!("{label} 视觉分析请求失败({status}): {body}"));
    }

    let value: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    extract_openai_content(&value).ok_or_else(|| format!("{label} 视觉分析返回内容为空或格式不兼容"))
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
    // Shell Agent 模式：使用 history 中的 system 消息
    let system_prompt = history
        .iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.clone())
        .unwrap_or_else(|| guardrails::compose_system_prompt(provider, permission_level, allowed_actions));
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

async fn call_anthropic_prompt(
    provider: &ProviderConfig,
    api_key: &str,
    planner_prompt: &str,
    user_input: &str,
) -> Result<String, String> {
    let endpoint = provider
        .base_url
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "https://api.anthropic.com/v1/messages".to_string());
    let client = Client::new();
    let payload = json!({
        "model": provider.model,
        "system": planner_prompt,
        "max_tokens": 512,
        "messages": build_anthropic_messages_from_texts(user_input),
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
        return Err(format!("Anthropic 规划请求失败({status}): {body}"));
    }

    let value: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    value
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .map(|text| text.to_string())
        .ok_or_else(|| "Anthropic 规划返回内容为空或格式不兼容".to_string())
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

fn build_openai_messages_from_texts(system_prompt: &str, user_input: &str) -> Vec<Value> {
    let mut messages = Vec::new();

    if !system_prompt.trim().is_empty() {
        messages.push(json!({
            "role": "system",
            "content": system_prompt,
        }));
    }

    messages.push(json!({
        "role": "user",
        "content": user_input,
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

fn build_anthropic_messages_from_texts(user_input: &str) -> Vec<Value> {
    vec![json!({
        "role": "user",
        "content": user_input,
    })]
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

fn build_image_data_url(image_path: &Path) -> Result<String, String> {
    let media_type = image_media_type(image_path);
    let bytes = fs::read(image_path)
        .map_err(|error| format!("读取活动窗口截图失败：{error}"))?;
    Ok(format!(
        "data:{};base64,{}",
        media_type,
        STANDARD.encode(bytes)
    ))
}

fn image_media_type(image_path: &Path) -> &'static str {
    match image_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
}

fn normalize_vision_request_error(error: reqwest::Error, label: &str) -> String {
    if error.is_timeout() {
        return format!("{label} 视觉分析请求超时。");
    }
    error.to_string()
}
