//! 统一 Agent 响应类型
//!
//! 定义 AI 可以返回的响应格式，支持：
//! - 纯文本回复
//! - 工具调用请求
//! - 需要确认的操作

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// AI 响应的动作类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    /// 纯文本回复
    TextReply {
        message: String,
    },
    /// 调用工具
    ToolCall {
        tool: String,
        #[serde(default = "empty_object")]
        args: Value,
        /// 工具调用的简要说明
        #[serde(default)]
        summary: Option<String>,
    },
    /// 多步骤工具调用（顺序执行）
    ToolSequence {
        steps: Vec<ToolStep>,
        /// 整体任务说明
        #[serde(default)]
        task_summary: Option<String>,
    },
    /// 查询记忆系统
    MemoryQuery {
        /// 查询类型: status, profile, episodic, procedural, policy
        query_type: String,
    },
    /// 需要用户确认的操作
    ConfirmationRequired {
        tool: String,
        args: Value,
        /// 向用户展示的确认消息
        prompt: String,
    },
}

/// 工具调用步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStep {
    pub tool: String,
    #[serde(default = "empty_object")]
    pub args: Value,
    #[serde(default)]
    pub summary: Option<String>,
}

/// 完整的 Agent 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentResponse {
    /// AI 选择的动作
    pub action: AgentAction,
    /// AI 的内部推理（可选，用于调试）
    #[serde(default)]
    pub reasoning: Option<String>,
}

impl AgentResponse {
    /// 创建文本回复
    pub fn text(message: impl Into<String>) -> Self {
        Self {
            action: AgentAction::TextReply {
                message: message.into(),
            },
            reasoning: None,
        }
    }

    /// 创建工具调用
    pub fn tool_call(tool: impl Into<String>, args: Value) -> Self {
        Self {
            action: AgentAction::ToolCall {
                tool: tool.into(),
                args,
                summary: None,
            },
            reasoning: None,
        }
    }

    /// 判断是否为纯文本回复
    pub fn is_text_reply(&self) -> bool {
        matches!(self.action, AgentAction::TextReply { .. })
    }

    /// 提取文本消息（如果是文本回复）
    pub fn text_message(&self) -> Option<&str> {
        match &self.action {
            AgentAction::TextReply { message } => Some(message),
            _ => None,
        }
    }
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// 从 AI 原始输出解析响应
pub fn parse_response(raw: &str) -> Result<AgentResponse, String> {
    let trimmed = raw.trim();

    // 尝试直接解析 JSON
    if let Ok(response) = serde_json::from_str::<AgentResponse>(trimmed) {
        return Ok(response);
    }

    // 尝试提取 JSON 块
    if let Some(json_str) = extract_json_block(trimmed) {
        if let Ok(response) = serde_json::from_str::<AgentResponse>(&json_str) {
            return Ok(response);
        }
        // 尝试只解析 action 部分
        if let Ok(action) = serde_json::from_str::<AgentAction>(&json_str) {
            return Ok(AgentResponse {
                action,
                reasoning: None,
            });
        }
    }

    // 没有找到有效 JSON，视为纯文本回复
    Ok(AgentResponse::text(trimmed))
}

/// 从文本中提取 JSON 块
fn extract_json_block(text: &str) -> Option<String> {
    // 查找 ```json ... ``` 块
    if let Some(start) = text.find("```json") {
        let content_start = start + 7;
        if let Some(end) = text[content_start..].find("```") {
            return Some(text[content_start..content_start + end].trim().to_string());
        }
    }

    // 查找 { ... } 块
    let start = text.find('{')?;
    let mut depth = 0;
    let mut end = start;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if depth == 0 && end > start {
        Some(text[start..end].to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_reply() {
        let response = parse_response("你好，我是你的桌面助手！").unwrap();
        assert!(response.is_text_reply());
        assert_eq!(response.text_message(), Some("你好，我是你的桌面助手！"));
    }

    #[test]
    fn test_parse_tool_call() {
        let json = r#"{"action":{"type":"tool_call","tool":"open_app","args":{"name":"notepad"}}}"#;
        let response = parse_response(json).unwrap();
        assert!(!response.is_text_reply());
        match response.action {
            AgentAction::ToolCall { tool, .. } => assert_eq!(tool, "open_app"),
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_json_in_markdown() {
        let text = r#"好的，我来帮你打开记事本：
```json
{"action":{"type":"tool_call","tool":"open_app","args":{"name":"notepad"}}}
```"#;
        let response = parse_response(text).unwrap();
        match response.action {
            AgentAction::ToolCall { tool, .. } => assert_eq!(tool, "open_app"),
            _ => panic!("Expected ToolCall"),
        }
    }
}
