//! Shell Agent 执行器
//!
//! 核心循环：AI 决策 → 规则检查 → 权限验证 → 执行 → 反馈 → 记忆写回 → AI 决策
//!
//! ## 三层架构集成
//!
//! 1. **记忆层**: 从 MemoryService 检索相关记忆，任务完成后写回经验
//! 2. **规则层**: RuleEngine 应用行为规则，调整 AI 行为
//! 3. **权限层**: PermissionChecker 验证操作权限，AI 不能自主修改权限

#![allow(dead_code)]

use std::path::Path;
use std::process::Command;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::app_state::now_millis;
use crate::memory::{MemoryService, MemoryQuery, WriteBackRequest, RuntimeContextDigest, KeyEntity};
use crate::permission::{PermissionChecker, PermissionScope, GrantSource, PermissionCheckResult, PermissionStore};
use crate::rule_engine::{RuleEngine, RuleContext, RuleApplicationResult};
use super::risk::{is_high_risk_command, is_forbidden_command, get_risk_description};
use super::prompt::{build_system_prompt_with_permissions, build_context_with_memory, CommandExecution};

/// Agent 循环结果
#[derive(Debug, Clone)]
pub struct AgentLoopResult {
    pub success: bool,
    pub message: String,
    pub steps_executed: usize,
    pub history: Vec<CommandExecution>,
    /// 如果需要用户确认，返回待确认的命令
    pub pending_confirmation: Option<PendingShellConfirmation>,
    /// 如果请求退出应用
    pub request_exit: bool,
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
    ExitApp { exit_app: String },
}

/// Shell Agent 执行器
pub struct ShellAgentExecutor {
    /// 系统保护上限（防止无限循环烧钱）
    max_steps: usize,
    /// 执行历史
    history: Vec<CommandExecution>,
    /// 当前步数
    current_step: usize,
    /// 权限检查器
    permission_checker: PermissionChecker,
    /// 规则引擎
    rule_engine: RuleEngine,
}

impl ShellAgentExecutor {
    pub fn new() -> Self {
        Self {
            max_steps: 100,  // 系统保护，不是业务逻辑
            history: Vec::new(),
            current_step: 0,
            permission_checker: PermissionChecker::new(),
            rule_engine: RuleEngine::default(),
        }
    }

    /// 从应用数据目录加载权限状态创建执行器
    pub fn with_app_data(app_data_dir: &Path) -> Self {
        let permission_store_path = app_data_dir.join("permissions");
        let permission_store = PermissionStore::new(&permission_store_path);
        let mut permission_checker = PermissionChecker::new();

        // 从持久化存储加载权限
        if let Ok(state) = permission_store.load() {
            for permission in state.permissions {
                if permission.is_valid() {
                    permission_checker.restore_permission(permission);
                }
            }
            for policy in state.policies {
                permission_checker.add_policy(policy);
            }
        }

        let rule_store_path = app_data_dir.join("rules");
        let mut rule_engine = RuleEngine::new(&rule_store_path);
        let _ = rule_engine.load();

        Self {
            max_steps: 100,
            history: Vec::new(),
            current_step: 0,
            permission_checker,
            rule_engine,
        }
    }

    /// 检查命令权限
    fn check_command_permission(&mut self, cmd: &str) -> PermissionCheckResult {
        // 根据命令类型构建权限 ID
        let permission_id = if cmd.contains("rm ") || cmd.contains("del ") || cmd.contains("rmdir") {
            "shell:delete"
        } else if cmd.contains("mv ") || cmd.contains("move ") || cmd.contains("ren ") {
            "shell:modify"
        } else if cmd.contains("curl ") || cmd.contains("wget ") || cmd.contains("Invoke-WebRequest") {
            "shell:network"
        } else if cmd.contains("reg ") || cmd.contains("regedit") {
            "shell:registry"
        } else if cmd.contains("shutdown") || cmd.contains("reboot") {
            "shell:system"
        } else {
            "shell:execute"
        };

        self.permission_checker.check(permission_id, "shell_agent")
    }

    /// 获取当前权限摘要，用于 AI 回复
    pub fn get_permission_summary(&mut self) -> String {
        let permissions = [
            ("shell:execute", "基本执行"),
            ("shell:modify", "文件修改"),
            ("shell:delete", "文件删除"),
            ("shell:network", "网络访问"),
            ("shell:system", "系统操作"),
            ("shell:registry", "注册表"),
        ];

        let mut enabled = Vec::new();
        let mut disabled = Vec::new();

        for (id, label) in permissions {
            let result = self.permission_checker.check(id, "shell_agent");
            if result.allowed {
                enabled.push(label);
            } else {
                disabled.push(label);
            }
        }

        if enabled.is_empty() {
            "Shell Agent 已禁用，无任何 shell 权限。".to_string()
        } else {
            format!(
                "已启用: {}。未启用: {}。",
                enabled.join("、"),
                if disabled.is_empty() { "无".to_string() } else { disabled.join("、") }
            )
        }
    }

