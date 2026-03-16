use serde_json::Value;

use super::{
    screen_context::ScreenContext,
    types::{
        DiscoveredEntity, DiscoveredEntityPayload, DiscoveredEntitySource, EntityPayloadType,
        RuntimeContext,
    },
};

pub const ALLOWED_ENTITY_REFS: &[&str] = &[
    "active_window",
    // 重命名：current_* -> latest_* 以体现动态发现的语义
    "latest_browser_window",
    "latest_notepad_window",
    "latest_wechat_window",
    "latest_visible_input",
    "latest_file_ref",
    "latest_text_value",
];

pub fn merge_screen_context_entities(
    context: &mut RuntimeContext,
    screen: &ScreenContext,
    step: usize,
) {
    upsert_entity(
        context,
        DiscoveredEntity {
            id: "active_window".to_string(),
            label: screen.active_window.title.clone(),
            payload_type: EntityPayloadType::WindowRef,
            payload: DiscoveredEntityPayload::WindowRef {
                title: screen.active_window.title.clone(),
                class_name: screen.active_window.class_name.clone(),
                kind: browser_kind_from_title(&screen.active_window.title),
            },
            created_at_step: step,
            last_seen_step: step,
            source: DiscoveredEntitySource::ScreenContext,
            confidence: 1.0,
        },
    );

    let window_inventory = context.window_inventory.clone();
    for item in &window_inventory {
        let Some(title) = item.get("title").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()) else {
            continue;
        };
        let class_name = item
            .get("className")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let kind = browser_kind_from_title(title)
            .or_else(|| window_kind_hint(title).map(ToString::to_string));
        let entity_id = format!("window:{}", sanitize_id(title));
        upsert_entity(
            context,
            DiscoveredEntity {
                id: entity_id,
                label: title.to_string(),
                payload_type: EntityPayloadType::WindowRef,
                payload: DiscoveredEntityPayload::WindowRef {
                    title: title.to_string(),
                    class_name,
                    kind,
                },
                created_at_step: step,
                last_seen_step: step,
                source: DiscoveredEntitySource::ScreenContext,
                confidence: 0.9,
            },
        );
    }

    if let Some(uia) = &screen.uia {
        for element in &uia.visible_elements {
            let label = element
                .name
                .clone()
                .or_else(|| element.automation_id.clone())
                .or_else(|| element.class_name.clone())
                .unwrap_or_else(|| element.role.clone());
            let entity_id = format!(
                "element:{}:{}",
                sanitize_id(&uia.window_title),
                sanitize_id(&label)
            );
            upsert_entity(
                context,
                DiscoveredEntity {
                    id: entity_id,
                    label: label.clone(),
                    payload_type: EntityPayloadType::ElementRef,
                    payload: DiscoveredEntityPayload::ElementRef {
                        window_title: Some(uia.window_title.clone()),
                        role: Some(element.role.clone()),
                        name: element.name.clone(),
                        automation_id: element.automation_id.clone(),
                        class_name: element.class_name.clone(),
                    },
                    created_at_step: step,
                    last_seen_step: step,
                    source: DiscoveredEntitySource::ScreenContext,
                    confidence: 0.8,
                },
            );
        }

        if let Some(input) = uia
            .visible_elements
            .iter()
            .find(|item| matches!(item.role.as_str(), "Edit" | "Document" | "ComboBox"))
        {
            let label = input
                .name
                .clone()
                .or_else(|| input.automation_id.clone())
                .unwrap_or_else(|| "latest_visible_input".to_string());
            upsert_entity(
                context,
                DiscoveredEntity {
                    id: "latest_visible_input".to_string(),
                    label,
                    payload_type: EntityPayloadType::ElementRef,
                    payload: DiscoveredEntityPayload::ElementRef {
                        window_title: Some(uia.window_title.clone()),
                        role: Some(input.role.clone()),
                        name: input.name.clone(),
                        automation_id: input.automation_id.clone(),
                        class_name: input.class_name.clone(),
                    },
                    created_at_step: step,
                    last_seen_step: step,
                    source: DiscoveredEntitySource::ScreenContext,
                    confidence: 0.85,
                },
            );
        }
    }

    if let Some(text) = &context.clipboard {
        upsert_entity(
            context,
            DiscoveredEntity {
                id: "clipboard_text".to_string(),
                label: "clipboard_text".to_string(),
                payload_type: EntityPayloadType::TextValue,
                payload: DiscoveredEntityPayload::TextValue { text: text.clone() },
                created_at_step: step,
                last_seen_step: step,
                source: DiscoveredEntitySource::ScreenContext,
                confidence: 1.0,
            },
        );
    }
}

