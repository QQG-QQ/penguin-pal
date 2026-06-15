mod agent;
mod ai;
mod app_update;
mod app_state;
mod codex_config;
mod codex_runtime;
mod codex_update;
mod control;
mod audio;
mod desktop;
mod history;
mod memory;
mod permission;
mod research;
mod rule_engine;
mod security;
mod session_thread;
mod shell_agent;
mod testing;
mod tray;
mod window;

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{process::Command, sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};

/// Memory 维护线程停止标志
static MEMORY_MAINTENANCE_SHUTDOWN: AtomicBool = AtomicBool::new(false);
const SESSION_TURN_CONTEXT_LIMIT: usize = 12;
const PROVIDER_HISTORY_LIMIT: usize = 24;
const WHISPER_PUSH_TO_TALK_EVENT: &str = "penguinpal://whisper-push-to-talk";
const SHELL_CONFIRMATION_TIMEOUT_MS: u64 = 60_000;
const MCP_CONTROL_PENDING_TOOL: &str = "mcp_tool_call";
const MCP_CONTROL_PENDING_TTL_MS: u64 = 30_000;

/// 请求停止 memory 维护线程
#[allow(dead_code)]
pub fn request_memory_maintenance_shutdown() {
    MEMORY_MAINTENANCE_SHUTDOWN.store(true, Ordering::Relaxed);
}

use crate::{
    agent::{
        agent_turn::{self, AgentExecutionDomain, AgentTurnMode},
        router as agent_router,
        types::{AgentMessageMeta, AgentRoute},
        workspace_context,
        AgentTaskState,
    },
    ai::{guardrails, memory as ai_memory, provider},
    app_state::{
        default_system_prompt, load, now_millis, save, ActionExecutionResult,
        AssistantSnapshot, AuthMode, ChatMessage, ChatResponse, DesktopAction, OAuthFlowResult,
        PendingShellCommand, PendingShellConfirmationInfo, PetMode, ProviderConfigInput,
        ProviderConfig, ProviderKind, RuntimeState, SavedWindowPosition, ShellPermissionSettings, VoiceInputMode,
        DEFAULT_OAUTH_REDIRECT_URL,
    },
    audio::{types as audio_types, TranscriberService},
    codex_runtime::{apply_private_env, initialize_codex_config, load_codex_config, private_auth_path, resolve_for_app},
    control::types::{ControlPendingRequest, ControlRiskLevel, ToolInvokeResponse},
    control::{router as control_router, types::ControlServiceStatus, ControlServiceState},
    history::ReplyHistoryEntry,
    security::{audit, oauth, policy},
    shell_agent::{ShellAgentExecutor, BehaviorState},
    permission::{PermissionScope, GrantSource},
};

fn snapshot_from_runtime(runtime: &RuntimeState) -> AssistantSnapshot {
    let allowed_actions = policy::actions_for_level(runtime.permission_level);
    let ai_constraints = guardrails::build_profile(
        &runtime.provider,
        runtime.permission_level,
        &allowed_actions,
    );
    runtime.to_snapshot(
        audio::default_audio_profile(
            runtime.provider.voice_input_mode,
            &runtime.provider.push_to_talk_shortcut,
        ),
        allowed_actions,
        ai_constraints,
    )
}

fn join_prompt_sections(sections: &[Option<&str>]) -> Option<String> {
    let parts = sections
        .iter()
        .filter_map(|section| section.map(str::trim))
        .filter(|section| !section.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn current_codex_cli_model(app: &AppHandle) -> String {
    load_codex_config(app)
        .ok()
        .map(|config| config.model.trim().to_string())
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| ProviderKind::CodexCli.default_model().to_string())
}

fn sync_codex_provider_view(app: &AppHandle, runtime: &mut RuntimeState) {
    if !matches!(runtime.provider.kind, ProviderKind::CodexCli) {
        return;
    }

    runtime.provider.model = current_codex_cli_model(app);
    runtime.provider.auth_mode = AuthMode::OAuth;
    runtime.provider.base_url = None;
}

fn normalized_push_to_talk_shortcut(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        "CommandOrControl+Alt+Space".to_string()
    } else {
        trimmed.to_string()
    }
}

fn whisper_push_to_talk_shortcut(runtime: &RuntimeState) -> Option<String> {
    if matches!(runtime.provider.voice_input_mode, VoiceInputMode::PushToTalk) {
        Some(normalized_push_to_talk_shortcut(
            &runtime.provider.push_to_talk_shortcut,
        ))
    } else {
        None
    }
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
fn sync_launch_at_startup(app: &AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;

    let autostart_manager = app.autolaunch();
    let current = autostart_manager
        .is_enabled()
        .map_err(|error| format!("读取开机自启状态失败: {error}"))?;

    if cfg!(debug_assertions)
        && std::env::current_exe()
            .ok()
            .map(|path| {
                let normalized = path.to_string_lossy().replace('\\', "/").to_lowercase();
                normalized.contains("/target/debug/")
            })
            .unwrap_or(false)
    {
        #[cfg(windows)]
        if let Ok(current_exe) = std::env::current_exe() {
            cleanup_dev_autostart_run_entries(&current_exe);
        }
        if current {
            autostart_manager
                .disable()
                .map_err(|error| format!("关闭开发版开机自启失败: {error}"))?;
        }
        return Ok(false);
    }

    if current != enabled {
        if enabled {
            autostart_manager
                .enable()
                .map_err(|error| format!("启用开机自启失败: {error}"))?;
        } else {
            autostart_manager
                .disable()
                .map_err(|error| format!("关闭开机自启失败: {error}"))?;
        }
    }

    autostart_manager
        .is_enabled()
        .map_err(|error| format!("确认开机自启状态失败: {error}"))
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn sync_launch_at_startup(_app: &AppHandle, enabled: bool) -> Result<bool, String> {
    Ok(enabled)
}

#[cfg(windows)]
fn cleanup_dev_autostart_run_entries(current_exe: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let run_key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    let current_path = current_exe
        .to_string_lossy()
        .replace('/', "\\")
        .trim()
        .to_lowercase();

    for name in ["PenguinPal", "PenguinPal Assistant"] {
        let output = Command::new("reg")
            .args(["query", run_key, "/v", name])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        let Ok(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
        let matches_current = stdout
            .lines()
            .any(|line| line.contains(&current_path));
        if !matches_current {
            continue;
        }

        let _ = Command::new("reg")
            .args(["delete", run_key, "/v", name, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
}

#[cfg(desktop)]
fn register_whisper_input_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

    let global_shortcut = app.global_shortcut();
    let emitted_shortcut = shortcut.to_string();
    global_shortcut
        .on_shortcut(shortcut, move |app, _, event| {
            let state = match event.state {
                ShortcutState::Pressed => "pressed",
                ShortcutState::Released => "released",
            };

            let payload = serde_json::json!({
                "state": state,
                "shortcut": emitted_shortcut,
            });
            let _ = app.emit(WHISPER_PUSH_TO_TALK_EVENT, payload);
        })
        .map_err(|error| format!("注册按键说话快捷键失败: {error}"))?;

    Ok(())
}

fn sync_whisper_input_shortcut(
    app: &AppHandle,
    previous_runtime: Option<&RuntimeState>,
    next_runtime: &RuntimeState,
) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;

        let global_shortcut = app.global_shortcut();
        let previous_shortcut = previous_runtime.and_then(whisper_push_to_talk_shortcut);
        let next_shortcut = whisper_push_to_talk_shortcut(next_runtime);

        if previous_shortcut == next_shortcut {
            if let Some(shortcut) = next_shortcut.as_deref() {
                if !global_shortcut.is_registered(shortcut) {
                    register_whisper_input_shortcut(app, shortcut)?;
                }
            }
            return Ok(());
        }

        if let Some(shortcut) = next_shortcut.as_deref() {
            register_whisper_input_shortcut(app, shortcut)?;
        }

        if let Some(shortcut) = previous_shortcut.as_deref() {
            if global_shortcut.is_registered(shortcut) {
                if let Err(error) = global_shortcut.unregister(shortcut) {
                    if let Some(next_shortcut) = next_shortcut.as_deref() {
                        if next_shortcut != shortcut && global_shortcut.is_registered(next_shortcut) {
                            let _ = global_shortcut.unregister(next_shortcut);
                        }
                    }
                    return Err(format!("卸载旧按键说话快捷键失败: {error}"));
                }
            }
        } else if previous_runtime.is_none() && next_shortcut.is_none() {
            global_shortcut
                .unregister_all()
                .map_err(|error| format!("清理按键说话快捷键失败: {error}"))?;
        }
    }

    Ok(())
}

/// 将设置中的 Shell 权限同步到 PermissionChecker
fn sync_shell_permissions_to_checker(
    app: &AppHandle,
    settings: &ShellPermissionSettings,
) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;

    let behavior_state = BehaviorState::new(&app_data_dir);

    // 计算权限有效期
    let duration_ms = if settings.duration_hours == 0 {
        None // 永久
    } else {
        Some(settings.duration_hours * 60 * 60 * 1000) // 转换为毫秒
    };

    // 如果 Shell Agent 未启用，撤销所有权限
    if !settings.enabled {
        behavior_state.revoke_all_shell_permissions()?;
        return Ok(());
    }

    // 根据设置授予或撤销权限
    let permission_map = [
        ("shell:execute", settings.allow_execute),
        ("shell:modify", settings.allow_file_modify),
        ("shell:delete", settings.allow_file_delete),
        ("shell:network", settings.allow_network),
        ("shell:system", settings.allow_system),
    ];

    for (permission_id, enabled) in permission_map {
        if enabled {
            behavior_state.grant_permission(permission_id, PermissionScope::Global, duration_ms)?;
        } else {
            behavior_state.revoke_permission(permission_id)?;
        }
    }

    Ok(())
}

fn hide_instead_of_close<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    let event_window = window.clone();
    let hide_window = window.clone();
    event_window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = hide_window.hide();
        }
    });
}

fn bundled_fastmcp_script(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .resolve(
            "resources/mcp/fastmcp_local_tools.py",
            tauri::path::BaseDirectory::Resource,
        )
        .ok()
        .filter(|path| path.is_file())
        .or_else(|| {
            let dev_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("mcp")
                .join("fastmcp_local_tools.py");
            dev_path.is_file().then_some(dev_path)
        })
}

fn prepare_fastmcp_local_script(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let source = bundled_fastmcp_script(app)
        .ok_or_else(|| "未找到内置 FastMCP 本地服务脚本。".to_string())?;
    let target_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?
        .join("mcp");
    std::fs::create_dir_all(&target_dir)
        .map_err(|error| format!("创建 MCP 数据目录失败: {error}"))?;
    let target = target_dir.join("fastmcp_local_tools.py");
    std::fs::copy(&source, &target)
        .map_err(|error| format!("准备 FastMCP 本地服务失败: {error}"))?;
    Ok(target)
}

