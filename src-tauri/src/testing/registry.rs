use serde_json::json;

use crate::control::types::ControlRiskLevel;

use super::types::{
    TestAssertion, TestCase, TestDestructiveLevel, TestSelection, TestStep, TestTargetPolicy,
};

const WECHAT_TITLE_TOKENS: &[&str] = &["微信", "WeChat"];

pub fn builtin_cases() -> Vec<TestCase> {
    vec![
        TestCase {
            id: "smoke.windows.list".to_string(),
            title: "窗口列表冒烟".to_string(),
            suite: "smoke.desktop_agent".to_string(),
            feature: "desktop.window_inventory".to_string(),
            tags: vec!["smoke".to_string(), "safety".to_string(), "regression".to_string()],
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
            id: "clipboard.read".to_string(),
            title: "剪贴板读取".to_string(),
            suite: "clipboard.core".to_string(),
            feature: "clipboard.read".to_string(),
            tags: vec!["clipboard".to_string(), "regression".to_string(), "safety".to_string()],
            max_probes: 0,
            destructive_level: TestDestructiveLevel::Low,
            test_target_policy: TestTargetPolicy::ReadOnlyCurrentContext,
            risk_level: ControlRiskLevel::ReadOnly,
            preconditions: vec![],
            steps: vec![
                seed_clipboard_step("[PenguinPal TEST clipboard read]", "写入测试剪贴板"),
                control_step("read_clipboard", json!({}), "读取剪贴板"),
            ],
            assertions: vec![assertion(
                "last_result_field_contains",
                json!({ "field": "text", "contains": "[PenguinPal TEST clipboard read]" }),
                "剪贴板读取结果应包含测试文本",
            )],
        },
        TestCase {
            id: "capture.active_window".to_string(),
            title: "活动窗口截图".to_string(),
            suite: "capture.core".to_string(),
            feature: "capture.active_window".to_string(),
            tags: vec!["capture".to_string(), "regression".to_string(), "safety".to_string()],
            max_probes: 0,
            destructive_level: TestDestructiveLevel::None,
            test_target_policy: TestTargetPolicy::ActiveWindowRequired,
            risk_level: ControlRiskLevel::ReadOnly,
            preconditions: vec![],
            steps: vec![control_step(
                "capture_active_window",
                json!({}),
                "截取当前活动窗口",
            )],
            assertions: vec![assertion(
                "last_result_field_non_empty",
                json!({ "field": "path" }),
                "截图结果应返回有效路径",
            )],
        },
        TestCase {
            id: "notepad.open".to_string(),
            title: "记事本打开".to_string(),
            suite: "notepad.adapter".to_string(),
            feature: "notepad.open".to_string(),
            tags: vec!["notepad".to_string(), "regression".to_string()],
            max_probes: 2,
            destructive_level: TestDestructiveLevel::Low,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::WriteLow,
            preconditions: vec![],
            steps: vec![
                control_step("open_app", json!({ "name": "notepad" }), "打开记事本"),
                control_step("list_windows", json!({}), "刷新窗口列表"),
            ],
            assertions: vec![assertion(
                "var_present",
                json!({ "var": "notepadWindow" }),
                "窗口列表中应出现记事本窗口",
            )],
        },
        TestCase {
            id: "notepad.input_text".to_string(),
            title: "记事本输入文本".to_string(),
            suite: "notepad.adapter".to_string(),
            feature: "notepad.input_text".to_string(),
            tags: vec!["notepad".to_string(), "regression".to_string()],
            max_probes: 2,
            destructive_level: TestDestructiveLevel::Draft,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::WriteLow,
            preconditions: vec![],
            steps: vec![
                control_step("open_app", json!({ "name": "notepad" }), "打开记事本"),
                control_step("list_windows", json!({}), "刷新窗口列表"),
                control_step(
                    "focus_window",
                    json!({ "title": "$notepadWindow", "match": "contains" }),
                    "聚焦记事本窗口",
                ),
                control_step(
                    "type_text",
                    json!({ "text": "[PenguinPal TEST type_text]" }),
                    "输入测试文本",
                ),
                control_step(
                    "send_hotkey",
                    json!({ "keys": ["CTRL", "A"] }),
                    "全选文本",
                ),
                control_step(
                    "send_hotkey",
                    json!({ "keys": ["CTRL", "C"] }),
                    "复制选中文本",
                ),
                control_step("read_clipboard", json!({}), "回读剪贴板"),
            ],
            assertions: vec![assertion(
                "last_result_field_contains",
                json!({ "field": "text", "contains": "[PenguinPal TEST type_text]" }),
                "记事本中的输入文本应可被回读",
            )],
        },
        TestCase {
            id: "notepad.paste_clipboard".to_string(),
            title: "记事本粘贴剪贴板".to_string(),
            suite: "notepad.adapter".to_string(),
            feature: "notepad.paste_clipboard".to_string(),
            tags: vec!["notepad".to_string(), "regression".to_string()],
            max_probes: 2,
            destructive_level: TestDestructiveLevel::Draft,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::WriteLow,
            preconditions: vec![],
            steps: vec![
                control_step("open_app", json!({ "name": "notepad" }), "打开记事本"),
                control_step("list_windows", json!({}), "刷新窗口列表"),
                control_step(
                    "focus_window",
                    json!({ "title": "$notepadWindow", "match": "contains" }),
                    "聚焦记事本窗口",
                ),
                seed_clipboard_step("[PenguinPal TEST paste_clipboard]", "写入待粘贴文本"),
                control_step(
                    "send_hotkey",
                    json!({ "keys": ["CTRL", "V"] }),
                    "粘贴剪贴板内容",
                ),
                control_step(
                    "send_hotkey",
                    json!({ "keys": ["CTRL", "A"] }),
                    "全选文本",
                ),
                control_step(
                    "send_hotkey",
                    json!({ "keys": ["CTRL", "C"] }),
                    "复制选中文本",
                ),
                control_step("read_clipboard", json!({}), "回读剪贴板"),
            ],
            assertions: vec![assertion(
                "last_result_field_contains",
                json!({ "field": "text", "contains": "[PenguinPal TEST paste_clipboard]" }),
                "记事本粘贴内容应可被回读",
            )],
        },
        TestCase {
            id: "uia.notepad.find_element".to_string(),
            title: "UIA 查找记事本文件菜单".to_string(),
            suite: "uia.core".to_string(),
            feature: "uia.find_element".to_string(),
            tags: vec!["uia".to_string(), "regression".to_string(), "safety".to_string()],
            max_probes: 1,
            destructive_level: TestDestructiveLevel::None,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::ReadOnly,
            preconditions: vec![],
            steps: vec![
                control_step("open_app", json!({ "name": "notepad" }), "打开记事本"),
                control_step("list_windows", json!({}), "刷新窗口列表"),
                control_step(
                    "focus_window",
                    json!({ "title": "$notepadWindow", "match": "contains" }),
                    "聚焦记事本窗口",
                ),
                control_step(
                    "find_element",
                    json!({
                        "selector": {
                            "windowTitle": "$notepadWindow",
                            "automationId": "File",
                            "controlType": "MenuItem",
                            "matchMode": "contains"
                        }
                    }),
                    "查找记事本文件菜单",
                ),
            ],
            assertions: vec![assertion(
                "last_result_field_contains",
                json!({ "field": "element.automationId", "contains": "File" }),
                "应能定位到记事本文件菜单元素",
            )],
        },
        TestCase {
            id: "uia.notepad.get_element_text".to_string(),
            title: "UIA 读取记事本菜单文本".to_string(),
            suite: "uia.core".to_string(),
            feature: "uia.get_element_text".to_string(),
            tags: vec!["uia".to_string(), "regression".to_string(), "safety".to_string()],
            max_probes: 1,
            destructive_level: TestDestructiveLevel::None,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::ReadOnly,
            preconditions: vec![],
            steps: vec![
                control_step("open_app", json!({ "name": "notepad" }), "打开记事本"),
                control_step("list_windows", json!({}), "刷新窗口列表"),
                control_step(
                    "focus_window",
                    json!({ "title": "$notepadWindow", "match": "contains" }),
                    "聚焦记事本窗口",
                ),
                control_step(
                    "get_element_text",
                    json!({
                        "selector": {
                            "windowTitle": "$notepadWindow",
                            "automationId": "File",
                            "controlType": "MenuItem",
                            "matchMode": "contains"
                        }
                    }),
                    "读取记事本文件菜单文本",
                ),
            ],
            assertions: vec![assertion(
                "last_result_field_non_empty",
                json!({ "field": "text" }),
                "应能读取到 UIA 元素文本",
            )],
        },
        TestCase {
            id: "uia.notepad.wait_for_element".to_string(),
            title: "UIA 等待记事本菜单元素".to_string(),
            suite: "uia.core".to_string(),
            feature: "uia.wait_for_element".to_string(),
            tags: vec!["uia".to_string(), "regression".to_string(), "safety".to_string()],
            max_probes: 1,
            destructive_level: TestDestructiveLevel::None,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::ReadOnly,
            preconditions: vec![],
            steps: vec![
                control_step("open_app", json!({ "name": "notepad" }), "打开记事本"),
                control_step("list_windows", json!({}), "刷新窗口列表"),
                control_step(
                    "focus_window",
                    json!({ "title": "$notepadWindow", "match": "contains" }),
                    "聚焦记事本窗口",
                ),
                control_step(
                    "wait_for_element",
                    json!({
                        "selector": {
                            "windowTitle": "$notepadWindow",
                            "automationId": "File",
                            "controlType": "MenuItem",
                            "matchMode": "contains"
                        },
                        "timeoutMs": 3000
                    }),
                    "等待记事本文件菜单元素出现",
                ),
            ],
            assertions: vec![assertion(
                "last_result_field_contains",
                json!({ "field": "element.automationId", "contains": "File" }),
                "wait_for_element 应返回目标元素",
            )],
        },
        TestCase {
            id: "uia.notepad.set_element_value".to_string(),
            title: "UIA 设置记事本文本".to_string(),
            suite: "uia.core".to_string(),
            feature: "uia.set_element_value".to_string(),
            tags: vec!["uia".to_string(), "regression".to_string()],
            max_probes: 1,
            destructive_level: TestDestructiveLevel::Draft,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::WriteHigh,
            preconditions: vec![],
            steps: vec![
                control_step("open_app", json!({ "name": "notepad" }), "打开记事本"),
                control_step("list_windows", json!({}), "刷新窗口列表"),
                control_step(
                    "focus_window",
                    json!({ "title": "$notepadWindow", "match": "contains" }),
                    "聚焦记事本窗口",
                ),
                control_step(
                    "set_element_value",
                    json!({
                        "selector": {
                            "windowTitle": "$notepadWindow",
                            "controlType": "Document",
                            "className": "RichEditD2DPT",
                            "matchMode": "contains"
                        },
                        "text": "[PenguinPal TEST set_element_value]"
                    }),
                    "通过 UIA 写入记事本文本",
                ),
                control_step(
                    "send_hotkey",
                    json!({ "keys": ["CTRL", "A"] }),
                    "全选文本",
                ),
                control_step(
                    "send_hotkey",
                    json!({ "keys": ["CTRL", "C"] }),
                    "复制文本",
                ),
                control_step("read_clipboard", json!({}), "回读剪贴板"),
            ],
            assertions: vec![assertion(
                "last_result_field_contains",
                json!({ "field": "text", "contains": "[PenguinPal TEST set_element_value]" }),
                "UIA 写入内容应可被回读",
            )],
        },
        TestCase {
            id: "uia.notepad.click_element_pending".to_string(),
            title: "UIA 点击记事本菜单".to_string(),
            suite: "uia.core".to_string(),
            feature: "uia.click_element".to_string(),
            tags: vec!["uia".to_string(), "regression".to_string(), "safety".to_string()],
            max_probes: 1,
            destructive_level: TestDestructiveLevel::Medium,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::WriteHigh,
            preconditions: vec![],
            steps: vec![
                control_step("open_app", json!({ "name": "notepad" }), "打开记事本"),
                control_step("list_windows", json!({}), "刷新窗口列表"),
                control_step(
                    "focus_window",
                    json!({ "title": "$notepadWindow", "match": "contains" }),
                    "聚焦记事本窗口",
                ),
                control_step(
                    "click_element",
                    json!({
                        "selector": {
                            "windowTitle": "$notepadWindow",
                            "automationId": "File",
                            "controlType": "MenuItem",
                            "matchMode": "contains"
                        }
                    }),
                    "点击记事本文件菜单",
                ),
            ],
            assertions: vec![],
        },
        TestCase {
            id: "browser.open".to_string(),
            title: "浏览器打开".to_string(),
            suite: "browser.adapter".to_string(),
            feature: "browser.open".to_string(),
            tags: vec!["browser".to_string(), "regression".to_string()],
            max_probes: 2,
            destructive_level: TestDestructiveLevel::Low,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::WriteLow,
            preconditions: vec![],
            steps: vec![
                control_step("open_app", json!({ "name": "browser" }), "打开浏览器"),
                control_step("list_windows", json!({}), "刷新窗口列表"),
            ],
            assertions: vec![assertion(
                "var_present",
                json!({ "var": "browserWindow" }),
                "窗口列表中应出现浏览器窗口",
            )],
        },
        TestCase {
            id: "browser.focus".to_string(),
            title: "浏览器聚焦".to_string(),
            suite: "browser.adapter".to_string(),
            feature: "browser.focus".to_string(),
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
                capture_step("采集浏览器屏幕上下文"),
            ],
            assertions: vec![assertion(
                "screen_context_browser_like",
                json!({}),
                "当前上下文应表现为浏览器窗口",
            )],
        },
        TestCase {
            id: "browser.new_tab".to_string(),
            title: "浏览器新建标签页".to_string(),
            suite: "browser.adapter".to_string(),
            feature: "browser.new_tab".to_string(),
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
                    json!({ "keys": ["CTRL", "T"] }),
                    "发送 Ctrl+T",
                ),
                capture_step("补采浏览器屏幕上下文"),
            ],
            assertions: vec![assertion(
                "screen_context_browser_like",
                json!({}),
                "新建标签页后上下文仍应表现为浏览器窗口",
            )],
        },
        TestCase {
            id: "browser.address_bar.focus".to_string(),
            title: "浏览器地址栏聚焦".to_string(),
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
            id: "browser.address_bar.input_text".to_string(),
            title: "浏览器地址栏输入文本".to_string(),
            suite: "browser.adapter".to_string(),
            feature: "browser.address_bar_input".to_string(),
            tags: vec!["browser".to_string(), "regression".to_string()],
            max_probes: 2,
            destructive_level: TestDestructiveLevel::Draft,
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
                control_step(
                    "type_text",
                    json!({ "text": "https://example.com" }),
                    "输入 URL 文本",
                ),
                capture_step("补采浏览器屏幕上下文"),
            ],
            assertions: vec![assertion(
                "screen_context_browser_like",
                json!({}),
                "输入文本后上下文仍应表现为浏览器/地址栏场景",
            )],
        },
        TestCase {
            id: "browser.address_bar.paste_clipboard".to_string(),
            title: "浏览器地址栏粘贴剪贴板".to_string(),
            suite: "browser.adapter".to_string(),
            feature: "browser.address_bar_paste".to_string(),
            tags: vec!["browser".to_string(), "regression".to_string()],
            max_probes: 2,
            destructive_level: TestDestructiveLevel::Draft,
            test_target_policy: TestTargetPolicy::NamedWindowRequired,
            risk_level: ControlRiskLevel::WriteLow,
            preconditions: vec![],
            steps: vec![
                seed_clipboard_step("https://example.com/paste-test", "写入浏览器测试剪贴板"),
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
                control_step(
                    "send_hotkey",
                    json!({ "keys": ["CTRL", "V"] }),
                    "发送 Ctrl+V",
                ),
                capture_step("补采浏览器屏幕上下文"),
            ],
            assertions: vec![assertion(
                "screen_context_browser_like",
                json!({}),
                "粘贴文本后上下文仍应表现为浏览器/地址栏场景",
            )],
        },
        TestCase {
            id: "browser.page.scroll".to_string(),
            title: "浏览器页面滚动".to_string(),
            suite: "browser.adapter".to_string(),
            feature: "browser.page_scroll".to_string(),
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
                    "scroll_at",
                    json!({ "delta": -120, "steps": 4 }),
                    "向下滚动页面",
                ),
                capture_step("补采浏览器屏幕上下文"),
            ],
            assertions: vec![assertion(
                "screen_context_browser_like",
                json!({}),
                "滚动后上下文仍应表现为浏览器窗口",
            )],
        },
        TestCase {
            id: "browser.page.safe_click_pending".to_string(),
            title: "浏览器受控页面点击".to_string(),
            suite: "browser.adapter".to_string(),
            feature: "browser.safe_click".to_string(),
            tags: vec!["browser".to_string(), "regression".to_string(), "safety".to_string()],
            max_probes: 2,
            destructive_level: TestDestructiveLevel::Medium,
            test_target_policy: TestTargetPolicy::ActiveWindowRequired,
            risk_level: ControlRiskLevel::WriteHigh,
            preconditions: vec![],
            steps: vec![
                control_step("list_windows", json!({}), "列出窗口"),
                control_step(
                    "focus_window",
                    json!({ "title": "$browserWindow", "match": "contains" }),
                    "聚焦浏览器窗口",
                ),
                capture_step("采集浏览器屏幕上下文"),
                control_step(
                    "click_at",
                    json!({
                        "x": "$activeWindowSafeCenterX",
                        "y": "$activeWindowSafeCenterY",
                        "button": "left"
                    }),
                    "点击页面中间安全区域",
                ),
            ],
            assertions: vec![],
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
                capture_step("采集微信窗口上下文"),
            ],
            assertions: vec![assertion(
                "screen_context_active_title_contains_any",
                json!({ "tokens": WECHAT_TITLE_TOKENS }),
                "当前活动窗口应仍为微信窗口",
            )],
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

fn seed_clipboard_step(text: &str, summary: &str) -> TestStep {
    TestStep::SeedClipboardText {
        text: text.to_string(),
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