pub fn merge_tool_result_entities(
    context: &mut RuntimeContext,
    tool: &str,
    result: &Value,
    step: usize,
) {
    match tool {
        "focus_window" | "open_app" => {
            if let Some(title) = result
                .get("title")
                .and_then(Value::as_str)
                .or_else(|| result.get("app").and_then(Value::as_str))
            {
                upsert_entity(
                    context,
                    DiscoveredEntity {
                        id: format!("tool-window:{}", sanitize_id(title)),
                        label: title.to_string(),
                        payload_type: EntityPayloadType::WindowRef,
                        payload: DiscoveredEntityPayload::WindowRef {
                            title: title.to_string(),
                            class_name: None,
                            kind: window_kind_hint(title).map(ToString::to_string),
                        },
                        created_at_step: step,
                        last_seen_step: step,
                        source: DiscoveredEntitySource::ToolResult,
                        confidence: 0.7,
                    },
                );
            }
        }
        "read_clipboard" | "type_text" | "get_element_text" => {
            if let Some(text) = result
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| result.get("value").and_then(Value::as_str))
            {
                upsert_entity(
                    context,
                    DiscoveredEntity {
                        id: format!("text:{}", step),
                        label: format!("text_step_{step}"),
                        payload_type: EntityPayloadType::TextValue,
                        payload: DiscoveredEntityPayload::TextValue {
                            text: text.to_string(),
                        },
                        created_at_step: step,
                        last_seen_step: step,
                        source: DiscoveredEntitySource::ToolResult,
                        confidence: 0.8,
                    },
                );
            }
        }
        "capture_active_window" => {
            if let Some(path) = result.get("path").and_then(Value::as_str) {
                upsert_entity(
                    context,
                    DiscoveredEntity {
                        id: format!("file:{}", sanitize_id(path)),
                        label: path.to_string(),
                        payload_type: EntityPayloadType::FileRef,
                        payload: DiscoveredEntityPayload::FileRef {
                            path: path.to_string(),
                        },
                        created_at_step: step,
                        last_seen_step: step,
                        source: DiscoveredEntitySource::ToolResult,
                        confidence: 0.9,
                    },
                );
            }
        }
        "list_directory" => {
            if let Some(path) = result.get("path").and_then(Value::as_str) {
                upsert_entity(
                    context,
                    DiscoveredEntity {
                        id: format!("file:{}", sanitize_id(path)),
                        label: path.to_string(),
                        payload_type: EntityPayloadType::FileRef,
                        payload: DiscoveredEntityPayload::FileRef {
                            path: path.to_string(),
                        },
                        created_at_step: step,
                        last_seen_step: step,
                        source: DiscoveredEntitySource::ToolResult,
                        confidence: 0.85,
                    },
                );
            }
            if let Some(items) = result.get("items").and_then(Value::as_array) {
                for item in items.iter().take(20) {
                    if let Some(path) = item.get("path").and_then(Value::as_str) {
                        upsert_entity(
                            context,
                            DiscoveredEntity {
                                id: format!("file:{}", sanitize_id(path)),
                                label: item
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or(path)
                                    .to_string(),
                                payload_type: EntityPayloadType::FileRef,
                                payload: DiscoveredEntityPayload::FileRef {
                                    path: path.to_string(),
                                },
                                created_at_step: step,
                                last_seen_step: step,
                                source: DiscoveredEntitySource::ToolResult,
                                confidence: 0.75,
                            },
                        );
                    }
                }
            }
        }
        "read_file_text" | "write_file_text" | "create_directory" | "move_path" => {
            if tool == "move_path" {
                if let Some(from_path) = result.get("fromPath").and_then(Value::as_str) {
                    remove_file_entity(context, from_path);
                }
            }
            if let Some(path) = result
                .get("path")
                .and_then(Value::as_str)
                .or_else(|| result.get("toPath").and_then(Value::as_str))
            {
                upsert_entity(
                    context,
                    DiscoveredEntity {
                        id: format!("file:{}", sanitize_id(path)),
                        label: path.to_string(),
                        payload_type: EntityPayloadType::FileRef,
                        payload: DiscoveredEntityPayload::FileRef {
                            path: path.to_string(),
                        },
                        created_at_step: step,
                        last_seen_step: step,
                        source: DiscoveredEntitySource::ToolResult,
                        confidence: 0.9,
                    },
                );
            }
            if let Some(text) = result.get("text").and_then(Value::as_str) {
                upsert_entity(
                    context,
                    DiscoveredEntity {
                        id: format!("text:file_step:{step}"),
                        label: format!("file_text_step_{step}"),
                        payload_type: EntityPayloadType::TextValue,
                        payload: DiscoveredEntityPayload::TextValue {
                            text: text.to_string(),
                        },
                        created_at_step: step,
                        last_seen_step: step,
                        source: DiscoveredEntitySource::ToolResult,
                        confidence: 0.8,
                    },
                );
            }
        }
        "delete_path" => {
            if let Some(path) = result.get("path").and_then(Value::as_str) {
                remove_file_entity(context, path);
            }
        }
        "run_shell_command" => {
            if let Some(workdir) = result.get("workdir").and_then(Value::as_str) {
                upsert_entity(
                    context,
                    DiscoveredEntity {
                        id: format!("file:{}", sanitize_id(workdir)),
                        label: workdir.to_string(),
                        payload_type: EntityPayloadType::FileRef,
                        payload: DiscoveredEntityPayload::FileRef {
                            path: workdir.to_string(),
                        },
                        created_at_step: step,
                        last_seen_step: step,
                        source: DiscoveredEntitySource::ToolResult,
                        confidence: 0.8,
                    },
                );
            }
            if let Some(stdout) = result.get("stdout").and_then(Value::as_str) {
                let trimmed = stdout.trim();
                if !trimmed.is_empty() {
                    upsert_entity(
                        context,
                        DiscoveredEntity {
                            id: format!("text:shell_step:{step}"),
                            label: format!("shell_output_step_{step}"),
                            payload_type: EntityPayloadType::TextValue,
                            payload: DiscoveredEntityPayload::TextValue {
                                text: trimmed.to_string(),
                            },
                            created_at_step: step,
                            last_seen_step: step,
                            source: DiscoveredEntitySource::ToolResult,
                            confidence: 0.8,
                        },
                    );
                }
            }
        }
        "launch_installer_file" => {
            if let Some(path) = result.get("path").and_then(Value::as_str) {
                upsert_entity(
                    context,
                    DiscoveredEntity {
                        id: format!("file:{}", sanitize_id(path)),
                        label: path.to_string(),
                        payload_type: EntityPayloadType::FileRef,
                        payload: DiscoveredEntityPayload::FileRef {
                            path: path.to_string(),
                        },
                        created_at_step: step,
                        last_seen_step: step,
                        source: DiscoveredEntitySource::ToolResult,
                        confidence: 0.85,
                    },
                );
            }
        }
        "query_registry_key" | "read_registry_value" | "write_registry_value" | "delete_registry_value" => {
            let summary_text = result
                .get("value")
                .and_then(Value::as_str)
                .or_else(|| result.get("stdout").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(text) = summary_text {
                upsert_entity(
                    context,
                    DiscoveredEntity {
                        id: format!("text:registry_step:{step}"),
                        label: format!("registry_output_step_{step}"),
                        payload_type: EntityPayloadType::TextValue,
                        payload: DiscoveredEntityPayload::TextValue {
                            text: text.to_string(),
                        },
                        created_at_step: step,
                        last_seen_step: step,
                        source: DiscoveredEntitySource::ToolResult,
                        confidence: 0.7,
                    },
                );
            }
        }
        "find_element" => {
            let label = result
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| result.get("automationId").and_then(Value::as_str))
                .unwrap_or("found_element");
            upsert_entity(
                context,
                DiscoveredEntity {
                    id: format!("element:{}", sanitize_id(label)),
                    label: label.to_string(),
                    payload_type: EntityPayloadType::ElementRef,
                    payload: DiscoveredEntityPayload::ElementRef {
                        window_title: result.get("windowTitle").and_then(Value::as_str).map(ToString::to_string),
                        role: result.get("controlType").and_then(Value::as_str).map(ToString::to_string),
                        name: result.get("name").and_then(Value::as_str).map(ToString::to_string),
                        automation_id: result.get("automationId").and_then(Value::as_str).map(ToString::to_string),
                        class_name: result.get("className").and_then(Value::as_str).map(ToString::to_string),
                    },
                    created_at_step: step,
                    last_seen_step: step,
                    source: DiscoveredEntitySource::ToolResult,
                    confidence: 0.75,
                },
            );
        }
        _ => {}
    }
}