fn configure_builtin_mcp_defaults(app: &AppHandle, runtime: &mut RuntimeState) -> Result<(), String> {
    let fastmcp_script = prepare_fastmcp_local_script(app)?;
    let fastmcp_script = fastmcp_script.to_string_lossy().to_string();
    runtime.agent_roadmap.plugin_tools.enabled = true;
    runtime.agent_roadmap.plugin_tools.mcp_gateway_enabled = true;

    if runtime.agent_roadmap.plugin_tools.mcp_servers.is_empty() {
        runtime.agent_roadmap.plugin_tools.mcp_servers = app_state::default_mcp_servers();
    }

    for server in &mut runtime.agent_roadmap.plugin_tools.mcp_servers {
        if server.id == "github-mcp"
            && (server.command == "github-mcp-server"
                || server.command.trim().is_empty()
                || server.args.iter().any(|arg| arg == "--read-only"))
        {
            server.enabled = true;
            server.transport = "stdio".to_string();
            server.command = "docker".to_string();
            server.args = vec![
                "run".to_string(),
                "--rm".to_string(),
                "-i".to_string(),
                "-e".to_string(),
                "GITHUB_PERSONAL_ACCESS_TOKEN".to_string(),
                "ghcr.io/github/github-mcp-server".to_string(),
                "stdio".to_string(),
                "--read-only".to_string(),
            ];
            if server.env.is_empty() {
                server.env = vec!["GITHUB_PERSONAL_ACCESS_TOKEN=".to_string()];
            }
            server.read_only = true;
            server.notes = "官方 GitHub MCP Server。需要 Docker 和 GitHub PAT。".to_string();
        }

        if server.id == "fastmcp-local"
            && (server.args.iter().any(|arg| arg == "path/to/server.py")
                || server.args.iter().any(|arg| arg.ends_with("fastmcp_local_tools.py"))
                || server.args.is_empty())
        {
            server.enabled = true;
            server.transport = "stdio".to_string();
            server.command = "python".to_string();
            server.args = vec![fastmcp_script.clone()];
            server.tool_allowlist = vec!["penguin_status".to_string(), "echo_note".to_string()];
            server.read_only = true;
            server.notes = "桌宠内置的本地 MCP 示例服务，不依赖额外安装。".to_string();
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexCliStatus {
    installed: bool,
    version: Option<String>,
    logged_in: bool,
    credential_present: bool,
    auth_path: Option<String>,
    runtime_path: Option<String>,
    source: String,
    status_kind: String,
    status_label: String,
    relogin_recommended: bool,
    message: String,
}

struct BasicCodexCliStatus {
    installed: bool,
    version: Option<String>,
    credential_present: bool,
    auth_path: Option<String>,
    runtime_path: Option<String>,
    source: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthCheckItem {
    id: String,
    title: String,
    status: String,
    severity: String,
    detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthCheckReport {
    generated_at: u64,
    overall_status: String,
    items: Vec<HealthCheckItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RoadmapCapabilityStatus {
    enabled: bool,
    status: String,
    summary: String,
    detail: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentRoadmapStatus {
    generated_at: u64,
    plugin_tools: RoadmapCapabilityStatus,
    document_memory: RoadmapCapabilityStatus,
    browser_automation: RoadmapCapabilityStatus,
    local_voice_loop: RoadmapCapabilityStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentImportResult {
    id: String,
    title: String,
    path: String,
    memory_written: bool,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DocumentMemoryRecord {
    id: String,
    title: String,
    path: String,
    imported_at: u64,
    size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredBrowserAutomationTask {
    id: String,
    goal: String,
    url: Option<String>,
    status: String,
    requires_confirmation: bool,
    message: String,
    created_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserAutomationTask {
    id: String,
    goal: String,
    url: Option<String>,
    status: String,
    requires_confirmation: bool,
    message: String,
    created_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginToolManifest {
    id: String,
    name: String,
    path: String,
    kind: String,
    enabled: bool,
    summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PluginToolScanResult {
    scanned_at: u64,
    directories: Vec<String>,
    tools: Vec<PluginToolManifest>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct McpGatewayDiagnosis {
    ok: bool,
    message: String,
    tools: Vec<PluginToolManifest>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DockerSetupResult {
    status: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpNaturalToolDecision {
    should_call: bool,
    #[serde(default)]
    server_id: Option<String>,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default)]
    arguments: Option<serde_json::Value>,
    #[serde(default)]
    reason: Option<String>,
}

fn first_non_empty_output(output: &std::process::Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Some(stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return Some(stderr);
    }

    None
}

fn read_codex_version(command: &str, app: &AppHandle) -> Option<String> {
    let runtime = resolve_for_app(app).ok()?;
    let output = {
        let mut cmd = Command::new(command);
        apply_private_env(&mut cmd, &runtime.home_root);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }
        .arg("--version")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    first_non_empty_output(&output)
}

fn inspect_codex_cli_status_basic(app: &AppHandle) -> BasicCodexCliStatus {
    let runtime = match resolve_for_app(app) {
        Ok(runtime) => runtime,
        Err(error) => {
            return BasicCodexCliStatus {
                installed: false,
                version: None,
                credential_present: false,
                auth_path: None,
                runtime_path: None,
                source: "未找到".to_string(),
                message: error,
            }
        }
    };

    let command_label = runtime
        .command
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let version = runtime
        .command
        .as_ref()
        .and_then(|path| {
            codex_update::get_runtime_command_package_version(path)
                .or_else(|| read_codex_version(&path.to_string_lossy(), app))
        });
    let installed = version.is_some();

    let auth_path = private_auth_path(app).ok();
    let auth_file_exists = auth_path.as_ref().is_some_and(|path| {
        path.is_file() && path.metadata().map(|meta| meta.len() > 0).unwrap_or(false)
    });
    let credential_present = installed && auth_file_exists;
    let auth_path_label = auth_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());

    let message = if !installed {
        if runtime.source == "未找到" {
            "未检测到桌宠内置 Codex 运行时。请把 Codex 私有运行时打包进应用资源后再试；开发期仍可临时回退到系统安装。".to_string()
        } else {
            format!("已发现 {}，但当前无法执行 Codex CLI。", runtime.source)
        }
    } else if credential_present {
        format!("已检测到 Codex CLI 私有凭据，当前来源：{}。", runtime.source)
    } else {
        format!("Codex CLI 未登录，请点击按钮启动登录。当前来源：{}。", runtime.source)
    };

    BasicCodexCliStatus {
        installed,
        version,
        credential_present,
        auth_path: auth_path_label,
        runtime_path: command_label,
        source: runtime.source.to_string(),
        message,
    }
}

fn inspect_codex_cli_status_unverified(app: &AppHandle) -> CodexCliStatus {
    let basic = inspect_codex_cli_status_basic(app);

    if !basic.installed {
        return CodexCliStatus {
            installed: false,
            version: basic.version,
            logged_in: false,
            credential_present: false,
            auth_path: basic.auth_path,
            runtime_path: basic.runtime_path,
            source: basic.source,
            status_kind: "notInstalled".to_string(),
            status_label: "未安装".to_string(),
            relogin_recommended: false,
            message: basic.message,
        };
    }

    if !basic.credential_present {
        return CodexCliStatus {
            installed: true,
            version: basic.version,
            logged_in: false,
            credential_present: false,
            auth_path: basic.auth_path,
            runtime_path: basic.runtime_path,
            source: basic.source,
            status_kind: "notLoggedIn".to_string(),
            status_label: "未登录".to_string(),
            relogin_recommended: false,
            message: basic.message,
        };
    }

    CodexCliStatus {
        installed: true,
        version: basic.version,
        logged_in: false,
        credential_present: true,
        auth_path: basic.auth_path,
        runtime_path: basic.runtime_path,
        source: basic.source,
        status_kind: "credentialOnly".to_string(),
        status_label: "凭据已检测".to_string(),
        relogin_recommended: false,
        message: "已检测到 Codex CLI 私有凭据，但还没有验证当前 workspace 是否可用。点击“刷新状态”可做一次真实可用性检查。".to_string(),
    }
}

fn normalize_codex_status_probe_error(error: &str) -> (&'static str, &'static str, bool, String) {
    let lowered = error.to_lowercase();

    if lowered.contains("deactivated_workspace") {
        return (
            "workspaceDeactivated",
            "需重新登录",
            true,
            "检测到 Codex 登录凭据，但当前 workspace 已失效或已停用。请点击“重新登录”，并确认选择一个可用的账号/工作区。".to_string(),
        );
    }

    if lowered.contains("payment required") {
        return (
            "paymentRequired",
            "权限受限",
            true,
            "Codex 后端返回了付费/权限限制，当前 workspace 不能正常调用模型。建议重新登录到可用 workspace，或切换其他 provider。".to_string(),
        );
    }

    if lowered.contains("401")
        || lowered.contains("unauthorized")
        || lowered.contains("auth error")
    {
        return (
            "authError",
            "需重新登录",
            true,
            "Codex 凭据校验失败，当前登录状态已失效。请点击“重新登录”后再刷新状态。".to_string(),
        );
    }

    if lowered.contains("timeout") || lowered.contains("timed out") {
        return (
            "timeout",
            "连接超时",
            false,
            "Codex 状态检查超时，当前无法确认 workspace 是否可用。你可以稍后再刷新一次。".to_string(),
        );
    }

    (
        "error",
        "状态异常",
        false,
        format!("Codex CLI 可用性检查失败：{error}"),
    )
}

async fn inspect_codex_cli_status(app: &AppHandle) -> CodexCliStatus {
    let basic = inspect_codex_cli_status_basic(app);

    if !basic.installed || !basic.credential_present {
        return inspect_codex_cli_status_unverified(app);
    }

    let runtime = match resolve_for_app(app) {
        Ok(runtime) => runtime,
        Err(error) => {
            let mut status = inspect_codex_cli_status_unverified(app);
            status.status_kind = "error".to_string();
            status.status_label = "状态异常".to_string();
            status.message = error;
            return status;
        }
    };

    let Some(command) = runtime.command.as_ref() else {
        let mut status = inspect_codex_cli_status_unverified(app);
        status.status_kind = "error".to_string();
        status.status_label = "状态异常".to_string();
        status.message = "未找到可执行的 Codex 命令。".to_string();
        return status;
    };

    match provider::probe_codex_cli_runtime(
        command.to_string_lossy().to_string(),
        runtime.home_root.to_string_lossy().to_string(),
    )
    .await
    {
        Ok(()) => {
            let source = basic.source.clone();
            CodexCliStatus {
                installed: true,
                version: basic.version,
                logged_in: true,
                credential_present: true,
                auth_path: basic.auth_path,
                runtime_path: basic.runtime_path,
                source,
                status_kind: "ready".to_string(),
                status_label: "已可用".to_string(),
                relogin_recommended: false,
                message: format!("Codex CLI 当前可用，当前来源：{}。", basic.source),
            }
        }
        Err(error) => {
            let (kind, label, relogin_recommended, message) =
                normalize_codex_status_probe_error(&error);
            CodexCliStatus {
                installed: true,
                version: basic.version,
                logged_in: false,
                credential_present: true,
                auth_path: basic.auth_path,
                runtime_path: basic.runtime_path,
                source: basic.source,
                status_kind: kind.to_string(),
                status_label: label.to_string(),
                relogin_recommended,
                message,
            }
        }
    }
}

#[tauri::command]
async fn get_codex_cli_status(app: AppHandle) -> CodexCliStatus {
    inspect_codex_cli_status(&app).await
}

#[tauri::command]
async fn check_codex_update(app: AppHandle) -> Result<codex_update::CodexUpdateStatus, String> {
    let status = inspect_codex_cli_status_basic(&app);
    Ok(codex_update::check_update_status(&app, status.version).await)
}#[tauri::command]
async fn check_app_update() -> Result<app_update::AppUpdateStatus, String> {
    Ok(app_update::check_update_status().await)
}

#[tauri::command]
async fn open_app_update_download() -> Result<app_update::AppUpdateStatus, String> {
    app_update::open_update_download().await
}

#[tauri::command]
async fn update_codex(app: AppHandle) -> Result<codex_update::CodexUpdateStatus, String> {
    let mut notes = Vec::new();
    let mut codex_update_error = None;

    match codex_update::ensure_bootstrap_runtime(&app, true, |msg| {
        eprintln!("[Codex Bootstrap] {}", msg);
    })
    .await
    {
        Ok(Some(message)) => notes.push(message),
        Ok(None) => {}
        Err(error) => {
            eprintln!("[Codex Bootstrap] manual update failed: {}", error);
            notes.push(format!("Bootstrap 更新失败: {}", error));
        }
    }

    let target_version = codex_update::fetch_latest_version().await.ok();
    let codex_install_dir = codex_update::managed_install_dir(&app)?;

    if codex_update::npm_available() {
        match codex_update::install_or_update_codex(
            &codex_install_dir,
            target_version.as_deref(),
            |msg| {
                eprintln!("[Codex Update] {}", msg);
            },
        ) {
            Ok(message) => notes.push(message),
            Err(error) => {
                eprintln!("[Codex Update] manual npm update failed: {}", error);
                codex_update_error = Some(error);
            }
        }
    } else {
        notes.push("系统 npm 不可用，已跳过 npm 精细更新。".to_string());
    }

    let status = inspect_codex_cli_status_basic(&app);
    if !status.installed {
        if let Some(error) = codex_update_error {
            return Err(error);
        }
        return Err("当前没有可用的 Codex 运行时，bootstrap 更新也未成功。".to_string());
    }

    let mut update_status = codex_update::check_update_status(&app, status.version).await;
    if !notes.is_empty() {
        update_status.message = format!("{}；{}", notes.join("；"), update_status.message);
    }
    Ok(update_status)
}

/// 启动时自动检查并更新 Codex
async fn auto_update_codex(app: &AppHandle) -> Result<(), String> {
    let current_status = inspect_codex_cli_status_basic(app);
    let update_status = codex_update::check_update_status(app, current_status.version.clone()).await;

    if !update_status.update_available {
        eprintln!(
            "[Codex] private runtime is already up to date: {:?}",
            update_status.current_version
        );
        return Ok(());
    }

    eprintln!(
        "[Codex] found update {:?} -> {:?}, starting auto update...",
        current_status.version,
        update_status.latest_version
    );

    match codex_update::ensure_bootstrap_runtime(app, false, |msg| {
        eprintln!("[Codex Bootstrap] {}", msg);
    })
    .await
    {
        Ok(Some(message)) => eprintln!("[Codex] {}", message),
        Ok(None) => {}
        Err(error) => {
            eprintln!("[Codex Bootstrap] auto update failed: {}", error);
        }
    }

    let status_after_bootstrap = inspect_codex_cli_status_basic(app);
    if !codex_update::npm_available() {
        if status_after_bootstrap.installed {
            eprintln!("[Codex] npm unavailable, keeping bootstrap runtime.");
            return Ok(());
        }
        return Err("系统 npm 不可用，且当前没有可用的 Codex 运行时。".to_string());
    }

    let codex_install_dir = codex_update::managed_install_dir(app)?;
    match codex_update::install_or_update_codex(
        &codex_install_dir,
        update_status.latest_version.as_deref(),
        |msg| {
            eprintln!("[Codex Update] {}", msg);
        },
    ) {
        Ok(result) => {
            eprintln!("[Codex] 自动更新完成: {}", result);
            Ok(())
        }
        Err(error) => {
            eprintln!("[Codex] 自动更新失败: {}", error);
            let fallback_status = inspect_codex_cli_status_basic(app);
            if fallback_status.installed {
                eprintln!("[Codex] npm update failed, but an existing runtime is still usable.");
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}#[tauri::command]
fn get_control_service_status(app: AppHandle) -> Result<ControlServiceStatus, String> {
    control_router::service_status(&app).map_err(|error| error.to_string())
}

#[tauri::command]
async fn confirm_control_pending(app: AppHandle, pending_id: String) -> Result<control::types::ToolInvokeResponse, String> {
    if let Some(pending) = try_take_mcp_pending(&app, &pending_id)? {
        return confirm_mcp_pending(&app, pending).await;
    }
    agent_router::confirm_control_pending(&app, &pending_id).await
}

#[tauri::command]
async fn cancel_control_pending(app: AppHandle, pending_id: String) -> Result<control::types::ToolInvokeResponse, String> {
    agent_router::cancel_control_pending(&app, &pending_id).await
}

#[tauri::command]
fn start_codex_cli_login(app: AppHandle) -> Result<CodexCliStatus, String> {
    let status = inspect_codex_cli_status_unverified(&app);
    if !status.installed {
        return Err(status.message);
    }

    let runtime = resolve_for_app(&app)?;
    let codex_command = runtime
        .command
        .ok_or_else(|| "未找到桌宠可用的 Codex 运行时。".to_string())?;
    let wrapped = if codex_command.to_string_lossy().contains(' ') {
        format!("\"{}\"", codex_command.to_string_lossy())
    } else {
        codex_command.to_string_lossy().to_string()
    };
    let login_cmd = format!("{wrapped} --login || {wrapped} login || {wrapped}");

    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        apply_private_env(&mut cmd, &runtime.home_root);
        cmd.args(["/C", "start", "", "cmd", "/K", &login_cmd])
            .spawn()
            .map_err(|error| format!("启动 codex login 失败：{error}"))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = Command::new("sh");
        apply_private_env(&mut cmd, &runtime.home_root);
        cmd.args(["-lc", &login_cmd])
            .spawn()
            .map_err(|error| format!("启动 codex login 失败：{error}"))?;
    }

    let mut next = inspect_codex_cli_status_unverified(&app);
    next.message = "已启动 codex login，请在新终端完成登录后点击“刷新状态”。".to_string();
    next.status_kind = "pendingLogin".to_string();
    next.status_label = "登录中".to_string();
    Ok(next)
}

#[tauri::command]
fn restart_codex_cli_login(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<CodexCliStatus, String> {
    if let Ok(mut runtime) = state.lock() {
        runtime.codex_thread_id = None;
    }

    if let Ok(path) = private_auth_path(&app) {
        match std::fs::remove_file(&path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("清理旧 Codex 凭据失败：{error}")),
        }
    }

    start_codex_cli_login(app)
}

#[tauri::command]
fn show_settings_window(app: AppHandle) -> Result<bool, String> {
    let window = app
        .get_webview_window("settings")
        .ok_or_else(|| "未找到设置窗口".to_string())?;

    let _ = window.unminimize();
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn hide_settings_window(app: AppHandle) -> Result<bool, String> {
    let Some(window) = app.get_webview_window("settings") else {
        return Ok(false);
    };

    window.hide().map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn show_research_window(app: AppHandle) -> Result<bool, String> {
    let window = app
        .get_webview_window("research")
        .ok_or_else(|| "未找到研究窗口".to_string())?;

    let _ = window.unminimize();
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn hide_research_window(app: AppHandle) -> Result<bool, String> {
    let Some(window) = app.get_webview_window("research") else {
        return Ok(false);
    };

    window.hide().map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn hide_main_window(app: AppHandle) -> Result<bool, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;
    window.hide().map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn start_main_window_drag(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口".to_string())?;
    window.start_dragging().map_err(|error| error.to_string())
}

#[tauri::command]
fn remember_main_window_position(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    x: i32,
    y: i32,
) -> Result<bool, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    let next_position = SavedWindowPosition { x, y };

    if runtime.main_window_position.as_ref() == Some(&next_position) {
        return Ok(false);
    }

    runtime.main_window_position = Some(next_position);
    save(&app, &runtime)?;
    Ok(true)
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() || normalized.iter().any(|item: &String| item == trimmed) {
            continue;
        }
        normalized.push(trimmed.to_string());
    }
    normalized
}

fn normalize_agent_roadmap_config(
    mut config: app_state::AgentRoadmapConfig,
) -> app_state::AgentRoadmapConfig {
    config.plugin_tools.tool_dirs = normalize_string_list(config.plugin_tools.tool_dirs);
    if config.plugin_tools.mcp_servers.is_empty() {
        config.plugin_tools.mcp_servers = app_state::default_mcp_servers();
    } else {
        config.plugin_tools.mcp_servers = config
            .plugin_tools
            .mcp_servers
            .into_iter()
            .filter_map(|mut server| {
                server.id = server.id.trim().to_string();
                server.name = server.name.trim().to_string();
                server.transport = server.transport.trim().to_lowercase();
                server.command = server.command.trim().to_string();
                server.args = normalize_string_list(server.args);
                server.env = normalize_string_list(server.env);
                server.tool_allowlist = normalize_string_list(server.tool_allowlist);
                server.notes = server.notes.trim().to_string();
                if server.transport.is_empty() {
                    server.transport = "stdio".to_string();
                }
                if server.id.is_empty() {
                    server.id = server.name.to_lowercase().replace(' ', "-");
                }
                if server.name.is_empty() || server.command.is_empty() {
                    None
                } else {
                    Some(server)
                }
            })
            .collect();
    }
    config.document_memory.watched_dirs =
        normalize_string_list(config.document_memory.watched_dirs);
    config.browser_automation.allowed_domains =
        normalize_string_list(config.browser_automation.allowed_domains)
            .into_iter()
            .map(|domain| {
                domain
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_matches('/')
                    .to_lowercase()
            })
            .filter(|domain| !domain.is_empty())
            .collect();
    if config.local_voice_loop.wake_phrase.trim().is_empty() {
        config.local_voice_loop.wake_phrase = "小企鹅".to_string();
    } else {
        config.local_voice_loop.wake_phrase =
            config.local_voice_loop.wake_phrase.trim().to_string();
    }
    config
}

fn parse_scopes(value: &str) -> Vec<String> {
    let mut scopes = Vec::new();
    for item in value
        .split(|char: char| char == ',' || char.is_whitespace())
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        if !scopes.iter().any(|existing| existing == item) {
            scopes.push(item.to_string());
        }
    }
    scopes
}

fn clear_oauth_session(runtime: &mut RuntimeState) {
    runtime.oauth_access_token = None;
    runtime.oauth_refresh_token = None;
    runtime.oauth_access_expires_at = None;
    runtime.oauth_account_hint = None;
    runtime.oauth_last_error = None;
    runtime.pending_oauth = None;
}

fn ensure_network_allowed(allow_network: bool) -> Result<(), String> {
    if !allow_network {
        return Err(
            "当前已禁用网络访问。OAuth 登录和令牌交换同样需要你先显式开启外网访问。"
                .to_string(),
        );
    }

    Ok(())
}

fn expire_transient_state(runtime: &mut RuntimeState) {
    policy::cleanup_expired_approvals(&mut runtime.pending_action_approvals);

    if let Some(pending) = &runtime.pending_oauth {
        if pending.expires_at <= now_millis() {
            runtime.pending_oauth = None;
            runtime.oauth_last_error = Some("上一次 OAuth 登录已过期，请重新发起授权。".to_string());
        }
    }
}

#[tauri::command]
fn get_assistant_snapshot(
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);
    Ok(snapshot_from_runtime(&runtime))
}

#[tauri::command]
fn save_provider_config(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    input: ProviderConfigInput,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);
    let previous_runtime = runtime.clone();

    let previous_provider_kind = runtime.provider.kind;
    let previous_auth_mode = runtime.provider.auth_mode;
    let previous_base_url = runtime.provider.base_url.clone();
    let previous_oauth_authorize_url = runtime.provider.oauth.authorize_url.clone();
    let previous_oauth_token_url = runtime.provider.oauth.token_url.clone();
    let previous_oauth_client_id = runtime.provider.oauth.client_id.clone();
    let previous_oauth_redirect_url = runtime.provider.oauth.redirect_url.clone();
    let previous_oauth_scopes = runtime.provider.oauth.scopes.clone();

    runtime.provider.kind = input.kind;
    runtime.provider.model = if input.model.trim().is_empty() {
        input.kind.default_model().to_string()
    } else {
        input.model.trim().to_string()
    };
    runtime.provider.base_url = normalize_optional(input.base_url);
    runtime.provider.system_prompt = if input.system_prompt.trim().is_empty() {
        default_system_prompt()
    } else {
        input.system_prompt.trim().to_string()
    };
    runtime.provider.allow_network = input.allow_network;
    runtime.launch_at_startup = input.launch_at_startup;
    runtime.auto_update_codex = input.auto_update_codex;
    runtime.auto_check_app_update = input.auto_check_app_update;
    runtime.pet_status_visible = input.pet_status_visible;
    runtime.research = research::normalize_config(&input.research);
    runtime.provider.voice_reply = input.voice_reply;
    runtime.provider.retain_history = input.retain_history;
    runtime.provider.voice_input_mode = input.voice_input_mode;
    runtime.provider.push_to_talk_shortcut =
        normalized_push_to_talk_shortcut(&input.push_to_talk_shortcut);
    runtime.workspace_root = normalize_optional(input.workspace_root);
    runtime.permission_level = policy::clamp_permission_level(input.permission_level);
    runtime.provider.auth_mode = input.auth_mode;
    runtime.provider.oauth.authorize_url = normalize_optional(input.oauth_authorize_url);
    runtime.provider.oauth.token_url = normalize_optional(input.oauth_token_url);
    runtime.provider.oauth.client_id = normalize_optional(input.oauth_client_id);
    runtime.provider.oauth.redirect_url = normalize_optional(input.oauth_redirect_url)
        .or_else(|| Some(DEFAULT_OAUTH_REDIRECT_URL.to_string()));
    runtime.provider.oauth.scopes = parse_scopes(&input.oauth_scopes);
    runtime.vision_channel.enabled = input.vision_channel.enabled;
    runtime.vision_channel.kind = input.vision_channel.kind;
    runtime.vision_channel.model = if input.vision_channel.model.trim().is_empty() {
        input.vision_channel.kind.default_model().to_string()
    } else {
        input.vision_channel.model.trim().to_string()
    };
    runtime.vision_channel.base_url = normalize_optional(input.vision_channel.base_url);
    runtime.vision_channel.allow_network = input.vision_channel.allow_network;
    runtime.vision_channel.timeout_ms = input.vision_channel.timeout_ms.clamp(1_000, 60_000);
    runtime.vision_channel.max_image_bytes =
        input.vision_channel.max_image_bytes.clamp(64 * 1024, 10 * 1024 * 1024);
    runtime.vision_channel.max_image_width = input.vision_channel.max_image_width.clamp(320, 4096);
    runtime.vision_channel.max_image_height =
        input.vision_channel.max_image_height.clamp(240, 4096);
    runtime.vision_channel.last_error = None;

    if input.clear_api_key.unwrap_or(false) {
        runtime.api_key = None;
    }

    if let Some(api_key) = input
        .api_key
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        runtime.api_key = Some(api_key.to_string());
    }

    if input.vision_channel.clear_api_key.unwrap_or(false) {
        runtime.vision_api_key = None;
    }

    if let Some(api_key) = input
        .vision_channel
        .api_key
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        runtime.vision_api_key = Some(api_key.to_string());
    }

    runtime.vision_channel_status = app_state::current_vision_channel_status(
        &runtime.vision_channel,
        runtime.vision_api_key.as_ref(),
    );
    runtime.shell_permissions = input.shell_permissions.clone();
    runtime.agent_roadmap = normalize_agent_roadmap_config(input.agent_roadmap);

    let oauth_identity_changed = previous_provider_kind != runtime.provider.kind
        || previous_auth_mode != runtime.provider.auth_mode
        || previous_base_url != runtime.provider.base_url
        || previous_oauth_authorize_url != runtime.provider.oauth.authorize_url
        || previous_oauth_token_url != runtime.provider.oauth.token_url
        || previous_oauth_client_id != runtime.provider.oauth.client_id
        || previous_oauth_redirect_url != runtime.provider.oauth.redirect_url
        || previous_oauth_scopes != runtime.provider.oauth.scopes;

    if input.clear_oauth_token.unwrap_or(false) {
        clear_oauth_session(&mut runtime);
    } else if !matches!(runtime.provider.auth_mode, AuthMode::OAuth) {
        clear_oauth_session(&mut runtime);
    } else if oauth_identity_changed {
        let had_oauth_state = runtime.pending_oauth.is_some()
            || runtime
                .oauth_access_token
                .as_ref()
                .is_some_and(|token| !token.trim().is_empty());
        clear_oauth_session(&mut runtime);
        if had_oauth_state {
            runtime.oauth_last_error = Some("OAuth 配置已变更，请重新发起登录。".to_string());
        }
    }

    sync_codex_provider_view(&app, &mut runtime);
    if let Err(error) = sync_launch_at_startup(&app, runtime.launch_at_startup) {
        *runtime = previous_runtime;
        return Err(error);
    }
    if let Err(error) = sync_whisper_input_shortcut(&app, Some(&previous_runtime), &runtime) {
        let _ = sync_launch_at_startup(&app, previous_runtime.launch_at_startup);
        *runtime = previous_runtime;
        return Err(error);
    }
    if let Err(error) = sync_shell_permissions_to_checker(&app, &input.shell_permissions) {
        let _ = sync_shell_permissions_to_checker(&app, &previous_runtime.shell_permissions);
        let _ = sync_launch_at_startup(&app, previous_runtime.launch_at_startup);
        let _ = sync_whisper_input_shortcut(&app, Some(&runtime), &previous_runtime);
        *runtime = previous_runtime;
        return Err(error);
    }

    if let Err(error) = research::sync_research_memory(&app, &runtime.research) {
        let _ = sync_shell_permissions_to_checker(&app, &previous_runtime.shell_permissions);
        let _ = sync_launch_at_startup(&app, previous_runtime.launch_at_startup);
        let _ = sync_whisper_input_shortcut(&app, Some(&runtime), &previous_runtime);
        *runtime = previous_runtime;
        return Err(error);
    }

    runtime.mode = PetMode::Idle;

    let audit_detail = format!(
        "provider={} auth={} network={} permission=L{}",
        runtime.provider.kind.label(),
        match runtime.provider.auth_mode {
            AuthMode::ApiKey => "apiKey",
            AuthMode::OAuth => "oauth",
        },
        runtime.provider.allow_network,
        runtime.permission_level
    );
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record("save_provider_config", "ok", audit_detail, 1),
    );

    if let Err(error) = save(&app, &runtime) {
        let _ = sync_shell_permissions_to_checker(&app, &previous_runtime.shell_permissions);
        let _ = sync_launch_at_startup(&app, previous_runtime.launch_at_startup);
        let _ = sync_whisper_input_shortcut(&app, Some(&runtime), &previous_runtime);
        *runtime = previous_runtime;
        return Err(error);
    }
    if let Some(agent_state) = app.try_state::<AgentTaskState>() {
        if let Ok(mut cache) = agent_state.vision_cache() {
            *cache = None;
        }
    }
    Ok(snapshot_from_runtime(&runtime))
}

#[tauri::command]
fn start_oauth_sign_in(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<OAuthFlowResult, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);

    if !matches!(runtime.provider.auth_mode, AuthMode::OAuth) {
        return Err("请先在设置中把认证方式切换到 OAuth。".to_string());
    }
    ensure_network_allowed(runtime.provider.allow_network)?;

    let pending = oauth::prepare_authorization(&runtime.provider)?;
    let authorization_url = pending.authorization_url.clone();
    runtime.pending_oauth = Some(pending);
    runtime.oauth_last_error = None;
    runtime.mode = PetMode::Idle;

    let audit_detail = format!(
        "provider={} redirect={}",
        runtime.provider.kind.label(),
        runtime
            .provider
            .oauth
            .redirect_url
            .clone()
            .unwrap_or_default()
    );
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record("oauth_login_started", "pending", audit_detail, 1),
    );

    save(&app, &runtime)?;
    Ok(OAuthFlowResult {
        message: "已生成 OAuth 授权链接。请在系统浏览器中完成登录后，把回调地址粘贴回来。"
            .to_string(),
        authorization_url: Some(authorization_url),
        snapshot: snapshot_from_runtime(&runtime),
    })
}

#[tauri::command]
async fn start_oauth_sign_in_auto(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<OAuthFlowResult, String> {
    let (authorization_url, redirect_url) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);

        if !matches!(runtime.provider.auth_mode, AuthMode::OAuth) {
            return Err("请先在设置中把认证方式切换到 OAuth。".to_string());
        }
        ensure_network_allowed(runtime.provider.allow_network)?;

        let pending = oauth::prepare_authorization(&runtime.provider)?;
        let authorization_url = pending.authorization_url.clone();
        let redirect_url = runtime
            .provider
            .oauth
            .redirect_url
            .clone()
            .unwrap_or_else(|| DEFAULT_OAUTH_REDIRECT_URL.to_string());

        runtime.pending_oauth = Some(pending);
        runtime.oauth_last_error = None;
        runtime.mode = PetMode::Idle;
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "oauth_login_started",
                "pending",
                "自动登录已启动，等待浏览器回调。",
                1,
            ),
        );
        save(&app, &runtime)?;
        (authorization_url, redirect_url)
    };

    if let Err(error) = oauth::open_authorization_in_browser(&authorization_url) {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.oauth_last_error = Some(error.clone());
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record("oauth_login_started", "error", &error, 1),
        );
        save(&app, &runtime)?;
        return Ok(OAuthFlowResult {
            message: format!(
                "自动打开浏览器失败：{}。你可以改用“生成授权链接”手动登录。",
                error
            ),
            authorization_url: Some(authorization_url),
            snapshot: snapshot_from_runtime(&runtime),
        });
    }

    let callback_url = match tauri::async_runtime::spawn_blocking({
        let redirect_url = redirect_url.clone();
        move || oauth::wait_for_callback(&redirect_url, Duration::from_secs(180))
    })
    .await
    .map_err(|error| error.to_string())? {
        Ok(callback_url) => callback_url,
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_started", "timeout", &error, 1),
            );
            save(&app, &runtime)?;
            return Ok(OAuthFlowResult {
                message: format!(
                    "自动 OAuth 回调未完成：{}。你可以继续在当前设置窗口里手动粘贴回调地址完成登录。",
                    error
                ),
                authorization_url: Some(authorization_url),
                snapshot: snapshot_from_runtime(&runtime),
            });
        }
    };

    let (provider_config, pending) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);
        let pending = runtime
            .pending_oauth
            .take()
            .ok_or_else(|| "当前没有进行中的 OAuth 登录。".to_string())?;
        save(&app, &runtime)?;
        (runtime.provider.clone(), pending)
    };

    let (code, returned_state) = match oauth::parse_callback(&callback_url) {
        Ok(parsed) => parsed,
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "error", "OAuth 回调解析失败。", 1),
            );
            save(&app, &runtime)?;
            return Err(error);
        }
    };

    if returned_state != pending.state {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.oauth_last_error = Some("OAuth 状态校验失败，请重新发起登录。".to_string());
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "oauth_login_completed",
                "rejected",
                "OAuth 状态校验失败。",
                1,
            ),
        );
        save(&app, &runtime)?;
        return Err("OAuth 状态校验失败，请重新发起登录。".to_string());
    }

    match oauth::exchange_code(&provider_config, &pending, &code).await {
        Ok(exchange) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_access_token = Some(exchange.access_token);
            runtime.oauth_refresh_token = exchange.refresh_token;
            runtime.oauth_access_expires_at = exchange.expires_at;
            runtime.oauth_account_hint = exchange.account_hint;
            runtime.oauth_last_error = None;
            runtime.mode = PetMode::Idle;
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "ok", "自动登录成功。", 1),
            );
            save(&app, &runtime)?;
            Ok(OAuthFlowResult {
                message: "OAuth 登录成功。当前只会在运行内存中保留访问令牌，并优先把它用于支持 bearer token 的模型网关。"
                    .to_string(),
                authorization_url: None,
                snapshot: snapshot_from_runtime(&runtime),
            })
        }
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "error", &error, 1),
            );
            save(&app, &runtime)?;
            Err(error)
        }
    }
}

