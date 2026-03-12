use serde_json::{json, Map, Value};
use tauri::AppHandle;

use crate::{
    app_state::now_millis,
    control::{
        router as control_router,
        types::{ControlPendingRequest, ToolInvokeRequest, ToolInvokeResponse},
    },
};

use super::{
    task_store,
    types::{
        is_agent_tool_allowed, AgentPlan, AgentTaskProgress, AgentTaskRun, AgentTaskStatus,
        AgentToolStep,
    },
};

#[derive(Debug, Clone)]
pub struct AgentExecutionResult {
    pub reply_text: String,
    pub outcome: String,
    pub detail: String,
    pub planned_tools: Vec<String>,
    pub pending_request: Option<ControlPendingRequest>,
    pub task: Option<AgentTaskProgress>,
}

pub fn execute_plan(
    app: &AppHandle,
    plan: AgentPlan,
    original_request: &str,
) -> Result<AgentExecutionResult, String> {
    validate_plan(&plan)?;

    if plan.steps.len() > 1 && task_store::has_active_task(app)? {
        return Ok(blocked_execution(
            plan_steps(&plan),
            "当前还有一个未完成的桌面任务，请先确认或取消。".to_string(),
        ));
    }

    let mut task = AgentTaskRun::new(plan, original_request);
    continue_task(app, &mut task, None)
}

pub fn confirm_pending(app: &AppHandle, pending_id: &str) -> Result<ToolInvokeResponse, String> {
    let Some(mut task) = task_store::take_task_waiting_on_pending(app, pending_id)? else {
        return control_router::confirm(app, pending_id).map_err(|error| error.to_string());
    };

    let confirmed = control_router::confirm(app, pending_id).map_err(|error| error.to_string())?;
    let step_index = task
        .waiting_step_index
        .ok_or_else(|| "当前桌面任务没有等待确认的步骤。".to_string())?;

    let result = confirmed.result.clone().unwrap_or_else(|| json!({}));
    let response = continue_task(
        app,
        &mut task,
        Some(ConfirmedStep {
            step_index,
            result,
        }),
    )?;
    Ok(execution_to_response(response))
}

pub fn cancel_pending(app: &AppHandle, pending_id: &str) -> Result<ToolInvokeResponse, String> {
    let matched_task = task_store::take_task_waiting_on_pending(app, pending_id)?;
    let cancelled = control_router::cancel(app, pending_id).map_err(|error| error.to_string())?;

    let Some(mut task) = matched_task else {
        return Ok(cancelled);
    };

    task.waiting_pending_id = None;
    task.waiting_step_index = None;
    task.updated_at = now_millis();

    let mut lines = task.completed_notes.clone();
    lines.push(format!("任务“{}”已取消。", task.task_title));

    Ok(ToolInvokeResponse {
        status: "success".to_string(),
        result: Some(json!({
            "cancelled": true,
            "task": build_task_progress(
                &task,
                AgentTaskStatus::Cancelled,
                task.next_step_index.max(1),
                None,
                Some("当前多步桌面任务已取消。".to_string()),
            ),
        })),
        message: Some(lines.join("\n")),
        pending_request: None,
        error: None,
    })
}

struct ConfirmedStep {
    step_index: usize,
    result: Value,
}

