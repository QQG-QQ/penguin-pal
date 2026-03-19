use chrono::{TimeZone, Utc};
use tauri::AppHandle;

use crate::{
    app_state::{ChatMessage, RuntimeState},
    codex_config::{SessionManager, SessionMessage},
    codex_runtime::codex_config_dir,
};

fn build_manager(app: &AppHandle) -> Result<SessionManager, String> {
    let codex_home = codex_config_dir(app)?;
    Ok(SessionManager::new(&codex_home))
}

fn timestamp_to_utc(millis: u64) -> Result<chrono::DateTime<Utc>, String> {
    Utc.timestamp_millis_opt(millis as i64)
        .single()
        .ok_or_else(|| format!("无效的消息时间戳: {}", millis))
}

fn sync_session_from_runtime(
    manager: &mut SessionManager,
    messages: &[ChatMessage],
) -> Result<(), String> {
    if manager
        .current()
        .map(|session| !session.messages.is_empty())
        .unwrap_or(false)
    {
        return Ok(());
    }

    for message in messages {
        manager.add_message_at(
            &message.role,
            &message.content,
            timestamp_to_utc(message.created_at)?,
        )?;
    }
    Ok(())
}

fn sync_runtime_from_session(runtime: &mut RuntimeState, manager: &SessionManager) {
    let Some(session) = manager.current() else {
        return;
    };

    if session.messages.is_empty() {
        return;
    }

    runtime.messages = chat_messages_from_session(&session.messages);
}

fn chat_messages_from_session(messages: &[SessionMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .enumerate()
        .map(|(index, message)| ChatMessage {
            id: format!(
                "session-{}-{}",
                message.timestamp.timestamp_millis(),
                index
            ),
            role: message.role.clone(),
            content: message.content.clone(),
            created_at: message.timestamp.timestamp_millis() as u64,
        })
        .collect()
}

fn resume_runtime_thread(
    app: &AppHandle,
    runtime: &mut RuntimeState,
    manager: &mut SessionManager,
) -> Result<(), String> {
    if runtime.session_thread_id.is_none() {
        bootstrap_runtime_thread(app, runtime)?;
    }

    if let Some(thread_id) = runtime.session_thread_id.clone() {
        if manager.resume(&thread_id).is_ok() {
            return Ok(());
        }
    }

    bootstrap_runtime_thread(app, runtime)?;
    let Some(thread_id) = runtime.session_thread_id.clone() else {
        return Err("当前没有活动会话线程。".to_string());
    };
    manager.resume(&thread_id)?;
    Ok(())
}

pub fn bootstrap_runtime_thread(app: &AppHandle, runtime: &mut RuntimeState) -> Result<(), String> {
    let mut manager = build_manager(app)?;

    let current_id = if runtime.provider.retain_history {
        if let Some(thread_id) = runtime.session_thread_id.clone() {
            if manager.resume(&thread_id).is_ok() {
                thread_id
            } else {
                let session = manager.new_session()?;
                session.id.clone()
            }
        } else {
            let session = manager.new_session()?;
            session.id.clone()
        }
    } else {
        let session = manager.new_session()?;
        session.id.clone()
    };

    runtime.session_thread_id = Some(current_id);
    sync_session_from_runtime(&mut manager, &runtime.messages)?;
    if runtime.provider.retain_history {
        sync_runtime_from_session(runtime, &manager);
    }
    manager.save_current()?;
    Ok(())
}

pub fn append_message(app: &AppHandle, runtime: &mut RuntimeState, message: &ChatMessage) -> Result<(), String> {
    let mut manager = build_manager(app)?;
    resume_runtime_thread(app, runtime, &mut manager)?;

    manager.add_message_at(
        &message.role,
        &message.content,
        timestamp_to_utc(message.created_at)?,
    )?;
    Ok(())
}

pub fn start_new_thread(
    app: &AppHandle,
    runtime: &mut RuntimeState,
    seed_messages: &[ChatMessage],
) -> Result<(), String> {
    let mut manager = build_manager(app)?;
    if let Some(thread_id) = runtime.session_thread_id.clone() {
        if manager.resume(&thread_id).is_ok() {
            let _ = manager.complete_current();
        }
    }

    let session = manager.new_session()?;
    runtime.session_thread_id = Some(session.id.clone());
    for message in seed_messages {
        manager.add_message_at(
            &message.role,
            &message.content,
            timestamp_to_utc(message.created_at)?,
        )?;
    }
    Ok(())
}

pub fn recent_messages(
    app: &AppHandle,
    runtime: &mut RuntimeState,
    limit: usize,
) -> Result<Vec<ChatMessage>, String> {
    let mut manager = build_manager(app)?;
    resume_runtime_thread(app, runtime, &mut manager)?;

    let Some(session) = manager.current() else {
        return Ok(runtime.messages.clone());
    };

    if session.messages.is_empty() {
        return Ok(runtime.messages.clone());
    }

    let start = session.messages.len().saturating_sub(limit);
    Ok(chat_messages_from_session(&session.messages[start..]))
}
