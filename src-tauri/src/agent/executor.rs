use serde_json::{json, Value};
use tauri::AppHandle;

use crate::{
    app_state::now_millis,
    control::{
        router as control_router,
        types::{ControlPendingRequest, ToolInvokeRequest},
    },
};

use super::{
    runtime_context,
    types::{is_agent_tool_allowed, AgentStepRecord, AgentTaskMode, AgentTaskRun, AgentToolStep},
};

#[derive(Debug, Clone)]
pub enum LoopToolExecution {
    Success,
    Pending {
        note: String,
        pending_request: ControlPendingRequest,
    },
    Failure {
        reason: String,
    },
}

pub fn execute_loop_tool(
    app: &AppHandle,
    task: &mut AgentTaskRun,
    tool: &str,
    args: Value,
    summary: Option<String>,
) -> Result<LoopToolExecution, String> {
    if !is_agent_tool_allowed(tool) {
        return Ok(LoopToolExecution::Failure {
            reason: format!("工具 {tool} 不在当前桌面代理白名单中。"),
        });
    }

    let step = AgentToolStep {
        id: None,
        summary: summary.clone(),
        tool: tool.to_string(),
        args: args.clone(),
    };
    let request = ToolInvokeRequest {
        tool: tool.to_string(),
        args,
    };

    match control_router::invoke(app, request) {
        Ok(response) if response.status == "pending_confirmation" => {
            let pending_request = response.pending_request.clone().ok_or_else(|| {
                "控制层返回了 pending_confirmation，但没有 pendingRequest。".to_string()
            })?;
            let note = format!("{} 需要确认。", step_label(&step));
            task.waiting_pending_id = Some(pending_request.id.clone());
            task.pending_action_id = Some(pending_request.id.clone());
            task.pending_action_summary = step.summary.clone().or_else(|| Some(step.tool.clone()));
            task.task_status = super::types::AgentLoopTaskStatus::WaitingConfirmation;
            task.updated_at = now_millis();
            task.recent_steps.push(AgentStepRecord {
                summary: step_label(&step),
                tool: Some(step.tool.clone()),
                args: Some(step.args.clone()),
                outcome: "pending".to_string(),
                detail: Some(note.clone()),
            });
            runtime_context::append_runtime_tool_result(
                task,
                &step.tool,
                "pending",
                Some(json!({
                    "pendingRequest": pending_request.clone(),
                    "note": note.clone(),
                })),
            );
            Ok(LoopToolExecution::Pending {
                note,
                pending_request,
            })
        }
        Ok(response) if response.status == "success" => {
            let result = response.result.unwrap_or_else(|| json!({}));
            let note = render_success(&step, &result);
            task.last_tool_result = Some(result.clone());
            task.completed_notes.push(note.clone());
            task.completed_results.push(result.clone());
            task.updated_at = now_millis();
            task.recent_steps.push(AgentStepRecord {
                summary: step_label(&step),
                tool: Some(step.tool.clone()),
                args: Some(step.args.clone()),
                outcome: "success".to_string(),
                detail: Some(note.clone()),
            });
            runtime_context::append_runtime_tool_result(task, &step.tool, "success", Some(result));
            Ok(LoopToolExecution::Success)
        }
        Ok(response) => {
            let reason = response
                .message
                .unwrap_or_else(|| "控制工具返回了未知状态。".to_string());
            task.last_tool_result = Some(json!({
                "status": response.status,
                "message": reason,
            }));
            task.updated_at = now_millis();
            task.recent_steps.push(AgentStepRecord {
                summary: step_label(&step),
                tool: Some(step.tool.clone()),
                args: Some(step.args.clone()),
                outcome: "failure".to_string(),
                detail: Some(reason.clone()),
            });
            runtime_context::append_runtime_tool_result(
                task,
                &step.tool,
                "failure",
                task.last_tool_result.clone(),
            );
            Ok(LoopToolExecution::Failure { reason })
        }
        Err(error) => {
            let reason = error.payload().message;
            task.last_tool_result = Some(json!({
                "status": "error",
                "message": reason,
            }));
            task.updated_at = now_millis();
            task.recent_steps.push(AgentStepRecord {
                summary: step_label(&step),
                tool: Some(step.tool.clone()),
                args: Some(step.args.clone()),
                outcome: "failure".to_string(),
                detail: Some(reason.clone()),
            });
            runtime_context::append_runtime_tool_result(
                task,
                &step.tool,
                "failure",
                task.last_tool_result.clone(),
            );
            Ok(LoopToolExecution::Failure { reason })
        }
    }
}

