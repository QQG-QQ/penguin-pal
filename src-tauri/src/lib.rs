mod agent;
mod ai;
mod app_state;
mod codex_config;
mod codex_runtime;
mod codex_update;
mod control;
mod audio;
mod desktop;
mod history;
mod memory;
mod permission;
mod rule_engine;
mod security;
mod shell_agent;
mod testing;
mod tray;
mod window;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{process::Command, sync::Mutex, time::Duration};
use tauri::{AppHandle, Manager, State};

/// Memory 维护线程停止标志
static MEMORY_MAINTENANCE_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// 请求停止 memory 维护线程
#[allow(dead_code)]
pub fn request_memory_maintenance_shutdown() {
    MEMORY_MAINTENANCE_SHUTDOWN.store(true, Ordering::Relaxed);
}

use crate::{
    agent::{router as agent_router, AgentTaskState},
    ai::{guardrails, memory as ai_memory, provider},
    app_state::{
        default_system_prompt, load, now_millis, save, ActionExecutionResult,
        AssistantSnapshot, AuthMode, ChatMessage, ChatResponse, OAuthFlowResult,
        PendingShellCommand, PendingShellConfirmationInfo, PetMode, ProviderConfigInput,
        ProviderKind, RuntimeState, ShellPermissionSettings, DEFAULT_OAUTH_REDIRECT_URL,
    },
    audio::{types as audio_types, TranscriberService},
    codex_runtime::{apply_private_env, initialize_codex_config, load_codex_config, private_auth_path, resolve_for_app, save_codex_config},
    control::{router as control_router, types::ControlServiceStatus, ControlServiceState},
    history::ReplyHistoryEntry,
    security::{audit, oauth, policy},
    shell_agent::{ShellAgentExecutor, BehaviorState},
    permission::{PermissionScope, GrantSource},
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

/// 将设置中的 Shell 权限同步到 PermissionChecker
fn sync_shell_permissions_to_checker(
    app: &AppHandle,
    settings: &ShellPermissionSettings,
) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;

    let behavior_state = BehaviorState::new(&app_data_dir);

    // 计算权限有效期
    let duration_ms = if settings.duration_hours == 0 {
        None // 永久
    } else {
        Some(settings.duration_hours * 60 * 60 * 1000) // 转换为毫秒
    };

    // 如果 Shell Agent 未启用，撤销所有权限
    if !settings.enabled {
        behavior_state.revoke_all_shell_permissions()?;
        return Ok(());
    }

    // 根据设置授予或撤销权限
    let permission_map = [
        ("shell:execute", settings.allow_execute),
        ("shell:modify", settings.allow_file_modify),
        ("shell:delete", settings.allow_file_delete),
        ("shell:network", settings.allow_network),
        ("shell:system", settings.allow_system),
    ];

    for (permission_id, enabled) in permission_map {
        if enabled {
            behavior_state.grant_permission(permission_id, PermissionScope::Global, duration_ms)?;
        } else {
            behavior_state.revoke_permission(permission_id)?;
        }
    }

    Ok(())
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
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
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

#[tauri::command]
fn get_codex_cli_status(app: AppHandle) -> CodexCliStatus {
    inspect_codex_cli_status(&app)
}

#[tauri::command]
async fn check_codex_update(app: AppHandle) -> Result<codex_update::CodexUpdateStatus, String> {
    let status = inspect_codex_cli_status(&app);
    Ok(codex_update::check_update_status(&app, status.version).await)
}

#[tauri::command]
async fn update_codex(app: AppHandle) -> Result<codex_update::CodexUpdateStatus, String> {
    let install_dir = app
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

    let codex_install_dir = install_dir.join("codex").join(platform_dir);

    // 执行更新
    codex_update::install_or_update_codex(&codex_install_dir, |msg| {
        eprintln!("[Codex Update] {}", msg);
    })?;

    // 返回更新后的状态
    let status = inspect_codex_cli_status(&app);
    Ok(codex_update::check_update_status(&app, status.version).await)
}

#[tauri::command]
fn get_control_service_status(app: AppHandle) -> Result<ControlServiceStatus, String> {
    control_router::service_status(&app).map_err(|error| error.to_string())
}

#[tauri::command]
async fn confirm_control_pending(app: AppHandle, pending_id: String) -> Result<control::types::ToolInvokeResponse, String> {
    agent_router::confirm_control_pending(&app, &pending_id).await
}

#[tauri::command]
async fn cancel_control_pending(app: AppHandle, pending_id: String) -> Result<control::types::ToolInvokeResponse, String> {
    agent_router::cancel_control_pending(&app, &pending_id).await
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

    // Shell Agent 权限设置
    runtime.shell_permissions = input.shell_permissions.clone();
    // 同步到 PermissionChecker
    sync_shell_permissions_to_checker(&app, &input.shell_permissions)?;

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

    // 同步 Shell Agent 权限设置
    let previous_shell_enabled = runtime.shell_permissions.enabled;
    runtime.shell_permissions = input.shell_permissions.clone();

    // 同步到 BehaviorState 权限系统
    if let Ok(app_data) = app.path().app_data_dir() {
        let behavior_state = shell_agent::BehaviorState::new(&app_data);
        let duration_ms = if input.shell_permissions.duration_hours > 0 {
            Some(input.shell_permissions.duration_hours as u64 * 60 * 60 * 1000)
        } else {
            None // 永久
        };

        if input.shell_permissions.enabled {
            // 根据设置授予对应权限
            if input.shell_permissions.allow_execute {
                let _ = behavior_state.grant_permission(
                    "shell:execute",
                    crate::permission::PermissionScope::Global,
                    duration_ms,
                );
            } else {
                let _ = behavior_state.revoke_permission("shell:execute");
            }

            if input.shell_permissions.allow_file_modify {
                let _ = behavior_state.grant_permission(
                    "shell:modify",
                    crate::permission::PermissionScope::Global,
                    duration_ms,
                );
            } else {
                let _ = behavior_state.revoke_permission("shell:modify");
            }

            if input.shell_permissions.allow_file_delete {
                let _ = behavior_state.grant_permission(
                    "shell:delete",
                    crate::permission::PermissionScope::Global,
                    duration_ms,
                );
            } else {
                let _ = behavior_state.revoke_permission("shell:delete");
            }

            if input.shell_permissions.allow_network {
                let _ = behavior_state.grant_permission(
                    "shell:network",
                    crate::permission::PermissionScope::Global,
                    duration_ms,
                );
            } else {
                let _ = behavior_state.revoke_permission("shell:network");
            }

            if input.shell_permissions.allow_system {
                let _ = behavior_state.grant_permission(
                    "shell:system",
                    crate::permission::PermissionScope::Global,
                    duration_ms,
                );
            } else {
                let _ = behavior_state.revoke_permission("shell:system");
            }
        } else if previous_shell_enabled {
            // 如果之前启用现在禁用，撤销所有权限
            let _ = behavior_state.revoke_all_shell_permissions();
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

    // 如果使用 Codex CLI，同步模型设置到 config.toml
    if matches!(runtime.provider.kind, ProviderKind::CodexCli) {
        if let Ok(mut codex_config) = load_codex_config(&app) {
            if codex_config.model != runtime.provider.model {
                codex_config.model = runtime.provider.model.clone();
                let _ = save_codex_config(&app, &codex_config);
                eprintln!("[save_provider_config] Synced model to Codex config: {}", runtime.provider.model);
            }
        }
    }

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

// ============================================================================
// Shell Agent 权限管理
// ============================================================================

/// 权限状态响应
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PermissionStatusResponse {
    granted_permissions: Vec<String>,
    pending_requests: Vec<String>,
    message: String,
}

#[tauri::command]
fn get_shell_permissions(app: AppHandle) -> Result<PermissionStatusResponse, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let behavior_state = BehaviorState::new(&app_data);
    let checker = behavior_state.permission_checker();

    let granted: Vec<String> = checker
        .granted_permissions()
        .iter()
        .map(|p| p.id.clone())
        .collect();

    let pending: Vec<String> = checker
        .pending_requests()
        .iter()
        .map(|r| format!("{}: {}", r.permission_id, r.reason))
        .collect();

    Ok(PermissionStatusResponse {
        granted_permissions: granted,
        pending_requests: pending,
        message: "权限状态查询成功".to_string(),
    })
}

#[tauri::command]
fn grant_shell_permission(
    app: AppHandle,
    permission_id: String,
    duration_hours: Option<u64>,
) -> Result<PermissionStatusResponse, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let behavior_state = BehaviorState::new(&app_data);

    let duration_ms = duration_hours.map(|h| h * 60 * 60 * 1000);

    {
        let mut checker = behavior_state.permission_checker();
        checker.grant(
            &permission_id,
            GrantSource::User,
            PermissionScope::Session,
            duration_ms,
        )?;
    }

    // 保存状态
    behavior_state.save()?;

    // 返回更新后的状态
    get_shell_permissions(app)
}

#[tauri::command]
fn revoke_shell_permission(
    app: AppHandle,
    permission_id: String,
) -> Result<PermissionStatusResponse, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let behavior_state = BehaviorState::new(&app_data);

    {
        let mut checker = behavior_state.permission_checker();
        checker.revoke(&permission_id, "user")?;
    }

    behavior_state.save()?;
    get_shell_permissions(app)
}

#[tauri::command]
fn grant_basic_shell_access(app: AppHandle) -> Result<PermissionStatusResponse, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let behavior_state = BehaviorState::new(&app_data);

    // 授予基本 shell 执行权限（24 小时）
    let duration_ms = Some(24 * 60 * 60 * 1000);

    {
        let mut checker = behavior_state.permission_checker();
        checker.grant("shell:execute", GrantSource::User, PermissionScope::Session, duration_ms)?;
    }

    behavior_state.save()?;
    get_shell_permissions(app)
}

