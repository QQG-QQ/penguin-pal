mod agent;
mod ai;
mod app_state;
mod codex_runtime;
mod control;
mod audio;
mod desktop;
mod history;
mod security;
mod test_agent;
mod testing;
mod tray;
mod window;

use serde::{Deserialize, Serialize};
use std::{process::Command, sync::Mutex, time::Duration};
use tauri::{AppHandle, Manager, State};

use crate::{
    agent::{intent_classifier, router as agent_router, AgentTaskState},
    ai::{guardrails, memory, provider},
    app_state::{
        default_system_prompt, load, now_millis, save, ActionExecutionResult,
        AssistantSnapshot, AuthMode, ChatMessage, ChatResponse, DesktopAction, OAuthFlowResult,
        PetMode, ProviderConfig, ProviderConfigInput, RuntimeState,
        DEFAULT_OAUTH_REDIRECT_URL,
    },
    codex_runtime::{apply_private_env, private_auth_path, resolve_for_app},
    control::{router as control_router, types::ControlServiceStatus, ControlServiceState},
    history::ReplyHistoryEntry,
    security::{audit, oauth, policy},
    testing::TestingState,
};

fn snapshot_from_runtime(runtime: &RuntimeState) -> AssistantSnapshot {
    let allowed_actions = policy::actions_for_level(runtime.permission_level);
    let ai_constraints = guardrails::build_profile(
        &runtime.provider,
        runtime.permission_level,
        &allowed_actions,
    );
    runtime.to_snapshot(audio::default_audio_profile(), allowed_actions, ai_constraints)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexCliStatus {
    installed: bool,
    version: Option<String>,
    logged_in: bool,
    auth_path: Option<String>,
    runtime_path: Option<String>,
    source: String,
    message: String,
}

fn first_non_empty_output(output: &std::process::Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Some(stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return Some(stderr);
    }

    None
}

fn read_codex_version(command: &str, app: &AppHandle) -> Option<String> {
    let runtime = resolve_for_app(app).ok()?;
    let output = {
        let mut cmd = Command::new(command);
        apply_private_env(&mut cmd, &runtime.home_root);
        cmd
    }
        .arg("--version")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    first_non_empty_output(&output)
}

fn inspect_codex_cli_status(app: &AppHandle) -> CodexCliStatus {
    let runtime = match resolve_for_app(app) {
        Ok(runtime) => runtime,
        Err(error) => {
            return CodexCliStatus {
                installed: false,
                version: None,
                logged_in: false,
                auth_path: None,
                runtime_path: None,
                source: "未找到".to_string(),
                message: error,
            }
        }
    };

    let command_label = runtime
        .command
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let version = runtime
        .command
        .as_ref()
        .and_then(|path| read_codex_version(&path.to_string_lossy(), app));
    let installed = version.is_some();

    let auth_path = private_auth_path(app).ok();
    let auth_file_exists = auth_path.as_ref().is_some_and(|path| {
        path.is_file() && path.metadata().map(|meta| meta.len() > 0).unwrap_or(false)
    });
    let logged_in = installed && auth_file_exists;
    let auth_path_label = auth_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());

    let message = if !installed {
        if runtime.source == "未找到" {
            "未检测到桌宠内置 Codex 运行时。请把 Codex 私有运行时打包进应用资源后再试；开发期仍可临时回退到系统安装。".to_string()
        } else {
            format!("已发现 {}，但当前无法执行 Codex CLI。", runtime.source)
        }
    } else if logged_in {
        format!("Codex CLI 已登录，当前来源：{}。", runtime.source)
    } else {
        format!("Codex CLI 未登录，请点击按钮启动登录。当前来源：{}。", runtime.source)
    };

    CodexCliStatus {
        installed,
        version,
        logged_in,
        auth_path: auth_path_label,
        runtime_path: command_label,
        source: runtime.source.to_string(),
        message,
    }
}

async fn provider_response(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    history_window: &[ChatMessage],
) -> (String, String, String, String, Option<agent::types::AgentMessageMeta>) {
    match provider::respond(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        permission_level,
        allowed_actions,
        history_window,
    )
    .await
    {
        Ok((reply, label)) => (
            reply,
            label,
            "ok".to_string(),
            format!(
                "provider={} auth={} model={}",
                provider_config.kind.label(),
                match provider_config.auth_mode {
                    AuthMode::ApiKey => "apiKey",
                    AuthMode::OAuth => "oauth",
                },
                provider_config.model
            ),
            None,
        ),
        Err(error) => (
            provider::fallback_reply(&error),
            "Safety fallback".to_string(),
            "fallback".to_string(),
            error,
            None,
        ),
    }
}