pub fn clear_loop_pending(task: &mut AgentTaskRun) {
    task.waiting_pending_id = None;
    task.pending_action_id = None;
    task.pending_action_summary = None;
    task.updated_at = now_millis();
}

pub fn is_loop_task(task: &AgentTaskRun) -> bool {
    matches!(task.mode, AgentTaskMode::Loop)
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
        "list_directory" => format!(
            "已读取目录：{}，共 {} 项。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("目标目录"),
            result
                .as_object()
                .and_then(|map| map.get("entryCount"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        ),
        "read_file_text" => format!(
            "已读取文件：{}。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("目标文件")
        ),
        "write_file_text" => format!(
            "已写入文件：{}。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("目标文件")
        ),
        "create_directory" => format!(
            "已确保目录存在：{}。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("目标目录")
        ),
        "move_path" => format!(
            "已移动路径：{} -> {}。",
            result
                .as_object()
                .and_then(|map| map.get("fromPath"))
                .and_then(Value::as_str)
                .unwrap_or("源路径"),
            result
                .as_object()
                .and_then(|map| map.get("toPath"))
                .and_then(Value::as_str)
                .unwrap_or("目标路径")
        ),
        "delete_path" => format!(
            "已删除路径：{}。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("目标路径")
        ),
        "run_shell_command" => format!(
            "已执行 shell 命令：{}。",
            result
                .as_object()
                .and_then(|map| map.get("displayName"))
                .and_then(Value::as_str)
                .unwrap_or("受控命令")
        ),
        "launch_installer_file" => format!(
            "已启动安装器：{}。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("安装器文件")
        ),
        "query_registry_key" => format!(
            "已读取注册表项：{}。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("目标注册表项")
        ),
        "read_registry_value" => format!(
            "已读取注册表值：{} / {}。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("目标注册表路径"),
            result
                .as_object()
                .and_then(|map| map.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("值")
        ),
        "write_registry_value" => format!(
            "已写入注册表值：{} / {}。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("目标注册表路径"),
            result
                .as_object()
                .and_then(|map| map.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("值")
        ),
        "delete_registry_value" => format!(
            "已删除注册表值：{} / {}。",
            result
                .as_object()
                .and_then(|map| map.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("目标注册表路径"),
            result
                .as_object()
                .and_then(|map| map.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("值")
        ),
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
            "list_directory" => "列出目录".to_string(),
            "read_file_text" => "读取文件".to_string(),
            "write_file_text" => "写入文件".to_string(),
            "create_directory" => "创建目录".to_string(),
            "move_path" => "移动路径".to_string(),
            "delete_path" => "删除路径".to_string(),
            "run_shell_command" => "执行 shell 命令".to_string(),
            "launch_installer_file" => "启动安装器".to_string(),
            "query_registry_key" => "读取注册表项".to_string(),
            "read_registry_value" => "读取注册表值".to_string(),
            "write_registry_value" => "写入注册表值".to_string(),
            "delete_registry_value" => "删除注册表值".to_string(),
            "type_text" => "输入文本".to_string(),
            "send_hotkey" => "发送快捷键".to_string(),
            "click_at" => "点击坐标".to_string(),
            _ => step.tool.clone(),
        })
}
