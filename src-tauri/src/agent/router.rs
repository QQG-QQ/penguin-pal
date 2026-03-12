use serde_json::Value;
use tauri::AppHandle;

use crate::{
    app_state::{DesktopAction, ProviderConfig},
    control::{
        router as control_router,
        types::{ControlPendingRequest, ToolInvokeRequest},
    },
};

use super::{
    intent,
    planner,
    types::{is_agent_tool_allowed, AgentMessageMeta, AgentPlan, AgentRoute, AgentToolStep},
};

#[derive(Debug, Clone)]
pub struct AgentHandleResult {
    pub reply_text: String,
    pub provider_label: String,
    pub outcome: String,
    pub detail: String,
    pub meta: AgentMessageMeta,
}

pub async fn maybe_handle_control_message(
    app: &AppHandle,
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
) -> Result<Option<AgentHandleResult>, String> {
    let trimmed = user_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let plan = if let Some(plan) = intent::parse_simple_control_plan(trimmed) {
        Some(plan)
    } else if intent::looks_like_control_request(trimmed) {
        match planner::plan_with_model(
            provider_config,
            api_key,
            oauth_access_token,
            codex_command,
            codex_home,
            permission_level,
            allowed_actions,
            trimmed,
        )
        .await
        {
            Ok(plan) => Some(plan),
            Err(error) => {
                return Ok(Some(AgentHandleResult {
                    reply_text: format!(
                        "这句更像桌面控制请求，但我这次没能生成安全动作计划。请换成更明确的说法，例如“打开记事本”或“切到微信”。\n\n详细原因：{}",
                        error
                    ),
                    provider_label: "Desktop Agent".to_string(),
                    outcome: "planner_error".to_string(),
                    detail: error,
                    meta: AgentMessageMeta {
                        route: AgentRoute::Control,
                        planned_tools: vec![],
                        pending_request: None,
                    },
                }));
            }
        }
    } else {
        None
    };

    let Some(plan) = plan else {
        return Ok(None);
    };

    if matches!(plan.route, AgentRoute::Chat) {
        return Ok(None);
    }

    Ok(Some(execute_plan(app, plan)))
}

fn execute_plan(app: &AppHandle, plan: AgentPlan) -> AgentHandleResult {
    let planned_tools = plan
        .steps
        .iter()
        .map(|step| step.tool.clone())
        .collect::<Vec<_>>();
    let mut success_notes = Vec::new();

    for step in &plan.steps {
        if !is_agent_tool_allowed(&step.tool) {
            return blocked_response(
                planned_tools.clone(),
                format!("工具 {} 不在当前自然语言代理白名单中。", step.tool),
            );
        }

        match control_router::invoke(
            app,
            ToolInvokeRequest {
                tool: step.tool.clone(),
                args: step.args.clone(),
            },
        ) {
            Ok(response) if response.status == "pending_confirmation" => {
                let pending_request = response.pending_request.clone();
                if !success_notes.is_empty() {
                    let prefix = success_notes.join("\n");
                    let mut next = pending_response(planned_tools.clone(), step, pending_request);
                    next.reply_text = format!("{prefix}\n\n{}", next.reply_text);
                    return next;
                }
                return pending_response(planned_tools.clone(), step, pending_request);
            }
            Ok(response) => {
                if response.status == "success" {
                    success_notes.push(render_success(step, response.result.as_ref()));
                    continue;
                }

                return blocked_response(
                    planned_tools.clone(),
                    response
                        .message
                        .unwrap_or_else(|| "控制工具返回了未知状态。".to_string()),
                );
            }
            Err(error) => {
                return blocked_response(planned_tools.clone(), error.payload().message);
            }
        }
    }

    if success_notes.is_empty() {
        return blocked_response(planned_tools.clone(), "没有执行到任何受控动作。".to_string());
    }

    AgentHandleResult {
        reply_text: success_notes.join("\n"),
        provider_label: "Desktop Agent".to_string(),
        outcome: "control_ok".to_string(),
        detail: format!("tools={}", planned_tools.join(",")),
        meta: AgentMessageMeta {
            route: AgentRoute::Control,
            planned_tools,
            pending_request: None,
        },
    }
}