pub fn materialize_tool_args(context: &RuntimeContext, tool: &str, args: &Value) -> Result<Value, String> {
    let mut map = args
        .as_object()
        .cloned()
        .ok_or_else(|| "execute_tool.args 必须是 object。".to_string())?;

    let Some(reference) = map
        .remove("targetRef")
        .and_then(|value| value.as_str().map(ToString::to_string))
    else {
        return Ok(Value::Object(map));
    };

    // AI-first: 支持两种引用方式
    // 1. 动态引用：discovered_entity:$TYPE:$ID（精确）
    // 2. 语义引用：latest_browser_window 等（便捷）
    let entity = if reference.starts_with("discovered_entity:") {
        resolve_discovered_entity_ref(context, &reference)
            .ok_or_else(|| format!("当前 runtime context 中没有匹配的动态引用：{reference}"))?
    } else {
        if !ALLOWED_ENTITY_REFS.contains(&reference.as_str()) {
            return Err(format!("未允许的语义引用：{reference}"));
        }
        resolve_entity_ref(context, &reference)
            .ok_or_else(|| format!("当前 runtime context 中没有可用的引用：{reference}"))?
    };

    match (tool, &entity.payload) {
        ("focus_window", DiscoveredEntityPayload::WindowRef { title, .. }) => {
            map.insert("title".to_string(), Value::String(title.clone()));
            map.entry("match".to_string())
                .or_insert_with(|| Value::String("exact".to_string()));
        }
        ("get_element_text" | "find_element" | "click_element" | "set_element_value" | "wait_for_element", DiscoveredEntityPayload::ElementRef { window_title, role, name, automation_id, class_name }) => {
            let selector = serde_json::json!({
                "windowTitle": window_title,
                "controlType": role,
                "name": name,
                "automationId": automation_id,
                "className": class_name,
                "matchMode": "exact"
            });
            map.insert("selector".to_string(), selector);
        }
        ("type_text", DiscoveredEntityPayload::TextValue { text }) => {
            map.insert("text".to_string(), Value::String(text.clone()));
        }
        ("list_directory" | "read_file_text" | "create_directory" | "delete_path" | "launch_installer_file", DiscoveredEntityPayload::FileRef { path }) => {
            map.insert("path".to_string(), Value::String(path.clone()));
        }
        ("write_file_text", DiscoveredEntityPayload::FileRef { path }) => {
            map.insert("path".to_string(), Value::String(path.clone()));
        }
        ("write_file_text", DiscoveredEntityPayload::TextValue { text }) => {
            map.insert("content".to_string(), Value::String(text.clone()));
        }
        ("move_path", DiscoveredEntityPayload::FileRef { path }) => {
            map.insert("fromPath".to_string(), Value::String(path.clone()));
        }
        ("run_shell_command", DiscoveredEntityPayload::FileRef { path }) => {
            map.insert("workdir".to_string(), Value::String(path.clone()));
        }
        _ => {
            return Err(format!("语义引用 {reference} 不能用于工具 {tool}。"));
        }
    }

    Ok(Value::Object(map))
}

