use std::sync::Mutex;

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

use crate::{
    app_state::{now_millis, save, RuntimeState},
    control::logging,
    security::{audit, policy as action_policy},
};

use super::{
    errors::{ControlError, ControlResult},
    files,
    installer,
    pending,
    policy,
    registry,
    shell,
    system_registry,
    types::{ControlPendingRequest, ControlServiceStatus, ToolInvokeRequest, ToolInvokeResponse},
    windows,
    ControlServiceState,
};

fn expire_runtime_state(runtime: &mut RuntimeState) {
    action_policy::cleanup_expired_approvals(&mut runtime.pending_action_approvals);

    if let Some(pending) = &runtime.pending_oauth {
        if pending.expires_at <= now_millis() {
            runtime.pending_oauth = None;
            runtime.oauth_last_error =
                Some("上一次 OAuth 登录已过期，请重新发起授权。".to_string());
        }
    }
}

fn cleanup_expired_control_pending(app: &AppHandle) -> ControlResult<()> {
    let control_state: tauri::State<'_, ControlServiceState> = app.state();
    let expired = {
        let mut pending_requests = control_state.pending_requests().map_err(ControlError::internal)?;
        pending::cleanup_expired_pending(&mut pending_requests)
    };

    if expired.is_empty() {
        return Ok(());
    }

    let runtime_state: tauri::State<'_, Mutex<RuntimeState>> = app.state();
    let mut runtime = runtime_state
        .lock()
        .map_err(|_| ControlError::internal("助手状态锁定失败"))?;
    expire_runtime_state(&mut runtime);
    for item in expired {
        let _ = logging::append_log(
            app,
            "control_pending",
            "expired",
            format!(
                "id={} tool={} expiresAt={}",
                item.id, item.tool, item.expires_at
            ),
        );
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "control_pending_expired",
                "expired",
                format!("tool={} pending_id={}", item.tool, item.id),
                2,
            ),
        );
    }
    save(app, &runtime).map_err(ControlError::internal)
}

fn result_ok(result: Value, message: Option<String>) -> ToolInvokeResponse {
    ToolInvokeResponse {
        status: "success".to_string(),
        result: Some(result),
        message,
        pending_request: None,
        error: None,
    }
}

fn result_pending(
    pending_request: ControlPendingRequest,
    message: Option<String>,
) -> ToolInvokeResponse {
    ToolInvokeResponse {
        status: "pending_confirmation".to_string(),
        result: None,
        message,
        pending_request: Some(pending_request),
        error: None,
    }
}

fn args_as_object(args: Value) -> ControlResult<Map<String, Value>> {
    match args {
        Value::Object(map) => Ok(map),
        Value::Null => Ok(Map::new()),
        _ => Err(ControlError::invalid_argument("工具参数必须是 JSON object。")),
    }
}

fn get_required_string(
    map: &Map<String, Value>,
    key: &str,
    label: &str,
    max_len: usize,
) -> ControlResult<String> {
    let value = map
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ControlError::invalid_argument(format!("{label} 不能为空")))?;

    if value.chars().count() > max_len {
        return Err(ControlError::invalid_argument(format!(
            "{label} 长度不能超过 {max_len}"
        )));
    }

    Ok(value.to_string())
}

fn get_optional_string(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn get_required_i64(map: &Map<String, Value>, key: &str, label: &str) -> ControlResult<i64> {
    map.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| ControlError::invalid_argument(format!("{label} 必须是整数")))
}

