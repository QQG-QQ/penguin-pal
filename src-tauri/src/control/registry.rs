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
                summary: "应用别名，例如 browser / notepad / calculator / explorer / settings。".to_string(),
                example: Some(json!("browser")),
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
            name: "list_directory".to_string(),
            title: "列出目录".to_string(),
            summary: "读取目录内容并返回文件/目录列表。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: vec![file_path_arg("path", "目标目录路径。", json!("C:\\\\Users\\\\Admin\\\\Desktop"))],
        },
        ControlToolDefinition {
            name: "read_file_text".to_string(),
            title: "读取文本文件".to_string(),
            summary: "读取 UTF-8 文本文件内容，大小上限 256KB。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: vec![file_path_arg("path", "目标文本文件路径。", json!("C:\\\\Users\\\\Admin\\\\Desktop\\\\notes.txt"))],
        },
        ControlToolDefinition {
            name: "write_file_text".to_string(),
            title: "写入文本文件".to_string(),
            summary: "创建或写入 UTF-8 文本文件；覆盖现有文件时会进入确认。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteLow,
            requires_confirmation: false,
            args: vec![
                file_path_arg("path", "目标文本文件路径。", json!("C:\\\\Users\\\\Admin\\\\Desktop\\\\notes.txt")),
                ControlToolArgSpec {
                    name: "content".to_string(),
                    required: true,
                    summary: "要写入的文本内容。".to_string(),
                    example: Some(json!("hello from penguin")),
                },
                ControlToolArgSpec {
                    name: "overwrite".to_string(),
                    required: false,
                    summary: "是否覆盖已存在文件，默认 false；true 时进入确认。".to_string(),
                    example: Some(json!(true)),
                },
                ControlToolArgSpec {
                    name: "ensureParent".to_string(),
                    required: false,
                    summary: "父目录不存在时是否自动创建，默认 false。".to_string(),
                    example: Some(json!(true)),
                },
            ],
        },
        ControlToolDefinition {
            name: "create_directory".to_string(),
            title: "创建目录".to_string(),
            summary: "创建目录；默认递归创建中间层级。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteLow,
            requires_confirmation: false,
            args: vec![
                file_path_arg("path", "目标目录路径。", json!("C:\\\\Users\\\\Admin\\\\Desktop\\\\penguin-output")),
                ControlToolArgSpec {
                    name: "recursive".to_string(),
                    required: false,
                    summary: "是否递归创建，默认 true。".to_string(),
                    example: Some(json!(true)),
                },
            ],
        },
        ControlToolDefinition {
            name: "move_path".to_string(),
            title: "移动或重命名路径".to_string(),
            summary: "移动文件或目录；覆盖目标文件时会进入确认。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteLow,
            requires_confirmation: false,
            args: vec![
                ControlToolArgSpec {
                    name: "fromPath".to_string(),
                    required: true,
                    summary: "源路径。".to_string(),
                    example: Some(json!("C:\\\\Users\\\\Admin\\\\Desktop\\\\notes.txt")),
                },
                ControlToolArgSpec {
                    name: "toPath".to_string(),
                    required: true,
                    summary: "目标路径。".to_string(),
                    example: Some(json!("C:\\\\Users\\\\Admin\\\\Desktop\\\\notes-renamed.txt")),
                },
                ControlToolArgSpec {
                    name: "overwrite".to_string(),
                    required: false,
                    summary: "目标已存在时是否覆盖，默认 false；true 时进入确认。".to_string(),
                    example: Some(json!(true)),
                },
            ],
        },
        ControlToolDefinition {
            name: "delete_path".to_string(),
            title: "删除路径".to_string(),
            summary: "删除文件或目录；始终需要确认。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteHigh,
            requires_confirmation: true,
            args: vec![
                file_path_arg("path", "要删除的文件或目录路径。", json!("C:\\\\Users\\\\Admin\\\\Desktop\\\\notes.txt")),
                ControlToolArgSpec {
                    name: "recursive".to_string(),
                    required: false,
                    summary: "删除目录时是否递归删除，默认 false。".to_string(),
                    example: Some(json!(false)),
                },
            ],
        },
        ControlToolDefinition {
            name: "run_shell_command".to_string(),
            title: "执行受控 shell 命令".to_string(),
            summary: "执行受控白名单 shell 命令，仅允许 git/rg/dir/type/where 与有限 build/test 命令。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteLow,
            requires_confirmation: false,
            args: vec![
                ControlToolArgSpec {
                    name: "command".to_string(),
                    required: true,
                    summary: "命令名，例如 pwd / dir / type / where / rg / git / npm / cargo。".to_string(),
                    example: Some(json!("git")),
                },
                ControlToolArgSpec {
                    name: "args".to_string(),
                    required: false,
                    summary: "命令参数数组，只允许白名单子集。".to_string(),
                    example: Some(json!(["status"])),
                },
                ControlToolArgSpec {
                    name: "workdir".to_string(),
                    required: false,
                    summary: "可选工作目录。".to_string(),
                    example: Some(json!("D:\\\\新建文件夹\\\\penguin-pal")),
                },
                ControlToolArgSpec {
                    name: "timeoutMs".to_string(),
                    required: false,
                    summary: "超时毫秒，默认 20000，范围 1000..300000。".to_string(),
                    example: Some(json!(20000)),
                },
            ],
        },
        ControlToolDefinition {
            name: "launch_installer_file".to_string(),
            title: "启动安装器文件".to_string(),
            summary: "启动本地 .exe 或 .msi 安装器；始终需要确认。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteHigh,
            requires_confirmation: true,
            args: vec![file_path_arg("path", "本地安装器路径（.exe/.msi）。", json!("D:\\\\Downloads\\\\setup.exe"))],
        },
        ControlToolDefinition {
            name: "query_registry_key".to_string(),
            title: "读取注册表项".to_string(),
            summary: "查询注册表键内容，只读。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: vec![ControlToolArgSpec {
                name: "path".to_string(),
                required: true,
                summary: "注册表路径，例如 HKCU\\\\Software\\\\PenguinPal。".to_string(),
                example: Some(json!("HKCU\\\\Software\\\\PenguinPal")),
            }],
        },
        ControlToolDefinition {
            name: "read_registry_value".to_string(),
            title: "读取注册表值".to_string(),
            summary: "读取注册表键下的单个值，只读。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::ReadOnly,
            requires_confirmation: false,
            args: vec![
                ControlToolArgSpec {
                    name: "path".to_string(),
                    required: true,
                    summary: "注册表路径。".to_string(),
                    example: Some(json!("HKCU\\\\Software\\\\PenguinPal")),
                },
                ControlToolArgSpec {
                    name: "name".to_string(),
                    required: true,
                    summary: "值名称。".to_string(),
                    example: Some(json!("InstallPath")),
                },
            ],
        },
        ControlToolDefinition {
            name: "write_registry_value".to_string(),
            title: "写入注册表值".to_string(),
            summary: "写入注册表值；只允许 HKCU\\\\Software 或 HKCU\\\\Environment，始终需要确认。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteHigh,
            requires_confirmation: true,
            args: vec![
                ControlToolArgSpec {
                    name: "path".to_string(),
                    required: true,
                    summary: "注册表路径。".to_string(),
                    example: Some(json!("HKCU\\\\Software\\\\PenguinPal")),
                },
                ControlToolArgSpec {
                    name: "name".to_string(),
                    required: true,
                    summary: "值名称。".to_string(),
                    example: Some(json!("InstallPath")),
                },
                ControlToolArgSpec {
                    name: "valueType".to_string(),
                    required: true,
                    summary: "REG_SZ / REG_EXPAND_SZ / REG_DWORD / REG_QWORD。".to_string(),
                    example: Some(json!("REG_SZ")),
                },
                ControlToolArgSpec {
                    name: "value".to_string(),
                    required: true,
                    summary: "要写入的值。".to_string(),
                    example: Some(json!("D:\\\\Apps\\\\PenguinPal")),
                },
            ],
        },
        ControlToolDefinition {
            name: "delete_registry_value".to_string(),
            title: "删除注册表值".to_string(),
            summary: "删除注册表值；只允许 HKCU\\\\Software 或 HKCU\\\\Environment，始终需要确认。".to_string(),
            minimum_permission_level: 0,
            risk_level: ControlRiskLevel::WriteHigh,
            requires_confirmation: true,
            args: vec![
                ControlToolArgSpec {
                    name: "path".to_string(),
                    required: true,
                    summary: "注册表路径。".to_string(),
                    example: Some(json!("HKCU\\\\Software\\\\PenguinPal")),
                },
                ControlToolArgSpec {
                    name: "name".to_string(),
                    required: true,
                    summary: "值名称。".to_string(),
                    example: Some(json!("InstallPath")),
                },
            ],
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

fn file_path_arg(name: &str, summary: &str, example: serde_json::Value) -> ControlToolArgSpec {
    ControlToolArgSpec {
        name: name.to_string(),
        required: true,
        summary: summary.to_string(),
        example: Some(example),
    }
}