fn continue_task(
    app: &AppHandle,
    task: &mut AgentTaskRun,
    confirmed_step: Option<ConfirmedStep>,
) -> Result<AgentExecutionResult, String> {
    if let Some(confirmed_step) = confirmed_step {
        finish_waiting_step(task, confirmed_step)?;
    }

    while task.next_step_index < task.plan.steps.len() {
        let step_index = task.next_step_index;
        let step = task.plan.steps[step_index].clone();

        if !is_agent_tool_allowed(&step.tool) {
            task_store::replace_active_task(app, None)?;
            return Ok(blocked_execution(
                task.planned_tools(),
                format!("工具 {} 不在当前多步桌面代理白名单中。", step.tool),
            ));
        }

        let resolved_args = resolve_step_args(task, &step)?;
        let request = ToolInvokeRequest {
            tool: step.tool.clone(),
            args: resolved_args,
        };

        match control_router::invoke(app, request) {
            Ok(response) if response.status == "pending_confirmation" => {
                let pending_request = response.pending_request.clone().ok_or_else(|| {
                    "控制层返回了 pending_confirmation，但没有 pendingRequest。".to_string()
                })?;
                task.waiting_step_index = Some(step_index);
                task.waiting_pending_id = Some(pending_request.id.clone());
                task.next_step_index = step_index + 1;
                task.updated_at = now_millis();
                task_store::replace_active_task(app, Some(task.clone()))?;
                return Ok(pending_execution(task, &step, pending_request));
            }
            Ok(response) if response.status == "success" => {
                let result = response.result.unwrap_or_else(|| json!({}));
                let note = render_success(&step, &result);
                task.completed_notes.push(note);
                task.completed_results.push(result.clone());
                task.next_step_index = step_index + 1;
                task.updated_at = now_millis();

                if step.tool == "read_clipboard" && clipboard_text(&result).trim().is_empty() {
                    task_store::replace_active_task(app, None)?;
                    return Ok(failed_execution(
                        task,
                        step_index,
                        &step,
                        "剪贴板当前没有文本内容，任务已停止。".to_string(),
                    ));
                }
            }
            Ok(response) => {
                task_store::replace_active_task(app, None)?;
                return Ok(failed_execution(
                    task,
                    step_index,
                    &step,
                    response
                        .message
                        .unwrap_or_else(|| "控制工具返回了未知状态。".to_string()),
                ));
            }
            Err(error) => {
                task_store::replace_active_task(app, None)?;
                return Ok(failed_execution(
                    task,
                    step_index,
                    &step,
                    error.payload().message,
                ));
            }
        }
    }

    task_store::replace_active_task(app, None)?;
    Ok(completed_execution(task))
}

fn finish_waiting_step(task: &mut AgentTaskRun, confirmed_step: ConfirmedStep) -> Result<(), String> {
    let waiting_step_index = task
        .waiting_step_index
        .ok_or_else(|| "桌面任务状态异常：缺少 waiting_step_index。".to_string())?;
    if waiting_step_index != confirmed_step.step_index {
        return Err("桌面任务状态异常：确认步骤索引不匹配。".to_string());
    }

    let step = task
        .plan
        .steps
        .get(waiting_step_index)
        .ok_or_else(|| "桌面任务状态异常：待确认步骤不存在。".to_string())?
        .clone();
    let note = render_success(&step, &confirmed_step.result);
    task.completed_notes.push(note);
    task.completed_results.push(confirmed_step.result);
    task.waiting_step_index = None;
    task.waiting_pending_id = None;
    task.updated_at = now_millis();
    Ok(())
}

fn validate_plan(plan: &AgentPlan) -> Result<(), String> {
    if plan.steps.is_empty() {
        return Err("动作计划为空。".to_string());
    }

    if plan.steps.len() > 4 {
        return Err("第一版多步桌面代理最多只允许 4 个步骤。".to_string());
    }

    Ok(())
}

fn pending_execution(
    task: &AgentTaskRun,
    step: &AgentToolStep,
    pending_request: ControlPendingRequest,
) -> AgentExecutionResult {
    let mut lines = task.completed_notes.clone();
    lines.push(format!(
        "任务“{}”已执行到第 {}/{} 步：{}。",
        task.task_title,
        task.waiting_step_index.unwrap_or_default() + 1,
        task.step_count(),
        step_label(step)
    ));
    lines.push(format!(
        "{}\n\n你可以直接输入 yes / no，或使用 /confirm /cancel。",
        pending_request.prompt
    ));

    AgentExecutionResult {
        reply_text: lines.join("\n"),
        outcome: "control_pending".to_string(),
        detail: format!(
            "task={} step={}/{} tool={}",
            task.task_id,
            task.waiting_step_index.unwrap_or_default() + 1,
            task.step_count(),
            step.tool
        ),
        planned_tools: task.planned_tools(),
        pending_request: Some(pending_request),
        task: Some(task.waiting_progress()),
    }
}

