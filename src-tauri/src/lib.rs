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
        default_system_prompt, load, save, ActionExecutionResult, AssistantSnapshot, ChatMessage,
        ChatResponse, PetMode, ProviderConfigInput, RuntimeState,
    },
    security::{audit, policy},
};

fn snapshot_from_runtime(runtime: &RuntimeState) -> AssistantSnapshot {
    runtime.to_snapshot(
        audio::default_audio_profile(),
        policy::actions_for_level(runtime.permission_level),
    )
}

#[tauri::command]
fn get_assistant_snapshot(
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<AssistantSnapshot, String> {
    let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    Ok(snapshot_from_runtime(&runtime))
}

#[tauri::command]
fn save_provider_config(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    input: ProviderConfigInput,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;

    runtime.provider.kind = input.kind.clone();
    runtime.provider.model = if input.model.trim().is_empty() {
        input.kind.default_model().to_string()
    } else {
        input.model.trim().to_string()
    };
    runtime.provider.base_url = input
        .base_url
        .clone()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string());
    runtime.provider.system_prompt = if input.system_prompt.trim().is_empty() {
        default_system_prompt()
    } else {
        input.system_prompt.trim().to_string()
    };
    runtime.provider.allow_network = input.allow_network;
    runtime.provider.voice_reply = input.voice_reply;
    runtime.provider.retain_history = input.retain_history;
    runtime.permission_level = policy::clamp_permission_level(input.permission_level);

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

    runtime.provider.api_key_loaded = runtime
        .api_key
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    runtime.mode = PetMode::Idle;

    let audit_detail = format!(
        "provider={} network={} permission=L{}",
        runtime.provider.kind.label(),
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
    let (provider_config, api_key, history_window) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.mode = PetMode::Thinking;
        runtime.messages.push(user_message);
        memory::trim_history(&mut runtime.messages);
        save(&app, &runtime)?;
        (
            runtime.provider.clone(),
            runtime.api_key.clone(),
            memory::context_window(&runtime.messages),
        )
    };

    let (reply_text, provider_label, outcome, detail) =
        match provider::respond(&provider_config, api_key, &history_window).await {
            Ok((reply, label)) => (
                reply,
                label,
                "ok".to_string(),
                format!(
                    "provider={} model={}",
                    provider_config.kind.label(),
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
    confirmed: bool,
) -> Result<ActionExecutionResult, String> {
    let permission_level = {
        let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.permission_level
    };

    let action = policy::resolve_action(&action_id, permission_level)
        .ok_or_else(|| "动作不在白名单中或当前权限不足".to_string())?;
    policy::validate_action(&action, permission_level, confirmed)?;

    let execution = desktop::execute_action(&app, &action.id);
    let (status, message) = match execution {
        Ok(message) => ("ok".to_string(), message),
        Err(error) => ("blocked".to_string(), error),
    };

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.mode = PetMode::Idle;
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(&action.id, &status, &message, action.risk_level),
        );
        save(&app, &runtime)?;
        snapshot_from_runtime(&runtime)
    };

    Ok(ActionExecutionResult {
        status,
        message,
        snapshot,
    })
}

#[tauri::command]
fn clear_conversation(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    runtime.mode = PetMode::Idle;
    runtime.messages = vec![ChatMessage::assistant(
        "会话已经清空。现在重新回到严格白名单模式，你可以继续测试 UI、语音和动作面板。",
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
            let runtime = load(&app.handle()).map_err(|error| {
                std::io::Error::new(std::io::ErrorKind::Other, error)
            })?;
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
            send_chat_message,
            request_desktop_action,
            clear_conversation
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
