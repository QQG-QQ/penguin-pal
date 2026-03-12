use serde_json::json;

use super::types::{ControlRiskLevel, ControlToolArgSpec, ControlToolDefinition};

pub fn tool_definitions() -> Vec<ControlToolDefinition> {
    vec![
        ControlToolDefinition {
            name: "list_windows".to_string(),
            title: "列出可见窗口".to_string(),
            summary: "返回当前桌面的可见窗口列表、标题和基础位置。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: vec![],
        },
        ControlToolDefinition {
            name: "focus_window".to_string(),
            title: "聚焦窗口".to_string(),
            summary: "按窗口标题匹配并切换到目标窗口。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteLow,
            requires_confirmation: false,
            args: vec![
                ControlToolArgSpec {
                    name: "title".to_string(),
                    required: true,
                    summary: "目标窗口标题关键词。".to_string(),
                    example: Some(json!("微信")),
                },
                ControlToolArgSpec {
                    name: "match".to_string(),
                    required: false,
                    summary: "匹配方式：contains / exact / prefix，默认 contains。".to_string(),
                    example: Some(json!("contains")),
                },
            ],
        },
        ControlToolDefinition {
            name: "open_app".to_string(),
            title: "打开应用".to_string(),
            summary: "按 allowlist 别名启动应用，不接受任意路径和自定义参数。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteLow,
            requires_confirmation: false,
            args: vec![ControlToolArgSpec {
                name: "name".to_string(),
                required: true,
                summary: "应用别名，例如 notepad / calculator / explorer / settings。".to_string(),
                example: Some(json!("notepad")),
            }],
        },
        ControlToolDefinition {
            name: "capture_active_window".to_string(),
            title: "截取当前活动窗口".to_string(),
            summary: "保存当前前台窗口截图到本地 appData/captures。".to_string(),
            minimum_permission_level: 1,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: vec![],
        },
        ControlToolDefinition {
            name: "read_clipboard".to_string(),
            title: "读取剪贴板".to_string(),
            summary: "读取当前文本剪贴板内容，长度上限 8KB。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: vec![],
        },
        ControlToolDefinition {
            name: "type_text".to_string(),
            title: "输入文本".to_string(),
            summary: "向当前活动窗口输入纯文本，不附带回车。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteLow,
            requires_confirmation: false,
            args: vec![ControlToolArgSpec {
                name: "text".to_string(),
                required: true,
                summary: "单行纯文本，长度不超过 500。".to_string(),
                example: Some(json!("你好，这是一条测试文本")),
            }],
        },
        ControlToolDefinition {
            name: "send_hotkey".to_string(),
            title: "发送热键".to_string(),
            summary: "向当前活动窗口发送受限热键组合。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteLow,
            requires_confirmation: false,
            args: vec![ControlToolArgSpec {
                name: "keys".to_string(),
                required: true,
                summary: "热键数组，例如 [\"CTRL\", \"V\"]。".to_string(),
                example: Some(json!(["CTRL", "V"])),
            }],
        },
        ControlToolDefinition {
            name: "click_at".to_string(),
            title: "点击坐标".to_string(),
            summary: "对当前活动窗口内部的相对坐标执行点击。".to_string(),
            minimum_permission_level: 2,
            risk_level: ControlRiskLevel::WriteHigh,
            requires_confirmation: true,
            args: vec![
                ControlToolArgSpec {
                    name: "x".to_string(),
                    required: true,
                    summary: "活动窗口内相对 X 坐标。".to_string(),
                    example: Some(json!(120)),
                },
                ControlToolArgSpec {
                    name: "y".to_string(),
                    required: true,
                    summary: "活动窗口内相对 Y 坐标。".to_string(),
                    example: Some(json!(240)),
                },
                ControlToolArgSpec {
                    name: "button".to_string(),
                    required: false,
                    summary: "left / right / double，默认 left。".to_string(),
                    example: Some(json!("left")),
                },
            ],
        },
        ControlToolDefinition {
            name: "scroll_at".to_string(),
            title: "滚动坐标".to_string(),
            summary: "对活动窗口内指定坐标或默认中心点发送滚轮事件。".to_string(),
            minimum_permission_level: 1,
            risk_level: ControlRiskLevel::WriteLow,
            requires_confirmation: false,
            args: vec![
                ControlToolArgSpec {
                    name: "delta".to_string(),
                    required: true,
                    summary: "单步滚轮增量，正数向上、负数向下。".to_string(),
                    example: Some(json!(-120)),
                },
                ControlToolArgSpec {
                    name: "steps".to_string(),
                    required: false,
                    summary: "重复步数，默认 1，最大 10。".to_string(),
                    example: Some(json!(3)),
                },
                ControlToolArgSpec {
                    name: "x".to_string(),
                    required: false,
                    summary: "活动窗口内相对 X 坐标，不填则取窗口中心。".to_string(),
                    example: Some(json!(200)),
                },
                ControlToolArgSpec {
                    name: "y".to_string(),
                    required: false,
                    summary: "活动窗口内相对 Y 坐标，不填则取窗口中心。".to_string(),
                    example: Some(json!(360)),
                },
            ],
        },
        ControlToolDefinition {
            name: "find_element".to_string(),
            title: "查找 UI 元素".to_string(),
            summary: "按最小 selector 在指定窗口中查找 UI Automation 元素。".to_string(),
            minimum_permission_level: 1,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: selector_args(),
        },
        ControlToolDefinition {
            name: "click_element".to_string(),
            title: "点击 UI 元素".to_string(),
            summary: "按 selector 定位 UI 元素并执行点击。".to_string(),
            minimum_permission_level: 2,
            risk_level: ControlRiskLevel::WriteHigh,
            requires_confirmation: true,
            args: selector_args(),
        },
        ControlToolDefinition {
            name: "get_element_text".to_string(),
            title: "读取元素文本".to_string(),
            summary: "按 selector 定位元素并读取 Value/Text/Name。".to_string(),
            minimum_permission_level: 1,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: selector_args(),
        },
        ControlToolDefinition {
            name: "set_element_value".to_string(),
            title: "设置元素值".to_string(),
            summary: "按 selector 定位元素并设置文本值。".to_string(),
            minimum_permission_level: 2,
            risk_level: ControlRiskLevel::WriteHigh,
            requires_confirmation: true,
            args: {
                let mut args = selector_args();
                args.push(ControlToolArgSpec {
                    name: "text".to_string(),
                    required: true,
                    summary: "要写入元素的文本，长度不超过 500。".to_string(),
                    example: Some(json!("测试内容")),
                });
                args
            },
        },
        ControlToolDefinition {
            name: "wait_for_element".to_string(),
            title: "等待 UI 元素出现".to_string(),
            summary: "按 selector 轮询等待元素出现。".to_string(),
            minimum_permission_level: 1,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: {
                let mut args = selector_args();
                args.push(ControlToolArgSpec {
                    name: "timeoutMs".to_string(),
                    required: false,
                    summary: "等待超时，毫秒，默认 3000，范围 500..10000。".to_string(),
                    example: Some(json!(5000)),
                });
                args
            },
        },
    ]
}

pub fn find_tool_definition(name: &str) -> Option<ControlToolDefinition> {
    tool_definitions()
        .into_iter()
        .find(|definition| definition.name == name)
}

fn selector_args() -> Vec<ControlToolArgSpec> {
    vec![ControlToolArgSpec {
        name: "selector".to_string(),
        required: true,
        summary:
            "最小 selector，支持 windowTitle / automationId / name / controlType / className。"
                .to_string(),
        example: Some(json!({
            "windowTitle": "微信",
            "name": "发送",
            "controlType": "Button"
        })),
    }]
}