    /// 应用规则引擎
    fn apply_rules(&self, cmd: &str) -> RuleApplicationResult {
        let context = RuleContext::new()
            .with_tool("shell")
            .with_goal(cmd)
            .with_step(self.current_step as u32);

        self.rule_engine.apply_rules(&context)
    }

    /// 检索相关记忆上下文
    fn retrieve_memory_context(&self, app: &AppHandle, user_task: &str) -> Option<String> {
        let app_data_dir = match app.path().app_data_dir() {
            Ok(dir) => dir,
            Err(_) => return None,
        };

        let memory_service = MemoryService::new(&app_data_dir);
        let query = MemoryQuery {
            goal: Some(user_task.to_string()),
            intent: Some("shell_command".to_string()),
            limit: 3,  // 最多返回 3 条相关记忆
            ..Default::default()
        };

        match memory_service.render_for_prompt(&query) {
            Ok(context) if !context.is_empty() => Some(context),
            _ => None,
        }
    }

    /// 写回任务结果到记忆系统
    fn write_back_result(&self, app: &AppHandle, user_task: &str, success: bool, _message: &str) {
        let app_data_dir = match app.path().app_data_dir() {
            Ok(dir) => dir,
            Err(_) => return,
        };

        let memory_service = MemoryService::new(&app_data_dir);

        // 提取使用的工具（命令）
        let used_tools: Vec<String> = self.history
            .iter()
            .filter(|h| h.success)
            .map(|h| {
                // 提取命令的第一个词作为工具名
                h.command.split_whitespace().next().unwrap_or("unknown").to_string()
            })
            .collect();

        // 提取关键实体
        let key_entities: Vec<KeyEntity> = self.history
            .iter()
            .filter_map(|h| {
                // 尝试提取文件路径等实体
                if h.command.contains(":\\") || h.command.contains("/") {
                    Some(KeyEntity {
                        entity_type: "path".to_string(),
                        id: h.command.clone(),
                        label: h.command.clone(),
                    })
                } else {
                    None
                }
            })
            .take(5)  // 最多 5 个
            .collect();

        let request = WriteBackRequest {
            task_id: format!("shell_{}", crate::memory::now_millis()),
            goal: user_task.to_string(),
            intent: "shell_command".to_string(),
            final_status: if success { "completed".to_string() } else { "failed".to_string() },
            failure_reason_code: if success { None } else { Some("execution_error".to_string()) },
            failure_stage: if success { None } else { Some("execution".to_string()) },
            runtime_context_digest: RuntimeContextDigest {
                active_window_title: None,
                active_window_class: None,
                had_vision_context: false,
                had_uia_context: false,
                clipboard_preview: None,
            },
            key_entities,
            used_tools,
            used_retry: false,
            used_probe: false,
            steps_taken: self.current_step,
        };

        // 写回结果（忽略错误，不影响主流程）
        let _ = memory_service.write_back(request);
    }