#[tauri::command]
async fn complete_oauth_sign_in(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    callback_url: String,
) -> Result<OAuthFlowResult, String> {
    let (provider_config, pending) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);

        if !matches!(runtime.provider.auth_mode, AuthMode::OAuth) {
            return Err("请先在设置中把认证方式切换到 OAuth。".to_string());
        }
        ensure_network_allowed(runtime.provider.allow_network)?;

        let pending = runtime
            .pending_oauth
            .take()
            .ok_or_else(|| "当前没有进行中的 OAuth 登录。".to_string())?;
        save(&app, &runtime)?;
        (runtime.provider.clone(), pending)
    };

    let (code, returned_state) = match oauth::parse_callback(&callback_url) {
        Ok(parsed) => parsed,
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "error", "OAuth 回调解析失败。", 1),
            );
            save(&app, &runtime)?;
            return Err(error);
        }
    };
    if returned_state != pending.state {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.pending_oauth = None;
        runtime.oauth_last_error = Some("OAuth 状态校验失败，请重新发起登录。".to_string());
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "oauth_login_completed",
                "rejected",
                "OAuth 状态校验失败。",
                1,
            ),
        );
        save(&app, &runtime)?;
        return Err("OAuth 状态校验失败，请重新发起登录。".to_string());
    }

    match oauth::exchange_code(&provider_config, &pending, &code).await {
        Ok(exchange) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_access_token = Some(exchange.access_token);
            runtime.oauth_refresh_token = exchange.refresh_token;
            runtime.oauth_access_expires_at = exchange.expires_at;
            runtime.oauth_account_hint = exchange.account_hint;
            runtime.oauth_last_error = None;
            runtime.mode = PetMode::Idle;
            let audit_detail = format!(
                "provider={} auth=oauth account={}",
                runtime.provider.kind.label(),
                runtime
                    .oauth_account_hint
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
            );
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "ok", audit_detail, 1),
            );
            save(&app, &runtime)?;
            Ok(OAuthFlowResult {
                message: "OAuth 登录成功。当前只会在运行内存中保留访问令牌，并优先把它用于支持 bearer token 的模型网关。"
                    .to_string(),
                authorization_url: None,
                snapshot: snapshot_from_runtime(&runtime),
            })
        }
        Err(error) => {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.oauth_last_error = Some(error.clone());
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record("oauth_login_completed", "error", &error, 1),
            );
            save(&app, &runtime)?;
            Err(error)
        }
    }
}

#[tauri::command]
fn disconnect_oauth_sign_in(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<OAuthFlowResult, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    clear_oauth_session(&mut runtime);
    runtime.mode = PetMode::Idle;
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record("oauth_logout", "ok", "已清空 OAuth 内存令牌状态。", 0),
    );
    save(&app, &runtime)?;
    Ok(OAuthFlowResult {
        message: "已退出 OAuth 登录，并清空内存中的令牌状态。".to_string(),
        authorization_url: None,
        snapshot: snapshot_from_runtime(&runtime),
    })
}

// ============================================================================
// Shell Agent 权限管理
// ============================================================================

/// 权限状态响应
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PermissionStatusResponse {
    granted_permissions: Vec<String>,
    pending_requests: Vec<String>,
    message: String,
}

#[tauri::command]
fn get_shell_permissions(app: AppHandle) -> Result<PermissionStatusResponse, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let behavior_state = BehaviorState::new(&app_data);
    let checker = behavior_state.permission_checker();

    let granted: Vec<String> = checker
        .granted_permissions()
        .iter()
        .map(|p| p.id.clone())
        .collect();

    let pending: Vec<String> = checker
        .pending_requests()
        .iter()
        .map(|r| format!("{}: {}", r.permission_id, r.reason))
        .collect();

    Ok(PermissionStatusResponse {
        granted_permissions: granted,
        pending_requests: pending,
        message: "权限状态查询成功".to_string(),
    })
}

#[tauri::command]
fn grant_shell_permission(
    app: AppHandle,
    permission_id: String,
    duration_hours: Option<u64>,
) -> Result<PermissionStatusResponse, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let behavior_state = BehaviorState::new(&app_data);

    let duration_ms = duration_hours.map(|h| h * 60 * 60 * 1000);

    {
        let mut checker = behavior_state.permission_checker();
        checker.grant(
            &permission_id,
            GrantSource::User,
            PermissionScope::Session,
            duration_ms,
        )?;
    }

    // 保存状态
    behavior_state.save()?;

    // 返回更新后的状态
    get_shell_permissions(app)
}

