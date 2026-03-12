use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::control::windows::{uia_context, windowing};

use super::vision_context;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenRect {
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveWindowContext {
    pub title: String,
    #[serde(default)]
    pub class_name: Option<String>,
    #[serde(default)]
    pub bounds: Option<ScreenRect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenContextSource {
    pub uia_available: bool,
    pub used_vision_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionFallbackInfo {
    pub image_path: String,
    pub width: i64,
    pub height: i64,
    pub window_title: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenContextSummary {
    pub visible_element_count: usize,
    #[serde(default)]
    pub primary_actions: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenContext {
    pub source: ScreenContextSource,
    pub active_window: ActiveWindowContext,
    #[serde(default)]
    pub uia: Option<uia_context::WindowUiDescription>,
    #[serde(default)]
    pub vision: Option<VisionFallbackInfo>,
    pub summary: ScreenContextSummary,
}

pub fn describe_current_screen(app: &AppHandle) -> ScreenContext {
    let mut warnings = Vec::new();
    let active_window_from_list = match windowing::list_windows(app) {
        Ok(value) => extract_active_window(&value),
        Err(error) => {
            warnings.push(format!("窗口枚举失败：{}", error.payload().message));
            None
        }
    };

    let uia = match uia_context::describe_active_window_ui(app) {
        Ok(description) => Some(description),
        Err(error) => {
            warnings.push(format!("UIA 描述失败：{}", error.payload().message));
            None
        }
    };

    let mut active_window = active_window_from_list.unwrap_or_else(|| ActiveWindowContext {
        title: uia
            .as_ref()
            .map(|item| item.window_title.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "未知窗口".to_string()),
        class_name: None,
        bounds: None,
    });

    if active_window.class_name.is_none() {
        active_window.class_name = uia
            .as_ref()
            .and_then(|item| item.window_class_name.clone())
            .filter(|value| !value.trim().is_empty());
    }

    let needs_vision_fallback = uia
        .as_ref()
        .map(|description| description.visible_elements.len() < 3)
        .unwrap_or(true);

    let vision = if needs_vision_fallback {
        match vision_context::vision_fallback_for_active_window(app) {
            Ok(info) => Some(info),
            Err(error) => {
                warnings.push(format!("活动窗口截图失败：{error}"));
                None
            }
        }
    } else {
        None
    };

    let summary = ScreenContextSummary {
        visible_element_count: uia
            .as_ref()
            .map(|item| item.visible_elements.len())
            .unwrap_or(0),
        primary_actions: summarize_primary_actions(uia.as_ref()),
        warnings,
    };

    ScreenContext {
        source: ScreenContextSource {
            uia_available: uia.is_some(),
            used_vision_fallback: vision.is_some(),
        },
        active_window,
        uia,
        vision,
        summary,
    }
}

pub fn render_screen_context_for_prompt(context: &ScreenContext) -> String {
    let mut lines = vec![
        "screen_context:".to_string(),
        format!("- activeWindow.title: {}", context.active_window.title),
        format!(
            "- activeWindow.className: {}",
            context
                .active_window
                .class_name
                .as_deref()
                .unwrap_or("unknown")
        ),
        format!(
            "- source: uiaAvailable={} usedVisionFallback={}",
            context.source.uia_available, context.source.used_vision_fallback
        ),
        format!(
            "- visibleElementCount: {}",
            context.summary.visible_element_count
        ),
    ];

    if let Some(bounds) = &context.active_window.bounds {
        lines.push(format!(
            "- activeWindow.bounds: left={} top={} width={} height={}",
            bounds.left, bounds.top, bounds.width, bounds.height
        ));
    }

    if let Some(uia) = &context.uia {
        lines.push("- visibleElements:".to_string());
        for (index, element) in uia.visible_elements.iter().take(10).enumerate() {
            lines.push(format!(
                "  {}. role={} name={} automationId={} className={} enabled={} valuePreview={}",
                index + 1,
                element.role,
                element.name.as_deref().unwrap_or("-"),
                element.automation_id.as_deref().unwrap_or("-"),
                element.class_name.as_deref().unwrap_or("-"),
                element.is_enabled,
                element.value_preview.as_deref().unwrap_or("-"),
            ));
        }
    } else {
        lines.push("- visibleElements: unavailable".to_string());
    }

    if let Some(vision) = &context.vision {
        lines.push(format!(
            "- visionFallback: captured=true windowTitle={} imagePath={} size={}x{} note={}",
            vision.window_title, vision.image_path, vision.width, vision.height, vision.note
        ));
    }

    if !context.summary.primary_actions.is_empty() {
        lines.push(format!(
            "- primaryActions: {}",
            context.summary.primary_actions.join(" | ")
        ));
    }

    if !context.summary.warnings.is_empty() {
        lines.push("- warnings:".to_string());
        for warning in &context.summary.warnings {
            lines.push(format!("  - {warning}"));
        }
    }

    lines.join("\n")
}

fn extract_active_window(value: &Value) -> Option<ActiveWindowContext> {
    let windows = value.as_array()?;
    let active = windows.iter().find(|item| {
        item.as_object()
            .and_then(|entry| entry.get("isActive"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    })?;

    let title = active
        .as_object()
        .and_then(|entry| entry.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();

    let bounds = active
        .as_object()
        .and_then(|entry| entry.get("bounds"))
        .and_then(Value::as_object)
        .map(|bounds| ScreenRect {
            left: bounds.get("left").and_then(Value::as_i64).unwrap_or_default(),
            top: bounds.get("top").and_then(Value::as_i64).unwrap_or_default(),
            width: bounds.get("width").and_then(Value::as_i64).unwrap_or_default(),
            height: bounds.get("height").and_then(Value::as_i64).unwrap_or_default(),
        });

    Some(ActiveWindowContext {
        title,
        class_name: None,
        bounds,
    })
}

fn summarize_primary_actions(
    description: Option<&uia_context::WindowUiDescription>,
) -> Vec<String> {
    let Some(description) = description else {
        return vec![];
    };

    description
        .visible_elements
        .iter()
        .filter_map(|element| {
            let label = element
                .name
                .as_ref()
                .or(element.automation_id.as_ref())
                .or(element.class_name.as_ref())?;
            if label.trim().is_empty() {
                return None;
            }
            Some(format!("{}:{}", element.role, label.trim()))
        })
        .take(6)
        .collect::<Vec<_>>()
}