fn resolve_entity_ref<'a>(context: &'a RuntimeContext, reference: &str) -> Option<&'a DiscoveredEntity> {
    match reference {
        "active_window" | "latest_visible_input" => context
            .discovered_entities
            .iter()
            .rev()
            .find(|entity| entity.id == reference),
        "latest_browser_window" => context.discovered_entities.iter().rev().find(|entity| {
            matches!(
                &entity.payload,
                DiscoveredEntityPayload::WindowRef {
                    kind: Some(kind),
                    ..
                } if kind == "browser"
            )
        }),
        "latest_notepad_window" => context.discovered_entities.iter().rev().find(|entity| {
            matches!(
                &entity.payload,
                DiscoveredEntityPayload::WindowRef {
                    kind: Some(kind),
                    ..
                } if kind == "notepad"
            )
        }),
        "latest_wechat_window" => context.discovered_entities.iter().rev().find(|entity| {
            matches!(
                &entity.payload,
                DiscoveredEntityPayload::WindowRef {
                    kind: Some(kind),
                    ..
                } if kind == "wechat"
            )
        }),
        "latest_file_ref" => context.discovered_entities.iter().rev().find(|entity| {
            matches!(&entity.payload, DiscoveredEntityPayload::FileRef { .. })
        }),
        "latest_text_value" => context.discovered_entities.iter().rev().find(|entity| {
            matches!(&entity.payload, DiscoveredEntityPayload::TextValue { .. })
        }),
        _ => None,
    }
}

