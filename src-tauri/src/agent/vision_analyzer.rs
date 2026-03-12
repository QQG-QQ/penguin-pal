use std::{fs, sync::Mutex};

use serde_json::Value;
use tauri::{AppHandle, Manager, State};

use crate::{
    ai::provider,
    app_state::{now_millis, RuntimeState, VisionChannelConfig},
};

use super::{
    vision_context,
    vision_types::{
        CachedVisionContext, VisionContext, VisionProviderStatus, VisionProviderStatusKind,
        VisionWindowSummary, VISION_CACHE_TTL_MS, VISION_SCHEMA_VERSION,
    },
    AgentTaskState,
};

pub async fn analyze_active_window(
    app: &AppHandle,
    vision_channel: &VisionChannelConfig,
    api_key: Option<String>,
    active_window_title: &str,
    active_window_class_name: Option<&str>,
    vision_prompt: &str,
) -> VisionContext {
    if let Ok(Some(cached)) = get_cached_context(app, active_window_title, active_window_class_name) {
        let mut cached_context = cached.context;
        cached_context.cache_hit = true;
        update_runtime_vision_status(app, &cached_context.provider_status);
        return cached_context;
    }

    let provider_status = provider::vision_support_status(vision_channel, api_key.as_deref());
    if !matches!(
        provider_status.kind,
        VisionProviderStatusKind::Supported | VisionProviderStatusKind::Unknown
    ) {
        update_runtime_vision_status(app, &provider_status);
        return VisionContext {
            provider_status,
            cache_hit: false,
            capture: None,
            summary: None,
        };
    }

    let capture = match vision_context::vision_fallback_for_active_window(app) {
        Ok(capture) => capture,
        Err(error) => {
            let provider_status = VisionProviderStatus {
                kind: VisionProviderStatusKind::AnalysisFailed,
                message: format!("活动窗口截图失败：{error}"),
            };
            update_runtime_vision_status(app, &provider_status);
            return VisionContext {
                provider_status,
                cache_hit: false,
                capture: None,
                summary: None,
            };
        }
    };

    if let Some(limit_error) = validate_capture_against_limits(vision_channel, &capture) {
        let provider_status = VisionProviderStatus {
            kind: VisionProviderStatusKind::AnalysisFailed,
            message: limit_error,
        };
        update_runtime_vision_status(app, &provider_status);
        return VisionContext {
            provider_status,
            cache_hit: false,
            capture: Some(capture),
            summary: None,
        };
    }

    let analysis = provider::analyze_window_image(
        vision_channel,
        api_key,
        std::path::Path::new(&capture.image_path),
        vision_prompt,
    )
    .await;

    let context = match analysis {
        Ok(raw) => match parse_vision_summary(&raw) {
            Ok(summary) => VisionContext {
                provider_status: provider_status.clone(),
                cache_hit: false,
                capture: Some(capture),
                summary: Some(summary),
            },
            Err(error) => VisionContext {
                provider_status: VisionProviderStatus {
                    kind: VisionProviderStatusKind::AnalysisFailed,
                    message: format!("视觉摘要解析失败：{error}"),
                },
                cache_hit: false,
                capture: Some(capture),
                summary: None,
            },
        },
        Err(error) => VisionContext {
            provider_status: status_from_analysis_error(&error),
            cache_hit: false,
            capture: Some(capture),
            summary: None,
        },
    };

    update_runtime_vision_status(app, &context.provider_status);

    let _ = set_cached_context(
        app,
        active_window_title,
        active_window_class_name,
        &context,
    );

    context
}

fn validate_capture_against_limits(
    vision_channel: &VisionChannelConfig,
    capture: &super::vision_types::VisionCaptureInfo,
) -> Option<String> {
    if capture.width > i64::from(vision_channel.max_image_width)
        || capture.height > i64::from(vision_channel.max_image_height)
    {
        return Some(format!(
            "活动窗口截图尺寸 {}x{} 超出视觉副通道限制 {}x{}。",
            capture.width,
            capture.height,
            vision_channel.max_image_width,
            vision_channel.max_image_height
        ));
    }

    let file_size = fs::metadata(&capture.image_path).ok().map(|meta| meta.len());
    if file_size.is_some_and(|size| size > vision_channel.max_image_bytes) {
        return Some(format!(
            "活动窗口截图大小 {} 字节超出视觉副通道限制 {} 字节。",
            file_size.unwrap_or_default(),
            vision_channel.max_image_bytes
        ));
    }

    None
}

