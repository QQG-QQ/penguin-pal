use serde_json::Value;
use tauri::AppHandle;

use crate::control::windows::capture;

use super::vision_types::VisionCaptureInfo;

pub fn vision_fallback_for_active_window(app: &AppHandle) -> Result<VisionCaptureInfo, String> {
    let payload = capture::capture_active_window(app).map_err(|error| error.payload().message)?;
    let object = payload
        .as_object()
        .ok_or_else(|| "活动窗口截图结果结构无效。".to_string())?;

    let image_path = object
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "活动窗口截图缺少 path。".to_string())?
        .to_string();

    Ok(VisionCaptureInfo {
        image_path,
        width: object.get("width").and_then(Value::as_i64).unwrap_or_default(),
        height: object
            .get("height")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        window_title: object
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("未知窗口")
            .to_string(),
        note: "UIA 信息不足，已保存活动窗口截图作为视觉兜底工件。".to_string(),
    })
}