fn get_optional_bool(map: &Map<String, Value>, key: &str, default: bool) -> bool {
    map.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn parse_hotkey_keys(value: Option<&Value>) -> ControlResult<Vec<String>> {
    let Some(value) = value else {
        return Err(ControlError::invalid_argument("keys 必须是数组或字符串。"));
    };

    let keys = if let Some(items) = value.as_array() {
        items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(|key| key.trim().to_uppercase())
                    .filter(|key| !key.is_empty())
                    .ok_or_else(|| ControlError::invalid_argument("keys 数组必须全部是非空字符串。"))
            })
            .collect::<ControlResult<Vec<_>>>()?
    } else if let Some(raw) = value.as_str() {
        raw.split(['+', ',', ' '])
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| item.to_uppercase())
            .collect::<Vec<_>>()
    } else {
        return Err(ControlError::invalid_argument("keys 必须是数组或字符串。"));
    };

    if keys.is_empty() {
        return Err(ControlError::invalid_argument("keys 不能为空。"));
    }

    Ok(keys)
}

fn canonicalize_request(request: ToolInvokeRequest) -> ToolInvokeRequest {
    match request.tool.as_str() {
        "click_element_or_coords" => {
            let use_selector = request
                .args
                .as_object()
                .and_then(|map| map.get("selector"))
                .is_some_and(|value| !value.is_null());
            ToolInvokeRequest {
                tool: if use_selector {
                    "click_element".to_string()
                } else {
                    "click_at".to_string()
                },
                args: request.args,
            }
        }
        "scroll_active_window" => ToolInvokeRequest {
            tool: "scroll_at".to_string(),
            args: request.args,
        },
        other => ToolInvokeRequest {
            tool: other.to_string(),
            args: request.args,
        },
    }
}

