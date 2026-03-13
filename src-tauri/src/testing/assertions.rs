use serde_json::{Map, Value};

use super::types::{TestAssertion, TestFailureStage};

#[derive(Debug, Clone)]
pub struct AssertionContext {
    pub vars: Map<String, Value>,
    pub screen_context: Option<Value>,
    pub last_result: Option<Value>,
}

pub fn evaluate(
    assertion: &TestAssertion,
    context: &AssertionContext,
) -> Result<(), (TestFailureStage, String)> {
    match assertion.kind.as_str() {
        "list_windows_non_empty" => {
            let count = context
                .last_result
                .as_ref()
                .and_then(|value| value.get("windows"))
                .and_then(Value::as_array)
                .map(|items| items.len())
                .unwrap_or_else(|| {
                    context
                        .last_result
                        .as_ref()
                        .and_then(Value::as_array)
                        .map(|items| items.len())
                        .unwrap_or(0)
                });
            if count > 0 {
                Ok(())
            } else {
                Err((TestFailureStage::Assertion, "窗口列表为空。".to_string()))
            }
        }
        "var_present" => {
            let var_name = assertion
                .params
                .get("var")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if var_name.is_empty() {
                return Err((
                    TestFailureStage::Assertion,
                    "var_present 缺少 var 参数。".to_string(),
                ));
            }
            if context.vars.contains_key(var_name) {
                Ok(())
            } else {
                Err((
                    TestFailureStage::Assertion,
                    format!("缺少动态变量：{var_name}。"),
                ))
            }
        }
        "screen_context_available" => {
            let Some(screen_context) = &context.screen_context else {
                return Err((TestFailureStage::Assertion, "没有采集到 screen context。".to_string()));
            };
            let title = screen_context
                .get("activeWindow")
                .and_then(|value| value.get("title"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !title.trim().is_empty() && title != "未知窗口" {
                Ok(())
            } else {
                Err((TestFailureStage::Assertion, "当前活动窗口标题为空。".to_string()))
            }
        }
        "vision_status_exposed" => {
            let kind = context
                .screen_context
                .as_ref()
                .and_then(|value| value.get("source"))
                .and_then(|value| value.get("visionProviderStatus"))
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !kind.trim().is_empty() {
                Ok(())
            } else {
                Err((TestFailureStage::Assertion, "当前视觉状态未暴露。".to_string()))
            }
        }
        "consistency_state_known" => {
            let status = context
                .screen_context
                .as_ref()
                .and_then(|value| value.get("consistency"))
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !status.trim().is_empty() {
                Ok(())
            } else {
                Err((TestFailureStage::Assertion, "一致性状态缺失。".to_string()))
            }
        }
        "screen_context_browser_like" => {
            let Some(screen_context) = &context.screen_context else {
                return Err((TestFailureStage::Assertion, "没有采集到浏览器 screen context。".to_string()));
            };
            let active_title = screen_context
                .get("activeWindow")
                .and_then(|value| value.get("title"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_lowercase();
            let vision_window_kind = screen_context
                .get("vision")
                .and_then(|value| value.get("windowKind"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_lowercase();
            let vision_regions = screen_context
                .get("vision")
                .and_then(|value| value.get("primaryRegions"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let browser_like_title = ["chrome", "edge", "firefox", "浏览器"]
                .iter()
                .any(|token| active_title.contains(token));
            let has_address_bar = vision_regions.iter().any(|item| {
                item.get("regionType")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.eq_ignore_ascii_case("addressBar"))
            });
            if browser_like_title
                || vision_window_kind == "browser"
                || has_address_bar
            {
                Ok(())
            } else {
                Err((
                    TestFailureStage::Assertion,
                    "当前 screen context 没有表现出浏览器/地址栏特征。".to_string(),
                ))
            }
        }
        "screen_context_active_title_contains_any" => {
            let Some(screen_context) = &context.screen_context else {
                return Err((
                    TestFailureStage::Assertion,
                    "没有采集到 screen context。".to_string(),
                ));
            };
            let title = screen_context
                .get("activeWindow")
                .and_then(|value| value.get("title"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let tokens = assertion
                .params
                .get("tokens")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let matched = tokens.iter().filter_map(Value::as_str).any(|token| {
                let token = token.trim();
                !token.is_empty() && title.contains(token)
            });
            if matched {
                Ok(())
            } else {
                Err((
                    TestFailureStage::Assertion,
                    format!("当前活动窗口标题“{title}”未包含任何目标 token。"),
                ))
            }
        }
        "last_result_field_contains" => {
            let field = assertion
                .params
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let contains = assertion
                .params
                .get("contains")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if field.is_empty() || contains.is_empty() {
                return Err((
                    TestFailureStage::Assertion,
                    "last_result_field_contains 缺少 field/contains 参数。".to_string(),
                ));
            }
            let Some(last_result) = &context.last_result else {
                return Err((
                    TestFailureStage::Assertion,
                    "没有可用于断言的 last_result。".to_string(),
                ));
            };
            let value = resolve_path(last_result, field)
                .and_then(Value::as_str)
                .unwrap_or_default();
            if value.contains(contains) {
                Ok(())
            } else {
                Err((
                    TestFailureStage::Assertion,
                    format!("字段 {field} 不包含预期内容：{contains}。"),
                ))
            }
        }
        "last_result_field_non_empty" => {
            let field = assertion
                .params
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if field.is_empty() {
                return Err((
                    TestFailureStage::Assertion,
                    "last_result_field_non_empty 缺少 field 参数。".to_string(),
                ));
            }
            let Some(last_result) = &context.last_result else {
                return Err((
                    TestFailureStage::Assertion,
                    "没有可用于断言的 last_result。".to_string(),
                ));
            };
            let value = resolve_path(last_result, field)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if !value.is_empty() {
                Ok(())
            } else {
                Err((
                    TestFailureStage::Assertion,
                    format!("字段 {field} 为空。"),
                ))
            }
        }
        other => Err((
            TestFailureStage::Assertion,
            format!("未知测试断言：{other}"),
        )),
    }
}

fn resolve_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.trim().is_empty() {
        return Some(value);
    }

    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}