fn completed_execution(task: &AgentTaskRun) -> AgentExecutionResult {
    let mut lines = task.completed_notes.clone();
    lines.push(format!("任务“{}”已完成。", task.task_title));
    AgentExecutionResult {
        reply_text: lines.join("\n"),
        outcome: "control_ok".to_string(),
        detail: format!(
            "task={} createdAt={} originalRequest={}",
            task.task_id, task.created_at, task.original_request
        ),
        planned_tools: task.planned_tools(),
        pending_request: None,
        task: Some(build_task_progress(
            task,
            AgentTaskStatus::Completed,
            task.step_count(),
            None,
            Some("任务已完成。".to_string()),
        )),
    }
}

fn failed_execution(
    task: &AgentTaskRun,
    step_index: usize,
    step: &AgentToolStep,
    reason: String,
) -> AgentExecutionResult {
    let failure_reason = reason.clone();
    let mut lines = task.completed_notes.clone();
    lines.push(format!(
        "任务“{}”在第 {}/{} 步失败：{}。\n原因：{}",
        task.task_title,
        step_index + 1,
        task.step_count(),
        step_label(step),
        failure_reason
    ));

    AgentExecutionResult {
        reply_text: lines.join("\n"),
        outcome: "control_failed".to_string(),
        detail: reason.clone(),
        planned_tools: task.planned_tools(),
        pending_request: None,
        task: Some(build_task_progress(
            task,
            AgentTaskStatus::Failed,
            step_index + 1,
            Some(step_label(step)),
            Some(reason),
        )),
    }
}

fn blocked_execution(planned_tools: Vec<String>, message: String) -> AgentExecutionResult {
    AgentExecutionResult {
        reply_text: format!("这次桌面任务未执行。\n\n原因：{message}"),
        outcome: "control_blocked".to_string(),
        detail: message,
        planned_tools,
        pending_request: None,
        task: None,
    }
}

fn execution_to_response(result: AgentExecutionResult) -> ToolInvokeResponse {
    let AgentExecutionResult {
        reply_text,
        outcome,
        detail,
        planned_tools,
        pending_request,
        task,
    } = result;

    let is_error = matches!(outcome.as_str(), "control_failed" | "control_blocked");
    let payload = if task.is_some() || !planned_tools.is_empty() {
        Some(json!({
            "plannedTools": planned_tools,
            "task": task,
        }))
    } else {
        None
    };
    ToolInvokeResponse {
        status: if is_error {
            "error".to_string()
        } else if pending_request.is_some() {
            "pending_confirmation".to_string()
        } else {
            "success".to_string()
        },
        result: payload,
        message: Some(reply_text),
        pending_request,
        error: is_error.then_some(crate::control::types::ControlErrorPayload {
            code: outcome,
            message: detail,
            detail: None,
            retryable: false,
        }),
    }
}

fn build_task_progress(
    task: &AgentTaskRun,
    status: AgentTaskStatus,
    step_index: usize,
    step_summary: Option<String>,
    detail: Option<String>,
) -> AgentTaskProgress {
    AgentTaskProgress {
        task_id: task.task_id.clone(),
        task_title: task.task_title.clone(),
        step_index,
        step_count: task.step_count(),
        status,
        step_summary,
        detail,
    }
}