#[tauri::command]
fn get_codex_cli_status(app: AppHandle) -> CodexCliStatus {
    inspect_codex_cli_status(&app)
}

#[tauri::command]
fn get_control_service_status(app: AppHandle) -> Result<ControlServiceStatus, String> {
    control_router::service_status(&app).map_err(|error| error.to_string())
}

#[tauri::command]
async fn confirm_control_pending(app: AppHandle, pending_id: String) -> Result<control::types::ToolInvokeResponse, String> {
    if let Some(response) = test_agent::router::confirm_control_pending(&app, &pending_id).await? {
        return Ok(response);
    }
    agent_router::confirm_control_pending(&app, &pending_id)
}

#[tauri::command]
async fn cancel_control_pending(app: AppHandle, pending_id: String) -> Result<control::types::ToolInvokeResponse, String> {
    if let Some(response) = test_agent::router::cancel_control_pending(&app, &pending_id).await? {
        return Ok(response);
    }
    agent_router::cancel_control_pending(&app, &pending_id)
}

#[tauri::command]
fn start_codex_cli_login(app: AppHandle) -> Result<CodexCliStatus, String> {
    let status = inspect_codex_cli_status(&app);
    if !status.installed {
        return Err(status.message);
    }

    let runtime = resolve_for_app(&app)?;
    let codex_command = runtime
        .command
        .ok_or_else(|| "未找到桌宠可用的 Codex 运行时。".to_string())?;
    let wrapped = if codex_command.to_string_lossy().contains(' ') {
        format!("\"{}\"", codex_command.to_string_lossy())
    } else {
        codex_command.to_string_lossy().to_string()
    };
    let login_cmd = format!("{wrapped} --login || {wrapped} login || {wrapped}");

    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        apply_private_env(&mut cmd, &runtime.home_root);
        cmd.args(["/C", "start", "", "cmd", "/K", &login_cmd])
            .spawn()
            .map_err(|error| format!("启动 codex login 失败：{error}"))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = Command::new("sh");
        apply_private_env(&mut cmd, &runtime.home_root);
        cmd.args(["-lc", &login_cmd])
            .spawn()
            .map_err(|error| format!("启动 codex login 失败：{error}"))?;
    }

    let mut next = inspect_codex_cli_status(&app);
    next.message = "已启动 codex login，请在新终端完成登录后点击“刷新状态”。".to_string();
    Ok(next)
}

