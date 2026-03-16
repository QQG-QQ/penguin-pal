use serde_json::Value;
use tauri::AppHandle;

use crate::control::windows::{clipboard, windowing};

use super::{
    runtime_binding,
    screen_context,
    types::{AgentTaskRun, RuntimeContext, RuntimeObservation, RuntimeToolResult},
};
use crate::app_state::VisionChannelConfig;

pub async fn refresh_runtime_context(
    app: &AppHandle,
    task: &mut AgentTaskRun,
    vision_channel: &VisionChannelConfig,
    vision_api_key: Option<String>,
) -> RuntimeContext {
    let screen = screen_context::describe_current_screen(app, vision_channel, vision_api_key).await;
    let windows = match windowing::list_windows(app) {
        Ok(value) => value.as_array().cloned().unwrap_or_default(),
        Err(_) => vec![],
    };
    let clipboard_text = match clipboard::read_clipboard(app) {
        Ok(value) => value
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        Err(_) => None,
    };

    let mut context = RuntimeContext {
        raw_user_input: task.original_request.clone(),
        normalized_goal: task.goal.clone(),
        task_status: task.task_status.clone(),
        active_window: serde_json::to_value(&screen.active_window).ok(),
        window_inventory: windows,
        uia_summary: screen.uia.as_ref().and_then(|uia| serde_json::to_value(uia).ok()),
        vision_summary: screen
            .vision
            .as_ref()
            .and_then(|vision| serde_json::to_value(vision).ok()),
        clipboard: clipboard_text,
        recent_tool_results: task
            .runtime_context
            .as_ref()
            .map(|existing| existing.recent_tool_results.clone())
            .unwrap_or_default(),
        recent_observations: task
            .runtime_context
            .as_ref()
            .map(|existing| existing.recent_observations.clone())
            .unwrap_or_default(),
        discovered_entities: task
            .runtime_context
            .as_ref()
            .map(|existing| existing.discovered_entities.clone())
            .unwrap_or_default(),
        consistency: Some(screen_context_consistency(&screen)),
        last_error: task.failure_reason.clone(),
    };

    context.recent_observations.push(RuntimeObservation {
        step: task.recent_steps.len(),
        source: "screen_context".to_string(),
        summary: format!(
            "活动窗口={} consistency={}",
            screen.active_window.title,
            screen_context_consistency(&screen)
        ),
        payload: serde_json::to_value(&screen).ok(),
    });
    if context.recent_observations.len() > 8 {
        let drain = context.recent_observations.len() - 8;
        context.recent_observations.drain(0..drain);
    }

    runtime_binding::merge_screen_context_entities(&mut context, &screen, task.recent_steps.len());
    if let Some(last_result) = task.last_tool_result.clone() {
        runtime_binding::merge_tool_result_entities(
            &mut context,
            task.recent_steps
                .last()
                .and_then(|step| step.tool.as_deref())
                .unwrap_or_default(),
            &last_result,
            task.recent_steps.len(),
        );
    }

    task.last_observation = serde_json::to_value(&screen).ok();
    task.runtime_context = Some(context.clone());
    context
}

pub fn append_runtime_observation(
    task: &mut AgentTaskRun,
    source: &str,
    summary: String,
    payload: Option<Value>,
) {
    let step = task.recent_steps.len();
    let mut context = task
        .runtime_context
        .clone()
        .unwrap_or_else(|| empty_runtime_context(task));
    sync_context_header(task, &mut context);
    context.recent_observations.push(RuntimeObservation {
        step,
        source: source.to_string(),
        summary,
        payload,
    });
    if context.recent_observations.len() > 8 {
        let drain = context.recent_observations.len() - 8;
        context.recent_observations.drain(0..drain);
    }
    task.runtime_context = Some(context);
}