#[tauri::command]
fn revoke_shell_permission(
    app: AppHandle,
    permission_id: String,
) -> Result<PermissionStatusResponse, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let behavior_state = BehaviorState::new(&app_data);

    {
        let mut checker = behavior_state.permission_checker();
        checker.revoke(&permission_id, "user")?;
    }

    behavior_state.save()?;
    get_shell_permissions(app)
}

#[tauri::command]
fn grant_basic_shell_access(app: AppHandle) -> Result<PermissionStatusResponse, String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let behavior_state = BehaviorState::new(&app_data);

    // 授予基本 shell 执行权限（24 小时）
    let duration_ms = Some(24 * 60 * 60 * 1000);

    {
        let mut checker = behavior_state.permission_checker();
        checker.grant("shell:execute", GrantSource::User, PermissionScope::Session, duration_ms)?;
    }

    behavior_state.save()?;
    get_shell_permissions(app)
}

// ============================================================================
// Whisper 语音识别命令
// ============================================================================

#[tauri::command]
fn get_whisper_status(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::WhisperStatus, String> {
    Ok(transcriber.get_status())
}

#[tauri::command]
fn get_whisper_models(
    transcriber: State<'_, TranscriberService>,
) -> Result<Vec<audio_types::ModelInfo>, String> {
    Ok(transcriber.get_available_models())
}

#[tauri::command]
async fn download_whisper_model(
    transcriber: State<'_, TranscriberService>,
    model: audio_types::WhisperModel,
    progress: tauri::ipc::Channel<audio_types::DownloadProgress>,
) -> Result<String, String> {
    let path = transcriber.download_model(model, progress).await?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn load_whisper_model(
    transcriber: State<'_, TranscriberService>,
    model: audio_types::WhisperModel,
) -> Result<audio_types::WhisperStatus, String> {
    transcriber.load_model(model)?;
    Ok(transcriber.get_status())
}

#[tauri::command]
fn unload_whisper_model(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::WhisperStatus, String> {
    transcriber.unload_model();
    Ok(transcriber.get_status())
}

#[tauri::command]
fn delete_whisper_model(
    transcriber: State<'_, TranscriberService>,
    model: audio_types::WhisperModel,
) -> Result<audio_types::WhisperStatus, String> {
    transcriber.delete_model(model)?;
    Ok(transcriber.get_status())
}

#[tauri::command]
fn start_whisper_recording(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::RecordingState, String> {
    transcriber.start_recording()?;
    Ok(transcriber.get_recording_state())
}

#[tauri::command]
fn stop_whisper_recording(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::TranscriptionResult, String> {
    transcriber.stop_recording()
}

#[tauri::command]
fn get_whisper_recording_state(
    transcriber: State<'_, TranscriberService>,
) -> Result<audio_types::RecordingState, String> {
    Ok(transcriber.get_recording_state())
}

/// 执行待确认的 Shell 命令
async fn execute_pending_shell_command(
    app: &AppHandle,
    state: &State<'_, Mutex<RuntimeState>>,
    pending: &PendingShellCommand,
) -> Result<ChatResponse, String> {
    // 使用 Shell Executor 执行命令
    let mut shell_executor = if let Ok(app_data) = app.path().app_data_dir() {
        ShellAgentExecutor::with_app_data(&app_data)
    } else {
        ShellAgentExecutor::new()
    };

    let exec_result = shell_executor.confirm_and_continue(&pending.command);

    let reply_text = if exec_result.success {
        format!("命令已执行：\n```\n{}\n```\n\n输出：\n```\n{}\n```",
            pending.command, exec_result.output)
    } else {
        format!("命令执行失败：\n```\n{}\n```\n\n错误：{}", pending.command, exec_result.output)
    };

    let reply_message = ChatMessage::assistant(&reply_text);

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.pending_shell_command = None;
        let confirmation = ChatMessage::user("yes".to_string());
        runtime.messages.push(confirmation.clone());
        runtime.messages.push(reply_message.clone());
        runtime.mode = PetMode::Idle;
        ai_memory::trim_history(&mut runtime.messages);
        let _ = session_thread::append_message(app, &mut runtime, &confirmation);
        let _ = session_thread::append_message(app, &mut runtime, &reply_message);

        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "shell_command_confirmed",
                if exec_result.success { "ok" } else { "error" },
                format!("command={}", pending.command),
                1,
            ),
        );
        save(app, &runtime)?;
        snapshot_from_runtime(&runtime)
    };

    Ok(ChatResponse {
        reply: reply_message,
        provider_label: "Shell Agent".to_string(),
        snapshot,
        agent: None,
        pending_shell_confirmation: None,
    })
}

/// 取消待确认的 Shell 命令
async fn cancel_pending_shell_command(
    app: &AppHandle,
    state: &State<'_, Mutex<RuntimeState>>,
    pending: &PendingShellCommand,
) -> Result<ChatResponse, String> {
    let reply_text = format!("已取消执行命令：`{}`", pending.command);
    let reply_message = ChatMessage::assistant(&reply_text);

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.pending_shell_command = None;
        let confirmation = ChatMessage::user("no".to_string());
        runtime.messages.push(confirmation.clone());
        runtime.messages.push(reply_message.clone());
        runtime.mode = PetMode::Idle;
        ai_memory::trim_history(&mut runtime.messages);
        let _ = session_thread::append_message(app, &mut runtime, &confirmation);
        let _ = session_thread::append_message(app, &mut runtime, &reply_message);

        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record("shell_command_cancelled", "ok", format!("command={}", pending.command), 1),
        );
        save(app, &runtime)?;
        snapshot_from_runtime(&runtime)
    };

    Ok(ChatResponse {
        reply: reply_message,
        provider_label: "Shell Agent".to_string(),
        snapshot,
        agent: None,
        pending_shell_confirmation: None,
    })
}

fn pending_shell_confirmation_info(pending: &PendingShellCommand) -> PendingShellConfirmationInfo {
    PendingShellConfirmationInfo {
        id: pending.id.clone(),
        command: pending.command.clone(),
        risk_description: pending.risk_description.clone(),
        created_at: pending.created_at,
    }
}

fn pending_shell_command_expired(pending: &PendingShellCommand) -> bool {
    now_millis().saturating_sub(pending.created_at) > SHELL_CONFIRMATION_TIMEOUT_MS
}

fn reply_for_expired_shell_command(
    app: &AppHandle,
    state: &State<'_, Mutex<RuntimeState>>,
) -> Result<ChatResponse, String> {
    let reply_message =
        ChatMessage::assistant("之前那条待确认的 Shell 命令已经超时，未被执行。请重新发起。");
    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.pending_shell_command = None;
        runtime.messages.push(reply_message.clone());
        runtime.mode = PetMode::Idle;
        ai_memory::trim_history(&mut runtime.messages);
        let _ = session_thread::append_message(app, &mut runtime, &reply_message);
        save(app, &runtime)?;
        snapshot_from_runtime(&runtime)
    };

    Ok(ChatResponse {
        reply: reply_message,
        provider_label: "Shell Agent".to_string(),
        snapshot,
        agent: None,
        pending_shell_confirmation: None,
    })
}

fn reply_for_pending_shell_command(
    app: &AppHandle,
    state: &State<'_, Mutex<RuntimeState>>,
    pending: &PendingShellCommand,
    user_input: &str,
) -> Result<ChatResponse, String> {
    let reply_text = format!(
        "当前还有一个待确认的 Shell 命令尚未执行：\n```\n{}\n```\n\n风险说明：{}\n\n如果你要继续执行，请回复 `yes` / `确认`；如果要取消，请回复 `no` / `取消`。你刚才这句“{}”不会清除这条待确认命令。",
        pending.command,
        pending.risk_description,
        user_input.trim()
    );
    let reply_message = ChatMessage::assistant(&reply_text);

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        let user_message = ChatMessage::user(user_input.trim().to_string());
        runtime.messages.push(user_message.clone());
        runtime.messages.push(reply_message.clone());
        runtime.mode = PetMode::Idle;
        ai_memory::trim_history(&mut runtime.messages);
        let _ = session_thread::append_message(app, &mut runtime, &user_message);
        let _ = session_thread::append_message(app, &mut runtime, &reply_message);
        save(app, &runtime)?;
        snapshot_from_runtime(&runtime)
    };

    Ok(ChatResponse {
        reply: reply_message,
        provider_label: "Shell Agent".to_string(),
        snapshot,
        agent: None,
        pending_shell_confirmation: Some(pending_shell_confirmation_info(pending)),
    })
}

fn build_chat_history_for_provider(
    provider_config: &app_state::ProviderConfig,
    permission_level: u8,
    allowed_actions: &[app_state::DesktopAction],
    messages: &[ChatMessage],
    extra_context: Option<&str>,
) -> Vec<ChatMessage> {
    let mut history = Vec::with_capacity(messages.len() + 1);
    let mut system_prompt =
        guardrails::compose_system_prompt(provider_config, permission_level, allowed_actions);
    if let Some(extra_context) = extra_context
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(extra_context);
    }
    history.push(ChatMessage::new(
        "system",
        system_prompt,
    ));
    history.extend(
        messages
            .iter()
            .filter(|message| message.role != "system")
            .cloned(),
    );
    history
}

fn render_pet_status_context(status: &app_state::PetStatus) -> String {
    format!(
        "## 企鹅状态与性格\n- mood: {:?}\n- energy: {}\n- social_need: {}\n- orderliness: {}\n- affection: {}\n- curiosity: {}\n- summary: {}\n- personality_style: {}\n\n使用方式：这些状态由日常对话、任务压力、陪伴频率、维护操作和长期记忆自然形成。不要让用户喂食、清洁、休息或玩耍，也不要逐项汇报数值。把它们当作桌宠当前性格和语气的背景：结合长期记忆里的用户偏好，微调回答的亲近感、主动性、安抚程度和探索欲；功能性回答仍优先清晰、准确、可执行。",
        status.mood,
        status.energy,
        status.hunger,
        status.cleanliness,
        status.affection,
        status.curiosity,
        status.summary.trim(),
        status.personality_style()
    )
}

fn render_recent_conversation_context(messages: &[ChatMessage]) -> Option<String> {
    let rendered = messages
        .iter()
        .filter(|message| message.role == "user" || message.role == "assistant")
        .map(|message| {
            let role = if message.role == "user" { "用户" } else { "助手" };
            format!("{role}：{}", message.content.trim())
        })
        .collect::<Vec<_>>();

    if rendered.is_empty() {
        None
    } else {
        Some(format!(
            "## 最近聊天上下文\n{}\n",
            rendered.join("\n\n")
        ))
    }
}

fn render_active_task_context(task: Option<&agent::types::AgentTaskRun>) -> Option<String> {
    let task = task?;
    let mut lines = vec![
        "## 当前任务状态".to_string(),
        format!("- taskTitle: {}", task.task_title.trim()),
        format!("- intent: {:?}", task.intent),
        format!("- status: {:?}", task.task_status),
        format!("- stepBudget: {}", task.step_budget),
        format!("- retryBudget: {}", task.retry_budget),
    ];

    if let Some(summary) = task.pending_action_summary.as_ref().filter(|value| !value.trim().is_empty()) {
        lines.push(format!("- pendingAction: {}", summary.trim()));
    }
    if let Some(reason) = task.failure_reason.as_ref().filter(|value| !value.trim().is_empty()) {
        lines.push(format!("- lastFailure: {}", reason.trim()));
    }
    if let Some(last_step) = task.recent_steps.last() {
        lines.push(format!("- lastStep: {}", last_step.summary.trim()));
    }

    Some(format!("{}\n", lines.join("\n")))
}

fn render_workspace_context(workspace_root: Option<&str>) -> Option<String> {
    workspace_context::detect_workspace_context(workspace_root)
        .map(|context| context.render_for_prompt())
}

fn render_mcp_context(runtime: &RuntimeState) -> Option<String> {
    let config = &runtime.agent_roadmap.plugin_tools;
    if !config.enabled || !config.mcp_gateway_enabled {
        return None;
    }

    let mut lines = Vec::new();
    for server in config.mcp_servers.iter().filter(|server| server.enabled) {
        lines.push(format!(
            "- serverId: {}；name: {}；transport: {}；readOnly: {}",
            server.id, server.name, server.transport, server.read_only
        ));
        if !server.tool_allowlist.is_empty() {
            lines.push(format!("  allowlist: {}", server.tool_allowlist.join(", ")));
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(format!(
            "## MCP 外部工具\n{}\n使用方式：只有当用户明确要求查询外部 MCP 资源，或明确使用 /mcp 命令时才使用。当前自动直连只允许 readOnly=true 的 server；写操作必须等待确认流完善，不要自行调用。\n",
            lines.join("\n")
        ))
    }
}

fn parse_mcp_chat_command(input: &str) -> Option<(String, String, serde_json::Value)> {
    let rest = input
        .trim()
        .strip_prefix("/mcp")
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let mut parts = rest.splitn(3, char::is_whitespace);
    let server_id = parts.next()?.trim();
    let tool_name = parts.next()?.trim();
    if server_id.is_empty() || tool_name.is_empty() {
        return None;
    }
    let args = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        .flatten()
        .unwrap_or_else(|| serde_json::json!({}));

    Some((server_id.to_string(), tool_name.to_string(), args))
}

fn looks_like_mcp_natural_request(input: &str) -> bool {
    let lowered = input.trim().to_lowercase();
    if lowered.starts_with("/mcp") {
        return false;
    }
    [
        "mcp",
        "github mcp",
        "github",
        "仓库",
        "repo",
        "repository",
        "issue",
        "pull request",
        "pr",
        "actions",
        "外部工具",
        "外部资源",
    ]
    .iter()
    .any(|keyword| lowered.contains(keyword))
}

fn build_mcp_natural_prompt(mcp_context: &str) -> String {
    format!(
        "你是 PenguinPal 的 MCP 只读工具选择器。\n\
只能输出一个 JSON 对象，不要输出 markdown、解释或代码块。\n\
输出 schema：{{\"shouldCall\":true|false,\"serverId\":\"...|null\",\"toolName\":\"...|null\",\"arguments\":{{...}},\"reason\":\"...\"}}\n\
规则：\n\
1. 只有用户明确要求使用 MCP、GitHub MCP、GitHub/仓库/Issue/PR/Actions 等外部资源查询时，才 shouldCall=true。\n\
2. 只能选择上下文中 enabled 且 readOnly=true 的 server。\n\
3. toolName 必须来自 allowlist 或用户明确提到的已扫描工具名；不确定时 shouldCall=false。\n\
4. arguments 必须是 JSON object；不要猜隐私 token 或本地路径。\n\
5. 写操作、创建/修改/删除/评论/合并/触发工作流等请求必须 shouldCall=false。\n\n\
{mcp_context}"
    )
}

async fn plan_mcp_natural_tool_call(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    mcp_context: &str,
) -> Option<McpNaturalToolDecision> {
    if !looks_like_mcp_natural_request(user_input) {
        return None;
    }
    let prompt = build_mcp_natural_prompt(mcp_context);
    let raw = agent::model_adapter::request_structured_agent_output(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        codex_thread_id,
        permission_level,
        allowed_actions,
        &prompt,
        user_input,
    )
    .await
    .ok()?;
    serde_json::from_str::<McpNaturalToolDecision>(&raw).ok()
}

async fn summarize_mcp_tool_result(
    provider_config: &ProviderConfig,
    api_key: Option<String>,
    oauth_access_token: Option<String>,
    codex_command: Option<String>,
    codex_home: Option<String>,
    codex_thread_id: &mut Option<String>,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
    user_input: &str,
    server: &app_state::McpServerEntry,
    tool_name: &str,
    result: &serde_json::Value,
) -> Option<String> {
    let result_json = serde_json::to_string_pretty(result).ok()?;
    let system = ChatMessage::new(
        "system",
        "你是 PenguinPal 的 MCP 工具结果摘要器。把工具返回的 JSON 转成给用户看的简洁中文回答。不要编造 JSON 里没有的信息；保留关键名称、状态、链接、数量和下一步建议。若结果为空或报错，要直说。不要输出大段原始 JSON。".to_string(),
    );
    let user = ChatMessage::user(format!(
        "用户原始问题：{}\nMCP Server：{}\n工具：{}\n工具结果 JSON：\n{}",
        user_input.trim(),
        server.name,
        tool_name,
        result_json
    ));
    provider::respond(
        provider_config,
        api_key,
        oauth_access_token,
        codex_command,
        codex_home,
        codex_thread_id,
        permission_level,
        allowed_actions,
        &[system, user],
    )
    .await
    .ok()
    .map(|(reply, _)| reply)
    .filter(|reply| !reply.trim().is_empty())
}

fn fallback_mcp_json_reply(prefix: &str, tool_name: &str, result: &serde_json::Value) -> String {
    format!(
        "{prefix}\n\nMCP 工具 `{}` 调用完成：\n```json\n{}\n```",
        tool_name,
        serde_json::to_string_pretty(result).unwrap_or_else(|_| result.to_string())
    )
}

fn build_mcp_pending_request(
    server: &app_state::McpServerEntry,
    tool_name: &str,
    arguments: serde_json::Value,
) -> ControlPendingRequest {
    let created_at = now_millis();
    ControlPendingRequest {
        id: format!("mcp-{created_at}-{}", uuid::Uuid::new_v4().simple()),
        tool: MCP_CONTROL_PENDING_TOOL.to_string(),
        title: format!("待确认：{} / {}", server.name, tool_name),
        prompt: format!(
            "即将调用非只读 MCP 工具 `{}`。这可能会修改外部系统或远程资源，请确认。",
            tool_name
        ),
        preview: serde_json::json!({
            "serverId": server.id,
            "serverName": server.name,
            "toolName": tool_name,
            "readOnly": server.read_only,
            "arguments": arguments,
        }),
        args: serde_json::json!({
            "serverId": server.id,
            "toolName": tool_name,
            "arguments": arguments,
        }),
        created_at,
        expires_at: created_at + MCP_CONTROL_PENDING_TTL_MS,
        minimum_permission_level: 2,
        risk_level: ControlRiskLevel::WriteHigh,
    }
}

fn insert_mcp_pending(app: &AppHandle, pending_request: ControlPendingRequest) -> Result<(), String> {
    let control_state: State<'_, ControlServiceState> = app.state();
    let mut pending_requests = control_state
        .pending_requests()
        .map_err(|error| format!("MCP pending 写入失败: {error}"))?;
    pending_requests.insert(pending_request.id.clone(), pending_request);
    Ok(())
}

fn try_take_mcp_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<ControlPendingRequest>, String> {
    let control_state: State<'_, ControlServiceState> = app.state();
    let mut pending_requests = control_state
        .pending_requests()
        .map_err(|error| format!("读取 MCP pending 失败: {error}"))?;
    let Some(pending) = pending_requests.get(pending_id).cloned() else {
        return Ok(None);
    };
    if pending.tool != MCP_CONTROL_PENDING_TOOL {
        return Ok(None);
    }
    Ok(pending_requests.remove(pending_id))
}

async fn confirm_mcp_pending(app: &AppHandle, pending: ControlPendingRequest) -> Result<ToolInvokeResponse, String> {
    let server_id = pending
        .args
        .get("serverId")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "MCP pending 缺少 serverId。".to_string())?;
    let tool_name = pending
        .args
        .get("toolName")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "MCP pending 缺少 toolName。".to_string())?;
    let arguments = pending
        .args
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let (server, provider_config, api_key, oauth_access_token, permission_level, allowed_actions) = {
        let runtime_state: State<'_, Mutex<RuntimeState>> = app.state();
        let runtime = runtime_state
            .lock()
            .map_err(|_| "助手状态锁定失败".to_string())?;
        let server = runtime
            .agent_roadmap
            .plugin_tools
            .mcp_servers
            .iter()
            .find(|server| server.id == server_id)
            .cloned()
            .ok_or_else(|| format!("未找到 MCP Server：{server_id}"))?;
        (
            server,
            runtime.provider.clone(),
            runtime.api_key.clone(),
            runtime.oauth_access_token.clone(),
            runtime.permission_level,
            policy::actions_for_level(runtime.permission_level),
        )
    };
    let result = call_mcp_server_tool_confirmed(&server, tool_name, arguments)?;
    let codex_runtime = resolve_for_app(app).ok();
    let mut codex_thread_id = {
        let runtime_state: State<'_, Mutex<RuntimeState>> = app.state();
        runtime_state
            .lock()
            .ok()
            .and_then(|runtime| runtime.codex_thread_id.clone())
    };
    let message = summarize_mcp_tool_result(
        &provider_config,
        api_key,
        oauth_access_token,
        codex_runtime
            .as_ref()
            .and_then(|item| item.command.as_ref())
            .map(|path| path.to_string_lossy().to_string()),
        codex_runtime
            .as_ref()
            .map(|item| item.home_root.to_string_lossy().to_string()),
        &mut codex_thread_id,
        permission_level,
        &allowed_actions,
        "用户确认执行 MCP 写操作",
        &server,
        tool_name,
        &result,
    )
    .await
    .unwrap_or_else(|| fallback_mcp_json_reply("MCP 写操作已确认并执行完成。", tool_name, &result));
    {
        let runtime_state: State<'_, Mutex<RuntimeState>> = app.state();
        let mut runtime = runtime_state
            .lock()
            .map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.codex_thread_id = codex_thread_id.clone();
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "mcp_pending_confirmed",
                "ok",
                format!("server={} tool={} pending_id={}", server.id, tool_name, pending.id),
                3,
            ),
        );
        save(app, &runtime)?;
    }
    Ok(ToolInvokeResponse {
        status: "success".to_string(),
        result: Some(result),
        message: Some(message),
        pending_request: None,
        error: None,
    })
}