fn status_from_analysis_error(error: &str) -> VisionProviderStatus {
    if error.contains("超时") {
        return VisionProviderStatus {
            kind: VisionProviderStatusKind::Timeout,
            message: error.to_string(),
        };
    }

    VisionProviderStatus {
        kind: VisionProviderStatusKind::AnalysisFailed,
        message: error.to_string(),
    }
}

fn update_runtime_vision_status(app: &AppHandle, status: &VisionProviderStatus) {
    let Some(state) = app.try_state::<Mutex<RuntimeState>>() else {
        return;
    };
    let status = status.clone();
    if let Ok(mut runtime) = state.lock() {
        runtime.vision_channel_status = status.clone();
        runtime.vision_channel.last_error = match status.kind {
            VisionProviderStatusKind::AnalysisFailed | VisionProviderStatusKind::Timeout => {
                Some(status.message.clone())
            }
            _ => None,
        };
    };
}

fn get_cached_context(
    app: &AppHandle,
    active_window_title: &str,
    active_window_class_name: Option<&str>,
) -> Result<Option<CachedVisionContext>, String> {
    let state: State<'_, AgentTaskState> = app.state();
    let cache = state.vision_cache()?;
    let Some(entry) = cache.as_ref() else {
        return Ok(None);
    };

    let same_window = entry.window_title == active_window_title.trim()
        && entry.window_class_name.as_deref().unwrap_or_default()
            == active_window_class_name.unwrap_or_default().trim();
    let fresh = now_millis().saturating_sub(entry.created_at) <= VISION_CACHE_TTL_MS;
    if same_window && fresh {
        Ok(Some(entry.clone()))
    } else {
        Ok(None)
    }
}

fn set_cached_context(
    app: &AppHandle,
    active_window_title: &str,
    active_window_class_name: Option<&str>,
    context: &VisionContext,
) -> Result<(), String> {
    let state: State<'_, AgentTaskState> = app.state();
    let mut cache = state.vision_cache()?;
    *cache = Some(CachedVisionContext {
        window_title: active_window_title.trim().to_string(),
        window_class_name: active_window_class_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string),
        created_at: now_millis(),
        context: context.clone(),
    });
    Ok(())
}

fn parse_vision_summary(raw: &str) -> Result<VisionWindowSummary, String> {
    let payload = extract_json(raw)
        .ok_or_else(|| format!("视觉分析没有返回可解析的 JSON：{}", raw.trim()))?;
    let mut value =
        serde_json::from_str::<Value>(&payload).map_err(|error| format!("视觉摘要 JSON 无效：{error}"))?;

    let object = value
        .as_object_mut()
        .ok_or_else(|| "视觉摘要必须是 JSON object。".to_string())?;

    object
        .entry("schemaVersion".to_string())
        .or_insert_with(|| Value::String(VISION_SCHEMA_VERSION.to_string()));

    normalize_optional_string_field(object, "windowKind");
    normalize_optional_string_field(object, "pageKind");
    normalize_optional_string_field(object, "certainty");
    normalize_optional_bool_field(object, "hasObviousInteractiveTarget");
    normalize_optional_array_field(object, "primaryRegions");
    normalize_optional_array_field(object, "keyElements");
    normalize_optional_array_field(object, "notes");

    let mut summary: VisionWindowSummary =
        serde_json::from_value(Value::Object(object.clone())).map_err(|error| {
            format!("视觉摘要字段结构无效：{error}")
        })?;

    if summary.window_kind.trim().is_empty() {
        summary.window_kind = "unknown".to_string();
    }
    if summary.schema_version.trim().is_empty() {
        summary.schema_version = VISION_SCHEMA_VERSION.to_string();
    }
    if summary.primary_regions.len() > 8 {
        summary.primary_regions.truncate(8);
    }
    if summary.key_elements.len() > 12 {
        summary.key_elements.truncate(12);
    }

    Ok(summary)
}

fn extract_json(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value.to_string());
    }

    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end <= start {
        return None;
    }

    let candidate = &trimmed[start..=end];
    serde_json::from_str::<Value>(candidate)
        .ok()
        .map(|value| value.to_string())
}

fn normalize_optional_string_field(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
) {
    let normalized = object
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string();
    object.insert(key.to_string(), Value::String(normalized));
}

fn normalize_optional_bool_field(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
) {
    let normalized = object.get(key).and_then(Value::as_bool).unwrap_or(false);
    object.insert(key.to_string(), Value::Bool(normalized));
}

fn normalize_optional_array_field(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
) {
    let normalized = object
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    object.insert(key.to_string(), Value::Array(normalized));
}