    /// 执行 Agent 循环
    pub async fn run<F, Fut>(
        &mut self,
        app: &AppHandle,
        user_task: &str,
        ai_caller: F,
    ) -> AgentLoopResult
    where
        F: Fn(String, String) -> Fut,
        Fut: std::future::Future<Output = Result<String, String>>,
    {
        let permission_summary = self.get_permission_summary();
        let system_prompt = build_system_prompt_with_permissions(&permission_summary);

        // 1. 检索相关记忆
        let memory_context = self.retrieve_memory_context(app, user_task);

        loop {
            self.current_step += 1;

            // 系统保护上限
            if self.current_step > self.max_steps {
                // 写回失败记录
                self.write_back_result(app, user_task, false, "达到系统保护上限");
                return AgentLoopResult {
                    success: false,
                    message: format!("已达到系统保护上限({})，任务中止", self.max_steps),
                    steps_executed: self.current_step - 1,
                    history: self.history.clone(),
                    pending_confirmation: None,
                    request_exit: false,
                };
            }

            // 构建上下文（包含记忆）
            let context = build_context_with_memory(
                user_task,
                &self.history,
                self.current_step,
                memory_context.as_deref(),
            );

            // 调用 AI
            let ai_response = match ai_caller(system_prompt.clone(), context).await {
                Ok(response) => response,
                Err(e) => {
                    self.write_back_result(app, user_task, false, &format!("AI 调用失败：{}", e));
                    return AgentLoopResult {
                        success: false,
                        message: format!("AI 调用失败：{}", e),
                        steps_executed: self.current_step - 1,
                        history: self.history.clone(),
                        pending_confirmation: None,
                        request_exit: false,
                    };
                }
            };

            // 解析 AI 响应
            let parsed = match parse_ai_response(&ai_response) {
                Ok(p) => p,
                Err(_) => {
                    // 如果解析失败，把原始响应当作完成消息
                    self.write_back_result(app, user_task, true, &ai_response);
                    return AgentLoopResult {
                        success: true,
                        message: ai_response,
                        steps_executed: self.current_step - 1,
                        history: self.history.clone(),
                        pending_confirmation: None,
                        request_exit: false,
                    };
                }
            };

            match parsed {
                AIResponse::Reply { reply } => {
                    // 直接回复，不执行命令
                    self.write_back_result(app, user_task, true, &reply);
                    return AgentLoopResult {
                        success: true,
                        message: reply,
                        steps_executed: self.current_step,
                        history: self.history.clone(),
                        pending_confirmation: None,
                        request_exit: false,
                    };
                }
                AIResponse::Done { done } => {
                    self.write_back_result(app, user_task, true, &done);
                    return AgentLoopResult {
                        success: true,
                        message: done,
                        steps_executed: self.current_step,
                        history: self.history.clone(),
                        pending_confirmation: None,
                        request_exit: false,
                    };
                }
                AIResponse::Fail { fail } => {
                    self.write_back_result(app, user_task, false, &fail);
                    return AgentLoopResult {
                        success: false,
                        message: fail,
                        steps_executed: self.current_step,
                        history: self.history.clone(),
                        pending_confirmation: None,
                        request_exit: false,
                    };
                }
                AIResponse::ExitApp { exit_app } => {
                    // 请求退出应用
                    self.write_back_result(app, user_task, true, &exit_app);
                    return AgentLoopResult {
                        success: true,
                        message: exit_app,
                        steps_executed: self.current_step,
                        history: self.history.clone(),
                        pending_confirmation: None,
                        request_exit: true,
                    };
                }
                AIResponse::Command { cmd } => {
                    // 1. 检查是否被禁止（硬编码安全规则）
                    if let Some(reason) = is_forbidden_command(&cmd) {
                        self.history.push(CommandExecution {
                            command: cmd.clone(),
                            output: format!("命令被系统禁止：{}", reason),
                            success: false,
                        });
                        continue;
                    }

                    // 2. 应用规则引擎
                    let rule_result = self.apply_rules(&cmd);
                    if rule_result.blocked {
                        let block_reason = rule_result.block_reason.unwrap_or_else(|| "规则阻止".to_string());
                        self.history.push(CommandExecution {
                            command: cmd.clone(),
                            output: format!("命令被规则阻止：{}", block_reason),
                            success: false,
                        });
                        continue;
                    }

                    // 3. 检查权限（AI 不能自主授予权限）
                    let perm_result = self.check_command_permission(&cmd);
                    if !perm_result.allowed {
                        if perm_result.requires_confirmation {
                            // 需要用户授权
                            return AgentLoopResult {
                                success: false,
                                message: format!("命令需要权限授权：{}", cmd),
                                steps_executed: self.current_step,
                                history: self.history.clone(),
                                pending_confirmation: Some(PendingShellConfirmation {
                                    id: format!("perm-{}", now_millis()),
                                    command: cmd,
                                    risk_description: format!("需要 {} 权限", perm_result.permission_id),
                                    created_at: now_millis(),
                                }),
                                request_exit: false,
                            };
                        } else {
                            // 权限被拒绝
                            self.history.push(CommandExecution {
                                command: cmd.clone(),
                                output: format!("权限不足：{}", perm_result.reason),
                                success: false,
                            });
                            continue;
                        }
                    }

                    // 4. 检查是否需要高风险确认
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
                            request_exit: false,
                        };
                    }

                    // 5. 执行命令
                    let output = execute_shell_command(&cmd);
                    let success = !output.starts_with("命令执行失败") && !output.starts_with("命令执行错误");
                    self.history.push(CommandExecution {
                        command: cmd.clone(),
                        output: output.clone(),
                        success,
                    });

                    // 6. 更新规则置信度
                    for rule_id in &rule_result.applied_rules {
                        self.rule_engine.update_rule_confidence(rule_id, success);
                    }
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

/// 授予 Shell Agent 基本执行权限（需要用户确认）
///
/// 这个函数应该在用户首次同意使用 Shell Agent 时调用
pub fn grant_basic_shell_permissions(checker: &mut PermissionChecker) -> Result<(), String> {
    // 授予基本执行权限（会话级别）
    checker.grant(
        "shell:execute",
        GrantSource::User,
        PermissionScope::Session,
        Some(24 * 60 * 60 * 1000),  // 24 小时
    )?;
    Ok(())
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
    let output = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new("cmd")
            .args(["/C", cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };

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