// ============================================================================
// Whisper 语音识别命令
// ============================================================================

#[tauri::command]
fn get_whisper_status(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::WhisperStatus, String> {
    Ok(transcriber.get_status())
}

#[tauri::command]
fn get_whisper_models(
    transcriber: State<'_, TranscriberService>,
) -> Result<Vec<audio_types::ModelInfo>, String> {
    Ok(transcriber.get_available_models())
}

#[tauri::command]
async fn download_whisper_model(
    transcriber: State<'_, TranscriberService>,
    model: audio_types::WhisperModel,
    progress: tauri::ipc::Channel<audio_types::DownloadProgress>,
) -> Result<String, String> {
    let path = transcriber.download_model(model, progress).await?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn load_whisper_model(
    transcriber: State<'_, TranscriberService>,
    model: audio_types::WhisperModel,
) -> Result<audio_types::WhisperStatus, String> {
    transcriber.load_model(model)?;
    Ok(transcriber.get_status())
}

#[tauri::command]
fn unload_whisper_model(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::WhisperStatus, String> {
    transcriber.unload_model();
    Ok(transcriber.get_status())
}

#[tauri::command]
fn delete_whisper_model(
    transcriber: State<'_, TranscriberService>,
    model: audio_types::WhisperModel,
) -> Result<audio_types::WhisperStatus, String> {
    transcriber.delete_model(model)?;
    Ok(transcriber.get_status())
}

#[tauri::command]
fn start_whisper_recording(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::RecordingState, String> {
    transcriber.start_recording()?;
    Ok(transcriber.get_recording_state())
}

#[tauri::command]
fn stop_whisper_recording(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::TranscriptionResult, String> {
    transcriber.stop_recording()
}

#[tauri::command]
fn get_whisper_recording_state(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::RecordingState, String> {
    Ok(transcriber.get_recording_state())
}

/// 执行待确认的 Shell 命令
async fn execute_pending_shell_command(
    app: &AppHandle,
    state: &State<'_, Mutex<RuntimeState>>,
    pending: &PendingShellCommand,
) -> Result<ChatResponse, String> {
    // 使用 Shell Executor 执行命令
    let mut shell_executor = if let Ok(app_data) = app.path().app_data_dir() {
        ShellAgentExecutor::with_app_data(&app_data)
    } else {
        ShellAgentExecutor::new()
    };

    let exec_result = shell_executor.confirm_and_continue(&pending.command);

    let reply_text = if exec_result.success {
        format!("命令已执行：\n```\n{}\n```\n\n输出：\n```\n{}\n```",
            pending.command, exec_result.output)
    } else {
        format!("命令执行失败：\n```\n{}\n```\n\n错误：{}", pending.command, exec_result.output)
    };

    let reply_message = ChatMessage::assistant(&reply_text);

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.pending_shell_command = None;
        runtime.messages.push(ChatMessage::user("yes".to_string()));
        runtime.messages.push(reply_message.clone());
        runtime.mode = PetMode::Idle;
        ai_memory::trim_history(&mut runtime.messages);

        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "shell_command_confirmed",
                if exec_result.success { "ok" } else { "error" },
                format!("command={}", pending.command),
                1,
            ),
        );
        save(app, &runtime)?;
        snapshot_from_runtime(&runtime)
    };

    Ok(ChatResponse {
        reply: reply_message,
        provider_label: "Shell Agent".to_string(),
        snapshot,
        agent: None,
        pending_shell_confirmation: None,
    })
}

/// 取消待确认的 Shell 命令
async fn cancel_pending_shell_command(
    app: &AppHandle,
    state: &State<'_, Mutex<RuntimeState>>,
    pending: &PendingShellCommand,
) -> Result<ChatResponse, String> {
    let reply_text = format!("已取消执行命令：`{}`", pending.command);
    let reply_message = ChatMessage::assistant(&reply_text);

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.pending_shell_command = None;
        runtime.messages.push(ChatMessage::user("no".to_string()));
        runtime.messages.push(reply_message.clone());
        runtime.mode = PetMode::Idle;
        ai_memory::trim_history(&mut runtime.messages);

        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record("shell_command_cancelled", "ok", format!("command={}", pending.command), 1),
        );
        save(app, &runtime)?;
        snapshot_from_runtime(&runtime)
    };

    Ok(ChatResponse {
        reply: reply_message,
        provider_label: "Shell Agent".to_string(),
        snapshot,
        agent: None,
        pending_shell_confirmation: None,
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

    // 检查是否有待确认的 Shell 命令
    let pending_cmd = {
        let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.pending_shell_command.clone()
    };

    // 处理 yes/no 确认
    if let Some(pending) = pending_cmd {
        let lower = trimmed.to_lowercase();
        if lower == "yes" || lower == "y" || lower == "确认" {
            // 用户确认执行
            return execute_pending_shell_command(&app, &state, &pending).await;
        } else if lower == "no" || lower == "n" || lower == "取消" {
            // 用户取消执行
            return cancel_pending_shell_command(&app, &state, &pending).await;
        }
        // 其他输入继续正常流程，清除待确认命令
        {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.pending_shell_command = None;
        }
    }

    let user_message = ChatMessage::user(trimmed.to_string());
    let (
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        _history_window,  // Shell Agent 自己管理上下文
        permission_level,
        allowed_actions,
    ) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);
        runtime.mode = PetMode::Thinking;
        runtime.messages.push(user_message);
        ai_memory::trim_history(&mut runtime.messages);
        save(&app, &runtime)?;
        let allowed_actions = policy::actions_for_level(runtime.permission_level);
        let codex_runtime = resolve_for_app(&app).ok();
        (
            runtime.provider.clone(),
            runtime.api_key.clone(),
            runtime.oauth_access_token.clone(),
            codex_runtime
                .as_ref()
                .and_then(|item| item.command.as_ref())
                .map(|path| path.to_string_lossy().to_string()),
            codex_runtime
                .as_ref()
                .map(|item| item.home_root.to_string_lossy().to_string()),
            ai_memory::context_window(&runtime.messages),
            runtime.permission_level,
            allowed_actions,
        )
    };

    // Shell Agent 自主循环：AI 完全自主决定行动
    let mut shell_executor = if let Ok(app_data) = app.path().app_data_dir() {
        ShellAgentExecutor::with_app_data(&app_data)
    } else {
        ShellAgentExecutor::new()
    };

    // 创建 AI 调用闭包
    let provider_config_clone = provider_config.clone();
    let api_key_clone = api_key.clone();
    let oauth_token_clone = oauth_access_token.clone();
    let codex_cmd_clone = codex_command.clone();
    let codex_home_clone = codex_home.clone();
    let allowed_actions_clone = allowed_actions.clone();

    let ai_caller = |system_prompt: String, context: String| {
        let provider = provider_config_clone.clone();
        let key = api_key_clone.clone();
        let token = oauth_token_clone.clone();
        let cmd = codex_cmd_clone.clone();
        let home = codex_home_clone.clone();
        let actions = allowed_actions_clone.clone();
        let perm = permission_level;

        async move {
            // 构建包含 shell agent 上下文的历史
            let messages = vec![
                ChatMessage::new("system", system_prompt),
                ChatMessage::user(context),
            ];

            provider::respond(
                &provider,
                key,
                token,
                cmd,
                home,
                perm,
                &actions,
                &messages,
            )
            .await
            .map(|(reply, _label)| reply)
        }
    };

    let result = shell_executor.run(&app, trimmed, ai_caller).await;

    // 记录是否需要退出应用
    let should_exit = result.request_exit;

    // 提取待确认信息（如果有）并存储到 RuntimeState
    let pending_shell_confirmation = if let Some(p) = &result.pending_confirmation {
        // 存储待确认命令到 RuntimeState
        {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.pending_shell_command = Some(PendingShellCommand {
                id: p.id.clone(),
                command: p.command.clone(),
                risk_description: p.risk_description.clone(),
                created_at: p.created_at,
            });
        }
        Some(PendingShellConfirmationInfo {
            id: p.id.clone(),
            command: p.command.clone(),
            risk_description: p.risk_description.clone(),
            created_at: p.created_at,
        })
    } else {
        None
    };

    let (reply_text, provider_label, outcome, detail) = if result.pending_confirmation.is_some() {
        // 需要用户确认
        let pending = result.pending_confirmation.as_ref().unwrap();
        (
            format!(
                "需要确认执行以下命令：\n\n```\n{}\n```\n\n{}",
                pending.command,
                pending.risk_description
            ),
            "Shell Agent".to_string(),
            "pending_confirmation".to_string(),
            format!("pending_id={}", pending.id),
        )
    } else if result.success {
        (
            result.message,
            "Shell Agent".to_string(),
            "ok".to_string(),
            format!("steps={}", result.steps_executed),
        )
    } else {
        (
            result.message,
            "Shell Agent".to_string(),
            "error".to_string(),
            format!("steps={}", result.steps_executed),
        )
    };

    let reply_message = ChatMessage::assistant(reply_text);

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.messages.push(reply_message.clone());
        runtime.mode = PetMode::Idle;
        ai_memory::trim_history(&mut runtime.messages);

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

    // 如果请求退出应用，延迟一小段时间后退出（让 UI 有时间显示告别语）
    if should_exit {
        let app_clone = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            app_clone.exit(0);
        });
    }

    Ok(ChatResponse {
        reply: reply_message,
        provider_label,
        snapshot,
        agent: None,
        pending_shell_confirmation,
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
            eprintln!("[Setup] Starting application setup...");

            // 更健壮的状态加载：如果加载失败则使用默认状态
            let runtime = match load(&app.handle()) {
                Ok(runtime) => {
                    eprintln!("[Setup] RuntimeState loaded successfully");
                    runtime
                }
                Err(error) => {
                    eprintln!("[Setup] Failed to load runtime state: {}, using default", error);
                    RuntimeState::default()
                }
            };
            app.manage(Mutex::new(runtime));
            eprintln!("[Setup] RuntimeState managed");

            app.manage(ControlServiceState::new());
            eprintln!("[Setup] ControlServiceState managed");

            app.manage(AgentTaskState::new());
            eprintln!("[Setup] AgentTaskState managed");

            // 初始化 Whisper 语音识别服务
            let app_data_dir = app.path().app_data_dir()
                .map_err(|e| {
                    eprintln!("[Setup] Failed to get app_data_dir: {}", e);
                    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                })?;
            eprintln!("[Setup] app_data_dir: {:?}", app_data_dir);
            match TranscriberService::new(app_data_dir) {
                Ok(transcriber) => {
                    app.manage(transcriber);
                    eprintln!("Whisper transcriber service initialized");
                }
                Err(e) => {
                    eprintln!("Failed to initialize Whisper transcriber: {}", e);
                }
            }

            let _ = history::prepare_storage(&app.handle());

            // 初始化 Codex 配置目录结构
            if let Err(error) = initialize_codex_config(&app.handle()) {
                eprintln!("Codex config initialization failed: {error}");
            }

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

            // 启动三层架构维护后台任务（记忆 + 规则 + 权限）
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // 启动时执行一次维护
                if let Ok(app_data) = app_handle.path().app_data_dir() {
                    let behavior_state = shell_agent::BehaviorState::new(&app_data);
                    let result = behavior_state.run_maintenance();
                    if result.total_changes() > 0 {
                        eprintln!(
                            "Behavior maintenance: memory(decayed={}, merged={}, pruned={}), rules_generated={}",
                            result.memory_decayed, result.memory_merged, result.memory_pruned,
                            result.rules_generated
                        );
                    }
                }

                // 每小时执行一次维护（分段 sleep 以便响应 shutdown 信号）
                const MAINTENANCE_INTERVAL_SECS: u64 = 3600;
                const SLEEP_CHECK_INTERVAL_SECS: u64 = 60;

                while !MEMORY_MAINTENANCE_SHUTDOWN.load(Ordering::Relaxed) {
                    // 分段 sleep，每 60 秒检查一次 shutdown 信号
                    for _ in 0..(MAINTENANCE_INTERVAL_SECS / SLEEP_CHECK_INTERVAL_SECS) {
                        if MEMORY_MAINTENANCE_SHUTDOWN.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(SLEEP_CHECK_INTERVAL_SECS));
                    }

                    if MEMORY_MAINTENANCE_SHUTDOWN.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Ok(app_data) = app_handle.path().app_data_dir() {
                        let behavior_state = shell_agent::BehaviorState::new(&app_data);
                        let result = behavior_state.run_maintenance();
                        if result.total_changes() > 0 {
                            eprintln!(
                                "Behavior maintenance: memory(decayed={}, merged={}, pruned={}), rules_generated={}",
                                result.memory_decayed, result.memory_merged, result.memory_pruned,
                                result.rules_generated
                            );
                        }
                    }
                }
                eprintln!("Behavior maintenance thread stopped.");
            });

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
            check_codex_update,
            update_codex,
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
            clear_today_reply_history,
            // Shell Agent 权限管理
            get_shell_permissions,
            grant_shell_permission,
            revoke_shell_permission,
            grant_basic_shell_access,
            // Whisper 语音识别
            get_whisper_status,
            get_whisper_models,
            download_whisper_model,
            load_whisper_model,
            unload_whisper_model,
            delete_whisper_model,
            start_whisper_recording,
            stop_whisper_recording,
            get_whisper_recording_state
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