fn looks_like_control_confirmation_reply(input: &str) -> bool {
    let normalized = input.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }

    [
        "确认",
        "可以",
        "继续",
        "执行",
        "yes",
        "y",
        "ok",
        "好的",
        "取消",
        "不要",
        "停止",
        "no",
        "n",
        "算了",
    ]
    .iter()
    .any(|item| normalized == *item)
}

#[tauri::command]
async fn send_chat_message(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    content: String,
) -> Result<ChatResponse, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("消息不能为空".to_string());
    }

    // 检查是否有待确认的 Shell 命令
    let pending_cmd = {
        let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.pending_shell_command.clone()
    };

    // 处理 yes/no 确认
    if let Some(pending) = pending_cmd {
        if pending_shell_command_expired(&pending) {
            return reply_for_expired_shell_command(&app, &state);
        }
        let lower = trimmed.to_lowercase();
        if lower == "yes" || lower == "y" || lower == "确认" {
            // 用户确认执行
            return execute_pending_shell_command(&app, &state, &pending).await;
        } else if lower == "no" || lower == "n" || lower == "取消" {
            // 用户取消执行
            return cancel_pending_shell_command(&app, &state, &pending).await;
        }
        return reply_for_pending_shell_command(&app, &state, &pending, trimmed);
    }

    let pending_control_count = control_router::list_pending(&app)
        .map(|items| items.len())
        .unwrap_or_default();
    if pending_control_count > 0 && looks_like_control_confirmation_reply(trimmed) {
        let result = agent_router::handle_confirmation_response(&app, trimmed).await?;
        let reply_message = ChatMessage::assistant(&result.reply_text);
        let snapshot = {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            let user_message = ChatMessage::user(trimmed.to_string());
            runtime.messages.push(user_message.clone());
            runtime.messages.push(reply_message.clone());
            runtime.mode = PetMode::Idle;
            ai_memory::trim_history(&mut runtime.messages);
            let _ = session_thread::append_message(&app, &mut runtime, &user_message);
            let _ = session_thread::append_message(&app, &mut runtime, &reply_message);
            save(&app, &runtime)?;
            snapshot_from_runtime(&runtime)
        };

        return Ok(ChatResponse {
            reply: reply_message,
            provider_label: result.provider_label,
            snapshot,
            agent: Some(result.meta),
            pending_shell_confirmation: None,
        });
    }

    if let Some((server_id, tool_name, arguments)) = parse_mcp_chat_command(trimmed) {
        let runtime = state
            .lock()
            .map_err(|_| "助手状态锁定失败".to_string())?
            .clone();
        if !runtime.agent_roadmap.plugin_tools.enabled
            || !runtime.agent_roadmap.plugin_tools.mcp_gateway_enabled
        {
            return Err("请先在设置中启用插件/MCP 工具层和 MCP 网关。".to_string());
        }
        let server = runtime
            .agent_roadmap
            .plugin_tools
            .mcp_servers
            .iter()
            .find(|item| item.id == server_id)
            .cloned()
            .ok_or_else(|| format!("未找到 MCP Server：{server_id}"))?;
        if runtime.agent_roadmap.plugin_tools.require_confirmation && !server.read_only {
            let pending_request = build_mcp_pending_request(&server, &tool_name, arguments);
            insert_mcp_pending(&app, pending_request.clone())?;
            return Ok(ChatResponse {
                reply: ChatMessage::assistant(format!(
                    "MCP 工具 `{}` 不是只读调用，我已经放入确认队列。确认后再执行。",
                    tool_name
                )),
                provider_label: "MCP".to_string(),
                snapshot: snapshot_from_runtime(&runtime),
                agent: Some(AgentMessageMeta {
                    route: AgentRoute::Chat,
                    planned_tools: vec![format!("mcp://{server_id}/{tool_name}")],
                    pending_request: Some(pending_request),
                    task: None,
                    summary: None,
                }),
                pending_shell_confirmation: None,
            });
        }
        let result = call_mcp_server_tool(&server, &tool_name, arguments)?;
        let allowed_actions = policy::actions_for_level(runtime.permission_level);
        let codex_runtime = resolve_for_app(&app).ok();
        let mut codex_thread_id = runtime.codex_thread_id.clone();
        let reply_text = summarize_mcp_tool_result(
            &runtime.provider,
            runtime.api_key.clone(),
            runtime.oauth_access_token.clone(),
            codex_runtime
                .as_ref()
                .and_then(|item| item.command.as_ref())
                .map(|path| path.to_string_lossy().to_string()),
            codex_runtime
                .as_ref()
                .map(|item| item.home_root.to_string_lossy().to_string()),
            &mut codex_thread_id,
            runtime.permission_level,
            &allowed_actions,
            trimmed,
            &server,
            &tool_name,
            &result,
        )
        .await
        .unwrap_or_else(|| fallback_mcp_json_reply("MCP 工具调用完成。", &tool_name, &result));
        let reply_message = ChatMessage::assistant(reply_text);
        let snapshot = {
            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            let user_message = ChatMessage::user(trimmed.to_string());
            runtime.messages.push(user_message.clone());
            runtime.messages.push(reply_message.clone());
            runtime.codex_thread_id = codex_thread_id.clone();
            runtime.mode = PetMode::Idle;
            ai_memory::trim_history(&mut runtime.messages);
            let _ = session_thread::append_message(&app, &mut runtime, &user_message);
            let _ = session_thread::append_message(&app, &mut runtime, &reply_message);
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record(
                    "mcp_chat_tool_call",
                    "ok",
                    format!("server={} tool={} readOnly={}", server.id, tool_name, server.read_only),
                    if server.read_only { 1 } else { 3 },
                ),
            );
            save(&app, &runtime)?;
            snapshot_from_runtime(&runtime)
        };

        return Ok(ChatResponse {
            reply: reply_message,
            provider_label: "MCP".to_string(),
            snapshot,
            agent: Some(AgentMessageMeta {
                route: AgentRoute::Chat,
                planned_tools: vec![format!("mcp://{server_id}/{tool_name}")],
                pending_request: None,
                task: None,
                summary: None,
            }),
            pending_shell_confirmation: None,
        });
    }

    let user_message = ChatMessage::user(trimmed.to_string());
    let (
        provider_config,
        api_key,
        oauth_access_token,
        vision_channel,
        vision_api_key,
        codex_command,
        codex_home,
        workspace_root,
        provider_history,
        recent_conversation_context,
        permission_level,
        allowed_actions,
        pet_status_context,
        mcp_context,
    ) = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        expire_transient_state(&mut runtime);
        runtime.pet_status.nudge_for_user_message(now_millis(), trimmed);
        runtime.mode = PetMode::Thinking;
        runtime.messages.push(user_message.clone());
        ai_memory::trim_history(&mut runtime.messages);
        let _ = session_thread::append_message(&app, &mut runtime, &user_message);
        save(&app, &runtime)?;
        let allowed_actions = policy::actions_for_level(runtime.permission_level);
        let codex_runtime = resolve_for_app(&app).ok();
        let provider_history = session_thread::recent_messages(
            &app,
            &mut runtime,
            PROVIDER_HISTORY_LIMIT,
        )
        .unwrap_or_else(|_| runtime.messages.clone());
        let session_turn_messages = session_thread::recent_messages(
            &app,
            &mut runtime,
            SESSION_TURN_CONTEXT_LIMIT,
        )
        .unwrap_or_else(|_| ai_memory::context_window(&runtime.messages));
        (
            runtime.provider.clone(),
            runtime.api_key.clone(),
            runtime.oauth_access_token.clone(),
            runtime.vision_channel.clone(),
            runtime.vision_api_key.clone(),
            codex_runtime
                .as_ref()
                .and_then(|item| item.command.as_ref())
                .map(|path| path.to_string_lossy().to_string()),
            codex_runtime
                .as_ref()
                .map(|item| item.home_root.to_string_lossy().to_string()),
            runtime.workspace_root.clone(),
            provider_history,
            render_recent_conversation_context(&session_turn_messages),
            runtime.permission_level,
            allowed_actions,
            render_pet_status_context(&runtime.pet_status),
            render_mcp_context(&runtime),
        )
    };

    let current_task = agent::task_store::current_task(&app).ok().flatten();
    let current_task_context = render_active_task_context(current_task.as_ref());
    let workspace_context = render_workspace_context(workspace_root.as_deref());
    let archive_recall_context = history::render_archive_recall_for_prompt(&app, trimmed, 3)
        .ok()
        .flatten();
    let agent_turn_conversation_context = join_prompt_sections(&[
        Some(pet_status_context.as_str()),
        mcp_context.as_deref(),
        recent_conversation_context.as_deref(),
        archive_recall_context.as_deref(),
    ]);
    let provider_extra_context = join_prompt_sections(&[
        Some(pet_status_context.as_str()),
        mcp_context.as_deref(),
        archive_recall_context.as_deref(),
    ]);

    if let Some(mcp_context_text) = mcp_context.as_deref() {
        let mut codex_thread_id = {
            let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.codex_thread_id.clone()
        };
        if let Some(decision) = plan_mcp_natural_tool_call(
            &provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            codex_command.clone(),
            codex_home.clone(),
            &mut codex_thread_id,
            permission_level,
            &allowed_actions,
            trimmed,
            mcp_context_text,
        )
        .await
        {
            if decision.should_call {
                let server_id = decision.server_id.unwrap_or_default();
                let tool_name = decision.tool_name.unwrap_or_default();
                if !server_id.trim().is_empty() && !tool_name.trim().is_empty() {
                    let runtime = state
                        .lock()
                        .map_err(|_| "助手状态锁定失败".to_string())?
                        .clone();
                    let server = runtime
                        .agent_roadmap
                        .plugin_tools
                        .mcp_servers
                        .iter()
                        .find(|item| item.id == server_id)
                        .cloned()
                        .ok_or_else(|| format!("未找到 MCP Server：{server_id}"))?;
                    let arguments = decision.arguments.unwrap_or_else(|| serde_json::json!({}));
                    if runtime.agent_roadmap.plugin_tools.require_confirmation && !server.read_only {
                        let pending_request = build_mcp_pending_request(&server, &tool_name, arguments);
                        insert_mcp_pending(&app, pending_request.clone())?;
                        let reply_message = ChatMessage::assistant(format!(
                            "MCP 工具 `{}` 不是只读调用，我已经放入确认队列。确认后再执行。",
                            tool_name
                        ));
                        let snapshot = {
                            let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
                            runtime.messages.push(reply_message.clone());
                            runtime.codex_thread_id = codex_thread_id.clone();
                            runtime.mode = PetMode::Idle;
                            ai_memory::trim_history(&mut runtime.messages);
                            let _ = session_thread::append_message(&app, &mut runtime, &reply_message);
                            save(&app, &runtime)?;
                            snapshot_from_runtime(&runtime)
                        };
                        return Ok(ChatResponse {
                            reply: reply_message,
                            provider_label: "MCP".to_string(),
                            snapshot,
                            agent: Some(AgentMessageMeta {
                                route: AgentRoute::Chat,
                                planned_tools: vec![format!("mcp://{server_id}/{tool_name}")],
                                pending_request: Some(pending_request),
                                task: None,
                                summary: None,
                            }),
                            pending_shell_confirmation: None,
                        });
                    }
                    let result = call_mcp_server_tool(
                        &server,
                        &tool_name,
                        arguments,
                    )?;
                    let reason = decision.reason.unwrap_or_else(|| "自然语言 MCP 查询".to_string());
                    let reply_text = summarize_mcp_tool_result(
                        &provider_config,
                        api_key.clone(),
                        oauth_access_token.clone(),
                        codex_command.clone(),
                        codex_home.clone(),
                        &mut codex_thread_id,
                        permission_level,
                        &allowed_actions,
                        trimmed,
                        &server,
                        &tool_name,
                        &result,
                    )
                    .await
                    .unwrap_or_else(|| fallback_mcp_json_reply(&reason, &tool_name, &result));
                    let reply_message = ChatMessage::assistant(reply_text);
                    let snapshot = {
                        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
                        runtime.messages.push(reply_message.clone());
                        runtime.codex_thread_id = codex_thread_id.clone();
                        runtime
                            .pet_status
                            .nudge_for_assistant_reply(now_millis(), trimmed, &reply_message.content);
                        runtime.mode = PetMode::Idle;
                        ai_memory::trim_history(&mut runtime.messages);
                        let _ = session_thread::append_message(&app, &mut runtime, &reply_message);
                        audit::push_entry(
                            &mut runtime.audit_trail,
                            audit::record(
                                "mcp_natural_tool_call",
                                "ok",
                                format!("server={} tool={} readOnly={}", server.id, tool_name, server.read_only),
                                1,
                            ),
                        );
                        save(&app, &runtime)?;
                        snapshot_from_runtime(&runtime)
                    };

                    return Ok(ChatResponse {
                        reply: reply_message,
                        provider_label: "MCP".to_string(),
                        snapshot,
                        agent: Some(AgentMessageMeta {
                            route: AgentRoute::Chat,
                            planned_tools: vec![format!("mcp://{server_id}/{tool_name}")],
                            pending_request: None,
                            task: None,
                            summary: None,
                        }),
                        pending_shell_confirmation: None,
                    });
                }
            }
        }
    }

    let (
        reply_text,
        provider_label,
        outcome,
        detail,
        agent_meta,
        pending_shell_confirmation,
        should_exit,
        codex_thread_id,
    ) = if matches!(provider_config.kind, ProviderKind::Mock) {
        let mut codex_thread_id = {
            let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.codex_thread_id.clone()
        };
        let history = build_chat_history_for_provider(
            &provider_config,
            permission_level,
            &allowed_actions,
            &provider_history,
            provider_extra_context.as_deref(),
        );
        let (reply, label) = provider::respond(
            &provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            codex_command.clone(),
            codex_home.clone(),
            &mut codex_thread_id,
            permission_level,
            &allowed_actions,
            &history,
        )
        .await?;
        (
            reply,
            label,
            "chat_ok".to_string(),
            "agent_turn=mock_reply".to_string(),
            Some(AgentMessageMeta {
                route: AgentRoute::Chat,
                planned_tools: vec![],
                pending_request: None,
                task: None,
                summary: None,
            }),
            None,
            false,
            codex_thread_id,
        )
    } else {
        let mut codex_thread_id = {
            let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
            runtime.codex_thread_id.clone()
        };
        let agent_turn = agent_turn::decide_agent_turn(
            &provider_config,
            api_key.clone(),
            oauth_access_token.clone(),
            codex_command.clone(),
            codex_home.clone(),
            &mut codex_thread_id,
            permission_level,
            &allowed_actions,
            trimmed,
            agent_turn_conversation_context.as_deref(),
            current_task_context.as_deref(),
            workspace_context.as_deref(),
            pending_control_count,
        )
        .await;

        match agent_turn {
            Ok(decision) => match decision.mode {
                AgentTurnMode::ReplyOnly => (
                    decision.assistant_message.unwrap_or_default(),
                    provider_config.kind.label().to_string(),
                    "chat_ok".to_string(),
                    "agent_turn=reply_only".to_string(),
                    Some(AgentMessageMeta {
                        route: AgentRoute::Chat,
                        planned_tools: vec![],
                        pending_request: None,
                        task: None,
                        summary: None,
                    }),
                    None,
                    false,
                    codex_thread_id,
                ),
                AgentTurnMode::ExecuteDomain => {
                    let domain = decision
                        .execution_domain
                        .ok_or_else(|| "统一 agent turn 缺少 executionDomain。".to_string())?;
                    let result = agent_router::execute_agent_turn_domain(
                        &app,
                        &provider_config,
                        api_key.clone(),
                        oauth_access_token.clone(),
                        &vision_channel,
                        vision_api_key.clone(),
                        codex_command.clone(),
                        codex_home.clone(),
                        &mut codex_thread_id,
                        permission_level,
                        &allowed_actions,
                        trimmed,
                        agent_turn_conversation_context.as_deref(),
                        domain,
                        decision.assistant_message.as_deref(),
                    )
                    .await?;
                    let detail = match domain {
                        AgentExecutionDomain::Desktop => "agent_turn=execute_domain.desktop",
                        AgentExecutionDomain::Test => "agent_turn=execute_domain.test",
                        AgentExecutionDomain::Workspace => "agent_turn=execute_domain.workspace",
                        AgentExecutionDomain::Memory => "agent_turn=execute_domain.memory",
                    }
                    .to_string();
                    (
                        result.reply_text,
                        result.provider_label,
                        result.outcome,
                        detail,
                        Some(result.meta),
                        None,
                        false,
                        codex_thread_id,
                    )
                }
            },
            Err(_) => {
                let history = build_chat_history_for_provider(
                    &provider_config,
                    permission_level,
                    &allowed_actions,
                    &provider_history,
                    provider_extra_context.as_deref(),
                );
                let (reply, label) = provider::respond(
                    &provider_config,
                    api_key.clone(),
                    oauth_access_token.clone(),
                    codex_command.clone(),
                    codex_home.clone(),
                    &mut codex_thread_id,
                    permission_level,
                    &allowed_actions,
                    &history,
                )
                .await?;
                (
                    reply,
                    label,
                    "chat_ok".to_string(),
                    "agent_turn=fallback_reply".to_string(),
                    Some(AgentMessageMeta {
                        route: AgentRoute::Chat,
                        planned_tools: vec![],
                        pending_request: None,
                        task: None,
                        summary: None,
                    }),
                    None,
                    false,
                    codex_thread_id,
                )
            }
        }
    };

    let reply_message = ChatMessage::assistant(reply_text);

    let snapshot = {
        let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        runtime.messages.push(reply_message.clone());
        runtime
            .pet_status
            .nudge_for_assistant_reply(now_millis(), trimmed, &reply_message.content);
        runtime.mode = PetMode::Idle;
        runtime.codex_thread_id = codex_thread_id.clone();
        ai_memory::trim_history(&mut runtime.messages);
        let _ = session_thread::append_message(&app, &mut runtime, &reply_message);

        if let Err(error) = history::record_input_history(&app, trimmed) {
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record(
                    "input_history",
                    "warn",
                    format!("输入历史写入失败：{error}"),
                    0,
                ),
            );
        }

        if let Err(error) = history::record_reply_history(&app, trimmed, &reply_message.content) {
            audit::push_entry(
                &mut runtime.audit_trail,
                audit::record(
                    "reply_history",
                    "warn",
                    format!("今日回复历史写入失败：{error}"),
                    0,
                ),
            );
        }

        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record("chat_completion", &outcome, detail, 1),
        );
        save(&app, &runtime)?;
        snapshot_from_runtime(&runtime)
    };

    if let Ok(app_data) = app.path().app_data_dir() {
        let memory_service = crate::memory::MemoryService::new(&app_data);
        let _ = memory_service.write_conversation_turn(trimmed, &reply_message.content);
    }

    // 如果请求退出应用，延迟一小段时间后退出（让 UI 有时间显示告别语）
    if should_exit {
        let app_clone = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            app_clone.exit(0);
        });
    }

    Ok(ChatResponse {
        reply: reply_message,
        provider_label,
        snapshot,
        agent: agent_meta,
        pending_shell_confirmation,
    })
}

