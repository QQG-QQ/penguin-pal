use crate::app_state::{
    AiConstraintItem, AiConstraintProfile, AuthMode, DesktopAction, ProviderConfig, ProviderKind,
};

const PROFILE_LABEL: &str = "Shell Agent Mode";
const PROFILE_VERSION: &str = "2026-03-17";

fn item(id: &str, title: &str, summary: String, status: &str) -> AiConstraintItem {
    AiConstraintItem {
        id: id.to_string(),
        title: title.to_string(),
        summary,
        status: status.to_string(),
    }
}

fn enabled_actions(actions: &[DesktopAction]) -> Vec<&DesktopAction> {
    actions.iter().filter(|action| action.enabled).collect()
}

fn disabled_actions(actions: &[DesktopAction]) -> Vec<&DesktopAction> {
    actions.iter().filter(|action| !action.enabled).collect()
}

fn join_action_titles(actions: &[&DesktopAction]) -> String {
    if actions.is_empty() {
        "无".to_string()
    } else {
        actions
            .iter()
            .map(|action| action.title.as_str())
            .collect::<Vec<_>>()
            .join("、")
    }
}

pub fn build_profile(
    provider: &ProviderConfig,
    permission_level: u8,
    allowed_actions: &[DesktopAction],
) -> AiConstraintProfile {
    let enabled = enabled_actions(allowed_actions);
    let disabled = disabled_actions(allowed_actions);
    let approval_required = enabled
        .iter()
        .copied()
        .filter(|action| action.requires_confirmation)
        .collect::<Vec<_>>();

    let immutable_rules = vec![
        item(
            "shell-agent-mode",
            "Shell Agent 自主模式",
            "AI 通过 shell 命令自主操作电脑，高风险命令需用户确认。".to_string(),
            "已启用",
        ),
        item(
            "high-risk-confirmation",
            "高风险命令确认",
            "删除文件、注册表修改、网络请求、执行程序等高风险操作需要用户确认。".to_string(),
            "强制",
        ),
        item(
            "forbidden-commands",
            "禁止命令",
            "格式化系统盘、删除系统目录等破坏性操作被永久禁止。".to_string(),
            "硬限制",
        ),
        item(
            "privacy-first",
            "隐私保护",
            "AI 不会主动上传、暴露 API Key、密码、私人文件等敏感数据。".to_string(),
            "硬限制",
        ),
        item(
            "step-limit",
            "步数上限",
            "系统保护上限 100 步，防止无限循环。".to_string(),
            "硬限制",
        ),
    ];

    let capability_gates = vec![
        item(
            "chat",
            "对话陪伴",
            "允许正常对话、解释风险、提供建议和把设置命令路由到受控入口。".to_string(),
            "可用",
        ),
        item(
            "model-gateway",
            "模型网关访问",
            if provider.allow_network {
                format!(
                    "已允许访问 {} 模型网关，但请求仍然只能发往当前配置的 provider/base URL。",
                    provider.kind.label()
                )
            } else {
                "当前处于离线安全模式，外部模型 API 和 OAuth token exchange 都被阻止。"
                    .to_string()
            },
            if provider.allow_network { "受限可用" } else { "已阻止" },
        ),
        item(
            "desktop-actions",
            "桌面动作申请",
            format!(
                "当前可触发的白名单动作：{}。其中需要人工确认的动作：{}。",
                join_action_titles(&enabled),
                join_action_titles(&approval_required)
            ),
            if enabled.is_empty() {
                "未开放"
            } else if approval_required.is_empty() {
                "白名单可用"
            } else {
                "需审批"
            },
        ),
        item(
            "voice",
            "语音交互",
            if provider.voice_reply {
                "语音播报默认可用；语音输入仍然取决于本机麦克风和识别环境。".to_string()
            } else {
                "语音播报已关闭，但桌宠仍然只能通过受控输入和白名单动作工作。".to_string()
            },
            if provider.voice_reply { "可用" } else { "部分关闭" },
        ),
    ];

    let runtime_boundaries = vec![
        item(
            "permission-level",
            "权限等级",
            format!(
                "当前运行在 L{}。未开放动作：{}。",
                permission_level,
                join_action_titles(&disabled)
            ),
            format!("L{}", permission_level).as_str(),
        ),
        item(
            "auth-mode",
            "认证门禁",
            match provider.auth_mode {
                AuthMode::ApiKey => {
                    "当前使用 API Key 模式，密钥只保存在运行内存中，不会写入持久化状态文件。"
                        .to_string()
                }
                AuthMode::OAuth => {
                    "当前使用 OAuth 模式，访问令牌只保存在运行内存中，配置变化后会被主动清空。"
                        .to_string()
                }
            },
            match provider.auth_mode {
                AuthMode::ApiKey => "API Key",
                AuthMode::OAuth => "OAuth",
            },
        ),
        item(
            "history-retention",
            "会话保留",
            if provider.retain_history {
                "聊天上下文会保留到本地状态，但 API Key 和 OAuth 令牌不会被持久化。".to_string()
            } else {
                "聊天上下文不会在下次启动时恢复，桌宠每次启动都会回到临时会话。".to_string()
            },
            if provider.retain_history { "保留" } else { "临时" },
        ),
        item(
            "user-confirmation",
            "人工确认",
            "凡是高风险桌面动作，都必须由用户显式勾选确认项并输入一次性确认短语。".to_string(),
            "强制",
        ),
    ];

    AiConstraintProfile {
        label: PROFILE_LABEL.to_string(),
        version: PROFILE_VERSION.to_string(),
        summary: "Shell Agent 模式：AI 通过 shell 命令自主操作，高风险命令需确认。".to_string(),
        immutable_rules,
        capability_gates,
        runtime_boundaries,
    }
}

/// 构建系统提示（Shell Agent 兼容模式）
/// 注意：Shell Agent 模式下，此函数通常不会被调用，
/// 因为 provider.rs 会优先使用 history 中的 system 消息
pub fn compose_system_prompt(
    provider: &ProviderConfig,
    _permission_level: u8,
    _allowed_actions: &[DesktopAction],
) -> String {
    let user_prompt = provider.system_prompt.trim();
    let provider_label = match provider.kind {
        ProviderKind::Mock => "Mock",
        ProviderKind::CodexCli => "Codex CLI",
        ProviderKind::OpenAi => "OpenAI",
        ProviderKind::Anthropic => "Anthropic",
        ProviderKind::OpenAiCompatible => "OpenAI-Compatible",
    };

    format!(
        r#"你是运行在用户 Windows 电脑上的桌面助手。你有完整的 shell 权限，可以执行任何 cmd/powershell 命令。

你可以：
- 打开应用程序（start notepad, start msedge url）
- 读写文件（type, echo, copy, del）
- 管理目录（dir, cd, mkdir, rmdir）
- 查看系统信息（systeminfo, hostname, ipconfig）
- 执行任何 Windows 命令

输出格式（每次只输出一个 JSON）：
- 执行命令：{{"cmd": "命令内容"}}
- 直接回复：{{"reply": "回复内容"}}
- 任务完成：{{"done": "完成说明"}}
- 任务失败：{{"fail": "失败原因"}}

执行命令后你会看到输出结果，然后决定下一步。
如果用户只是聊天，直接用 reply 回复即可。
高风险命令（删除文件等）会提示用户确认后才执行。

当前模型来源: {provider_label}
当前模型标识: {model_name}

{user_prompt}"#,
        provider_label = provider_label,
        model_name = provider.model,
        user_prompt = if user_prompt.is_empty() {
            ""
        } else {
            user_prompt
        },
    )
}