/// AI-first: 支持动态引用 discovered_entity:$TYPE:$ID
/// 格式: discovered_entity:window:Chrome, discovered_entity:element:Edit_1
fn resolve_discovered_entity_ref<'a>(context: &'a RuntimeContext, reference: &str) -> Option<&'a DiscoveredEntity> {
    let rest = reference.strip_prefix("discovered_entity:")?;
    let parts: Vec<&str> = rest.splitn(2, ':').collect();
    if parts.len() < 2 {
        return None;
    }
    let entity_type = parts[0];
    let entity_id_part = parts[1];

    context.discovered_entities.iter().rev().find(|entity| {
        let type_match = match entity_type {
            "window" => matches!(&entity.payload, DiscoveredEntityPayload::WindowRef { .. }),
            "element" => matches!(&entity.payload, DiscoveredEntityPayload::ElementRef { .. }),
            "file" => matches!(&entity.payload, DiscoveredEntityPayload::FileRef { .. }),
            "text" => matches!(&entity.payload, DiscoveredEntityPayload::TextValue { .. }),
            _ => false,
        };
        type_match && (entity.id.contains(entity_id_part) || entity.label.contains(entity_id_part))
    })
}

fn upsert_entity(context: &mut RuntimeContext, mut next: DiscoveredEntity) {
    if let Some(existing) = context.discovered_entities.iter_mut().find(|item| item.id == next.id) {
        next.created_at_step = existing.created_at_step;
        *existing = next;
    } else {
        context.discovered_entities.push(next);
    }
}

fn remove_file_entity(context: &mut RuntimeContext, path: &str) {
    let entity_id = format!("file:{}", sanitize_id(path));
    context.discovered_entities.retain(|item| item.id != entity_id);
}

fn sanitize_id(input: &str) -> String {
    input
        .trim()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>()
}

fn browser_kind_from_title(title: &str) -> Option<String> {
    let lowered = title.to_lowercase();
    if ["chrome", "edge", "firefox", "brave", "opera", "vivaldi", "chromium", "浏览器"]
        .iter()
        .any(|token| lowered.contains(token))
    {
        Some("browser".to_string())
    } else {
        None
    }
}

fn window_kind_hint(title: &str) -> Option<&'static str> {
    let lowered = title.to_lowercase();
    if lowered.contains("记事本") || lowered.contains("notepad") {
        Some("notepad")
    } else if title.contains("微信") {
        Some("wechat")
    } else if browser_kind_from_title(title).is_some() {
        Some("browser")
    } else {
        None
    }
}