fn normalize_args(tool: &str, args: Value) -> ControlResult<Value> {
    let map = args_as_object(args)?;
    match tool {
        "list_windows" | "capture_active_window" | "read_clipboard" => Ok(json!({})),
        "list_directory" | "read_file_text" => {
            let path = get_required_string(&map, "path", "path", 400)?;
            Ok(json!({ "path": path }))
        }
        "write_file_text" => {
            let path = get_required_string(&map, "path", "path", 400)?;
            let content = map
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| ControlError::invalid_argument("content 不能为空"))?
                .to_string();
            if content.chars().count() > 20_000 {
                return Err(ControlError::invalid_argument("content 长度不能超过 20000。"));
            }
            Ok(json!({
                "path": path,
                "content": content,
                "overwrite": get_optional_bool(&map, "overwrite", false),
                "ensureParent": get_optional_bool(&map, "ensureParent", false),
            }))
        }
        "create_directory" => {
            let path = get_required_string(&map, "path", "path", 400)?;
            Ok(json!({
                "path": path,
                "recursive": get_optional_bool(&map, "recursive", true),
            }))
        }
        "move_path" => {
            let from_path = get_required_string(&map, "fromPath", "fromPath", 400)?;
            let to_path = get_required_string(&map, "toPath", "toPath", 400)?;
            Ok(json!({
                "fromPath": from_path,
                "toPath": to_path,
                "overwrite": get_optional_bool(&map, "overwrite", false),
            }))
        }
        "delete_path" => {
            let path = get_required_string(&map, "path", "path", 400)?;
            Ok(json!({
                "path": path,
                "recursive": get_optional_bool(&map, "recursive", false),
            }))
        }
        "run_shell_command" => {
            let command = get_required_string(&map, "command", "command", 32)?;
            let args = map
                .get("args")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|item| {
                    item.as_str()
                        .map(ToString::to_string)
                        .ok_or_else(|| ControlError::invalid_argument("run_shell_command.args 必须是字符串数组。"))
                })
                .collect::<ControlResult<Vec<_>>>()?;
            if args.len() > 8 || args.iter().any(|item| item.chars().count() > 300) {
                return Err(ControlError::invalid_argument(
                    "run_shell_command.args 长度或单项字符数超出安全范围。",
                ));
            }
            let workdir = get_optional_string(&map, "workdir");
            let timeout_ms = map
                .get("timeoutMs")
                .and_then(Value::as_i64)
                .unwrap_or(20_000)
                .clamp(1_000, 300_000);
            Ok(json!({
                "command": command,
                "args": args,
                "workdir": workdir,
                "timeoutMs": timeout_ms,
            }))
        }
        "launch_installer_file" => {
            let path = get_required_string(&map, "path", "path", 400)?;
            Ok(json!({ "path": path }))
        }
        "query_registry_key" => {
            let path = get_required_string(&map, "path", "path", 400)?;
            Ok(json!({ "path": path }))
        }
        "read_registry_value" | "delete_registry_value" => {
            let path = get_required_string(&map, "path", "path", 400)?;
            let name = get_required_string(&map, "name", "name", 128)?;
            Ok(json!({ "path": path, "name": name }))
        }
        "write_registry_value" => {
            let path = get_required_string(&map, "path", "path", 400)?;
            let name = get_required_string(&map, "name", "name", 128)?;
            let value_type = get_required_string(&map, "valueType", "valueType", 32)?;
            let value = get_required_string(&map, "value", "value", 2048)?;
            Ok(json!({
                "path": path,
                "name": name,
                "valueType": value_type,
                "value": value,
            }))
        }
        "focus_window" => {
            let title = get_required_string(&map, "title", "窗口标题", 120)?;
            let match_mode =
                get_optional_string(&map, "match").unwrap_or_else(|| "contains".to_string());
            if !["contains", "exact", "prefix"].contains(&match_mode.as_str()) {
                return Err(ControlError::invalid_argument(
                    "match 只允许 contains / exact / prefix。",
                ));
            }
            Ok(json!({ "title": title, "match": match_mode }))
        }
        "open_app" => {
            let name = get_required_string(&map, "name", "应用别名", 64)?;
            Ok(json!({ "name": name }))
        }
        "type_text" => {
            let text = map
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| ControlError::invalid_argument("text 不能为空"))?
                .to_string();
            if text.trim().is_empty() {
                return Err(ControlError::invalid_argument("text 不能为空"));
            }
            if text.chars().count() > 500 {
                return Err(ControlError::invalid_argument("text 长度不能超过 500。"));
            }
            if text.contains('\n') || text.contains('\r') || text.contains('\t') {
                return Err(ControlError::invalid_argument(
                    "第一版 type_text 只允许单行纯文本。",
                ));
            }
            Ok(json!({ "text": text }))
        }
        "send_hotkey" => {
            let keys = parse_hotkey_keys(map.get("keys"))?;
            Ok(json!({ "keys": keys }))
        }
        "click_at" => {
            let x = get_required_i64(&map, "x", "x")?;
            let y = get_required_i64(&map, "y", "y")?;
            if !(0..=4000).contains(&x) || !(0..=4000).contains(&y) {
                return Err(ControlError::invalid_argument(
                    "x / y 必须位于 0..4000 的安全范围内。",
                ));
            }
            let button =
                get_optional_string(&map, "button").unwrap_or_else(|| "left".to_string());
            if !["left", "right", "double"].contains(&button.as_str()) {
                return Err(ControlError::invalid_argument(
                    "button 只允许 left / right / double。",
                ));
            }
            Ok(json!({ "x": x, "y": y, "button": button }))
        }
        "scroll_at" => {
            let delta = get_required_i64(&map, "delta", "delta")?;
            if delta == 0 {
                return Err(ControlError::invalid_argument("delta 不能为 0。"));
            }
            let delta = delta.clamp(-1200, 1200);
            let steps = map
                .get("steps")
                .and_then(Value::as_i64)
                .unwrap_or(1)
                .clamp(1, 10);
            let x = map.get("x").and_then(Value::as_i64);
            let y = map.get("y").and_then(Value::as_i64);
            if x.is_some() ^ y.is_some() {
                return Err(ControlError::invalid_argument(
                    "scroll_at 如果提供坐标，x 和 y 必须同时提供。",
                ));
            }
            Ok(json!({ "delta": delta, "steps": steps, "x": x, "y": y }))
        }
        "find_element" | "click_element" | "get_element_text" => Ok(Value::Object(map)),
        "set_element_value" => {
            let text = get_required_string(&map, "text", "text", 500)?;
            if text.contains('\n') || text.contains('\r') || text.contains('\t') {
                return Err(ControlError::invalid_argument(
                    "第一版 set_element_value 只允许单行文本。",
                ));
            }
            let mut next_map = map.clone();
            next_map.insert("text".to_string(), Value::String(text));
            Ok(Value::Object(next_map))
        }
        "wait_for_element" => {
            let timeout_ms = map
                .get("timeoutMs")
                .and_then(Value::as_i64)
                .unwrap_or(3000)
                .clamp(500, 10_000);
            let mut next_map = map.clone();
            next_map.insert("timeoutMs".to_string(), Value::Number(timeout_ms.into()));
            Ok(Value::Object(next_map))
        }
        _ => Err(ControlError::not_found("tool_not_found", "未知控制工具。")),
    }
}

