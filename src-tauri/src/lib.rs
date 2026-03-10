mod ai;
mod app_state;
mod audio;
mod desktop;
mod security;
mod tray;
mod window;

use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

use crate::{
    ai::{memory, provider},
    app_state::{
        default_system_prompt, load, now_millis, save, ActionExecutionResult,
        AssistantSnapshot, AuthMode, ChatMessage, ChatResponse, OAuthFlowResult, PetMode,
        ProviderConfigInput, RuntimeState, DEFAULT_OAUTH_REDIRECT_URL,
    },
    security::{audit, oauth, policy},
};

fn snapshot_from_runtime(runtime: &RuntimeState) -> AssistantSnapshot {
    runtime.to_snapshot(
        audio::default_audio_profile(),
        policy::actions_for_level(runtime.permission_level),
    )
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
    let (provider_config, api_key, oauth_access_token, history_window) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);
        runtime.mode = PetMode::Thinking;
        runtime.messages.push(user_message);
        memory::trim_history(&mut runtime.messages);
        save(&app, &runtime)?;
        (
            runtime.provider.clone(),
            runtime.api_key.clone(),
            runtime.oauth_access_token.clone(),
            memory::context_window(&runtime.messages),
        )
    };

    let (reply_text, provider_label, outcome, detail) =
        match provider::respond(&provider_config, api_key, oauth_access_token, &history_window).await {
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
            ),
            Err(error) => (
                provider::fallback_reply(&error),
                "Safety fallback".to_string(),
                "fallback".to_string(),
                error,
            ),
        };

    let reply_message = ChatMessage::assistant(reply_text);

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.messages.push(reply_message.clone());
        runtime.mode = PetMode::Idle;
        memory::trim_history(&mut runtime.messages);
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let runtime = load(&app.handle())
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            app.manage(Mutex::new(runtime));

            tray::create_tray(app)?;

            if let Some(window) = app.get_webview_window("main") {
                window::setup_window(&window)?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_assistant_snapshot,
            save_provider_config,
            start_oauth_sign_in,
            complete_oauth_sign_in,
            disconnect_oauth_sign_in,
            send_chat_message,
            request_desktop_action,
            confirm_desktop_action,
            cancel_desktop_action_approval,
            clear_conversation
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