#[tauri::command]
fn request_desktop_action(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    action_id: String,
) -> Result<ActionExecutionResult, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);

    let action = policy::resolve_action(&action_id, runtime.permission_level)
        .ok_or_else(|| "动作不在白名单中或当前权限不足".to_string())?;
    policy::validate_action_access(&action, runtime.permission_level)?;

    if action.requires_confirmation {
        let approval = policy::build_action_approval(&action);
        runtime
            .pending_action_approvals
            .retain(|item| item.action.id != action.id);
        runtime.pending_action_approvals.push(approval.clone());
        let audit_detail = format!(
            "action={} approval_id={} expires_at={}",
            action.id, approval.id, approval.expires_at
        );
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record("action_approval_requested", "pending", audit_detail, action.risk_level),
        );
        save(&app, &runtime)?;
        return Ok(ActionExecutionResult {
            status: "needs_confirmation".to_string(),
            message: format!("{} 需要逐项确认后才能执行。", action.title),
            snapshot: snapshot_from_runtime(&runtime),
            approval_request: Some(approval),
        });
    }

    let execution = desktop::execute_action(&app, &action.id);
    let (status, message) = match execution {
        Ok(message) => ("ok".to_string(), message),
        Err(error) => ("blocked".to_string(), error),
    };

    runtime.mode = PetMode::Idle;
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record(&action.id, &status, &message, action.risk_level),
    );
    save(&app, &runtime)?;

    Ok(ActionExecutionResult {
        status,
        message,
        snapshot: snapshot_from_runtime(&runtime),
        approval_request: None,
    })
}

#[tauri::command]
fn confirm_desktop_action(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    approval_id: String,
    typed_phrase: String,
    acknowledged_checks: Vec<String>,
) -> Result<ActionExecutionResult, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    expire_transient_state(&mut runtime);

    let approval = runtime
        .pending_action_approvals
        .iter()
        .find(|item| item.id == approval_id)
        .cloned()
        .ok_or_else(|| "未找到待确认的动作授权。".to_string())?;
    let action = policy::resolve_action(&approval.action.id, runtime.permission_level)
        .ok_or_else(|| "动作不在白名单中或当前权限不足。".to_string())?;
    policy::validate_action_access(&action, runtime.permission_level)?;
    if let Err(error) = policy::validate_approval(&approval, &typed_phrase, &acknowledged_checks) {
        let reason = if error.contains("过期") {
            "approval_expired"
        } else if error.contains("确认短语") {
            "approval_phrase_mismatch"
        } else if error.contains("确认项") {
            "approval_checks_incomplete"
        } else {
            "approval_validation_failed"
        };
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "action_approval_rejected",
                "error",
                format!("action={} reason={}", action.id, reason),
                action.risk_level,
            ),
        );
        save(&app, &runtime)?;
        return Err(error);
    }

    runtime
        .pending_action_approvals
        .retain(|item| item.id != approval_id);
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record(
            "action_approval_confirmed",
            "ok",
            format!("action={} phrase_ok=true", action.id),
            action.risk_level,
        ),
    );

    let execution = desktop::execute_action(&app, &action.id);
    let (status, message) = match execution {
        Ok(message) => ("ok".to_string(), message),
        Err(error) => ("blocked".to_string(), error),
    };

    runtime.mode = PetMode::Idle;
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record(&action.id, &status, &message, action.risk_level),
    );
    save(&app, &runtime)?;

    Ok(ActionExecutionResult {
        status,
        message,
        snapshot: snapshot_from_runtime(&runtime),
        approval_request: None,
    })
}

#[tauri::command]
fn cancel_desktop_action_approval(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    approval_id: String,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    let cancelled = runtime
        .pending_action_approvals
        .iter()
        .find(|item| item.id == approval_id)
        .cloned();
    runtime
        .pending_action_approvals
        .retain(|item| item.id != approval_id);

    if let Some(approval) = cancelled {
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "action_approval_cancelled",
                "ok",
                format!("action={} cancelled_by_user=true", approval.action.id),
                approval.action.risk_level,
            ),
        );
    }

    save(&app, &runtime)?;
    Ok(snapshot_from_runtime(&runtime))
}

#[tauri::command]
fn clear_conversation(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<AssistantSnapshot, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    runtime.mode = PetMode::Idle;
    runtime.pending_action_approvals.clear();
    runtime.codex_thread_id = None;
    runtime.messages = vec![ChatMessage::assistant(
        "会话已经清空。现在重新回到严格白名单模式，你可以继续测试 UI、语音、OAuth 和动作面板。",
    )];
    let seed_messages = runtime.messages.clone();
    let _ = session_thread::start_new_thread(&app, &mut runtime, &seed_messages);
    audit::push_entry(
        &mut runtime.audit_trail,
        audit::record("clear_conversation", "ok", "用户主动清空了会话历史。", 0),
    );
    save(&app, &runtime)?;
    Ok(snapshot_from_runtime(&runtime))
}

#[tauri::command]
fn get_input_history(app: AppHandle) -> Result<Vec<String>, String> {
    history::get_input_history(&app)
}

#[tauri::command]
fn get_today_reply_history(app: AppHandle) -> Result<Vec<ReplyHistoryEntry>, String> {
    history::get_today_reply_history(&app)
}

#[tauri::command]
fn get_archive_recall_preview(
    app: AppHandle,
    query: String,
    limit: Option<usize>,
) -> Result<history::ArchiveRecallPreview, String> {
    history::preview_archive_recall(&app, &query, limit.unwrap_or(0))
}

#[tauri::command]
fn get_memory_management_snapshot(
    app: AppHandle,
) -> Result<memory::MemoryManagementSnapshot, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let memory_service = crate::memory::MemoryService::new(&app_data);
    memory_service.management_snapshot()
}

#[tauri::command]
fn delete_managed_memory(
    app: AppHandle,
    kind: memory::ManagedMemoryKind,
    id: String,
) -> Result<memory::MemoryManagementSnapshot, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let memory_service = crate::memory::MemoryService::new(&app_data);
    memory_service.delete_managed_memory(kind, &id)
}

#[tauri::command]
fn promote_memory_candidate(
    app: AppHandle,
    id: String,
) -> Result<memory::MemoryManagementSnapshot, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let memory_service = crate::memory::MemoryService::new(&app_data);
    memory_service.promote_memory_candidate(&id)
}

#[tauri::command]
fn resolve_memory_conflict(
    app: AppHandle,
    kind: memory::ManagedMemoryKind,
    group: String,
    keep_id: String,
) -> Result<memory::MemoryManagementSnapshot, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let memory_service = crate::memory::MemoryService::new(&app_data);
    memory_service.resolve_memory_conflict(kind, &group, &keep_id)
}

#[tauri::command]
fn run_memory_maintenance(app: AppHandle) -> Result<memory::MemoryManagementSnapshot, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let memory_service = crate::memory::MemoryService::new(&app_data);
    let _ = memory_service.run_maintenance();
    memory_service.management_snapshot()
}

#[tauri::command]
fn export_memory_backup(app: AppHandle) -> Result<String, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let memory_service = crate::memory::MemoryService::new(&app_data);
    let backup = memory_service.export_backup()?;
    serde_json::to_string_pretty(&backup).map_err(|error| format!("序列化记忆备份失败: {error}"))
}

#[tauri::command]
fn import_memory_backup(
    app: AppHandle,
    backup_json: String,
) -> Result<memory::MemoryManagementSnapshot, String> {
    let backup = serde_json::from_str::<memory::MemoryBackupSnapshot>(&backup_json)
        .map_err(|error| format!("解析记忆备份失败: {error}"))?;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
    let memory_service = crate::memory::MemoryService::new(&app_data);
    memory_service.import_backup(backup)
}

fn health_item(
    id: &str,
    title: &str,
    status: &str,
    severity: &str,
    detail: impl Into<String>,
) -> HealthCheckItem {
    HealthCheckItem {
        id: id.to_string(),
        title: title.to_string(),
        status: status.to_string(),
        severity: severity.to_string(),
        detail: detail.into(),
    }
}

fn roadmap_status_from_runtime(runtime: &RuntimeState) -> AgentRoadmapStatus {
    let config = &runtime.agent_roadmap;
    AgentRoadmapStatus {
        generated_at: now_millis(),
        plugin_tools: RoadmapCapabilityStatus {
            enabled: config.plugin_tools.enabled,
            status: if config.plugin_tools.enabled { "ready" } else { "disabled" }.to_string(),
            summary: if config.plugin_tools.enabled {
                "插件/MCP 工具层已启用，外部工具仍会按权限级别确认。"
            } else {
                "插件/MCP 工具层未启用。"
            }
            .to_string(),
            detail: vec![
                format!("MCP 网关：{}", if config.plugin_tools.mcp_gateway_enabled { "启用" } else { "关闭" }),
                format!("MCP Server：{} 个（启用 {} 个）", config.plugin_tools.mcp_servers.len(), config.plugin_tools.mcp_servers.iter().filter(|server| server.enabled).count()),
                format!("工具目录：{} 个", config.plugin_tools.tool_dirs.len()),
            ],
        },
        document_memory: RoadmapCapabilityStatus {
            enabled: config.document_memory.enabled,
            status: if config.document_memory.enabled { "ready" } else { "disabled" }.to_string(),
            summary: if config.document_memory.enabled {
                "文档导入知识库已启用，可把 Markdown 文本写入本地文档库和长期记忆。"
            } else {
                "文档导入知识库未启用。"
            }
            .to_string(),
            detail: vec![
                format!("监听目录：{} 个", config.document_memory.watched_dirs.len()),
                format!("写入长期记忆：{}", if config.document_memory.write_to_long_term_memory { "启用" } else { "关闭" }),
            ],
        },
        browser_automation: RoadmapCapabilityStatus {
            enabled: config.browser_automation.enabled,
            status: if config.browser_automation.enabled { "ready" } else { "disabled" }.to_string(),
            summary: if config.browser_automation.enabled {
                "浏览器自动化入口已启用，高风险网页操作会进入确认流。"
            } else {
                "浏览器自动化入口未启用。"
            }
            .to_string(),
            detail: vec![
                format!("投研网页抓取：{}", if config.browser_automation.allow_research_fetch { "允许" } else { "关闭" }),
                format!("允许域名：{} 个", config.browser_automation.allowed_domains.len()),
            ],
        },
        local_voice_loop: RoadmapCapabilityStatus {
            enabled: config.local_voice_loop.enabled,
            status: if config.local_voice_loop.enabled { "ready" } else { "disabled" }.to_string(),
            summary: if config.local_voice_loop.enabled {
                "本地语音闭环已启用，可与 Whisper 输入和本地语音回复策略联动。"
            } else {
                "本地语音闭环未启用。"
            }
            .to_string(),
            detail: vec![
                format!("唤醒词：{}", config.local_voice_loop.wake_phrase),
                format!("本地 TTS：{}", if config.local_voice_loop.local_tts_enabled { "启用" } else { "关闭" }),
            ],
        },
    }
}