fn pending_prompt(tool: &str, args: &Value) -> String {
    match tool {
        "type_text" => {
            let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
            format!("即将向当前活动窗口输入 {} 个字符。", text.chars().count())
        }
        "send_hotkey" => format!(
            "即将向当前活动窗口发送热键：{}。",
            args.get("keys")
                .and_then(Value::as_array)
                .map(|keys| keys.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" + "))
                .unwrap_or_default()
        ),
        "click_at" => format!(
            "即将对当前活动窗口执行坐标点击：x={}，y={}，button={}。",
            args.get("x").and_then(Value::as_i64).unwrap_or_default(),
            args.get("y").and_then(Value::as_i64).unwrap_or_default(),
            args.get("button").and_then(Value::as_str).unwrap_or("left"),
        ),
        "click_element" => "即将点击匹配的 UI 元素。".to_string(),
        "set_element_value" => format!(
            "即将向匹配的 UI 元素写入 {} 个字符。",
            args.get("text")
                .and_then(Value::as_str)
                .map(|text| text.chars().count())
                .unwrap_or_default()
        ),
        "write_file_text" => format!(
            "即将{}文本文件：{}。",
            if args.get("overwrite").and_then(Value::as_bool).unwrap_or(false) {
                "覆盖写入"
            } else {
                "写入"
            },
            args.get("path").and_then(Value::as_str).unwrap_or_default()
        ),
        "move_path" => format!(
            "即将移动路径：{} -> {}。",
            args.get("fromPath").and_then(Value::as_str).unwrap_or_default(),
            args.get("toPath").and_then(Value::as_str).unwrap_or_default()
        ),
        "delete_path" => format!(
            "即将删除路径：{}。",
            args.get("path").and_then(Value::as_str).unwrap_or_default()
        ),
        "launch_installer_file" => format!(
            "即将启动安装器文件：{}。",
            args.get("path").and_then(Value::as_str).unwrap_or_default()
        ),
        "write_registry_value" => format!(
            "即将写入注册表值：{} / {}。",
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
            args.get("name").and_then(Value::as_str).unwrap_or_default()
        ),
        "delete_registry_value" => format!(
            "即将删除注册表值：{} / {}。",
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
            args.get("name").and_then(Value::as_str).unwrap_or_default()
        ),
        _ => "即将执行高风险控制动作。".to_string(),
    }
}