#[tauri::command]
fn show_settings_window(app: AppHandle) -> Result<bool, String> {
    let window = app
        .get_webview_window("settings")
        .ok_or_else(|| "未找到设置窗口".to_string())?;

    let _ = window.unminimize();
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn hide_settings_window(app: AppHandle) -> Result<bool, String> {
    let Some(window) = app.get_webview_window("settings") else {
        return Ok(false);
    };

    window.hide().map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn hide_main_window(app: AppHandle) -> Result<bool, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;
    window.hide().map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn start_main_window_drag(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;
    window.start_dragging().map_err(|error| error.to_string())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_scopes(value: &str) -> Vec<String> {
    let mut scopes = Vec::new();
    for item in value
        .split(|char: char| char == ',' || char.is_whitespace())
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        if !scopes.iter().any(|existing| existing == item) {
            scopes.push(item.to_string());
        }
    }
    scopes
}

fn clear_oauth_session(runtime: &mut RuntimeState) {
    runtime.oauth_access_token = None;
    runtime.oauth_refresh_token = None;
    runtime.oauth_access_expires_at = None;
    runtime.oauth_account_hint = None;
    runtime.oauth_last_error = None;
    runtime.pending_oauth = None;
}

fn ensure_network_allowed(allow_network: bool) -> Result<(), String> {
    if !allow_network {
        return Err(
            "当前已禁用网络访问。OAuth 登录和令牌交换同样需要你先显式开启外网访问。"
                .to_string(),
        );
    }

    Ok(())
}

fn expire_transient_state(runtime: &mut RuntimeState) {
    policy::cleanup_expired_approvals(&mut runtime.pending_action_approvals);

    if let Some(pending) = &runtime.pending_oauth {
        if pending.expires_at <= now_millis() {
            runtime.pending_oauth = None;
            runtime.oauth_last_error = Some("上一次 OAuth 登录已过期，请重新发起授权。".to_string());
        }
    }
}

#[tauri::command]
fn get_assistant_snapshot(
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);
    Ok(snapshot_from_runtime(&runtime))
}

#[tauri::command]
fn save_provider_config(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    input: ProviderConfigInput,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);

    let previous_provider_kind = runtime.provider.kind;
    let previous_auth_mode = runtime.provider.auth_mode;
    let previous_base_url = runtime.provider.base_url.clone();
    let previous_oauth_authorize_url = runtime.provider.oauth.authorize_url.clone();
    let previous_oauth_token_url = runtime.provider.oauth.token_url.clone();
    let previous_oauth_client_id = runtime.provider.oauth.client_id.clone();
    let previous_oauth_redirect_url = runtime.provider.oauth.redirect_url.clone();
    let previous_oauth_scopes = runtime.provider.oauth.scopes.clone();

    runtime.provider.kind = input.kind;
    runtime.provider.model = if input.model.trim().is_empty() {
        input.kind.default_model().to_string()
    } else {
        input.model.trim().to_string()
    };
    runtime.provider.base_url = normalize_optional(input.base_url);
    runtime.provider.system_prompt = if input.system_prompt.trim().is_empty() {
        default_system_prompt()
    } else {
        input.system_prompt.trim().to_string()
    };
    runtime.provider.allow_network = input.allow_network;
    runtime.provider.voice_reply = input.voice_reply;
    runtime.provider.retain_history = input.retain_history;
    runtime.permission_level = policy::clamp_permission_level(input.permission_level);
    runtime.provider.auth_mode = input.auth_mode;
    runtime.provider.oauth.authorize_url = normalize_optional(input.oauth_authorize_url);
    runtime.provider.oauth.token_url = normalize_optional(input.oauth_token_url);
    runtime.provider.oauth.client_id = normalize_optional(input.oauth_client_id);
    runtime.provider.oauth.redirect_url = normalize_optional(input.oauth_redirect_url)
        .or_else(|| Some(DEFAULT_OAUTH_REDIRECT_URL.to_string()));
    runtime.provider.oauth.scopes = parse_scopes(&input.oauth_scopes);
    runtime.vision_channel.enabled = input.vision_channel.enabled;
    runtime.vision_channel.kind = input.vision_channel.kind;
    runtime.vision_channel.model = if input.vision_channel.model.trim().is_empty() {
        input.vision_channel.kind.default_model().to_string()
    } else {
        input.vision_channel.model.trim().to_string()
    };
    runtime.vision_channel.base_url = normalize_optional(input.vision_channel.base_url);
    runtime.vision_channel.allow_network = input.vision_channel.allow_network;
    runtime.vision_channel.timeout_ms = input.vision_channel.timeout_ms.clamp(1_000, 60_000);
    runtime.vision_channel.max_image_bytes =
        input.vision_channel.max_image_bytes.clamp(64 * 1024, 10 * 1024 * 1024);
    runtime.vision_channel.max_image_width = input.vision_channel.max_image_width.clamp(320, 4096);
    runtime.vision_channel.max_image_height =
        input.vision_channel.max_image_height.clamp(240, 4096);
    runtime.vision_channel.last_error = None;

    if input.clear_api_key.unwrap_or(false) {
        runtime.api_key = None;
    }

    if let Some(api_key) = input
        .api_key
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        runtime.api_key = Some(api_key.to_string());
    }

    if input.vision_channel.clear_api_key.unwrap_or(false) {
        runtime.vision_api_key = None;
    }

    if let Some(api_key) = input
        .vision_channel
        .api_key
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        runtime.vision_api_key = Some(api_key.to_string());
    }

    runtime.vision_channel_status = app_state::current_vision_channel_status(
        &runtime.vision_channel,
        runtime.vision_api_key.as_ref(),
    );

    let oauth_identity_changed = previous_provider_kind != runtime.provider.kind
        || previous_auth_mode != runtime.provider.auth_mode
        || previous_base_url != runtime.provider.base_url
        || previous_oauth_authorize_url != runtime.provider.oauth.authorize_url
        || previous_oauth_token_url != runtime.provider.oauth.token_url
        || previous_oauth_client_id != runtime.provider.oauth.client_id
        || previous_oauth_redirect_url != runtime.provider.oauth.redirect_url
        || previous_oauth_scopes != runtime.provider.oauth.scopes;

    if input.clear_oauth_token.unwrap_or(false) {
        clear_oauth_session(&mut runtime);
    } else if !matches!(runtime.provider.auth_mode, AuthMode::OAuth) {
        clear_oauth_session(&mut runtime);
    } else if oauth_identity_changed {
        let had_oauth_state = runtime.pending_oauth.is_some()
            || runtime
                .oauth_access_token
                .as_ref()
                .is_some_and(|token| !token.trim().is_empty());
        clear_oauth_session(&mut runtime);
        if had_oauth_state {
            runtime.oauth_last_error = Some("OAuth 配置已变更，请重新发起登录。".to_string());
        }
    }

    runtime.mode = PetMode::Idle;

    let audit_detail = format!(
        "provider={} auth={} network={} permission=L{}",
        runtime.provider.kind.label(),
        match runtime.provider.auth_mode {
            AuthMode::ApiKey => "apiKey",
            AuthMode::OAuth => "oauth",
        },
        runtime.provider.allow_network,
        runtime.permission_level
    );
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record("save_provider_config", "ok", audit_detail, 1),
    );

    save(&app, &runtime)?;
    if let Some(agent_state) = app.try_state::<AgentTaskState>() {
        if let Ok(mut cache) = agent_state.vision_cache() {
            *cache = None;
        }
    }
    Ok(snapshot_from_runtime(&runtime))
}

