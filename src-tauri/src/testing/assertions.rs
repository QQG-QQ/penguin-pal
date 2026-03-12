use serde_json::Value;

use super::types::{TestAssertion, TestFailureStage};

#[derive(Debug, Clone)]
pub struct AssertionContext {
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
        other => Err((
            TestFailureStage::Assertion,
            format!("未知测试断言：{other}"),
        )),
    }
}
