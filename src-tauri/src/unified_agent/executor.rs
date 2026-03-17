//! 统一 Agent 执行器
//!
//! 负责执行 AI 决策的动作，包括工具调用和响应生成。

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::control::router as control_router;
use crate::control::types::{ToolInvokeRequest, ToolInvokeResponse};
use crate::memory::MemoryService;

use super::response::{AgentAction, AgentResponse, ToolStep};

/// 执行结果
pub struct ExecutionResult {
    /// 返回给用户的消息
    pub reply: String,
    /// 执行状态
    pub status: String,
    /// 详细信息
    pub detail: String,
    /// 是否有待确认的操作
    #[allow(dead_code)]
    pub pending_confirmation: Option<PendingConfirmation>,
}

/// 待确认的操作
#[allow(dead_code)]
pub struct PendingConfirmation {
    pub id: String,
    pub tool: String,
    pub prompt: String,
}

/// 统一 Agent 执行器
pub struct UnifiedAgentExecutor<'a> {
    app: &'a AppHandle,
    #[allow(dead_code)]
    permission_level: u8,
}

impl<'a> UnifiedAgentExecutor<'a> {
    pub fn new(app: &'a AppHandle, permission_level: u8) -> Self {
        Self { app, permission_level }
    }

    /// 执行 AI 响应
    pub async fn execute(&self, response: AgentResponse) -> ExecutionResult {
        match response.action {
            AgentAction::TextReply { message } => ExecutionResult {
                reply: message,
                status: "ok".to_string(),
                detail: "text_reply".to_string(),
                pending_confirmation: None,
            },

            AgentAction::ToolCall { tool, args, summary } => {
                self.execute_tool(&tool, args, summary)
            }

            AgentAction::ToolSequence { steps, task_summary } => {
                self.execute_sequence(steps, task_summary)
            }

            AgentAction::MemoryQuery { query_type } => {
                self.execute_memory_query(&query_type)
            }

            AgentAction::ConfirmationRequired { tool, args, prompt } => {
                self.create_confirmation(&tool, args, &prompt)
            }
        }
    }

    /// 执行单个工具调用
    fn execute_tool(
        &self,
        tool: &str,
        args: Value,
        summary: Option<String>,
    ) -> ExecutionResult {
        let request = ToolInvokeRequest {
            tool: tool.to_string(),
            args,
        };

        match control_router::invoke(self.app, request) {
            Ok(response) => self.handle_tool_response(tool, response, summary),
            Err(err) => {
                let error_msg = err.to_string();
                ExecutionResult {
                    reply: format!("工具执行失败：{}", error_msg),
                    status: "error".to_string(),
                    detail: error_msg,
                    pending_confirmation: None,
                }
            }
        }
    }

    /// 处理工具响应
    fn handle_tool_response(
        &self,
        tool: &str,
        response: ToolInvokeResponse,
        summary: Option<String>,
    ) -> ExecutionResult {
        // 检查是否需要确认
        if let Some(pending) = response.pending_request {
            return ExecutionResult {
                reply: format!("操作 {} 需要你的确认：{}", tool, pending.prompt),
                status: "pending_confirmation".to_string(),
                detail: format!("pending_id={}", pending.id),
                pending_confirmation: Some(PendingConfirmation {
                    id: pending.id,
                    tool: pending.tool,
                    prompt: pending.prompt,
                }),
            };
        }

        // 检查错误
        if let Some(error) = response.error {
            return ExecutionResult {
                reply: format!("操作失败：{}", error.message),
                status: "error".to_string(),
                detail: error.code,
                pending_confirmation: None,
            };
        }

        // 成功
        let result_desc = response
            .result
            .map(|v| format_tool_result(&v))
            .or(response.message)
            .unwrap_or_else(|| "操作完成".to_string());

        let reply = if let Some(s) = summary {
            format!("{}：{}", s, result_desc)
        } else {
            result_desc
        };

        ExecutionResult {
            reply,
            status: "ok".to_string(),
            detail: format!("tool={}", tool),
            pending_confirmation: None,
        }
    }

