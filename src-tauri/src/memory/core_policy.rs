//! Immutable Core Policy - 不可变的核心安全策略
//!
//! 这些策略是硬编码的，不能被 AI 或用户通过 memory 系统修改。
//! 任何 memory 建议都不能覆盖这些核心策略。

#![allow(dead_code)]

/// 核心策略版本
pub const CORE_POLICY_VERSION: &str = "1.0.0";

/// 检查是否为隐私/敏感数据外发动作
pub fn is_privacy_exfiltration_risk(tool: &str, args: &serde_json::Value) -> bool {
    // 任何可能外发数据的动作都需要特别审查
    match tool {
        // 这些工具本身不会外发，但需要检查参数
        "run_shell_command" => {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                let cmd_lower = cmd.to_lowercase();

                // 网络传输命令
                const NETWORK_COMMANDS: &[&str] = &[
                    "curl", "wget", "ftp", "scp", "rsync", "nc", "netcat", "ncat",
                    "ssh", "sftp", "telnet", "tftp",
                ];

                // PowerShell 网络 cmdlet
                const POWERSHELL_NETWORK: &[&str] = &[
                    "invoke-webrequest", "invoke-restmethod", "iwr", "irm",
                    "start-bitstransfer", "send-mailmessage",
                    "new-object system.net", "webclient", "httpwebrequest",
                ];

                // 脚本语言网络库调用模式
                const SCRIPT_NETWORK_PATTERNS: &[&str] = &[
                    // Python
                    "requests.", "urllib", "httpx", "aiohttp", "socket.",
                    // Node.js
                    "require('http')", "require(\"http\")", "require('https')",
                    "require(\"https\")", "fetch(", "axios", "got(",
                    // Ruby
                    "net/http", "open-uri",
                ];

                // 检查网络传输命令
                if NETWORK_COMMANDS.iter().any(|f| cmd_lower.contains(f)) {
                    return true;
                }

                // 检查 PowerShell 网络操作
                if cmd_lower.contains("powershell") || cmd_lower.contains("pwsh") {
                    if POWERSHELL_NETWORK.iter().any(|p| cmd_lower.contains(p)) {
                        return true;
                    }
                }

                // 检查脚本语言网络调用
                if cmd_lower.contains("python") || cmd_lower.contains("node")
                    || cmd_lower.contains("ruby") || cmd_lower.contains("perl")
                {
                    if SCRIPT_NETWORK_PATTERNS.iter().any(|p| cmd_lower.contains(p)) {
                        return true;
                    }
                }
            }

            // 检查命令参数中的 URL
            if let Some(cmd_args) = args.get("args").and_then(|v| v.as_array()) {
                for arg in cmd_args {
                    if let Some(s) = arg.as_str() {
                        let s_lower = s.to_lowercase();
                        // 检查是否包含外部 URL
                        if s_lower.starts_with("http://")
                            || s_lower.starts_with("https://")
                            || s_lower.starts_with("ftp://")
                            || s_lower.starts_with("sftp://")
                            || s_lower.starts_with("ssh://")
                        {
                            return true;
                        }
                    }
                }
            }
            false
        }
        _ => false,
    }
}

/// 检查是否为需要确认的高风险动作
pub fn requires_confirmation(tool: &str) -> bool {
    matches!(
        tool,
        "delete_path"
            | "launch_installer_file"
            | "write_registry_value"
            | "delete_registry_value"
            | "click_at"
            | "click_element"
            | "set_element_value"
    )
}

/// 检查工具是否在白名单内
pub fn is_tool_allowed(tool: &str) -> bool {
    const ALLOWED_TOOLS: &[&str] = &[
        "list_windows",
        "focus_window",
        "open_app",
        "capture_active_window",
        "read_clipboard",
        "list_directory",
        "read_file_text",
        "write_file_text",
        "create_directory",
        "move_path",
        "delete_path",
        "run_shell_command",
        "launch_installer_file",
        "query_registry_key",
        "read_registry_value",
        "write_registry_value",
        "delete_registry_value",
        "type_text",
        "send_hotkey",
        "scroll_at",
        "click_at",
        "find_element",
        "click_element",
        "get_element_text",
        "set_element_value",
        "wait_for_element",
    ];
    ALLOWED_TOOLS.contains(&tool)
}

