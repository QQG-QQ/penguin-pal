use tauri::{AppHandle, Manager, State};

use super::{types::AgentTaskRun, AgentTaskState};

pub fn has_active_task(app: &AppHandle) -> Result<bool, String> {
    let state: State<'_, AgentTaskState> = app.state();
    let task = state.active_task()?;
    Ok(task.is_some())
}

pub fn replace_active_task(app: &AppHandle, next_task: Option<AgentTaskRun>) -> Result<(), String> {
    let state: State<'_, AgentTaskState> = app.state();
    let mut task = state.active_task()?;
    *task = next_task;
    Ok(())
}

pub fn current_task(app: &AppHandle) -> Result<Option<AgentTaskRun>, String> {
    let state: State<'_, AgentTaskState> = app.state();
    let task = state.active_task()?;
    Ok(task.clone())
}

pub fn peek_task_waiting_on_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<AgentTaskRun>, String> {
    let state: State<'_, AgentTaskState> = app.state();
    let task = state.active_task()?;
    let matches = task
        .as_ref()
        .and_then(|item| item.waiting_pending_id.as_ref())
        .is_some_and(|item| item == pending_id);
    if matches {
        Ok(task.clone())
    } else {
        Ok(None)
    }
}

pub fn take_task_waiting_on_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<AgentTaskRun>, String> {
    let state: State<'_, AgentTaskState> = app.state();
    let mut task = state.active_task()?;
    let matches = task
        .as_ref()
        .and_then(|item| item.waiting_pending_id.as_ref())
        .is_some_and(|item| item == pending_id);
    if matches {
        Ok(task.take())
    } else {
        Ok(None)
    }
}