    /// 执行工具序列
    fn execute_sequence(
        &self,
        steps: Vec<ToolStep>,
        task_summary: Option<String>,
    ) -> ExecutionResult {
        let mut results = Vec::new();
        let mut last_error: Option<String> = None;

        for (i, step) in steps.iter().enumerate() {
            let result = self.execute_tool(&step.tool, step.args.clone(), step.summary.clone());

            // 如果需要确认，中断序列
            if result.pending_confirmation.is_some() {
                return result;
            }

            // 如果出错，记录并继续（或停止）
            if result.status == "error" {
                last_error = Some(result.reply.clone());
                break;
            }

            results.push(format!("步骤 {}: {}", i + 1, result.reply));
        }

        let has_error = last_error.is_some();
        let reply = if let Some(ref error) = last_error {
            format!(
                "任务执行中断：{}\n\n已完成步骤：\n{}",
                error,
                results.join("\n")
            )
        } else {
            let summary_text = task_summary.unwrap_or_else(|| "任务完成".to_string());
            format!("{}：\n{}", summary_text, results.join("\n"))
        };

        ExecutionResult {
            reply,
            status: if has_error { "partial" } else { "ok" }.to_string(),
            detail: format!("steps={}", steps.len()),
            pending_confirmation: None,
        }
    }

    /// 执行记忆查询
    fn execute_memory_query(&self, query_type: &str) -> ExecutionResult {
        let app_data = match self.app.path().app_data_dir() {
            Ok(path) => path,
            Err(e) => {
                let error_msg = e.to_string();
                return ExecutionResult {
                    reply: format!("无法获取数据目录：{}", error_msg),
                    status: "error".to_string(),
                    detail: error_msg,
                    pending_confirmation: None,
                }
            }
        };

        let memory_service = MemoryService::new(&app_data);
        let reply = match query_type {
            "status" => format_memory_status(&memory_service),
            "profile" => format_profile_memory(&memory_service),
            "episodic" => format_episodic_memory(&memory_service),
            _ => format!("未知的记忆查询类型：{}", query_type),
        };

        ExecutionResult {
            reply,
            status: "ok".to_string(),
            detail: format!("memory_query={}", query_type),
            pending_confirmation: None,
        }
    }

    /// 创建待确认的操作
    fn create_confirmation(
        &self,
        tool: &str,
        args: Value,
        prompt: &str,
    ) -> ExecutionResult {
        // 直接调用工具，让工具自己处理确认流程
        self.execute_tool(tool, args, Some(prompt.to_string()))
    }
}

/// 格式化工具结果为可读文本
fn format_tool_result(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(arr) if arr.len() <= 5 => {
            let items: Vec<String> = arr
                .iter()
                .map(|v| format_tool_result(v))
                .collect();
            items.join(", ")
        }
        Value::Array(arr) => format!("共 {} 项", arr.len()),
        Value::Object(obj) => {
            if let Some(message) = obj.get("message").and_then(|v| v.as_str()) {
                return message.to_string();
            }
            if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                return text.to_string();
            }
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        _ => value.to_string(),
    }
}

/// 格式化记忆系统状态
fn format_memory_status(service: &MemoryService) -> String {
    let profile = service.load_profile().unwrap_or_default();
    let episodic = service.store().load_episodic().unwrap_or_default();

    format!(
        r#"## 记忆系统状态

### Profile Memory (用户偏好)
- 常用应用：{} 个
- 工作目录：{} 个

### Episodic Memory (任务历史)
- 历史条目：{} 条"#,
        profile.preferred_apps.len(),
        profile.common_workdirs.len(),
        episodic.entries.len()
    )
}

fn format_profile_memory(service: &MemoryService) -> String {
    let profile = service.load_profile().unwrap_or_default();
    format!(
        "用户偏好：\n- 常用应用：{:?}\n- 工作目录：{:?}",
        profile.preferred_apps, profile.common_workdirs
    )
}

fn format_episodic_memory(service: &MemoryService) -> String {
    let episodic = service.store().load_episodic().unwrap_or_default();
    if episodic.entries.is_empty() {
        return "暂无任务历史记录。".to_string();
    }
    let recent: Vec<String> = episodic
        .entries
        .iter()
        .rev()
        .take(5)
        .map(|e| format!("- {}: {}", e.task_title, e.outcome))
        .collect();
    format!("最近任务：\n{}", recent.join("\n"))
}