fn pending_preview(tool: &str, args: &Value) -> Value {
    match tool {
        "type_text" | "set_element_value" => {
            let text = args.get("text").and_then(Value::as_str).unwrap_or_default();
            let preview: String = text.chars().take(80).collect();
            json!({
                "textLength": text.chars().count(),
                "textPreview": preview,
            })
        }
        "send_hotkey" => json!({
            "keys": args.get("keys").cloned().unwrap_or_else(|| json!([])),
        }),
        "click_at" => json!({
            "x": args.get("x").and_then(Value::as_i64).unwrap_or_default(),
            "y": args.get("y").and_then(Value::as_i64).unwrap_or_default(),
            "button": args.get("button").and_then(Value::as_str).unwrap_or("left"),
        }),
        "click_element" => json!({
            "selector": args.get("selector").cloned().unwrap_or_else(|| json!({})),
        }),
        "write_file_text" => {
            let content = args.get("content").and_then(Value::as_str).unwrap_or_default();
            let preview: String = content.chars().take(120).collect();
            json!({
                "path": args.get("path").cloned().unwrap_or(Value::Null),
                "contentLength": content.chars().count(),
                "contentPreview": preview,
                "overwrite": args.get("overwrite").cloned().unwrap_or(Value::Bool(false)),
            })
        }
        "move_path" => json!({
            "fromPath": args.get("fromPath").cloned().unwrap_or(Value::Null),
            "toPath": args.get("toPath").cloned().unwrap_or(Value::Null),
            "overwrite": args.get("overwrite").cloned().unwrap_or(Value::Bool(false)),
        }),
        "delete_path" => json!({
            "path": args.get("path").cloned().unwrap_or(Value::Null),
            "recursive": args.get("recursive").cloned().unwrap_or(Value::Bool(false)),
        }),
        "launch_installer_file" => json!({
            "path": args.get("path").cloned().unwrap_or(Value::Null),
        }),
        "write_registry_value" => json!({
            "path": args.get("path").cloned().unwrap_or(Value::Null),
            "name": args.get("name").cloned().unwrap_or(Value::Null),
            "valueType": args.get("valueType").cloned().unwrap_or(Value::Null),
            "value": args.get("value").cloned().unwrap_or(Value::Null),
        }),
        "delete_registry_value" => json!({
            "path": args.get("path").cloned().unwrap_or(Value::Null),
            "name": args.get("name").cloned().unwrap_or(Value::Null),
        }),
        _ => pending::default_preview("控制动作"),
    }
}