#[tauri::command]
fn start_oauth_sign_in(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<OAuthFlowResult, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);

    if !matches!(runtime.provider.auth_mode, AuthMode::OAuth) {
        return Err("请先在设置中把认证方式切换到 OAuth。".to_string());
    }
    ensure_network_allowed(runtime.provider.allow_network)?;

    let pending = oauth::prepare_authorization(&runtime.provider)?;
    let authorization_url = pending.authorization_url.clone();
    runtime.pending_oauth = Some(pending);
    runtime.oauth_last_error = None;
    runtime.mode = PetMode::Idle;

    let audit_detail = format!(
        "provider={} redirect={}",
        runtime.provider.kind.label(),
        runtime
            .provider
            .oauth
            .redirect_url
            .clone()
            .unwrap_or_default()
    );
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record("oauth_login_started", "pending", audit_detail, 1),
    );

    save(&app, &runtime)?;
    Ok(OAuthFlowResult {
        message: "已生成 OAuth 授权链接。请在系统浏览器中完成登录后，把回调地址粘贴回来。"
            .to_string(),
        authorization_url: Some(authorization_url),
        snapshot: snapshot_from_runtime(&runtime),
    })
}

#[tauri::command]
async fn start_oauth_sign_in_auto(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<OAuthFlowResult, String> {
    let (authorization_url, redirect_url) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);

        if !matches!(runtime.provider.auth_mode, AuthMode::OAuth) {
            return Err("请先在设置中把认证方式切换到 OAuth。".to_string());
        }
        ensure_network_allowed(runtime.provider.allow_network)?;

        let pending = oauth::prepare_authorization(&runtime.provider)?;
        let authorization_url = pending.authorization_url.clone();
        let redirect_url = runtime
            .provider
            .oauth
            .redirect_url
            .clone()
            .unwrap_or_else(|| DEFAULT_OAUTH_REDIRECT_URL.to_string());

        runtime.pending_oauth = Some(pending);
        runtime.oauth_last_error = None;
        runtime.mode = PetMode::Idle;
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "oauth_login_started",
                "pending",
                "自动登录已启动，等待浏览器回调。",
                1,
            ),
        );
        save(&app, &runtime)?;
        (authorization_url, redirect_url)
    };

    if let Err(error) = oauth::open_authorization_in_browser(&authorization_url) {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.oauth_last_error = Some(error.clone());
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record("oauth_login_started", "error", &error, 1),
        );
        save(&app, &runtime)?;
        return Ok(OAuthFlowResult {
            message: format!(
                "自动打开浏览器失败：{}。你可以改用“生成授权链接”手动登录。",
                error
            ),
            authorization_url: Some(authorization_url),
            snapshot: snapshot_from_runtime(&runtime),
        });
    }

    let callback_url = match tauri::async_runtime::spawn_blocking({
        let redirect_url = redirect_url.clone();
        move || oauth::wait_for_callback(&redirect_url, Duration::from_secs(180))
    })
    .await
    .map_err(|error| error.to_string())? {
        Ok(callback_url) => callback_url,
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_started", "timeout", &error, 1),
            );
            save(&app, &runtime)?;
            return Ok(OAuthFlowResult {
                message: format!(
                    "自动 OAuth 回调未完成：{}。你可以继续在当前设置窗口里手动粘贴回调地址完成登录。",
                    error
                ),
                authorization_url: Some(authorization_url),
                snapshot: snapshot_from_runtime(&runtime),
            });
        }
    };

    let (provider_config, pending) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);
        let pending = runtime
            .pending_oauth
            .take()
            .ok_or_else(|| "当前没有进行中的 OAuth 登录。".to_string())?;
        save(&app, &runtime)?;
        (runtime.provider.clone(), pending)
    };

    let (code, returned_state) = match oauth::parse_callback(&callback_url) {
        Ok(parsed) => parsed,
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "error", "OAuth 回调解析失败。", 1),
            );
            save(&app, &runtime)?;
            return Err(error);
        }
    };

    if returned_state != pending.state {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.oauth_last_error = Some("OAuth 状态校验失败，请重新发起登录。".to_string());
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "oauth_login_completed",
                "rejected",
                "OAuth 状态校验失败。",
                1,
            ),
        );
        save(&app, &runtime)?;
        return Err("OAuth 状态校验失败，请重新发起登录。".to_string());
    }

    match oauth::exchange_code(&provider_config, &pending, &code).await {
        Ok(exchange) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_access_token = Some(exchange.access_token);
            runtime.oauth_refresh_token = exchange.refresh_token;
            runtime.oauth_access_expires_at = exchange.expires_at;
            runtime.oauth_account_hint = exchange.account_hint;
            runtime.oauth_last_error = None;
            runtime.mode = PetMode::Idle;
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "ok", "自动登录成功。", 1),
            );
            save(&app, &runtime)?;
            Ok(OAuthFlowResult {
                message: "OAuth 登录成功。当前只会在运行内存中保留访问令牌，并优先把它用于支持 bearer token 的模型网关。"
                    .to_string(),
                authorization_url: None,
                snapshot: snapshot_from_runtime(&runtime),
            })
        }
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "error", &error, 1),
            );
            save(&app, &runtime)?;
            Err(error)
        }
    }
}

