use crate::control::windows::uia_context::WindowUiDescription;

use super::vision_types::{
    ScreenContextConsistency, ScreenContextConsistencyKind, VisionContext, VisionWindowSummary,
};

pub fn reconcile_screen_context(
    uia: Option<&WindowUiDescription>,
    vision: &VisionContext,
) -> ScreenContextConsistency {
    let vision_summary = vision.summary.as_ref();

    match (uia, vision_summary) {
        (Some(uia_description), Some(vision_summary)) => {
            reconcile_uia_with_vision(uia_description, vision_summary)
        }
        (Some(_), None) => ScreenContextConsistency {
            status: ScreenContextConsistencyKind::UiaOnly,
            reasons: vec![vision.provider_status.message.clone()],
        },
        (None, Some(_)) => ScreenContextConsistency {
            status: ScreenContextConsistencyKind::VisionOnly,
            reasons: vec!["当前无法提取有效 UIA 摘要，只能依赖视觉摘要。".to_string()],
        },
        (None, None) => ScreenContextConsistency {
            status: ScreenContextConsistencyKind::HardConflict,
            reasons: vec!["当前既没有可用 UIA 摘要，也没有可用视觉摘要。".to_string()],
        },
    }
}

fn reconcile_uia_with_vision(
    uia: &WindowUiDescription,
    vision: &VisionWindowSummary,
) -> ScreenContextConsistency {
    let uia_kind = classify_uia_window(uia);
    let vision_kind = normalize_kind(&vision.window_kind);
    let mut reasons = Vec::new();

    if uia_kind != "unknown"
        && vision_kind != "unknown"
        && uia_kind != vision_kind
        && is_hard_conflict_pair(&uia_kind, &vision_kind)
    {
        reasons.push(format!(
            "UIA 判断为 {uia_kind}，视觉判断为 {vision_kind}，两者存在强冲突。"
        ));
        return ScreenContextConsistency {
            status: ScreenContextConsistencyKind::HardConflict,
            reasons,
        };
    }

    let uia_has_interactive = uia
        .visible_elements
        .iter()
        .any(|element| element.is_enabled && is_interactive_role(&element.role));
    if uia_has_interactive != vision.has_obvious_interactive_target {
        reasons.push(format!(
            "UIA 认为交互目标{}，视觉摘要认为交互目标{}。",
            if uia_has_interactive { "明显存在" } else { "不明显" },
            if vision.has_obvious_interactive_target {
                "明显存在"
            } else {
                "不明显"
            }
        ));
    }

    if uia_kind != "unknown" && vision_kind != "unknown" && uia_kind != vision_kind {
        reasons.push(format!(
            "UIA 判断为 {uia_kind}，视觉判断为 {vision_kind}，存在弱冲突。"
        ));
    }

    let status = if reasons.is_empty() {
        ScreenContextConsistencyKind::Consistent
    } else {
        ScreenContextConsistencyKind::SoftConflict
    };

    ScreenContextConsistency { status, reasons }
}

fn classify_uia_window(description: &WindowUiDescription) -> String {
    let title = description.window_title.to_ascii_lowercase();
    let class_name = description
        .window_class_name
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();

    if title.contains("wechat")
        || description.window_title.contains("微信")
        || title.contains("qq")
        || title.contains("telegram")
        || title.contains("discord")
    {
        return "chat_app".to_string();
    }

    if title.contains("chrome")
        || title.contains("edge")
        || title.contains("firefox")
        || title.contains("browser")
        || class_name.contains("chrome")
        || class_name.contains("mozilla")
    {
        return "browser".to_string();
    }

    if title.contains("notepad")
        || description.window_title.contains("记事本")
        || class_name.contains("notepad")
    {
        return "editor".to_string();
    }

    let mut has_document = false;
    let mut has_edit = false;
    let mut has_menu_item = false;
    let mut list_item_count = 0usize;

    for element in &description.visible_elements {
        match normalize_role(&element.role).as_str() {
            "document" => has_document = true,
            "edit" | "combobox" => has_edit = true,
            "menuitem" => has_menu_item = true,
            "listitem" => list_item_count += 1,
            _ => {}
        }
    }

    if (has_document || has_edit) && has_menu_item {
        return "editor".to_string();
    }

    if list_item_count >= 2 && has_edit {
        return "chat_app".to_string();
    }

    "unknown".to_string()
}

fn normalize_kind(kind: &str) -> String {
    let normalized = kind.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "browser" | "webbrowser" | "web_browser" => "browser".to_string(),
        "editor" | "document" | "document_editor" | "text_editor" => "editor".to_string(),
        "chat" | "chatapp" | "chat_app" | "messenger" => "chat_app".to_string(),
        "desktop" => "desktop".to_string(),
        "" => "unknown".to_string(),
        other => other.to_string(),
    }
}

fn normalize_role(role: &str) -> String {
    role.trim().replace(' ', "").to_ascii_lowercase()
}

fn is_interactive_role(role: &str) -> bool {
    matches!(
        normalize_role(role).as_str(),
        "button"
            | "edit"
            | "document"
            | "menuitem"
            | "tabitem"
            | "listitem"
            | "combobox"
            | "hyperlink"
            | "checkbox"
            | "radiobutton"
    )
}

fn is_hard_conflict_pair(left: &str, right: &str) -> bool {
    matches!(
        (left, right),
        ("browser", "editor")
            | ("editor", "browser")
            | ("browser", "chat_app")
            | ("chat_app", "browser")
            | ("editor", "chat_app")
            | ("chat_app", "editor")
    )
}