fn execute_tool(app: &AppHandle, tool: &str, args: &Value) -> ControlResult<Value> {
    match tool {
        "list_windows" => windows::windowing::list_windows(app),
        "focus_window" => windows::windowing::focus_window(
            app,
            args.get("title").and_then(Value::as_str).unwrap_or_default(),
            args.get("match")
                .and_then(Value::as_str)
                .unwrap_or("contains"),
        ),
        "open_app" => windows::apps::open_app(
            app,
            args.get("name").and_then(Value::as_str).unwrap_or_default(),
        ),
        "capture_active_window" => windows::capture::capture_active_window(app),
        "read_clipboard" => windows::clipboard::read_clipboard(app),
        "list_directory" => files::list_directory(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
        ),
        "read_file_text" => files::read_file_text(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
        ),
        "write_file_text" => files::write_file_text(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
            args.get("content").and_then(Value::as_str).unwrap_or_default(),
            args.get("overwrite").and_then(Value::as_bool).unwrap_or(false),
            args.get("ensureParent").and_then(Value::as_bool).unwrap_or(false),
        ),
        "create_directory" => files::create_directory(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
            args.get("recursive").and_then(Value::as_bool).unwrap_or(true),
        ),
        "move_path" => files::move_path(
            app,
            args.get("fromPath").and_then(Value::as_str).unwrap_or_default(),
            args.get("toPath").and_then(Value::as_str).unwrap_or_default(),
            args.get("overwrite").and_then(Value::as_bool).unwrap_or(false),
        ),
        "delete_path" => files::delete_path(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
            args.get("recursive").and_then(Value::as_bool).unwrap_or(false),
        ),
        "run_shell_command" => {
            let command = args.get("command").and_then(Value::as_str).unwrap_or_default();
            let shell_args = args
                .get("args")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect::<Vec<_>>();
            shell::run_shell_command(
                app,
                command,
                &shell_args,
                args.get("workdir").and_then(Value::as_str),
                args.get("timeoutMs").and_then(Value::as_i64).unwrap_or(20_000),
            )
        }
        "launch_installer_file" => installer::launch_installer_file(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
        ),
        "query_registry_key" => system_registry::query_registry_key(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
        ),
        "read_registry_value" => system_registry::read_registry_value(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
            args.get("name").and_then(Value::as_str).unwrap_or_default(),
        ),
        "write_registry_value" => system_registry::write_registry_value(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
            args.get("name").and_then(Value::as_str).unwrap_or_default(),
            args.get("valueType").and_then(Value::as_str).unwrap_or_default(),
            args.get("value").and_then(Value::as_str).unwrap_or_default(),
        ),
        "delete_registry_value" => system_registry::delete_registry_value(
            app,
            args.get("path").and_then(Value::as_str).unwrap_or_default(),
            args.get("name").and_then(Value::as_str).unwrap_or_default(),
        ),
        "type_text" => windows::input::type_text(
            app,
            args.get("text").and_then(Value::as_str).unwrap_or_default(),
        ),
        "send_hotkey" => {
            let keys = parse_hotkey_keys(args.get("keys"))?;
            windows::input::send_hotkey(app, &keys)
        }
        "click_at" => windows::input::click_at(
            app,
            args.get("x").and_then(Value::as_i64).unwrap_or_default(),
            args.get("y").and_then(Value::as_i64).unwrap_or_default(),
            args.get("button").and_then(Value::as_str).unwrap_or("left"),
        ),
        "scroll_at" => windows::input::scroll_at(
            app,
            args.get("delta").and_then(Value::as_i64).unwrap_or_default(),
            args.get("steps").and_then(Value::as_i64).unwrap_or(1),
            args.get("x").and_then(Value::as_i64),
            args.get("y").and_then(Value::as_i64),
        ),
        "find_element" => windows::uia::find_element(app, args),
        "click_element" => windows::uia::click_element(app, args),
        "get_element_text" => windows::uia::get_element_text(app, args),
        "set_element_value" => windows::uia::set_element_value(
            app,
            args,
            args.get("text").and_then(Value::as_str).unwrap_or_default(),
        ),
        "wait_for_element" => windows::uia::wait_for_element(
            app,
            args,
            args.get("timeoutMs").and_then(Value::as_i64).unwrap_or(3000),
        ),
        _ => Err(ControlError::not_found("tool_not_found", "未知控制工具。")),
    }
}

fn audit_tool(
    app: &AppHandle,
    tool: &str,
    status: &str,
    detail: &str,
    risk_level: u8,
) -> ControlResult<()> {
    let runtime_state: tauri::State<'_, Mutex<RuntimeState>> = app.state();
    let mut runtime = runtime_state
        .lock()
        .map_err(|_| ControlError::internal("助手状态锁定失败"))?;
    expire_runtime_state(&mut runtime);
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record(&format!("control:{tool}"), status, detail, risk_level),
    );
    save(app, &runtime).map_err(ControlError::internal)
}

pub fn service_status(app: &AppHandle) -> ControlResult<ControlServiceStatus> {
    let control_state: tauri::State<'_, ControlServiceState> = app.state();
    let base_url = control_state.bind_address().map_err(ControlError::internal)?;
    Ok(ControlServiceStatus {
        running: base_url.is_some(),
        base_url: base_url.clone(),
        tool_count: registry::tool_definitions().len(),
        message: match base_url {
            Some(address) => format!("控制服务已启动：{address}"),
            None => "控制服务尚未启动。".to_string(),
        },
    })
}

pub fn list_tools() -> Vec<super::types::ControlToolDefinition> {
    registry::tool_definitions()
}

pub fn list_pending(app: &AppHandle) -> ControlResult<Vec<ControlPendingRequest>> {
    cleanup_expired_control_pending(app)?;
    let control_state: tauri::State<'_, ControlServiceState> = app.state();
    let pending = control_state.pending_requests().map_err(ControlError::internal)?;
    let items = pending::list_pending(&pending);
    let _ = logging::append_log(
        app,
        "control_pending",
        "list",
        format!("count={}", items.len()),
    );
    Ok(items)
}