#[tauri::command]
async fn complete_oauth_sign_in(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    callback_url: String,
) -> Result<OAuthFlowResult, String> {
    let (provider_config, pending) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);

        if !matches!(runtime.provider.auth_mode, AuthMode::OAuth) {
            return Err("请先在设置中把认证方式切换到 OAuth。".to_string());
        }
        ensure_network_allowed(runtime.provider.allow_network)?;

        let pending = runtime
            .pending_oauth
            .take()
            .ok_or_else(|| "当前没有进行中的 OAuth 登录。".to_string())?;
        save(&app, &runtime)?;
        (runtime.provider.clone(), pending)
    };

    let (code, returned_state) = match oauth::parse_callback(&callback_url) {
        Ok(parsed) => parsed,
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "error", "OAuth 回调解析失败。", 1),
            );
            save(&app, &runtime)?;
            return Err(error);
        }
    };
    if returned_state != pending.state {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.pending_oauth = None;
        runtime.oauth_last_error = Some("OAuth 状态校验失败，请重新发起登录。".to_string());
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "oauth_login_completed",
                "rejected",
                "OAuth 状态校验失败。",
                1,
            ),
        );
        save(&app, &runtime)?;
        return Err("OAuth 状态校验失败，请重新发起登录。".to_string());
    }

    match oauth::exchange_code(&provider_config, &pending, &code).await {
        Ok(exchange) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_access_token = Some(exchange.access_token);
            runtime.oauth_refresh_token = exchange.refresh_token;
            runtime.oauth_access_expires_at = exchange.expires_at;
            runtime.oauth_account_hint = exchange.account_hint;
            runtime.oauth_last_error = None;
            runtime.mode = PetMode::Idle;
            let audit_detail = format!(
                "provider={} auth=oauth account={}",
                runtime.provider.kind.label(),
                runtime
                    .oauth_account_hint
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
            );
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "ok", audit_detail, 1),
            );
            save(&app, &runtime)?;
            Ok(OAuthFlowResult {
                message: "OAuth 登录成功。当前只会在运行内存中保留访问令牌，并优先把它用于支持 bearer token 的模型网关。"
                    .to_string(),
                authorization_url: None,
                snapshot: snapshot_from_runtime(&runtime),
            })
        }
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "error", &error, 1),
            );
            save(&app, &runtime)?;
            Err(error)
        }
    }
}

#[tauri::command]
fn disconnect_oauth_sign_in(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<OAuthFlowResult, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    clear_oauth_session(&mut runtime);
    runtime.mode = PetMode::Idle;
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record("oauth_logout", "ok", "已清空 OAuth 内存令牌状态。", 0),
    );
    save(&app, &runtime)?;
    Ok(OAuthFlowResult {
        message: "已退出 OAuth 登录，并清空内存中的令牌状态。".to_string(),
        authorization_url: None,
        snapshot: snapshot_from_runtime(&runtime),
    })
}