/// 检查注册表路径是否允许写入
pub fn is_registry_path_writable(path: &str) -> bool {
    let upper = path.to_uppercase();
    upper.starts_with("HKCU\\SOFTWARE\\")
        || upper.starts_with("HKEY_CURRENT_USER\\SOFTWARE\\")
        || upper.starts_with("HKCU\\ENVIRONMENT")
        || upper.starts_with("HKEY_CURRENT_USER\\ENVIRONMENT")
}

/// 检查 shell 命令是否在白名单内
pub fn is_shell_command_allowed(command: &str, args: &[String]) -> bool {
    let cmd_lower = command.to_lowercase();

    // 只读命令 - 始终允许
    let readonly_commands = ["pwd", "dir", "type", "where", "echo"];
    if readonly_commands.contains(&cmd_lower.as_str()) {
        return true;
    }

    // 受限命令 - 检查子命令
    match cmd_lower.as_str() {
        "git" => {
            if args.is_empty() {
                return false;
            }
            let subcmd = args[0].to_lowercase();
            matches!(
                subcmd.as_str(),
                "status" | "branch" | "rev-parse" | "log" | "diff" | "show"
            )
        }
        "npm" | "pnpm" | "yarn" | "bun" => {
            if args.is_empty() {
                return false;
            }
            let subcmd = args[0].to_lowercase();
            matches!(subcmd.as_str(), "run" | "test" | "build" | "lint")
        }
        "cargo" => {
            if args.is_empty() {
                return false;
            }
            let subcmd = args[0].to_lowercase();
            matches!(subcmd.as_str(), "build" | "test" | "check" | "clippy")
        }
        _ => false,
    }
}

/// 核心策略检查结果
#[derive(Debug, Clone)]
pub struct CorePolicyCheck {
    pub allowed: bool,
    pub reason: Option<String>,
    pub requires_confirmation: bool,
}

/// 执行核心策略检查
pub fn check_action(tool: &str, args: &serde_json::Value) -> CorePolicyCheck {
    // 1. 工具白名单检查
    if !is_tool_allowed(tool) {
        return CorePolicyCheck {
            allowed: false,
            reason: Some(format!("工具 {} 不在白名单内", tool)),
            requires_confirmation: false,
        };
    }

    // 2. 隐私外发检查
    if is_privacy_exfiltration_risk(tool, args) {
        return CorePolicyCheck {
            allowed: false,
            reason: Some("检测到潜在的隐私数据外发风险".to_string()),
            requires_confirmation: false,
        };
    }

    // 3. 注册表路径检查
    if matches!(tool, "write_registry_value" | "delete_registry_value") {
        if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
            if !is_registry_path_writable(path) {
                return CorePolicyCheck {
                    allowed: false,
                    reason: Some(format!("注册表路径 {} 不允许写入", path)),
                    requires_confirmation: false,
                };
            }
        }
    }

    // 4. Shell 命令检查
    if tool == "run_shell_command" {
        let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let cmd_args: Vec<String> = args
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        if !is_shell_command_allowed(command, &cmd_args) {
            return CorePolicyCheck {
                allowed: false,
                reason: Some(format!("Shell 命令 {} 不在白名单内", command)),
                requires_confirmation: false,
            };
        }
    }

    // 5. 检查是否需要确认
    let needs_confirmation = requires_confirmation(tool);

    CorePolicyCheck {
        allowed: true,
        reason: None,
        requires_confirmation: needs_confirmation,
    }
}

/// 获取核心策略摘要 (用于 prompt)
pub fn get_policy_summary() -> String {
    format!(
        "## 核心安全策略 (不可覆盖)\n\
        - 工具白名单: 26 个允许的工具\n\
        - 高风险操作需用户确认: delete_path, launch_installer, registry write/delete, click 等\n\
        - Shell 命令受限: 仅允许 pwd/dir/type/where/git status/npm test/cargo build 等\n\
        - 注册表写入受限: 仅允许 HKCU\\Software 和 HKCU\\Environment\n\
        - 禁止隐私外发: 不允许 curl/wget/ftp 等外发命令\n\
        - 版本: {}\n",
        CORE_POLICY_VERSION
    )
}