fn pending_response(
    planned_tools: Vec<String>,
    step: &AgentToolStep,
    pending_request: Option<ControlPendingRequest>,
) -> AgentHandleResult {
    let prompt = pending_request
        .as_ref()
        .map(|item| item.prompt.clone())
        .unwrap_or_else(|| "该动作需要先确认后执行。".to_string());

    AgentHandleResult {
        reply_text: format!("{prompt}\n\n你可以直接输入 yes / no，或使用 /confirm /cancel。"),
        provider_label: "Desktop Agent".to_string(),
        outcome: "control_pending".to_string(),
        detail: format!("tool={}", step.tool),
        meta: AgentMessageMeta {
            route: AgentRoute::Control,
            planned_tools,
            pending_request,
        },
    }
}

fn blocked_response(planned_tools: Vec<String>, message: String) -> AgentHandleResult {
    AgentHandleResult {
        reply_text: format!("这次桌面代理请求未执行。\n\n原因：{message}"),
        provider_label: "Desktop Agent".to_string(),
        outcome: "control_blocked".to_string(),
        detail: message,
        meta: AgentMessageMeta {
            route: AgentRoute::Control,
            planned_tools,
            pending_request: None,
        },
    }
}

fn render_success(step: &AgentToolStep, result: Option<&Value>) -> String {
    match step.tool.as_str() {
        "list_windows" => render_window_list(result),
        "focus_window" => format!(
            "已尝试切到标题包含“{}”的窗口。",
            step.args
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("目标")
        ),
        "open_app" => format!(
            "已尝试打开 {}。",
            step.args
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("目标应用")
        ),
        "read_clipboard" => render_clipboard(result),
        "type_text" => format!(
            "已向当前活动窗口输入 {} 个字符。",
            result
                .and_then(Value::as_object)
                .and_then(|map| map.get("typedLength"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        ),
        "send_hotkey" => format!(
            "已发送快捷键：{}。",
            result
                .and_then(Value::as_object)
                .and_then(|map| map.get("sequence"))
                .and_then(Value::as_str)
                .unwrap_or("指定按键")
        ),
        "click_at" => format!(
            "已执行坐标点击：({}, {})。",
            step.args.get("x").and_then(Value::as_i64).unwrap_or_default(),
            step.args.get("y").and_then(Value::as_i64).unwrap_or_default()
        ),
        "find_element" => "已找到匹配的界面元素。".to_string(),
        "click_element" => "已尝试点击匹配的界面元素。".to_string(),
        "set_element_value" => "已尝试向匹配的界面元素写入文本。".to_string(),
        _ => "桌面代理动作已执行。".to_string(),
    }
}

fn render_window_list(result: Option<&Value>) -> String {
    let Some(items) = result.and_then(Value::as_array) else {
        return "已经读取窗口列表，但这次没有拿到可显示的标题。".to_string();
    };

    let mut titles = items
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|item| item.get("title").and_then(Value::as_str))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .take(8)
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    if titles.is_empty() {
        return "当前没有读到可见窗口标题。".to_string();
    }

    let count = items.len();
    let preview = titles
        .drain(..)
        .enumerate()
        .map(|(index, title)| format!("{}. {}", index + 1, title))
        .collect::<Vec<_>>()
        .join("\n");
    format!("当前可见窗口大约有 {count} 个：\n{preview}")
}

fn render_clipboard(result: Option<&Value>) -> String {
    let text = result
        .and_then(Value::as_object)
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if text.trim().is_empty() {
        return "剪贴板当前没有文本内容。".to_string();
    }

    let preview = text.chars().take(240).collect::<String>();
    let ellipsis = if text.chars().count() > 240 { "…" } else { "" };
    format!("剪贴板文本如下：\n{preview}{ellipsis}")
}