#[tauri::command]
async fn send_chat_message(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    content: String,
) -> Result<ChatResponse, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("消息不能为空".to_string());
    }

    let user_message = ChatMessage::user(trimmed.to_string());
    let (
        provider_config,
        api_key,
        oauth_access_token,
        vision_channel,
        vision_api_key,
        codex_command,
        codex_home,
        history_window,
        permission_level,
        allowed_actions,
    ) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);
        runtime.mode = PetMode::Thinking;
        runtime.messages.push(user_message);
        memory::trim_history(&mut runtime.messages);
        save(&app, &runtime)?;
        let allowed_actions = policy::actions_for_level(runtime.permission_level);
        let codex_runtime = resolve_for_app(&app).ok();
        (
            runtime.provider.clone(),
            runtime.api_key.clone(),
            runtime.oauth_access_token.clone(),
            runtime.vision_channel.clone(),
            runtime.vision_api_key.clone(),
            codex_runtime
                .as_ref()
                .and_then(|item| item.command.as_ref())
                .map(|path| path.to_string_lossy().to_string()),
            codex_runtime
                .as_ref()
                .map(|item| item.home_root.to_string_lossy().to_string()),
            memory::context_window(&runtime.messages),
            runtime.permission_level,
            allowed_actions,
        )
    };

    let classified_route = intent_classifier::classify_user_intent(
        &provider_config,
        api_key.clone(),
        oauth_access_token.clone(),
        codex_command.clone(),
        codex_home.clone(),
        permission_level,
        &allowed_actions,
        trimmed,
    )
    .await
    .ok()
    .map(|decision| decision.route);

    let (reply_text, provider_label, outcome, detail, agent_meta) = match classified_route {
        Some(agent::types::TopLevelIntent::TestRequest) => match test_agent::router::maybe_handle_test_message(
            &app,
            &provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            codex_command.clone(),
            codex_home.clone(),
            permission_level,
            &allowed_actions,
            trimmed,
            true,
        )
        .await
        {
            Ok(Some(result)) => (
                result.reply_text,
                result.provider_label,
                result.outcome,
                result.detail,
                Some(result.meta),
            ),
            Ok(None) => provider_response(
                &provider_config,
                api_key,
                oauth_access_token,
                codex_command,
                codex_home,
                permission_level,
                &allowed_actions,
                &history_window,
            )
            .await,
            Err(error) => (
                provider::fallback_reply(&error),
                "Safety fallback".to_string(),
                "fallback".to_string(),
                error,
                None,
            ),
        },
        Some(agent::types::TopLevelIntent::DesktopAction) => match agent_router::maybe_handle_control_message(
            &app,
            &provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            &vision_channel,
            vision_api_key.clone(),
            codex_command.clone(),
            codex_home.clone(),
            permission_level,
            &allowed_actions,
            trimmed,
            true,
        )
        .await
        {
            Ok(Some(result)) => (
                result.reply_text,
                result.provider_label,
                result.outcome,
                result.detail,
                Some(result.meta),
            ),
            Ok(None) => provider_response(
                &provider_config,
                api_key,
                oauth_access_token,
                codex_command,
                codex_home,
                permission_level,
                &allowed_actions,
                &history_window,
            )
            .await,
            Err(error) => (
                provider::fallback_reply(&error),
                "Safety fallback".to_string(),
                "fallback".to_string(),
                error,
                None,
            ),
        },
        Some(agent::types::TopLevelIntent::Chat) | Some(agent::types::TopLevelIntent::DebugRequest) => {
            provider_response(
                &provider_config,
                api_key,
                oauth_access_token,
                codex_command,
                codex_home,
                permission_level,
                &allowed_actions,
                &history_window,
            )
            .await
        }
        Some(agent::types::TopLevelIntent::MemoryRequest) => (
            "当前还没有启用真正的持久化记忆系统，但会保存本地历史数据：输入历史、今日回复历史、测试历史和回归记录。后续会在此基础上再扩成正式记忆层。".to_string(),
            "Memory Info".to_string(),
            "memory_info".to_string(),
            "top_level_intent=memory_request".to_string(),
            None,
        ),
        Some(agent::types::TopLevelIntent::ConfirmationResponse) => (
            "如果界面上当前没有待确认条，说明这次没有等待你确认的动作。真正的高风险动作仍然只会在确认面板或 /confirm /cancel 流程里继续执行。".to_string(),
            "Confirmation Info".to_string(),
            "confirmation_info".to_string(),
            "top_level_intent=confirmation_response".to_string(),
            None,
        ),
        None => match test_agent::router::maybe_handle_test_message(
            &app,
            &provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            codex_command.clone(),
            codex_home.clone(),
            permission_level,
            &allowed_actions,
            trimmed,
            false,
        )
        .await {
            Ok(Some(result)) => (
                result.reply_text,
                result.provider_label,
                result.outcome,
                result.detail,
                Some(result.meta),
            ),
            Ok(None) => match agent_router::maybe_handle_control_message(
                &app,
                &provider_config,
                api_key.clone(),
                oauth_access_token.clone(),
                &vision_channel,
                vision_api_key,
                codex_command.clone(),
                codex_home.clone(),
                permission_level,
                &allowed_actions,
                trimmed,
                false,
            )
            .await
            {
                Ok(Some(result)) => (
                    result.reply_text,
                    result.provider_label,
                    result.outcome,
                    result.detail,
                    Some(result.meta),
                ),
                Ok(None) => {
                    provider_response(
                        &provider_config,
                        api_key,
                        oauth_access_token,
                        codex_command,
                        codex_home,
                        permission_level,
                        &allowed_actions,
                        &history_window,
                    )
                    .await
                }
                Err(error) => (
                    provider::fallback_reply(&error),
                    "Safety fallback".to_string(),
                    "fallback".to_string(),
                    error,
                    None,
                ),
            },
            Err(error) => (
                provider::fallback_reply(&error),
                "Safety fallback".to_string(),
                "fallback".to_string(),
                error,
                None,
            ),
        },
    };

    let reply_message = ChatMessage::assistant(reply_text);

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.messages.push(reply_message.clone());
        runtime.mode = PetMode::Idle;
        memory::trim_history(&mut runtime.messages);

        if let Err(error) = history::record_input_history(&app, trimmed) {
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record(
                    "input_history",
                    "warn",
                    format!("输入历史写入失败：{error}"),
                    0,
                ),
            );
        }

        if let Err(error) = history::record_reply_history(&app, trimmed, &reply_message.content) {
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record(
                    "reply_history",
                    "warn",
                    format!("今日回复历史写入失败：{error}"),
                    0,
                ),
            );
        }

        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record("chat_completion", &outcome, detail, 1),
        );
        save(&app, &runtime)?;
        snapshot_from_runtime(&runtime)
    };

    Ok(ChatResponse {
        reply: reply_message,
        provider_label,
        snapshot,
        agent: agent_meta,
    })
}

