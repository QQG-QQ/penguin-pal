//! Shell Agent 执行器
//!
//! 核心循环：AI 决策 → 执行 → 反馈 → AI 决策

use std::process::Command;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::app_state::now_millis;
use super::risk::{is_high_risk_command, is_forbidden_command, get_risk_description};
use super::prompt::{build_system_prompt, build_context, CommandExecution};

/// Agent 循环结果
#[derive(Debug, Clone)]
pub struct AgentLoopResult {
    pub success: bool,
    pub message: String,
    pub steps_executed: usize,
    pub history: Vec<CommandExecution>,
    /// 如果需要用户确认，返回待确认的命令
    pub pending_confirmation: Option<PendingShellConfirmation>,
}

/// 待确认的 shell 命令
#[derive(Debug, Clone, Serialize)]
pub struct PendingShellConfirmation {
    pub id: String,
    pub command: String,
    pub risk_description: String,
    pub created_at: u64,
}

/// AI 响应类型
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum AIResponse {
    Command { cmd: String },
    Reply { reply: String },
    Done { done: String },
    Fail { fail: String },
}

/// Shell Agent 执行器
pub struct ShellAgentExecutor {
    /// 系统保护上限（防止无限循环烧钱）
    max_steps: usize,
    /// 执行历史
    history: Vec<CommandExecution>,
    /// 当前步数
    current_step: usize,
}

impl ShellAgentExecutor {
    pub fn new() -> Self {
        Self {
            max_steps: 100,  // 系统保护，不是业务逻辑
            history: Vec::new(),
            current_step: 0,
        }
    }

    /// 执行 Agent 循环
    pub async fn run<F, Fut>(
        &mut self,
        _app: &AppHandle,
        user_task: &str,
        ai_caller: F,
    ) -> AgentLoopResult
    where
        F: Fn(String, String) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        let system_prompt = build_system_prompt();

        loop {
            self.current_step += 1;

            // 系统保护上限
            if self.current_step > self.max_steps {
                return AgentLoopResult {
                    success: false,
                    message: format!("已达到系统保护上限({})，任务中止", self.max_steps),
                    steps_executed: self.current_step - 1,
                    history: self.history.clone(),
                    pending_confirmation: None,
                };
            }

            // 构建上下文
            let context = build_context(user_task, &self.history, self.current_step);

            // 调用 AI
            let ai_response = match ai_caller(system_prompt.clone(), context).await {
                Ok(response) => response,
                Err(e) => {
                    return AgentLoopResult {
                        success: false,
                        message: format!("AI 调用失败：{}", e),
                        steps_executed: self.current_step - 1,
                        history: self.history.clone(),
                        pending_confirmation: None,
                    };
                }
            };

            // 解析 AI 响应
            let parsed = match parse_ai_response(&ai_response) {
                Ok(p) => p,
                Err(_) => {
                    // 如果解析失败，把原始响应当作完成消息
                    return AgentLoopResult {
                        success: true,
                        message: ai_response,
                        steps_executed: self.current_step - 1,
                        history: self.history.clone(),
                        pending_confirmation: None,
                    };
                }
            };

            match parsed {
                AIResponse::Reply { reply } => {
                    // 直接回复，不执行命令
                    return AgentLoopResult {
                        success: true,
                        message: reply,
                        steps_executed: self.current_step,
                        history: self.history.clone(),
                        pending_confirmation: None,
                    };
                }
                AIResponse::Done { done } => {
                    return AgentLoopResult {
                        success: true,
                        message: done,
                        steps_executed: self.current_step,
                        history: self.history.clone(),
                        pending_confirmation: None,
                    };
                }
                AIResponse::Fail { fail } => {
                    return AgentLoopResult {
                        success: false,
                        message: fail,
                        steps_executed: self.current_step,
                        history: self.history.clone(),
                        pending_confirmation: None,
                    };
                }
                AIResponse::Command { cmd } => {
                    // 检查是否被禁止
                    if let Some(reason) = is_forbidden_command(&cmd) {
                        self.history.push(CommandExecution {
                            command: cmd.clone(),
                            output: format!("命令被系统禁止：{}", reason),
                            success: false,
                        });
                        continue;
                    }

                    // 检查是否需要确认
                    if is_high_risk_command(&cmd) {
                        let risk_desc = get_risk_description(&cmd);
                        return AgentLoopResult {
                            success: false,
                            message: format!("命令需要确认：{}", cmd),
                            steps_executed: self.current_step,
                            history: self.history.clone(),
                            pending_confirmation: Some(PendingShellConfirmation {
                                id: format!("shell-{}", now_millis()),
                                command: cmd,
                                risk_description: risk_desc,
                                created_at: now_millis(),
                            }),
                        };
                    }

                    // 执行命令
                    let output = execute_shell_command(&cmd);
                    self.history.push(CommandExecution {
                        command: cmd,
                        output: output.clone(),
                        success: true,
                    });
                }
            }
        }
    }

    /// 用户确认后继续执行
    pub fn confirm_and_continue(&mut self, command: &str) -> CommandExecution {
        let output = execute_shell_command(command);
        let exec = CommandExecution {
            command: command.to_string(),
            output,
            success: true,
        };
        self.history.push(exec.clone());
        exec
    }

    /// 用户拒绝命令
    pub fn reject_command(&mut self, command: &str) {
        self.history.push(CommandExecution {
            command: command.to_string(),
            output: "用户拒绝执行此命令".to_string(),
            success: false,
        });
    }
}

impl Default for ShellAgentExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// 解析 AI 响应
fn parse_ai_response(response: &str) -> Result<AIResponse, String> {
    let trimmed = response.trim();

    // 尝试直接解析
    if let Ok(parsed) = serde_json::from_str::<AIResponse>(trimmed) {
        return Ok(parsed);
    }

    // 尝试提取 JSON
    if let Some(json_str) = extract_json(trimmed) {
        if let Ok(parsed) = serde_json::from_str::<AIResponse>(&json_str) {
            return Ok(parsed);
        }
    }

    Err("无法解析 AI 响应".to_string())
}

/// 从文本中提取 JSON
fn extract_json(text: &str) -> Option<String> {
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

/// 执行 shell 命令
fn execute_shell_command(cmd: &str) -> String {
    #[cfg(target_os = "windows")]
    let output = Command::new("cmd")
        .args(["/C", cmd])
        .output();

    #[cfg(not(target_os = "windows"))]
    let output = Command::new("sh")
        .args(["-c", cmd])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            if out.status.success() {
                if stdout.is_empty() {
                    "命令执行成功（无输出）".to_string()
                } else {
                    stdout.to_string()
                }
            } else {
                format!("命令执行失败：{}", if stderr.is_empty() { &stdout } else { &stderr })
            }
        }
        Err(e) => format!("命令执行错误：{}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command() {
        let response = r#"{"cmd": "dir"}"#;
        let parsed = parse_ai_response(response).unwrap();
        match parsed {
            AIResponse::Command { cmd } => assert_eq!(cmd, "dir"),
            _ => panic!("Expected Command"),
        }
    }

    #[test]
    fn test_parse_done() {
        let response = r#"{"done": "任务完成"}"#;
        let parsed = parse_ai_response(response).unwrap();
        match parsed {
            AIResponse::Done { done } => assert_eq!(done, "任务完成"),
            _ => panic!("Expected Done"),
        }
    }

    #[test]
    fn test_extract_json() {
        let text = "好的，我来执行命令：{\"cmd\": \"dir\"}";
        let json = extract_json(text).unwrap();
        assert_eq!(json, "{\"cmd\": \"dir\"}");
    }
}