pub fn invoke(app: &AppHandle, request: ToolInvokeRequest) -> ControlResult<ToolInvokeResponse> {
    cleanup_expired_control_pending(app)?;
    let request = canonicalize_request(request);
    let definition = policy::resolve_tool(&request.tool)?;
    let normalized_args = normalize_args(&definition.name, request.args)?;
    let effective_definition = effective_tool_definition(&definition, &normalized_args);

    {
        let runtime_state: tauri::State<'_, Mutex<RuntimeState>> = app.state();
        let mut runtime = runtime_state
            .lock()
            .map_err(|_| ControlError::internal("助手状态锁定失败"))?;
        expire_runtime_state(&mut runtime);
        policy::validate_tool_access(&effective_definition, runtime.permission_level)?;
    }

    if effective_definition.requires_confirmation {
        let prompt = pending_prompt(&effective_definition.name, &normalized_args);
        let preview = pending_preview(&effective_definition.name, &normalized_args);
        let pending_request =
            pending::build_pending_request(&effective_definition, normalized_args, prompt, preview);
        let _ = logging::append_log(
            app,
            "control_pending",
            "created",
            format!(
                "id={} tool={} createdAt={} expiresAt={}",
                pending_request.id,
                pending_request.tool,
                pending_request.created_at,
                pending_request.expires_at
            ),
        );

        {
            let control_state: tauri::State<'_, ControlServiceState> = app.state();
            let mut pending_requests = control_state.pending_requests().map_err(ControlError::internal)?;
            pending::cleanup_expired_pending(&mut pending_requests);
            let count = pending::insert_pending(&mut pending_requests, pending_request.clone());
            let _ = logging::append_log(
                app,
                "control_pending",
                "inserted",
                format!(
                    "id={} tool={} count={}",
                    pending_request.id, pending_request.tool, count
                ),
            );
        }

        let detail = format!("tool={} pending_id={}", effective_definition.name, pending_request.id);
        let _ = logging::append_log(app, &effective_definition.name, "pending", detail.clone());
        audit_tool(app, "pending_requested", "pending", &detail, 2)?;

        return Ok(result_pending(
            pending_request,
            Some("该控制动作需要先确认后执行。".to_string()),
        ));
    }

    let execution = execute_tool(app, &effective_definition.name, &normalized_args);
    match execution {
        Ok(result) => {
            let message = format!("{} 已执行。", effective_definition.title);
            let _ = logging::append_log(app, &effective_definition.name, "ok", &message);
            audit_tool(
                app,
                &effective_definition.name,
                "ok",
                &message,
                if effective_definition.requires_confirmation { 2 } else { 1 },
            )?;
            Ok(result_ok(result, Some(message)))
        }
        Err(error) => {
            let payload = error.payload();
            let _ = logging::append_log(app, &effective_definition.name, &payload.code, &payload.message);
            audit_tool(
                app,
                &effective_definition.name,
                &payload.code,
                &payload.message,
                if effective_definition.requires_confirmation { 2 } else { 1 },
            )?;
            Err(error)
        }
    }
}

