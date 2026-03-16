use std::path::Path;

use serde_json::{json, Value};

use super::types::{AssertionResult, AssertionType, FailureReasonCode, RuntimeContext};

pub fn evaluate(
    assertion_type: &AssertionType,
    params: &Value,
    context: &RuntimeContext,
    pending_exists: bool,
) -> AssertionResult {
    match assertion_type {
        AssertionType::WindowExists => evaluate_window_exists(params, context),
        AssertionType::ActiveWindowMatches => evaluate_active_window_matches(params, context),
        AssertionType::TextContains => evaluate_text_contains(params, context),
        AssertionType::ScreenContextState => evaluate_screen_context_state(params, context),
        AssertionType::PendingState => evaluate_pending_state(params, pending_exists),
        AssertionType::ConsistencyState => evaluate_consistency_state(params, context),
        AssertionType::FileExists => evaluate_file_exists(params),
    }
}

fn evaluate_window_exists(params: &Value, context: &RuntimeContext) -> AssertionResult {
    let expected = params.get("titleContains").cloned().unwrap_or(Value::Null);
    let token = expected.as_str().unwrap_or_default().trim().to_string();
    let observed_titles = context
        .window_inventory
        .iter()
        .filter_map(|item| item.get("title").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let passed = !token.is_empty() && observed_titles.iter().any(|title| title.contains(&token));
    AssertionResult {
        assertion_type: AssertionType::WindowExists,
        passed,
        observed_value: json!(observed_titles),
        expected_value: expected,
        failure_reason_code: if passed {
            FailureReasonCode::None
        } else {
            FailureReasonCode::AssertionFailed
        },
    }
}

fn evaluate_active_window_matches(params: &Value, context: &RuntimeContext) -> AssertionResult {
    let expected = params.get("titleContains").cloned().unwrap_or(Value::Null);
    let token = expected.as_str().unwrap_or_default().trim().to_string();
    let observed_title = context
        .active_window
        .as_ref()
        .and_then(|value| value.get("title"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let passed = !token.is_empty() && observed_title.contains(&token);
    AssertionResult {
        assertion_type: AssertionType::ActiveWindowMatches,
        passed,
        observed_value: Value::String(observed_title),
        expected_value: expected,
        failure_reason_code: if passed {
            FailureReasonCode::None
        } else {
            FailureReasonCode::AssertionFailed
        },
    }
}

fn evaluate_text_contains(params: &Value, context: &RuntimeContext) -> AssertionResult {
    let expected = params.get("value").cloned().unwrap_or(Value::Null);
    let needle = expected.as_str().unwrap_or_default().trim().to_string();
    let source = params
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("clipboard");
    let observed = match source {
        "last_tool_result" => context
            .recent_tool_results
            .last()
            .and_then(|item| item.payload.clone())
            .and_then(|value| {
                value.get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .or_else(|| value.get("stdout").and_then(Value::as_str).map(ToString::to_string))
                    .or_else(|| value.get("value").and_then(Value::as_str).map(ToString::to_string))
            })
            .unwrap_or_default(),
        _ => context.clipboard.clone().unwrap_or_default(),
    };
    let passed = !needle.is_empty() && observed.contains(&needle);
    AssertionResult {
        assertion_type: AssertionType::TextContains,
        passed,
        observed_value: Value::String(observed),
        expected_value: json!({"source": source, "value": needle}),
        failure_reason_code: if passed {
            FailureReasonCode::None
        } else {
            FailureReasonCode::AssertionFailed
        },
    }
}

fn evaluate_screen_context_state(params: &Value, context: &RuntimeContext) -> AssertionResult {
    let field = params.get("field").and_then(Value::as_str).unwrap_or_default();
    let expected = params.get("equals").cloned().unwrap_or(Value::Null);
    let observed = match field {
        "uia_available" => Value::Bool(context.uia_summary.is_some()),
        "vision_available" => Value::Bool(context.vision_summary.is_some()),
        "clipboard_non_empty" => Value::Bool(context.clipboard.as_ref().is_some_and(|value| !value.trim().is_empty())),
        "window_count" => json!(context.window_inventory.len()),
        _ => Value::Null,
    };
    let passed = observed == expected && observed != Value::Null;
    AssertionResult {
        assertion_type: AssertionType::ScreenContextState,
        passed,
        observed_value: observed,
        expected_value: json!({"field": field, "equals": expected}),
        failure_reason_code: if passed {
            FailureReasonCode::None
        } else {
            FailureReasonCode::AssertionFailed
        },
    }
}

fn evaluate_pending_state(params: &Value, pending_exists: bool) -> AssertionResult {
    let expected = params
        .get("exists")
        .cloned()
        .unwrap_or_else(|| Value::Bool(false));
    let observed = Value::Bool(pending_exists);
    let passed = observed == expected;
    AssertionResult {
        assertion_type: AssertionType::PendingState,
        passed,
        observed_value: observed,
        expected_value: expected,
        failure_reason_code: if passed {
            FailureReasonCode::None
        } else {
            FailureReasonCode::AssertionFailed
        },
    }
}

fn evaluate_consistency_state(params: &Value, context: &RuntimeContext) -> AssertionResult {
    let expected = params.get("equals").cloned().unwrap_or(Value::Null);
    let observed = context
        .consistency
        .as_ref()
        .map(|value| Value::String(value.clone()))
        .unwrap_or(Value::Null);
    let passed = observed == expected && observed != Value::Null;
    AssertionResult {
        assertion_type: AssertionType::ConsistencyState,
        passed,
        observed_value: observed,
        expected_value: expected,
        failure_reason_code: if passed {
            FailureReasonCode::None
        } else {
            FailureReasonCode::AssertionFailed
        },
    }
}

fn evaluate_file_exists(params: &Value) -> AssertionResult {
    let expected = params.get("path").cloned().unwrap_or(Value::Null);
    let path = expected.as_str().unwrap_or_default().trim().to_string();
    let passed = !path.is_empty() && Path::new(&path).exists();
    AssertionResult {
        assertion_type: AssertionType::FileExists,
        passed,
        observed_value: Value::Bool(passed),
        expected_value: expected,
        failure_reason_code: if passed {
            FailureReasonCode::None
        } else {
            FailureReasonCode::FileMissing
        },
    }
}
