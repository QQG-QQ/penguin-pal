use crate::control::types::ControlToolDefinition;

pub fn build_planner_prompt(tools: &[ControlToolDefinition]) -> String {
    let tool_lines = tools
        .iter()
        .map(|tool| {
            let args = if tool.args.is_empty() {
                "无参数".to_string()
            } else {
                tool.args
                    .iter()
                    .map(|arg| format!("{}{}", arg.name, if arg.required { "*" } else { "" }))
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            format!("- {}: {}；参数：{}", tool.name, tool.summary, args)
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "你是 PenguinPal 的桌面控制规划器，不负责自由聊天，只负责判断用户输入是否属于桌面软件控制请求。\n\
        你只能输出一段 JSON，不能输出 markdown、解释、代码块或额外文字。\n\
        如果输入不是桌面控制请求，输出：{{\"route\":\"chat\",\"steps\":[]}}\n\
        如果输入是桌面控制请求，输出：{{\"route\":\"control\",\"taskTitle\":\"...\",\"steps\":[{{\"tool\":\"...\",\"summary\":\"...\",\"args\":{{...}}}}]}}\n\
        规则：\n\
        1. steps 最多 4 步。\n\
        2. 只能使用以下工具，不能发明新工具：\n\
        {tool_lines}\n\
        3. 禁止规划 shell、脚本、下载、安装、浏览器自动化、注册表修改、文件删除、消息自动发送、自动按回车发送内容。\n\
        4. 用户如果只是在聊天、询问、解释概念、要建议，而不是要求你操作电脑，必须输出 route=chat。\n\
        5. 优先生成最小动作，但允许 2~4 步顺序任务。比如“打开记事本并输入 hello”可以规划为 open_app -> list_windows -> focus_window -> type_text。\n\
        6. 对 type_text，只能填单行文本；不能擅自附加换行或 Enter。\n\
        7. 对 send_hotkey，keys 必须是字符串数组，例如 [\"CTRL\",\"V\"]。\n\
        8. 对聊天软件只能输入草稿，不要规划自动发送消息。\n\
        9. 如果请求缺少必要参数，仍输出 route=chat，不要猜测隐私内容或代用户补全文本。"
    )
}
