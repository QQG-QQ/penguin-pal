use serde_json::Value;

use crate::control::{
    errors::{ControlError, ControlResult},
    types::UiSelector,
};

pub fn parse_selector(value: &Value) -> ControlResult<UiSelector> {
    let selector_value = value
        .as_object()
        .and_then(|map| map.get("selector"))
        .ok_or_else(|| ControlError::invalid_argument("selector 不能为空。"))?;

    let selector: UiSelector = serde_json::from_value(selector_value.clone()).map_err(|error| {
        ControlError::invalid_argument(format!("selector 结构无效：{error}"))
    })?;

    validate_selector(&selector)?;
    Ok(selector)
}

pub fn selector_to_value(selector: &UiSelector) -> ControlResult<Value> {
    serde_json::to_value(selector)
        .map_err(|error| ControlError::internal(format!("selector 序列化失败：{error}")))
}

fn validate_selector(selector: &UiSelector) -> ControlResult<()> {
    if selector
        .window_title
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err(ControlError::invalid_argument(
            "selector.windowTitle 不能为空。",
        ));
    }

    let has_element_field = [
        selector.automation_id.as_ref(),
        selector.name.as_ref(),
        selector.control_type.as_ref(),
        selector.class_name.as_ref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| !value.trim().is_empty());

    if !has_element_field {
        return Ok(());
    }

    if !["contains", "exact", "prefix"].contains(&selector.match_mode.as_str()) {
        return Err(ControlError::invalid_argument(
            "selector.matchMode 只允许 contains / exact / prefix。",
        ));
    }

    Ok(())
}
