use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::{
    app_state::VisionChannelConfig,
    control::windows::{uia_context, windowing},
};

use super::{
    prompt, screen_reconciler, vision_analyzer,
    vision_types::{
        ScreenContextConsistency, ScreenContextConsistencyKind, VisionCaptureInfo, VisionProviderStatus,
        VisionProviderStatusKind, VisionWindowSummary,
    },
};

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
    pub vision_analyzed: bool,
    pub used_vision_fallback: bool,
    pub vision_cache_hit: bool,
    pub vision_provider_status: VisionProviderStatus,
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
    pub vision: Option<VisionWindowSummary>,
    #[serde(default)]
    pub vision_capture: Option<VisionCaptureInfo>,
    pub consistency: ScreenContextConsistency,
    pub summary: ScreenContextSummary,
}

pub async fn describe_current_screen(
    app: &AppHandle,
    vision_channel: &VisionChannelConfig,
    vision_api_key: Option<String>,
) -> ScreenContext {
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
    let vision_prompt = prompt::build_visual_analysis_prompt(&active_window.title);
    let vision_context = vision_analyzer::analyze_active_window(
        app,
        vision_channel,
        vision_api_key,
        &active_window.title,
        active_window.class_name.as_deref(),
        &vision_prompt,
    )
    .await;
    let provider_status = vision_context.provider_status.clone();
    let consistency = screen_reconciler::reconcile_screen_context(uia.as_ref(), &vision_context);
    let mut vision_summary = vision_context.summary.clone();

    if let Some(summary) = vision_summary.as_mut() {
        summary.uia_consistency_hint = Some(consistency_label(&consistency.status).to_string());
    }

    if !matches!(provider_status.kind, VisionProviderStatusKind::Supported) {
        warnings.push(format!("视觉状态：{}", provider_status.message));
    }
    warnings.extend(consistency.reasons.iter().cloned());

    let summary = ScreenContextSummary {
        visible_element_count: uia
            .as_ref()
            .map(|item| item.visible_elements.len())
            .unwrap_or(0),
        primary_actions: summarize_primary_actions(uia.as_ref(), vision_summary.as_ref()),
        warnings,
    };

    ScreenContext {
        source: ScreenContextSource {
            uia_available: uia.is_some(),
            vision_analyzed: vision_summary.is_some() || vision_context.capture.is_some(),
            used_vision_fallback: needs_vision_fallback
                && (vision_summary.is_some() || vision_context.capture.is_some()),
            vision_cache_hit: vision_context.cache_hit,
            vision_provider_status: provider_status,
        },
        active_window,
        uia,
        vision: vision_summary,
        vision_capture: vision_context.capture,
        consistency,
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
            "- source: uiaAvailable={} visionAnalyzed={} usedVisionFallback={} visionCacheHit={}",
            context.source.uia_available,
            context.source.vision_analyzed,
            context.source.used_vision_fallback,
            context.source.vision_cache_hit
        ),
        format!(
            "- visionProviderStatus: kind={} message={}",
            vision_status_label(&context.source.vision_provider_status.kind),
            context.source.vision_provider_status.message
        ),
        format!(
            "- consistency: status={} reasons={}",
            consistency_label(&context.consistency.status),
            if context.consistency.reasons.is_empty() {
                "none".to_string()
            } else {
                context.consistency.reasons.join(" | ")
            }
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
        lines.push("- uia.visibleElements:".to_string());
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
        lines.push("- uia.visibleElements: unavailable".to_string());
    }

    if let Some(vision) = &context.vision {
        lines.push(format!(
            "- vision.summary: schemaVersion={} windowKind={} pageKind={} certainty={} interactiveTarget={} confidence={}",
            vision.schema_version,
            vision.window_kind,
            vision.page_kind.as_deref().unwrap_or("unknown"),
            vision.certainty.as_deref().unwrap_or("unknown"),
            vision.has_obvious_interactive_target,
            vision
                .confidence
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "unknown".to_string()),
        ));
        if !vision.primary_regions.is_empty() {
            lines.push("- vision.primaryRegions:".to_string());
            for (index, region) in vision.primary_regions.iter().take(6).enumerate() {
                lines.push(format!(
                    "  {}. type={} desc={}",
                    index + 1,
                    region.region_type,
                    region.description
                ));
            }
        }
        if !vision.key_elements.is_empty() {
            lines.push("- vision.keyElements:".to_string());
            for (index, element) in vision.key_elements.iter().take(8).enumerate() {
                lines.push(format!(
                    "  {}. role={} label={} locationHint={} interactive={}",
                    index + 1,
                    element.role,
                    element.label.as_deref().unwrap_or("unknown"),
                    element.location_hint.as_deref().unwrap_or("unknown"),
                    element.is_interactive
                ));
            }
        }
    } else {
        lines.push("- vision.summary: unavailable".to_string());
    }

    if let Some(capture) = &context.vision_capture {
        lines.push(format!(
            "- vision.capture: path={} size={}x{} windowTitle={} note={}",
            capture.image_path, capture.width, capture.height, capture.window_title, capture.note
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
    uia: Option<&uia_context::WindowUiDescription>,
    vision: Option<&VisionWindowSummary>,
) -> Vec<String> {
    let mut actions = Vec::new();

    if let Some(description) = uia {
        actions.extend(
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
                .take(6),
        );
    }

    if actions.len() < 6 {
        if let Some(summary) = vision {
            actions.extend(
                summary
                    .key_elements
                    .iter()
                    .filter_map(|element| {
                        let label = element.label.as_deref()?.trim();
                        if label.is_empty() {
                            return None;
                        }
                        Some(format!("{}:{}", element.role, label))
                    })
                    .take(6 - actions.len()),
            );
        }
    }

    actions
}

fn vision_status_label(kind: &VisionProviderStatusKind) -> &'static str {
    match kind {
        VisionProviderStatusKind::Supported => "supported",
        VisionProviderStatusKind::Unknown => "unknown",
        VisionProviderStatusKind::Unsupported => "unsupported",
        VisionProviderStatusKind::Timeout => "timeout",
        VisionProviderStatusKind::DisabledOffline => "disabled_offline",
        VisionProviderStatusKind::AnalysisFailed => "analysis_failed",
    }
}

fn consistency_label(kind: &ScreenContextConsistencyKind) -> &'static str {
    match kind {
        ScreenContextConsistencyKind::Consistent => "consistent",
        ScreenContextConsistencyKind::UiaOnly => "uia_only",
        ScreenContextConsistencyKind::VisionOnly => "vision_only",
        ScreenContextConsistencyKind::SoftConflict => "soft_conflict",
        ScreenContextConsistencyKind::HardConflict => "hard_conflict",
    }
}