#[tauri::command]
fn request_desktop_action(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    action_id: String,
) -> Result<ActionExecutionResult, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);

    let action = policy::resolve_action(&action_id, runtime.permission_level)
        .ok_or_else(|| "动作不在白名单中或当前权限不足".to_string())?;
    policy::validate_action_access(&action, runtime.permission_level)?;

    if action.requires_confirmation {
        let approval = policy::build_action_approval(&action);
        runtime
            .pending_action_approvals
            .retain(|item| item.action.id != action.id);
        runtime.pending_action_approvals.push(approval.clone());
        let audit_detail = format!(
            "action={} approval_id={} expires_at={}",
            action.id, approval.id, approval.expires_at
        );
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record("action_approval_requested", "pending", audit_detail, action.risk_level),
        );
        save(&app, &runtime)?;
        return Ok(ActionExecutionResult {
            status: "needs_confirmation".to_string(),
            message: format!("{} 需要逐项确认后才能执行。", action.title),
            snapshot: snapshot_from_runtime(&runtime),
            approval_request: Some(approval),
        });
    }

    let execution = desktop::execute_action(&app, &action.id);
    let (status, message) = match execution {
        Ok(message) => ("ok".to_string(), message),
        Err(error) => ("blocked".to_string(), error),
    };

    runtime.mode = PetMode::Idle;
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record(&action.id, &status, &message, action.risk_level),
    );
    save(&app, &runtime)?;

    Ok(ActionExecutionResult {
        status,
        message,
        snapshot: snapshot_from_runtime(&runtime),
        approval_request: None,
    })
}