fn resolve_step_args(task: &AgentTaskRun, step: &AgentToolStep) -> Result<Value, String> {
    let mut map = match step.args.as_object() {
        Some(map) => map.clone(),
        None => Map::new(),
    };

    if step.tool != "focus_window" {
        return Ok(Value::Object(map));
    }

    if map
        .get("title")
        .and_then(Value::as_str)
        .is_some_and(|title| !title.trim().is_empty())
    {
        map.remove("titleCandidates");
        map.remove("windowCategory");
        return Ok(Value::Object(map));
    }

    let mut candidates = map
        .get("titleCandidates")
        .and_then(Value::as_array)
        .map(|items| {
            items.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if let Some(category) = map.get("windowCategory").and_then(Value::as_str) {
        if category == "browser" {
            candidates.extend(["Chrome", "Edge", "Firefox", "浏览器"].into_iter().map(str::to_string));
        }
    }

    let match_mode = map
        .get("match")
        .and_then(Value::as_str)
        .unwrap_or("contains");

    let Some(resolved_title) = resolve_title_from_windows(&task.completed_results, &candidates, match_mode) else {
        return Err(match map.get("windowCategory").and_then(Value::as_str) {
            Some("browser") => "未从窗口列表中匹配到浏览器窗口。".to_string(),
            _ => "未从窗口列表中匹配到目标窗口。".to_string(),
        });
    };

    map.insert("title".to_string(), Value::String(resolved_title));
    map.insert("match".to_string(), Value::String("exact".to_string()));
    map.remove("titleCandidates");
    map.remove("windowCategory");
    Ok(Value::Object(map))
}

fn resolve_title_from_windows(
    completed_results: &[Value],
    candidates: &[String],
    match_mode: &str,
) -> Option<String> {
    let windows = completed_results.iter().rev().find_map(|result| result.as_array())?;
    if windows.is_empty() {
        return None;
    }

    for candidate in candidates {
        for item in windows {
            let Some(title) = item
                .as_object()
                .and_then(|entry| entry.get("title"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
            else {
                continue;
            };
            if title_matches(title, candidate, match_mode) {
                return Some(title.to_string());
            }
        }
    }

    None
}

fn title_matches(actual: &str, candidate: &str, match_mode: &str) -> bool {
    let actual = actual.to_lowercase();
    let candidate = candidate.trim().to_lowercase();
    match match_mode {
        "exact" => actual == candidate,
        "prefix" => actual.starts_with(&candidate),
        _ => actual.contains(&candidate),
    }
}

fn render_success(step: &AgentToolStep, result: &Value) -> String {
    match step.tool.as_str() {
        "list_windows" => render_window_list(result),
        "focus_window" => format!(
            "已切到窗口：{}。",
            result
                .as_object()
                .and_then(|item| item.get("title"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| {
                    step.args
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("目标窗口")
                })
        ),
        "open_app" => format!(
            "已打开 {}。",
            result
                .as_object()
                .and_then(|item| item.get("app"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| step.args.get("name").and_then(Value::as_str).unwrap_or("目标应用"))
        ),
        "read_clipboard" => {
            let text = clipboard_text(result);
            if text.trim().is_empty() {
                "已读取剪贴板，但当前没有文本内容。".to_string()
            } else {
                "已读取剪贴板文本。".to_string()
            }
        }
        "type_text" => format!(
            "已输入 {} 个字符。",
            result
                .as_object()
                .and_then(|map| map.get("typedLength"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        ),
        "send_hotkey" => format!(
            "已发送快捷键：{}。",
            result
                .as_object()
                .and_then(|map| map.get("sequence"))
                .and_then(Value::as_str)
                .unwrap_or("指定按键")
        ),
        "click_at" => format!(
            "已执行坐标点击：({}, {})。",
            step.args.get("x").and_then(Value::as_i64).unwrap_or_default(),
            step.args.get("y").and_then(Value::as_i64).unwrap_or_default()
        ),
        _ => "桌面代理动作已执行。".to_string(),
    }
}

fn render_window_list(result: &Value) -> String {
    let Some(items) = result.as_array() else {
        return "已经读取窗口列表，但这次没有拿到可显示的标题。".to_string();
    };

    let titles = items
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|item| item.get("title").and_then(Value::as_str))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .take(6)
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    if titles.is_empty() {
        return "当前没有读到可见窗口标题。".to_string();
    }

    format!("已列出 {} 个窗口，前几项是：{}。", items.len(), titles.join("、"))
}

fn clipboard_text(result: &Value) -> String {
    result
        .as_object()
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn step_label(step: &AgentToolStep) -> String {
    step.summary
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| match step.tool.as_str() {
            "list_windows" => "列出窗口".to_string(),
            "focus_window" => "切换窗口".to_string(),
            "open_app" => "打开应用".to_string(),
            "read_clipboard" => "读取剪贴板".to_string(),
            "type_text" => "输入文本".to_string(),
            "send_hotkey" => "发送快捷键".to_string(),
            "click_at" => "点击坐标".to_string(),
            _ => step.tool.clone(),
        })
}

fn plan_steps(plan: &AgentPlan) -> Vec<String> {
    plan.steps.iter().map(|step| step.tool.clone()).collect::<Vec<_>>()
}