#[tauri::command]
fn get_agent_roadmap_status(
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<AgentRoadmapStatus, String> {
    let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    Ok(roadmap_status_from_runtime(&runtime))
}

fn agent_roadmap_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("获取应用数据目录失败: {error}"))?
        .join("agent-roadmap");
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建 Agent 路线目录失败: {error}"))?;
    Ok(dir)
}

fn document_memory_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = agent_roadmap_dir(app)?.join("documents");
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建文档库目录失败: {error}"))?;
    Ok(dir)
}

fn browser_task_queue_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(agent_roadmap_dir(app)?.join("browser-tasks.json"))
}

fn load_browser_task_queue(app: &AppHandle) -> Result<Vec<StoredBrowserAutomationTask>, String> {
    let path = browser_task_queue_path(app)?;
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str::<Vec<StoredBrowserAutomationTask>>(&content)
            .map_err(|error| format!("解析浏览器任务队列失败: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("读取浏览器任务队列失败: {error}")),
    }
}

fn save_browser_task_queue(
    app: &AppHandle,
    tasks: &[StoredBrowserAutomationTask],
) -> Result<(), String> {
    let content = serde_json::to_string_pretty(tasks)
        .map_err(|error| format!("序列化浏览器任务失败: {error}"))?;
    std::fs::write(browser_task_queue_path(app)?, content)
        .map_err(|error| format!("保存浏览器任务队列失败: {error}"))
}

fn safe_document_filename(title: &str, stamp: u64) -> String {
    let mut name = title
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else if ch.is_whitespace() {
                '-'
            } else {
                '_'
            }
        })
        .collect::<String>();
    name = name.trim_matches(['-', '_', '.']).to_string();
    if name.is_empty() {
        name = "document".to_string();
    }
    format!("{stamp}-{name}.md")
}

fn first_markdown_heading(content: &str) -> Option<String> {
    content
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(str::trim))
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
}

fn plugin_manifest_from_path(path: &std::path::Path) -> Option<PluginToolManifest> {
    let name = path.file_stem()?.to_string_lossy().to_string();
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let kind = match extension.as_str() {
        "json" => "manifest",
        "py" => "python",
        "ps1" => "powershell",
        "js" | "ts" => "node",
        _ => return None,
    };
    Some(PluginToolManifest {
        id: format!("tool-{name}"),
        name,
        path: path.to_string_lossy().to_string(),
        kind: kind.to_string(),
        enabled: true,
        summary: format!("从本地工具目录发现 {kind} 工具。"),
    })
}

fn mcp_tool_manifest(
    server: &app_state::McpServerEntry,
    tool: &serde_json::Value,
) -> Option<PluginToolManifest> {
    let name = tool.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    if !server.tool_allowlist.is_empty()
        && !server
            .tool_allowlist
            .iter()
            .any(|allowed| allowed == name || name.starts_with(&format!("{allowed}.")))
    {
        return None;
    }
    let summary = tool
        .get("description")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("MCP server 提供的工具。");

    Some(PluginToolManifest {
        id: format!("mcp-{}-{name}", server.id),
        name: format!("{} / {name}", server.name),
        path: format!("mcp://{}/{}", server.id, name),
        kind: "mcp".to_string(),
        enabled: server.enabled,
        summary: summary.to_string(),
    })
}

fn write_mcp_jsonrpc(
    stdin: &mut std::process::ChildStdin,
    id: u64,
    method: &str,
    params: serde_json::Value,
) -> Result<(), String> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    writeln!(stdin, "{request}").map_err(|error| format!("发送 MCP 请求失败: {error}"))?;
    stdin
        .flush()
        .map_err(|error| format!("刷新 MCP 请求失败: {error}"))
}

fn read_mcp_jsonrpc(
    reader: &mut BufReader<std::process::ChildStdout>,
    expected_id: u64,
) -> Result<serde_json::Value, String> {
    let mut line = String::new();
    for _ in 0..120 {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| format!("读取 MCP 响应失败: {error}"))?;
        if read == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value = match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("id").and_then(|id| id.as_u64()) != Some(expected_id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(format!("MCP 返回错误: {error}"));
        }
        return Ok(value.get("result").cloned().unwrap_or(serde_json::Value::Null));
    }
    Err("等待 MCP 响应超时或连接已关闭。".to_string())
}

fn scan_mcp_server_tools(
    server: &app_state::McpServerEntry,
) -> Result<Vec<PluginToolManifest>, String> {
    if !server.enabled {
        return Ok(Vec::new());
    }
    if server.transport != "stdio" {
        return Err(format!("{} 暂不支持 {} transport。", server.name, server.transport));
    }

    let mut command = Command::new(&server.command);
    command
        .args(&server.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for item in &server.env {
        if let Some((key, value)) = item.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                command.env(key, value.trim());
            }
        }
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 MCP Server {} 失败: {error}", server.name))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("{} 无法打开 stdin。", server.name))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{} 无法打开 stdout。", server.name))?;
    let mut reader = BufReader::new(stdout);

    let init_params = serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {
            "name": "PenguinPal",
            "version": env!("CARGO_PKG_VERSION")
        }
    });
    write_mcp_jsonrpc(&mut stdin, 1, "initialize", init_params)?;
    let _ = read_mcp_jsonrpc(&mut reader, 1)?;
    write_mcp_jsonrpc(&mut stdin, 2, "tools/list", serde_json::json!({}))?;
    let result = read_mcp_jsonrpc(&mut reader, 2)?;
    let tools = result
        .get("tools")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|tool| mcp_tool_manifest(server, tool))
        .collect::<Vec<_>>();

    let _ = child.kill();
    let _ = child.wait();
    Ok(tools)
}

fn call_mcp_server_tool(
    server: &app_state::McpServerEntry,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !server.read_only {
        return Err(format!(
            "{} 不是只读 MCP Server，暂不允许绕过确认流直接调用。",
            server.name
        ));
    }

    call_mcp_server_tool_confirmed(server, tool_name, arguments)
}

fn call_mcp_server_tool_confirmed(
    server: &app_state::McpServerEntry,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !server.enabled {
        return Err(format!("MCP Server {} 未启用。", server.name));
    }
    if server.transport != "stdio" {
        return Err(format!("{} 暂不支持 {} transport。", server.name, server.transport));
    }
    if !server.tool_allowlist.is_empty()
        && !server
            .tool_allowlist
            .iter()
            .any(|allowed| allowed == tool_name || tool_name.starts_with(&format!("{allowed}.")))
    {
        return Err(format!("{tool_name} 不在 {} 的工具 allowlist 中。", server.name));
    }

    call_mcp_server_tool_unchecked(server, tool_name, arguments)
}

fn call_mcp_server_tool_unchecked(
    server: &app_state::McpServerEntry,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut command = Command::new(&server.command);
    command
        .args(&server.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for item in &server.env {
        if let Some((key, value)) = item.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                command.env(key, value.trim());
            }
        }
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 MCP Server {} 失败: {error}", server.name))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("{} 无法打开 stdin。", server.name))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("{} 无法打开 stdout。", server.name))?;
    let mut reader = BufReader::new(stdout);

    write_mcp_jsonrpc(
        &mut stdin,
        1,
        "initialize",
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "PenguinPal",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )?;
    let _ = read_mcp_jsonrpc(&mut reader, 1)?;
    write_mcp_jsonrpc(
        &mut stdin,
        2,
        "tools/call",
        serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        }),
    )?;
    let result = read_mcp_jsonrpc(&mut reader, 2)?;
    let _ = child.kill();
    let _ = child.wait();
    Ok(result)
}

#[tauri::command]
fn invoke_mcp_tool(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    server_id: String,
    tool_name: String,
    arguments: serde_json::Value,
) -> Result<ToolInvokeResponse, String> {
    let (server, require_confirmation) = {
        let runtime = state
            .lock()
            .map_err(|_| "助手状态锁定失败".to_string())?
            .clone();
        if !runtime.agent_roadmap.plugin_tools.enabled
            || !runtime.agent_roadmap.plugin_tools.mcp_gateway_enabled
        {
            return Err("请先在设置中启用插件/MCP 工具层和 MCP 网关。".to_string());
        }
        let server = runtime
            .agent_roadmap
            .plugin_tools
            .mcp_servers
            .iter()
            .find(|item| item.id == server_id)
            .cloned()
            .ok_or_else(|| format!("未找到 MCP Server：{server_id}"))?;
        (server, runtime.agent_roadmap.plugin_tools.require_confirmation)
    };

    if require_confirmation && !server.read_only {
        let pending_request = build_mcp_pending_request(&server, &tool_name, arguments);
        insert_mcp_pending(&app, pending_request.clone())?;
        return Ok(ToolInvokeResponse {
            status: "pending_confirmation".to_string(),
            result: None,
            message: Some("MCP 写操作需要确认后执行。".to_string()),
            pending_request: Some(pending_request),
            error: None,
        });
    }

    let result = call_mcp_server_tool(&server, &tool_name, arguments)?;
    {
        let mut runtime = state
            .lock()
            .map_err(|_| "助手状态锁定失败".to_string())?;
        audit::push_entry(
            &mut runtime.audit_trail,
            audit::record(
                "mcp_tool_call",
                "ok",
                format!("server={} tool={} readOnly={}", server.id, tool_name, server.read_only),
                if server.read_only { 1 } else { 3 },
            ),
        );
        let _ = save(&app, &runtime);
    }

    Ok(ToolInvokeResponse {
        status: "success".to_string(),
        result: Some(result),
        message: Some(format!("已调用 MCP 工具：{} / {}", server.name, tool_name)),
        pending_request: None,
        error: None,
    })
}

#[tauri::command]
fn diagnose_mcp_gateway(
    config: app_state::PluginToolsConfig,
) -> Result<McpGatewayDiagnosis, String> {
    if !config.enabled || !config.mcp_gateway_enabled {
        return Ok(McpGatewayDiagnosis {
            ok: false,
            message: "先打开“插件/MCP 工具层”和“MCP 网关”，再点检测。".to_string(),
            tools: Vec::new(),
            warnings: Vec::new(),
        });
    }

    let enabled_servers = config
        .mcp_servers
        .iter()
        .filter(|server| server.enabled)
        .cloned()
        .collect::<Vec<_>>();
    if enabled_servers.is_empty() {
        return Ok(McpGatewayDiagnosis {
            ok: false,
            message: "还没有启用任何 MCP Server。先打开一个 Server，例如 GitHub MCP Server。".to_string(),
            tools: Vec::new(),
            warnings: Vec::new(),
        });
    }

    let mut tools = Vec::new();
    let mut warnings = Vec::new();
    for server in &enabled_servers {
        if server.id == "github-mcp"
            && server.env.iter().any(|item| {
                item.trim() == "GITHUB_PERSONAL_ACCESS_TOKEN="
                    || item.trim().eq_ignore_ascii_case("GITHUB_PERSONAL_ACCESS_TOKEN")
            })
        {
            warnings.push(format!(
                "{}：请先填写 GITHUB_PERSONAL_ACCESS_TOKEN。只读查询建议使用 repo/read:org 等最小权限。",
                server.name
            ));
            continue;
        }
        match scan_mcp_server_tools(server) {
            Ok(mut discovered) => tools.append(&mut discovered),
            Err(error) => {
                let lowered = error.to_lowercase();
                let hint = if server.command == "docker" && lowered.contains("program not found") {
                    "未找到 Docker 命令。请先安装并启动 Docker Desktop。".to_string()
                } else if server.command == "python" && lowered.contains("program not found") {
                    "未找到 Python 命令。请先安装 Python，或把 python 加入 PATH。".to_string()
                } else {
                    error
                };
                warnings.push(format!("{}：{}", server.name, hint));
            }
        }
    }

    let ok = !tools.is_empty();
    let message = if ok {
        format!(
            "MCP 已连通，发现 {} 个可用工具。之后你可以直接在聊天里说要查 GitHub 仓库、Issue 或 PR，桌宠会按需调用。",
            tools.len()
        )
    } else if warnings.is_empty() {
        "MCP Server 已启动，但没有返回可用工具。".to_string()
    } else {
        "MCP 还没连通。请看下面的失败原因，通常是 server 命令未安装、Token 未填写，或命令参数不对。".to_string()
    };

    Ok(McpGatewayDiagnosis {
        ok,
        message,
        tools,
        warnings,
    })
}

#[tauri::command]
fn setup_docker_desktop() -> Result<DockerSetupResult, String> {
    if Command::new("docker")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        let started = start_docker_desktop().unwrap_or(false);
        return Ok(DockerSetupResult {
            status: if started { "started" } else { "installed" }.to_string(),
            message: if started {
                "Docker 已安装，已尝试启动 Docker Desktop。启动完成后再点一键检测 MCP。".to_string()
            } else {
                "Docker 已安装。如果 Docker Desktop 还没启动，请从开始菜单打开它。".to_string()
            },
        });
    }

    if Command::new("winget")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
    {
        #[cfg(target_os = "windows")]
        {
            let mut command = Command::new("cmd");
            command.args([
                "/C",
                "start",
                "Docker Desktop Installer",
                "powershell",
                "-NoExit",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "Write-Host 'Installing Docker Desktop for PenguinPal...'; winget install --id Docker.DockerDesktop -e --accept-package-agreements --accept-source-agreements; Write-Host ''; Write-Host 'If installation finished, start Docker Desktop from the Start Menu, then click MCP detection again.'",
            ]);
            command
                .spawn()
                .map_err(|error| format!("启动 Docker Desktop 安装窗口失败: {error}"))?;
        }
        #[cfg(not(target_os = "windows"))]
        {
            let mut command = Command::new("winget");
            command.args([
                "install",
                "--id",
                "Docker.DockerDesktop",
                "-e",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ]);
            command
                .spawn()
                .map_err(|error| format!("启动 Docker Desktop 安装失败: {error}"))?;
        }
        return Ok(DockerSetupResult {
            status: "installing".to_string(),
            message: "已打开 Docker Desktop 安装窗口。请查看任务栏或新 PowerShell 窗口；安装后可能需要重启电脑或启动 Docker Desktop。".to_string(),
        });
    }

    Err("未找到 Docker，也未找到 winget。请手动安装 Docker Desktop 后再检测 MCP。".to_string())
}

fn start_docker_desktop() -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let candidates = [
            r"C:\Program Files\Docker\Docker\Docker Desktop.exe",
            r"C:\Program Files\Docker\Docker\Docker Desktop Installer.exe",
        ];
        for candidate in candidates {
            if std::path::Path::new(candidate).is_file() {
                Command::new(candidate)
                    .spawn()
                    .map_err(|error| format!("启动 Docker Desktop 失败: {error}"))?;
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[tauri::command]
fn scan_plugin_tools(
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<PluginToolScanResult, String> {
    let runtime = state
        .lock()
        .map_err(|_| "助手状态锁定失败".to_string())?
        .clone();
    if !runtime.agent_roadmap.plugin_tools.enabled {
        return Err("请先在设置中启用插件/MCP 工具层。".to_string());
    }

    let mut tools = Vec::new();
    let mut warnings = Vec::new();
    for dir in &runtime.agent_roadmap.plugin_tools.tool_dirs {
        let path = std::path::PathBuf::from(dir);
        if !path.is_dir() {
            warnings.push(format!("工具目录不可用：{dir}"));
            continue;
        }
        match std::fs::read_dir(&path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_file() {
                        if let Some(tool) = plugin_manifest_from_path(&entry_path) {
                            tools.push(tool);
                        }
                    } else if entry_path.is_dir() {
                        let manifest = entry_path.join("plugin.json");
                        if manifest.exists() {
                            if let Some(tool) = plugin_manifest_from_path(&manifest) {
                                tools.push(tool);
                            }
                        }
                    }
                }
            }
            Err(error) => warnings.push(format!("读取工具目录失败：{dir} / {error}")),
        }
    }

    if runtime.agent_roadmap.plugin_tools.mcp_gateway_enabled {
        for server in &runtime.agent_roadmap.plugin_tools.mcp_servers {
            match scan_mcp_server_tools(server) {
                Ok(mut discovered) => tools.append(&mut discovered),
                Err(error) => warnings.push(error),
            }
        }
    }

    Ok(PluginToolScanResult {
        scanned_at: now_millis(),
        directories: runtime.agent_roadmap.plugin_tools.tool_dirs,
        tools,
        warnings,
    })
}

#[tauri::command]
fn list_document_memory(app: AppHandle) -> Result<Vec<DocumentMemoryRecord>, String> {
    let dir = document_memory_dir(&app)?;
    let mut records = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|error| format!("读取文档库失败: {error}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let file_stem = path
            .file_stem()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| "document".to_string());
        let imported_at = file_stem
            .split('-')
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or_default();
        let title = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| first_markdown_heading(&content))
            .unwrap_or(file_stem);
        records.push(DocumentMemoryRecord {
            id: format!("doc-{imported_at}"),
            title,
            path: path.to_string_lossy().to_string(),
            imported_at,
            size_bytes: metadata.len(),
        });
    }
    records.sort_by(|left, right| right.imported_at.cmp(&left.imported_at));
    Ok(records)
}

