//! Shell Agent 极简 Prompt
//!
//! 只提供最基本的信息，让 AI 完全自主决策

/// 构建系统提示
pub fn build_system_prompt() -> String {
    r#"你是一个能够操作电脑的 AI 助手。你可以执行 shell 命令（Windows cmd）来完成任务。

输出格式（每次只输出一个 JSON）：
- 执行命令：{"cmd": "命令内容"}
- 直接回复：{"reply": "回复内容"}
- 任务完成：{"done": "完成说明"}
- 任务失败：{"fail": "失败原因"}

执行命令后你会看到输出结果，然后决定下一步。
如果用户只是聊天，直接用 reply 回复即可。"#.to_string()
}

/// 构建包含执行历史的上下文
pub fn build_context(
    user_task: &str,
    history: &[CommandExecution],
    current_step: usize,
) -> String {
    let mut context = format!("用户任务：{}\n\n", user_task);

    if !history.is_empty() {
        context.push_str("执行历史：\n");
        for (i, exec) in history.iter().enumerate() {
            context.push_str(&format!(
                "第{}步：{}\n结果：{}\n\n",
                i + 1,
                exec.command,
                truncate_output(&exec.output, 500)
            ));
        }
    }

    context.push_str(&format!("当前是第{}步，请决定下一步操作。", current_step));
    context
}

/// 命令执行记录
#[derive(Debug, Clone)]
pub struct CommandExecution {
    pub command: String,
    pub output: String,
    pub success: bool,
}

fn truncate_output(output: &str, max_len: usize) -> String {
    if output.len() <= max_len {
        output.to_string()
    } else {
        format!("{}...(截断)", &output[..max_len])
    }
}