pub fn append_runtime_tool_result(
    task: &mut AgentTaskRun,
    tool: &str,
    status: &str,
    payload: Option<Value>,
) {
    let step = task.recent_steps.len();
    let mut context = task
        .runtime_context
        .clone()
        .unwrap_or_else(|| empty_runtime_context(task));
    sync_context_header(task, &mut context);
    context.recent_tool_results.push(RuntimeToolResult {
        step,
        tool: tool.to_string(),
        status: status.to_string(),
        payload: payload.clone(),
    });
    if context.recent_tool_results.len() > 8 {
        let drain = context.recent_tool_results.len() - 8;
        context.recent_tool_results.drain(0..drain);
    }
    if let Some(value) = payload.as_ref() {
        runtime_binding::merge_tool_result_entities(&mut context, tool, value, step);
    }
    task.runtime_context = Some(context);
}

fn empty_runtime_context(task: &AgentTaskRun) -> RuntimeContext {
    RuntimeContext {
        raw_user_input: task.original_request.clone(),
        normalized_goal: task.goal.clone(),
        task_status: task.task_status.clone(),
        active_window: None,
        window_inventory: vec![],
        uia_summary: None,
        vision_summary: None,
        clipboard: None,
        recent_tool_results: vec![],
        recent_observations: vec![],
        discovered_entities: vec![],
        consistency: None,
        last_error: task.failure_reason.clone(),
    }
}

fn sync_context_header(task: &AgentTaskRun, context: &mut RuntimeContext) {
    context.raw_user_input = task.original_request.clone();
    context.normalized_goal = task.goal.clone();
    context.task_status = task.task_status.clone();
    context.last_error = task.failure_reason.clone();
}

pub fn render_runtime_context_for_prompt(context: &RuntimeContext) -> String {
    let entity_lines = context
        .discovered_entities
        .iter()
        .take(10)
        .map(|entity| {
            format!(
                "- id={} label={} source={:?} confidence={:.2} createdAtStep={} lastSeenStep={} payload={}",
                entity.id,
                entity.label,
                entity.source,
                entity.confidence,
                entity.created_at_step,
                entity.last_seen_step,
                serde_json::to_string(&entity.payload).unwrap_or_else(|_| "{}".to_string()),
            )
        })
        .collect::<Vec<_>>();
    let observation_lines = context
        .recent_observations
        .iter()
        .rev()
        .take(5)
        .map(|item| {
            format!(
                "- step={} source={} summary={}",
                item.step, item.source, item.summary
            )
        })
        .collect::<Vec<_>>();

    format!(
        "runtime_context:\n\
         - rawUserInput: {}\n\
         - normalizedGoal: {}\n\
         - taskStatus: {:?}\n\
         - activeWindow: {}\n\
         - windowInventoryCount: {}\n\
         - clipboard: {}\n\
         - consistency: {}\n\
         - recentToolResults: {}\n\
         - recentObservations:\n{}\n\
         - discoveredEntities:\n{}\n",
        context.raw_user_input,
        context.normalized_goal,
        context.task_status,
        context
            .active_window
            .as_ref()
            .and_then(|value| value.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("unknown"),
        context.window_inventory.len(),
        context.clipboard.as_deref().unwrap_or(""),
        context.consistency.as_deref().unwrap_or("unknown"),
        serde_json::to_string(&context.recent_tool_results).unwrap_or_else(|_| "[]".to_string()),
        if observation_lines.is_empty() {
            "- none".to_string()
        } else {
            observation_lines.join("\n")
        },
        if entity_lines.is_empty() {
            "- none".to_string()
        } else {
            entity_lines.join("\n")
        }
    )
}

fn screen_context_consistency(screen: &screen_context::ScreenContext) -> String {
    match screen.consistency.status {
        super::vision_types::ScreenContextConsistencyKind::Consistent => "consistent",
        super::vision_types::ScreenContextConsistencyKind::UiaOnly => "uia_only",
        super::vision_types::ScreenContextConsistencyKind::VisionOnly => "vision_only",
        super::vision_types::ScreenContextConsistencyKind::SoftConflict => "soft_conflict",
        super::vision_types::ScreenContextConsistencyKind::HardConflict => "hard_conflict",
    }
    .to_string()
}
