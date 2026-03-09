use crate::app_state::DesktopAction;

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

pub fn validate_action(action: &DesktopAction, level: u8, confirmed: bool) -> Result<(), String> {
    if level < action.minimum_level {
        return Err(format!(
            "当前权限等级不足：{} 需要 L{}，当前仅为 L{}",
            action.title, action.minimum_level, level
        ));
    }

    if action.requires_confirmation && !confirmed {
        return Err(format!("动作 {} 需要人工确认后才能执行", action.title));
    }

    Ok(())
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
            summary: "示例级白名单动作，仅在 Windows 上执行。".to_string(),
            risk_level: 2,
            minimum_level: 2,
            requires_confirmation: true,
            enabled: false,
        },
        DesktopAction {
            id: "open_calculator".to_string(),
            title: "打开计算器".to_string(),
            summary: "示例级白名单动作，仅在 Windows 上执行。".to_string(),
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
