//! Shell Agent 极简 Prompt
//!
//! 只提供最基本的信息，让 AI 完全自主决策
//! 同时注入从记忆系统检索到的相关经验

/// 构建系统提示
pub fn build_system_prompt() -> String {
    r#"你是运行在用户 Windows 电脑上的桌面助手。你有完整的 shell 权限，可以执行任何 cmd/powershell 命令。

你可以：
- 打开应用程序（start notepad, start msedge url）
- 读写文件（type, echo, copy, del）
- 管理目录（dir, cd, mkdir, rmdir）
- 查看系统信息（systeminfo, hostname, ipconfig）
- 执行任何 Windows 命令

输出格式（每次只输出一个 JSON）：
- 执行命令：{"cmd": "命令内容"}
- 直接回复：{"reply": "回复内容"}
- 任务完成：{"done": "完成说明"}
- 任务失败：{"fail": "失败原因"}

执行命令后你会看到输出结果，然后决定下一步。
如果用户只是聊天，直接用 reply 回复即可。
高风险命令（删除文件等）会提示用户确认后才执行。"#.to_string()
}

/// 构建包含执行历史的上下文
pub fn build_context(
    user_task: &str,
    history: &[CommandExecution],
    current_step: usize,
) -> String {
    build_context_with_memory(user_task, history, current_step, None)
}

/// 构建包含执行历史和记忆上下文的上下文
pub fn build_context_with_memory(
    user_task: &str,
    history: &[CommandExecution],
    current_step: usize,
    memory_context: Option<&str>,
) -> String {
    let mut context = String::new();

    // 1. 注入记忆上下文（相关经验）
    if let Some(memory) = memory_context {
        if !memory.is_empty() {
            context.push_str("## 相关经验\n");
            context.push_str(memory);
            context.push_str("\n\n");
        }
    }

    // 2. 用户任务
    context.push_str(&format!("## 当前任务\n{}\n\n", user_task));

    // 3. 执行历史
    if !history.is_empty() {
        context.push_str("## 执行历史\n");
        for (i, exec) in history.iter().enumerate() {
            let status = if exec.success { "✓" } else { "✗" };
            context.push_str(&format!(
                "第{}步 {}: {}\n输出: {}\n\n",
                i + 1,
                status,
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
