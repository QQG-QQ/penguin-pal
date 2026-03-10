use std::collections::HashSet;

use rand::{distributions::Alphanumeric, thread_rng, Rng};

use crate::app_state::{
    now_millis, ActionApprovalCheck, ActionApprovalRequest, DesktopAction,
};

const APPROVAL_TTL_MS: u64 = 2 * 60 * 1000;

pub fn clamp_permission_level(level: u8) -> u8 {
    level.min(2)
}

pub fn actions_for_level(level: u8) -> Vec<DesktopAction> {
    let effective_level = clamp_permission_level(level);

    all_actions()
        .into_iter()
        .map(|mut action| {
            action.enabled = effective_level >= action.minimum_level;
            action
        })
        .collect()
}

pub fn resolve_action(id: &str, level: u8) -> Option<DesktopAction> {
    actions_for_level(level)
        .into_iter()
        .find(|action| action.id == id)
}

pub fn validate_action_access(action: &DesktopAction, level: u8) -> Result<(), String> {
    if level < action.minimum_level {
        return Err(format!(
            "当前权限等级不足：{} 需要 L{}，当前仅为 L{}",
            action.title, action.minimum_level, level
        ));
    }

    Ok(())
}

pub fn build_action_approval(action: &DesktopAction) -> ActionApprovalRequest {
    let created_at = now_millis();
    ActionApprovalRequest {
        id: random_approval_id(created_at),
        action: action.clone(),
        prompt: format!(
            "你即将执行“{}”。这次授权只允许本次白名单动作，不会放大成持续电脑控制权限。",
            action.title
        ),
        required_phrase: format!("确认执行 {}", action.title),
        checks: approval_checks_for_action(action),
        created_at,
        expires_at: created_at + APPROVAL_TTL_MS,
    }
}

pub fn validate_approval(
    approval: &ActionApprovalRequest,
    typed_phrase: &str,
    acknowledged_checks: &[String],
) -> Result<(), String> {
    if approval.expires_at <= now_millis() {
        return Err("这次动作授权已经过期，请重新发起。".to_string());
    }

    if typed_phrase.trim() != approval.required_phrase {
        return Err(format!(
            "请完整输入确认短语：{}",
            approval.required_phrase
        ));
    }

    let acknowledged: HashSet<&str> = acknowledged_checks.iter().map(String::as_str).collect();
    if approval
        .checks
        .iter()
        .any(|check| !acknowledged.contains(check.id.as_str()))
    {
        return Err("请先完成所有确认项。".to_string());
    }

    Ok(())
}

pub fn cleanup_expired_approvals(approvals: &mut Vec<ActionApprovalRequest>) {
    let now = now_millis();
    approvals.retain(|approval| approval.expires_at > now);
}

fn random_approval_id(created_at: u64) -> String {
    let suffix: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();
    format!("approval-{}-{}", created_at, suffix)
}

fn approval_checks_for_action(action: &DesktopAction) -> Vec<ActionApprovalCheck> {
    let mut checks = vec![
        ActionApprovalCheck {
            id: "one_time".to_string(),
            label: "我确认这是一次性授权，不会让桌宠获得自由控制电脑的权限。"
                .to_string(),
        },
        ActionApprovalCheck {
            id: "visible_effect".to_string(),
            label: format!("我知道这会直接触发系统动作：{}。", action.summary),
        },
    ];

    if action.risk_level >= 2 {
        checks.push(ActionApprovalCheck {
            id: "privacy_boundary".to_string(),
            label: "我确认这次动作不应读取、上传或暴露我的隐私数据。".to_string(),
        });
    }

    checks
}

fn all_actions() -> Vec<DesktopAction> {
    vec![
        DesktopAction {
            id: "show_window".to_string(),
            title: "显示主面板".to_string(),
            summary: "重新显示桌宠控制台和聊天面板。".to_string(),
            risk_level: 0,
            minimum_level: 0,
            requires_confirmation: false,
            enabled: true,
        },
        DesktopAction {
            id: "hide_window".to_string(),
            title: "收起主面板".to_string(),
            summary: "隐藏窗口，保留系统托盘驻留。".to_string(),
            risk_level: 0,
            minimum_level: 0,
            requires_confirmation: false,
            enabled: true,
        },
        DesktopAction {
            id: "focus_window".to_string(),
            title: "聚焦桌宠".to_string(),
            summary: "将主窗口唤起并置于前台。".to_string(),
            risk_level: 0,
            minimum_level: 0,
            requires_confirmation: false,
            enabled: true,
        },
        DesktopAction {
            id: "open_notepad".to_string(),
            title: "打开记事本".to_string(),
            summary: "启动 Windows 记事本程序。".to_string(),
            risk_level: 2,
            minimum_level: 2,
            requires_confirmation: true,
            enabled: false,
        },
        DesktopAction {
            id: "open_calculator".to_string(),
            title: "打开计算器".to_string(),
            summary: "启动 Windows 计算器程序。".to_string(),
            risk_level: 2,
            minimum_level: 2,
            requires_confirmation: true,
            enabled: false,
        },
        DesktopAction {
            id: "open_downloads".to_string(),
            title: "打开下载目录".to_string(),
            summary: "通过资源管理器打开用户 Downloads 文件夹。".to_string(),
            risk_level: 2,
            minimum_level: 2,
            requires_confirmation: true,
            enabled: false,
        },
    ]
}
