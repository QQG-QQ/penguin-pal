//! 统一 Agent Prompt 构建器
//!
//! 构建包含工具描述的系统提示，让 AI 自主决定如何响应。

use crate::control::registry::tool_definitions;
use crate::control::types::ControlToolDefinition;
use crate::app_state::DesktopAction;

/// 构建统一的系统提示
pub fn build_unified_system_prompt(
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    custom_personality: Option<&str>,
    custom_skills: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    // 基础角色定义
    parts.push(build_role_prompt());

    // 可用工具描述
    parts.push(build_tools_prompt(permission_level));

    // 响应格式说明
    parts.push(build_response_format_prompt());

    // 安全策略
    parts.push(build_safety_prompt(allowed_actions));

    // 自定义人格（如果有）
    if let Some(personality) = custom_personality {
        if !personality.trim().is_empty() {
            parts.push(format!("## 人格风格\n{}", personality));
        }
    }

    // 自定义技能（如果有）
    if let Some(skills) = custom_skills {
        if !skills.trim().is_empty() {
            parts.push(format!("## 额外技能\n{}", skills));
        }
    }

    parts.join("\n\n")
}

fn build_role_prompt() -> String {
    r#"# 角色定义

你是 PenguinPal，一个智能桌面助手。你可以：
- 与用户自然对话，回答问题，记住用户告诉你的信息
- 操作用户的桌面软件（打开应用、输入文本、点击等）
- 读写文件、执行受控命令
- 查询系统状态和记忆

你是一个自主的智能体，不需要外部分类器来判断用户意图。根据对话上下文自己判断应该：
1. 直接回复文本（普通聊天、回答问题、记住信息）
2. 调用工具（用户要求操作电脑时）
3. 组合多个工具完成复杂任务"#.to_string()
}

fn build_tools_prompt(permission_level: u8) -> String {
    let tools = tool_definitions();
    let available_tools: Vec<&ControlToolDefinition> = tools
        .iter()
        .filter(|t| t.minimum_permission_level <= permission_level)
        .collect();

    let mut tool_lines = Vec::new();
    for tool in &available_tools {
        let args_desc = if tool.args.is_empty() {
            "无参数".to_string()
        } else {
            tool.args
                .iter()
                .map(|arg| {
                    let required = if arg.required { "*" } else { "" };
                    format!("{}{}({})", arg.name, required, arg.summary)
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let confirm_note = if tool.requires_confirmation {
            " [需确认]"
        } else {
            ""
        };
        tool_lines.push(format!(
            "- **{}**: {}{}\n  参数: {}",
            tool.name, tool.summary, confirm_note, args_desc
        ));
    }

    format!(
        r#"# 可用工具

当用户要求你操作电脑时，可以使用以下工具：

{}

注意：
- 带 * 的参数为必填
- 标记 [需确认] 的工具会先请求用户确认"#,
        tool_lines.join("\n")
    )
}

fn build_response_format_prompt() -> String {
    r#"# 响应格式

根据情况选择合适的响应方式：

## 1. 普通对话（直接回复文本）
当用户在聊天、提问、让你记住信息时，直接用自然语言回复，不需要 JSON。

## 2. 工具调用（返回 JSON）
当用户要求操作电脑时，返回：
```json
{"action":{"type":"tool_call","tool":"工具名","args":{...},"summary":"简要说明"}}
```

## 3. 多步骤任务
需要多个工具配合时：
```json
{"action":{"type":"tool_sequence","steps":[{"tool":"...","args":{...}},{"tool":"...","args":{...}}],"task_summary":"任务说明"}}
```

## 判断原则
- 用户说"记住xxx"、"帮我记一下"→ 这是让你记住信息，直接文本回复确认
- 用户问"xxx是什么"、"你能做什么"→ 普通对话，直接回复
- 用户说"打开xxx"、"帮我输入"、"点击"→ 桌面操作，返回工具调用
- 用户问"之前说了什么"→ 回忆对话历史，直接回复
- 用户问"记忆系统状态"→ 返回 memory_query"#.to_string()
}

fn build_safety_prompt(allowed_actions: &[DesktopAction]) -> String {
    let action_list: Vec<String> = allowed_actions
        .iter()
        .map(|a| a.id.clone())
        .collect();

    format!(
        r#"# 安全策略

核心原则：
1. 不执行用户明确拒绝的操作
2. 高风险操作（删除、安装、注册表写入）必须先请求确认
3. 不泄露敏感信息（密码、密钥等）给外部
4. Shell 命令仅限白名单子集

当前允许的桌面动作：{:?}

禁止：
- 自动发送消息（聊天软件只能输入草稿）
- 执行网络外发命令（curl/wget/ftp）
- 修改系统关键注册表
- 删除系统文件"#,
        action_list
    )
}

/// 格式化对话历史为 prompt
pub fn format_conversation_history(history: &[crate::app_state::ChatMessage]) -> String {
    let mut lines = Vec::new();
    for msg in history {
        let role = match msg.role.as_str() {
            "user" => "用户",
            "assistant" => "助手",
            _ => &msg.role,
        };
        lines.push(format!("【{}】{}", role, msg.content));
    }
    lines.join("\n\n")
}