pub fn confirm(app: &AppHandle, pending_id: &str) -> ControlResult<ToolInvokeResponse> {
    cleanup_expired_control_pending(app)?;
    let _ = logging::append_log(
        app,
        "control_pending",
        "confirm_lookup",
        format!("id={pending_id}"),
    );

    let pending_request = {
        let control_state: tauri::State<'_, ControlServiceState> = app.state();
        let mut pending_requests = control_state.pending_requests().map_err(ControlError::internal)?;
        let count_before = pending_requests.len();
        let found = pending::take_pending(&mut pending_requests, pending_id).ok_or_else(|| {
            let _ = logging::append_log(
                app,
                "control_pending",
                "confirm_miss",
                format!("id={} countBefore={}", pending_id, count_before),
            );
            ControlError::not_found("pending_not_found", "未找到待确认的控制请求。")
        })?;
        let _ = logging::append_log(
            app,
            "control_pending",
            "confirm_matched",
            format!(
                "id={} tool={} countBefore={} countAfter={}",
                found.id,
                found.tool,
                count_before,
                pending_requests.len()
            ),
        );
        found
    };

    let definition = policy::resolve_tool(&pending_request.tool)?;
    {
        let runtime_state: tauri::State<'_, Mutex<RuntimeState>> = app.state();
        let mut runtime = runtime_state
            .lock()
            .map_err(|_| ControlError::internal("助手状态锁定失败"))?;
        expire_runtime_state(&mut runtime);
        policy::validate_tool_access(&definition, runtime.permission_level)?;
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "control_pending_confirmed",
                "ok",
                format!("tool={} pending_id={}", definition.name, pending_request.id),
                2,
            ),
        );
        save(app, &runtime).map_err(ControlError::internal)?;
    }

    let execution = execute_tool(app, &definition.name, &pending_request.args);
    match execution {
        Ok(result) => {
            let message = format!("{} 已执行。", definition.title);
            let _ = logging::append_log(app, &definition.name, "ok", &message);
            audit_tool(app, &definition.name, "ok", &message, 2)?;
            Ok(result_ok(result, Some(message)))
        }
        Err(error) => {
            let payload = error.payload();
            let _ = logging::append_log(app, &definition.name, &payload.code, &payload.message);
            audit_tool(app, &definition.name, &payload.code, &payload.message, 2)?;
            Err(error)
        }
    }
}

pub fn cancel(app: &AppHandle, pending_id: &str) -> ControlResult<ToolInvokeResponse> {
    cleanup_expired_control_pending(app)?;
    let cancelled = {
        let control_state: tauri::State<'_, ControlServiceState> = app.state();
        let mut pending_requests = control_state.pending_requests().map_err(ControlError::internal)?;
        let count_before = pending_requests.len();
        pending::cancel_pending(&mut pending_requests, pending_id).ok_or_else(|| {
            let _ = logging::append_log(
                app,
                "control_pending",
                "cancel_miss",
                format!("id={} countBefore={}", pending_id, count_before),
            );
            ControlError::not_found("pending_not_found", "未找到待取消的控制请求。")
        }).map(|item| {
            let _ = logging::append_log(
                app,
                "control_pending",
                "cancelled",
                format!(
                    "id={} tool={} countBefore={} countAfter={}",
                    item.id,
                    item.tool,
                    count_before,
                    pending_requests.len()
                ),
            );
            item
        })?
    };

    let detail = format!("tool={} pending_id={}", cancelled.tool, cancelled.id);
    let _ = logging::append_log(app, "control_pending_cancelled", "ok", &detail);
    audit_tool(app, "pending_cancelled", "ok", &detail, 2)?;

    Ok(result_ok(
        json!({
            "cancelled": true,
            "pendingId": cancelled.id,
            "tool": cancelled.tool,
        }),
        Some("控制请求已取消。".to_string()),
    ))
}

fn effective_tool_definition(
    definition: &super::types::ControlToolDefinition,
    args: &Value,
) -> super::types::ControlToolDefinition {
    let mut effective = definition.clone();
    match definition.name.as_str() {
        "write_file_text" => {
            if args.get("overwrite").and_then(Value::as_bool).unwrap_or(false) {
                effective.requires_confirmation = true;
                effective.risk_level = super::types::ControlRiskLevel::WriteHigh;
            }
        }
        "move_path" => {
            if args.get("overwrite").and_then(Value::as_bool).unwrap_or(false) {
                effective.requires_confirmation = true;
                effective.risk_level = super::types::ControlRiskLevel::WriteHigh;
            }
        }
        _ => {}
    }
    effective
}
