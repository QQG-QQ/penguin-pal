use serde_json::{json, Value};
use std::process::Command;
use tauri::AppHandle;

use crate::control::{
    errors::{ControlError, ControlResult},
    logging,
};

fn normalize_alias(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '_', '-'], "")
}

pub fn open_app(app: &AppHandle, name: &str) -> ControlResult<Value> {
    let normalized = normalize_alias(name);
    let (label, mut command): (&str, Command) = match normalized.as_str() {
        "browser" => {
            let mut cmd = Command::new("cmd.exe");
            cmd.args(["/C", "start", "", "about:blank"]);
            ("browser", cmd)
        }
        "notepad" | "editor" => ("notepad", Command::new("notepad.exe")),
        "calculator" | "calc" => ("calculator", Command::new("calc.exe")),
        "explorer" | "files" => ("explorer", Command::new("explorer.exe")),
        "paint" | "mspaint" => ("paint", Command::new("mspaint.exe")),
        "settings" => {
            let mut cmd = Command::new("cmd.exe");
            cmd.args(["/C", "start", "", "ms-settings:"]);
            ("settings", cmd)
        }
        _ => {
            return Err(ControlError::invalid_argument(
                "当前只允许打开 allowlist 内应用：browser、notepad、calculator、explorer、paint、settings。",
            ))
        }
    };

    command
        .spawn()
        .map_err(|error| ControlError::backend("backend_exec_failed", "启动应用失败。", Some(error.to_string())))?;
    let _ = logging::append_log(app, "open_app", "ok", format!("app={label}"));
    Ok(json!({
        "app": label,
        "message": format!("已启动 {label}。"),
    }))
}