#[tauri::command]
fn confirm_desktop_action(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    approval_id: String,
    typed_phrase: String,
    acknowledged_checks: Vec<String>,
) -> Result<ActionExecutionResult, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);

    let approval = runtime
        .pending_action_approvals
        .iter()
        .find(|item| item.id == approval_id)
        .cloned()
        .ok_or_else(|| "未找到待确认的动作授权。".to_string())?;
    let action = policy::resolve_action(&approval.action.id, runtime.permission_level)
        .ok_or_else(|| "动作不在白名单中或当前权限不足。".to_string())?;
    policy::validate_action_access(&action, runtime.permission_level)?;
    if let Err(error) = policy::validate_approval(&approval, &typed_phrase, &acknowledged_checks) {
        let reason = if error.contains("过期") {
            "approval_expired"
        } else if error.contains("确认短语") {
            "approval_phrase_mismatch"
        } else if error.contains("确认项") {
            "approval_checks_incomplete"
        } else {
            "approval_validation_failed"
        };
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "action_approval_rejected",
                "error",
                format!("action={} reason={}", action.id, reason),
                action.risk_level,
            ),
        );
        save(&app, &runtime)?;
        return Err(error);
    }

    runtime
        .pending_action_approvals
        .retain(|item| item.id != approval_id);
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record(
            "action_approval_confirmed",
            "ok",
            format!("action={} phrase_ok=true", action.id),
            action.risk_level,
        ),
    );

    let execution = desktop::execute_action(&app, &action.id);
    let (status, message) = match execution {
        Ok(message) => ("ok".to_string(), message),
        Err(error) => ("blocked".to_string(), error),
    };

    runtime.mode = PetMode::Idle;
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record(&action.id, &status, &message, action.risk_level),
    );
    save(&app, &runtime)?;

    Ok(ActionExecutionResult {
        status,
        message,
        snapshot: snapshot_from_runtime(&runtime),
        approval_request: None,
    })
}

#[tauri::command]
fn cancel_desktop_action_approval(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    approval_id: String,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    let cancelled = runtime
        .pending_action_approvals
        .iter()
        .find(|item| item.id == approval_id)
        .cloned();
    runtime
        .pending_action_approvals
        .retain(|item| item.id != approval_id);

    if let Some(approval) = cancelled {
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "action_approval_cancelled",
                "ok",
                format!("action={} cancelled_by_user=true", approval.action.id),
                approval.action.risk_level,
            ),
        );
    }

    save(&app, &runtime)?;
    Ok(snapshot_from_runtime(&runtime))
}

#[tauri::command]
fn clear_conversation(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    runtime.mode = PetMode::Idle;
    runtime.pending_action_approvals.clear();
    runtime.messages = vec![ChatMessage::assistant(
        "会话已经清空。现在重新回到严格白名单模式，你可以继续测试 UI、语音、OAuth 和动作面板。",
    )];
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record("clear_conversation", "ok", "用户主动清空了会话历史。", 0),
    );
    save(&app, &runtime)?;
    Ok(snapshot_from_runtime(&runtime))
}

#[tauri::command]
fn get_input_history(app: AppHandle) -> Result<Vec<String>, String> {
    history::get_input_history(&app)
}

#[tauri::command]
fn get_today_reply_history(app: AppHandle) -> Result<Vec<ReplyHistoryEntry>, String> {
    history::get_today_reply_history(&app)
}

#[tauri::command]
fn clear_today_reply_history(app: AppHandle) -> Result<Vec<ReplyHistoryEntry>, String> {
    history::clear_today_reply_history(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let runtime = load(&app.handle())
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            app.manage(Mutex::new(runtime));
            app.manage(ControlServiceState::new());
            app.manage(AgentTaskState::new());
            app.manage(TestingState::new());
            let _ = history::prepare_storage(&app.handle());

            let control_service_status = match control::http::start(app.handle().clone()) {
                Ok(address) => {
                    eprintln!("PenguinPal local control service listening on {address}");
                    ("ok", format!("控制服务已启动：{address}"), 1u8)
                }
                Err(error) => {
                    eprintln!("PenguinPal local control service failed to start: {error}");
                    ("error", format!("控制服务启动失败：{error}"), 1u8)
                }
            };

            let runtime_state: State<'_, Mutex<RuntimeState>> = app.state();
            if let Ok(mut runtime) = runtime_state.lock() {
                audit::push_entry(
                    &mut runtime.audit_trail,
                    audit::record(
                        "control_service_startup",
                        control_service_status.0,
                        control_service_status.1,
                        control_service_status.2,
                    ),
                );
                let _ = save(&app.handle(), &runtime);
            }

            tray::create_tray(app)?;

            if let Some(window) = app.get_webview_window("main") {
                window::setup_window(&window)?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            show_settings_window,
            hide_settings_window,
            hide_main_window,
            start_main_window_drag,
            get_assistant_snapshot,
            save_provider_config,
            start_oauth_sign_in,
            start_oauth_sign_in_auto,
            complete_oauth_sign_in,
            disconnect_oauth_sign_in,
            get_codex_cli_status,
            get_control_service_status,
            confirm_control_pending,
            cancel_control_pending,
            start_codex_cli_login,
            send_chat_message,
            request_desktop_action,
            confirm_desktop_action,
            cancel_desktop_action_approval,
            clear_conversation,
            get_input_history,
            get_today_reply_history,
            clear_today_reply_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
