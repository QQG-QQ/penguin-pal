use serde_json::json;

use crate::control::types::ControlRiskLevel;

use super::types::{
    TestAssertion, TestCase, TestDestructiveLevel, TestSelection, TestStep,
    TestTargetPolicy,
};

pub fn builtin_cases() -> Vec<TestCase> {
    vec![
        TestCase {
            id: "smoke.windows.list".to_string(),
            title: "窗口列表冒烟".to_string(),
            suite: "smoke.desktop_agent".to_string(),
            feature: "desktop.window_inventory".to_string(),
            tags: vec!["smoke".to_string(), "safety".to_string()],
            max_probes: 0,
            destructive_level: TestDestructiveLevel::None,
            test_target_policy: TestTargetPolicy::ReadOnlyCurrentContext,
            risk_level: ControlRiskLevel::ReadOnly,
            preconditions: vec![],
            steps: vec![control_step("list_windows", json!({}), "列出可见窗口")],
            assertions: vec![assertion("list_windows_non_empty", json!({}), "窗口列表不能为空")],
        },
        TestCase {
            id: "smoke.screen.context".to_string(),
            title: "屏幕上下文冒烟".to_string(),
            suite: "smoke.desktop_agent".to_string(),
            feature: "screen.context".to_string(),
            tags: vec![
                "smoke".to_string(),
                "safety".to_string(),
                "regression".to_string(),
                "vision".to_string(),
            ],
            max_probes: 1,
            destructive_level: TestDestructiveLevel::None,
            test_target_policy: TestTargetPolicy::ReadOnlyCurrentContext,
            risk_level: ControlRiskLevel::ReadOnly,
            preconditions: vec![],
            steps: vec![capture_step("采集当前屏幕上下文")],
            assertions: vec![
                assertion("screen_context_available", json!({}), "应能返回当前屏幕上下文"),
                assertion("vision_status_exposed", json!({}), "应显式暴露视觉状态"),
            ],
        },
        TestCase {
            id: "vision.context.summary".to_string(),
            title: "视觉链路回归".to_string(),
            suite: "vision.core".to_string(),
            feature: "vision.screen_context".to_string(),
            tags: vec![
                "vision".to_string(),
                "regression".to_string(),
                "safety".to_string(),
            ],
            max_probes: 1,
            destructive_level: TestDestructiveLevel::None,
            test_target_policy: TestTargetPolicy::ReadOnlyCurrentContext,
            risk_level: ControlRiskLevel::ReadOnly,
            preconditions: vec![],
            steps: vec![capture_step("采集当前屏幕上下文")],
            assertions: vec![
                assertion("screen_context_available", json!({}), "screen context 应可用"),
                assertion("vision_status_exposed", json!({}), "视觉状态应可见"),
            ],
        },
        TestCase {
            id: "vision.consistency.guard".to_string(),
            title: "视觉一致性防护".to_string(),
            suite: "vision.core".to_string(),
            feature: "vision.consistency".to_string(),
            tags: vec![
                "vision".to_string(),
                "regression".to_string(),
                "safety".to_string(),
            ],
            max_probes: 1,
            destructive_level: TestDestructiveLevel::None,
            test_target_policy: TestTargetPolicy::ReadOnlyCurrentContext,
            risk_level: ControlRiskLevel::ReadOnly,
            preconditions: vec![],
            steps: vec![capture_step("采集当前屏幕上下文")],
            assertions: vec![assertion(
                "consistency_state_known",
                json!({}),
                "一致性状态应明确可见",
            )],
        },
        TestCase {
            id: "browser.address_bar.focus".to_string(),
            title: "浏览器地址栏".to_string(),
            suite: "browser.adapter".to_string(),
            feature: "browser.address_bar".to_string(),
            tags: vec!["browser".to_string(), "regression".to_string()],
            max_probes: 2,
            destructive_level: TestDestructiveLevel::Low,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::WriteLow,
            preconditions: vec![],
            steps: vec![
                control_step("list_windows", json!({}), "列出窗口"),
                control_step(
                    "focus_window",
                    json!({ "title": "$browserWindow", "match": "contains" }),
                    "聚焦浏览器窗口",
                ),
                control_step(
                    "send_hotkey",
                    json!({ "keys": ["CTRL", "L"] }),
                    "发送 Ctrl+L",
                ),
                capture_step("补采浏览器屏幕上下文"),
            ],
            assertions: vec![assertion(
                "screen_context_browser_like",
                json!({}),
                "上下文应表现为浏览器/地址栏场景",
            )],
        },
        TestCase {
            id: "wechat.draft.input".to_string(),
            title: "微信草稿输入".to_string(),
            suite: "wechat.draft".to_string(),
            feature: "wechat.draft_input".to_string(),
            tags: vec!["wechat".to_string(), "regression".to_string()],
            max_probes: 1,
            destructive_level: TestDestructiveLevel::Draft,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::WriteLow,
            preconditions: vec![],
            steps: vec![
                control_step("list_windows", json!({}), "列出窗口"),
                control_step(
                    "focus_window",
                    json!({ "title": "$wechatWindow", "match": "contains" }),
                    "聚焦微信窗口",
                ),
                control_step(
                    "type_text",
                    json!({ "text": "[PenguinPal TEST 草稿]" }),
                    "输入微信测试草稿",
                ),
            ],
            assertions: vec![],
        },
    ]
}

pub fn select_cases(selection: &TestSelection) -> Vec<TestCase> {
    let mut cases = builtin_cases();

    if !selection.case_ids.is_empty() {
        cases.retain(|case| selection.case_ids.iter().any(|id| id == &case.id));
        return cases;
    }

    if let Some(suite) = &selection.suite {
        cases.retain(|case| case.suite == *suite);
    }

    if let Some(feature) = &selection.feature {
        cases.retain(|case| case.feature == *feature);
    }

    if let Some(tag) = &selection.tag {
        cases.retain(|case| case.tags.iter().any(|value| value == tag));
    }

    cases
}

fn control_step(tool: &str, args: serde_json::Value, summary: &str) -> TestStep {
    TestStep::ControlInvoke {
        tool: tool.to_string(),
        args,
        summary: summary.to_string(),
    }
}

fn capture_step(summary: &str) -> TestStep {
    TestStep::CaptureScreenContext {
        summary: summary.to_string(),
    }
}

fn assertion(kind: &str, params: serde_json::Value, summary: &str) -> TestAssertion {
    TestAssertion {
        kind: kind.to_string(),
        params,
        summary: summary.to_string(),
    }
}