#[tauri::command]
fn import_document_memory(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    title: String,
    content: String,
) -> Result<DocumentImportResult, String> {
    let runtime = state
        .lock()
        .map_err(|_| "助手状态锁定失败".to_string())?
        .clone();
    if !runtime.agent_roadmap.document_memory.enabled {
        return Err("请先在设置中启用文档导入知识库。".to_string());
    }
    let trimmed_content = content.trim();
    if trimmed_content.is_empty() {
        return Err("文档内容不能为空。".to_string());
    }
    let stamp = now_millis();
    let title = if title.trim().is_empty() {
        first_markdown_heading(trimmed_content).unwrap_or_else(|| "未命名文档".to_string())
    } else {
        title.trim().to_string()
    };
    let path = document_memory_dir(&app)?.join(safe_document_filename(&title, stamp));
    std::fs::write(&path, format!("# {title}\n\n{trimmed_content}\n"))
        .map_err(|error| format!("写入文档失败: {error}"))?;

    let mut memory_written = false;
    if runtime.agent_roadmap.document_memory.write_to_long_term_memory {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("获取应用数据目录失败: {error}"))?;
        let memory_service = crate::memory::MemoryService::new(&app_data);
        let excerpt = trimmed_content.chars().take(800).collect::<String>();
        memory_service.store().upsert_semantic_entry(memory::SemanticEntry {
            id: memory::generate_id("document_memory"),
            memory_key: format!("document:{}", title.to_lowercase()),
            topic: title.clone(),
            knowledge: excerpt,
            source_type: "document_import".to_string(),
            confidence: 0.82,
            created_at: stamp,
            updated_at: stamp,
            tags: vec!["document".to_string(), "knowledge_base".to_string()],
            explicit: true,
            mention_count: 2,
            ttl: None,
            status: memory::MemoryStatus::Active,
            conflict_group: None,
        })?;
        memory_written = true;
    }

    Ok(DocumentImportResult {
        id: format!("doc-{stamp}"),
        title,
        path: path.to_string_lossy().to_string(),
        memory_written,
        message: if memory_written {
            "文档已保存，并写入长期记忆。".to_string()
        } else {
            "文档已保存到本地知识库。".to_string()
        },
    })
}

#[tauri::command]
fn list_browser_automation_tasks(
    app: AppHandle,
) -> Result<Vec<StoredBrowserAutomationTask>, String> {
    load_browser_task_queue(&app)
}

#[tauri::command]
fn create_browser_automation_task(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    goal: String,
    url: Option<String>,
) -> Result<BrowserAutomationTask, String> {
    let runtime = state
        .lock()
        .map_err(|_| "助手状态锁定失败".to_string())?
        .clone();
    if !runtime.agent_roadmap.browser_automation.enabled {
        return Err("请先在设置中启用浏览器自动化入口。".to_string());
    }
    let goal = goal.trim().to_string();
    if goal.is_empty() {
        return Err("浏览器任务目标不能为空。".to_string());
    }
    let created_at = now_millis();
    let task = StoredBrowserAutomationTask {
        id: format!("browser-task-{created_at}"),
        goal,
        url: normalize_optional(url),
        status: if runtime.agent_roadmap.browser_automation.require_confirmation {
            "waitingConfirmation".to_string()
        } else {
            "planned".to_string()
        },
        requires_confirmation: runtime.agent_roadmap.browser_automation.require_confirmation,
        message: "已创建浏览器自动化任务草案；下一步可接 browser-use/Playwright 执行器。".to_string(),
        created_at,
    };
    let mut tasks = load_browser_task_queue(&app)?;
    tasks.insert(0, task.clone());
    tasks.truncate(50);
    save_browser_task_queue(&app, &tasks)?;
    Ok(BrowserAutomationTask {
        id: task.id,
        goal: task.goal,
        url: task.url,
        status: task.status,
        requires_confirmation: task.requires_confirmation,
        message: task.message,
        created_at: task.created_at,
    })
}

fn build_health_check_report(
    app: &AppHandle,
    runtime: &RuntimeState,
    transcriber: Option<&TranscriberService>,
) -> HealthCheckReport {
    let mut items = Vec::new();
    let codex = inspect_codex_cli_status_unverified(app);
    items.push(if codex.installed && codex.credential_present {
        health_item("codex", "Codex CLI", "ok", "info", codex.message)
    } else if codex.installed {
        health_item("codex", "Codex CLI", "warning", "watch", codex.message)
    } else {
        health_item("codex", "Codex CLI", "error", "urgent", codex.message)
    });

    match transcriber {
        Some(service) => {
            let status = service.get_status();
            items.push(if status.model_loaded {
                health_item("whisper_model", "Whisper 模型", "ok", "info", "Whisper 模型已加载。")
            } else {
                health_item("whisper_model", "Whisper 模型", "warning", "watch", "Whisper 模型未加载。")
            });
            items.push(if status.input_ready {
                health_item("microphone", "麦克风", "ok", "info", "检测到可用输入设备。")
            } else {
                health_item(
                    "microphone",
                    "麦克风",
                    "error",
                    "urgent",
                    status.input_message.unwrap_or_else(|| "未检测到可用输入设备。".to_string()),
                )
            });
        }
        None => items.push(health_item(
            "whisper_model",
            "Whisper 服务",
            "error",
            "urgent",
            "Whisper 服务未初始化。",
        )),
    }

    let roadmap = roadmap_status_from_runtime(runtime);
    items.push(health_item("plugin_tools", "插件/MCP 工具层", if roadmap.plugin_tools.enabled { "ok" } else { "warning" }, "watch", roadmap.plugin_tools.summary));
    items.push(health_item("document_memory", "文档导入知识库", if roadmap.document_memory.enabled { "ok" } else { "warning" }, "watch", roadmap.document_memory.summary));
    items.push(health_item("browser_automation", "浏览器自动化", if roadmap.browser_automation.enabled { "ok" } else { "warning" }, "watch", roadmap.browser_automation.summary));
    items.push(health_item("local_voice_loop", "本地语音闭环", if roadmap.local_voice_loop.enabled { "ok" } else { "warning" }, "watch", roadmap.local_voice_loop.summary));

    if let Ok(app_data) = app.path().app_data_dir() {
        let memory_service = crate::memory::MemoryService::new(&app_data);
        match memory_service.management_snapshot() {
            Ok(snapshot) => items.push(health_item(
                "memory",
                "长期记忆",
                "ok",
                "info",
                format!("稳定 {} 条，候选 {} 条，冲突 {} 组。", snapshot.stats.stable_count, snapshot.stats.candidate_count, snapshot.stats.conflict_count),
            )),
            Err(error) => items.push(health_item("memory", "长期记忆", "error", "urgent", error)),
        }
    }

    let overall_status = if items.iter().any(|item| item.status == "error") {
        "error"
    } else if items.iter().any(|item| item.status == "warning") {
        "warning"
    } else {
        "ok"
    }.to_string();

    HealthCheckReport {
        generated_at: now_millis(),
        overall_status,
        items,
    }
}

#[tauri::command]
fn run_health_check(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<HealthCheckReport, String> {
    let runtime = state
        .lock()
        .map_err(|_| "助手状态锁定失败".to_string())?
        .clone();
    let transcriber = app.try_state::<TranscriberService>();
    Ok(build_health_check_report(&app, &runtime, transcriber.as_deref()))
}

#[tauri::command]
fn clear_today_reply_history(app: AppHandle) -> Result<Vec<ReplyHistoryEntry>, String> {
    history::clear_today_reply_history(&app)
}

#[tauri::command]
async fn get_research_brief_snapshot(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
) -> Result<research::ResearchBriefSnapshot, String> {
    let runtime = state
        .lock()
        .map_err(|_| "助手状态锁定失败".to_string())?
        .clone();
    research::build_brief(&app, &runtime).await
}

#[tauri::command]
fn acknowledge_research_brief(
    app: AppHandle,
    state: State<'_, Mutex<RuntimeState>>,
    day_key: String,
    alert_fingerprint: Option<String>,
    mark_startup_popup: Option<bool>,
) -> Result<bool, String> {
    let mut runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
    research::acknowledge_brief(
        &app,
        &mut runtime,
        &day_key,
        alert_fingerprint.as_deref().unwrap_or(""),
        mark_startup_popup.unwrap_or(false),
    )?;
    Ok(true)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .manage(Mutex::new(RuntimeState::default()))
        .manage(ControlServiceState::new())
        .manage(AgentTaskState::new());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_autostart::init(
        tauri_plugin_autostart::MacosLauncher::LaunchAgent,
        None::<Vec<&str>>,
    ));

    builder
        .setup(|app| {
            eprintln!("[Setup] Starting application setup...");

            // 更健壮的状态加载：如果加载失败则使用默认状态
            let runtime = match load(&app.handle()) {
                Ok(runtime) => {
                    eprintln!("[Setup] RuntimeState loaded successfully");
                    runtime
                }
                Err(error) => {
                    eprintln!("[Setup] Failed to load runtime state: {}, using default", error);
                    RuntimeState::default()
                }
            };
            let initial_window_position = runtime.main_window_position.clone();
            let runtime_state: State<'_, Mutex<RuntimeState>> = app.state();
            if let Ok(mut current) = runtime_state.lock() {
                *current = runtime;
            }
            eprintln!("[Setup] RuntimeState managed");
            eprintln!("[Setup] ControlServiceState managed");
            eprintln!("[Setup] AgentTaskState managed");

            // 初始化 Whisper 语音识别服务
            let app_data_dir = app.path().app_data_dir()
                .map_err(|e| {
                    eprintln!("[Setup] Failed to get app_data_dir: {}", e);
                    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
                })?;
            eprintln!("[Setup] app_data_dir: {:?}", app_data_dir);
            match TranscriberService::new(app_data_dir) {
                Ok(transcriber) => {
                    app.manage(transcriber);
                    eprintln!("Whisper transcriber service initialized");
                }
                Err(e) => {
                    eprintln!("Failed to initialize Whisper transcriber: {}", e);
                }
            }

            let _ = history::prepare_storage(&app.handle());

            // 初始化 Codex 配置目录结构
            if let Err(error) = initialize_codex_config(&app.handle()) {
                eprintln!("Codex config initialization failed: {error}");
            }

            let runtime_state: State<'_, Mutex<RuntimeState>> = app.state();
            if let Ok(mut runtime) = runtime_state.lock() {
                if let Err(error) = session_thread::bootstrap_runtime_thread(&app.handle(), &mut runtime) {
                    eprintln!("Session thread bootstrap failed: {error}");
                }
                if let Err(error) = configure_builtin_mcp_defaults(&app.handle(), &mut runtime) {
                    eprintln!("MCP bootstrap failed: {error}");
                }
                sync_codex_provider_view(&app.handle(), &mut runtime);
                match sync_launch_at_startup(&app.handle(), runtime.launch_at_startup) {
                    Ok(enabled) => runtime.launch_at_startup = enabled,
                    Err(error) => eprintln!("Autostart sync failed: {error}"),
                }
                if let Err(error) = sync_whisper_input_shortcut(&app.handle(), None, &runtime) {
                    eprintln!("Whisper push-to-talk shortcut sync failed: {error}");
                }
                let _ = save(&app.handle(), &runtime);
            }

            let auto_update_enabled = {
                let runtime_state: State<'_, Mutex<RuntimeState>> = app.state();
                runtime_state
                    .lock()
                    .map(|runtime| runtime.auto_update_codex)
                    .unwrap_or(true)
            };

            if auto_update_enabled {
                // 异步检查并自动更新 Codex
                let app_handle_for_update = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = auto_update_codex(&app_handle_for_update).await {
                        eprintln!("Codex auto-update failed: {e}");
                    }
                });
            } else {
                eprintln!("[Codex] 已关闭启动时自动更新。");
            }

            let control_service_status = match control::http::start(app.handle().clone()) {
                Ok(address) => {
                    eprintln!("PenguinPal local control service listening on {address}");
                    ("ok", format!("控制服务已启动：{address}"), 1u8)
                }
                Err(error) => {
                    eprintln!("PenguinPal local control service failed to start: {error}");
                    ("error", format!("控制服务启动失败：{error}"), 1u8)
                }
            };

            let runtime_state: State<'_, Mutex<RuntimeState>> = app.state();
            if let Ok(mut runtime) = runtime_state.lock() {
                audit::push_entry(
                    &mut runtime.audit_trail,
                    audit::record(
                        "control_service_startup",
                        control_service_status.0,
                        control_service_status.1,
                        control_service_status.2,
                    ),
                );
                let _ = save(&app.handle(), &runtime);
            }

            let tray_icon = tray::create_tray(app)?;
            app.manage(tray_icon);

            if let Some(window) = app.get_webview_window("main") {
                window::setup_window(&window, initial_window_position.as_ref())?;
            }
            if let Some(window) = app.get_webview_window("settings") {
                hide_instead_of_close(&window);
            }
            if let Some(window) = app.get_webview_window("research") {
                hide_instead_of_close(&window);
            }

            // 启动三层架构维护后台任务（记忆 + 规则 + 权限）
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                // 启动时执行一次维护
                if let Ok(app_data) = app_handle.path().app_data_dir() {
                    let behavior_state = shell_agent::BehaviorState::new(&app_data);
                    let result = behavior_state.run_maintenance();
                    if result.total_changes() > 0 {
                        eprintln!(
                            "Behavior maintenance: memory(decayed={}, merged={}, pruned={}), rules_generated={}",
                            result.memory_decayed, result.memory_merged, result.memory_pruned,
                            result.rules_generated
                        );
                    }
                }

                // 每小时执行一次维护（分段 sleep 以便响应 shutdown 信号）
                const MAINTENANCE_INTERVAL_SECS: u64 = 3600;
                const SLEEP_CHECK_INTERVAL_SECS: u64 = 60;

                while !MEMORY_MAINTENANCE_SHUTDOWN.load(Ordering::Relaxed) {
                    // 分段 sleep，每 60 秒检查一次 shutdown 信号
                    for _ in 0..(MAINTENANCE_INTERVAL_SECS / SLEEP_CHECK_INTERVAL_SECS) {
                        if MEMORY_MAINTENANCE_SHUTDOWN.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(SLEEP_CHECK_INTERVAL_SECS));
                    }

                    if MEMORY_MAINTENANCE_SHUTDOWN.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Ok(app_data) = app_handle.path().app_data_dir() {
                        let behavior_state = shell_agent::BehaviorState::new(&app_data);
                        let result = behavior_state.run_maintenance();
                        if result.total_changes() > 0 {
                            eprintln!(
                                "Behavior maintenance: memory(decayed={}, merged={}, pruned={}), rules_generated={}",
                                result.memory_decayed, result.memory_merged, result.memory_pruned,
                                result.rules_generated
                            );
                        }
                    }
                }
                eprintln!("Behavior maintenance thread stopped.");
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            show_settings_window,
            hide_settings_window,
            show_research_window,
            hide_research_window,
            hide_main_window,
            start_main_window_drag,
            remember_main_window_position,
            get_assistant_snapshot,
            save_provider_config,
            start_oauth_sign_in,
            start_oauth_sign_in_auto,
            complete_oauth_sign_in,
            disconnect_oauth_sign_in,
            get_codex_cli_status,
            check_codex_update,
            check_app_update,
            update_codex,
            open_app_update_download,
            get_control_service_status,
            confirm_control_pending,
            cancel_control_pending,
            start_codex_cli_login,
            restart_codex_cli_login,
            send_chat_message,
            request_desktop_action,
            confirm_desktop_action,
            cancel_desktop_action_approval,
            clear_conversation,
            get_input_history,
            get_today_reply_history,
            get_archive_recall_preview,
            get_memory_management_snapshot,
            delete_managed_memory,
            promote_memory_candidate,
            resolve_memory_conflict,
            run_memory_maintenance,
            export_memory_backup,
            import_memory_backup,
            run_health_check,
            get_agent_roadmap_status,
            scan_plugin_tools,
            diagnose_mcp_gateway,
            setup_docker_desktop,
            invoke_mcp_tool,
            list_document_memory,
            import_document_memory,
            list_browser_automation_tasks,
            create_browser_automation_task,
            clear_today_reply_history,
            get_research_brief_snapshot,
            acknowledge_research_brief,
            // Shell Agent 权限管理
            get_shell_permissions,
            grant_shell_permission,
            revoke_shell_permission,
            grant_basic_shell_access,
            // Whisper 语音识别
            get_whisper_status,
            get_whisper_models,
            download_whisper_model,
            load_whisper_model,
            unload_whisper_model,
            delete_whisper_model,
            start_whisper_recording,
            stop_whisper_recording,
            get_whisper_recording_state
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::ExitRequested { .. } | RunEvent::Exit => {
                tray::remove_tray(app);
            }
            _ => {}
        });
}
