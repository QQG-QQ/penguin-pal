use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::{json, Value};
use tauri::AppHandle;

use super::errors::{ControlError, ControlResult};

pub fn launch_installer_file(_app: &AppHandle, path: &str) -> ControlResult<Value> {
    let resolved = resolve_path(path)?;
    if !resolved.exists() || !resolved.is_file() {
        return Err(ControlError::not_found(
            "installer_not_found",
            format!("安装器文件不存在：{}", display_path(&resolved)),
        ));
    }

    let extension = resolved
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    let installer_path = display_path(&resolved);

    let child = match extension.as_str() {
        "exe" => Command::new(&resolved).spawn(),
        "msi" => Command::new("msiexec")
            .args(["/i", installer_path.as_str()])
            .spawn(),
        _ => {
            return Err(ControlError::invalid_argument(
                "launch_installer_file 目前只允许 .exe 或 .msi 安装器。",
            ))
        }
    }
    .map_err(|error| {
        ControlError::backend(
            "installer_launch_failed",
            "启动安装器失败。",
            Some(error.to_string()),
        )
    })?;

    Ok(json!({
        "path": installer_path,
        "kind": extension,
        "pid": child.id(),
        "started": true,
    }))
}

fn resolve_path(input: &str) -> ControlResult<PathBuf> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ControlError::invalid_argument("path 不能为空。"));
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        Ok(path)
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|_| ControlError::internal("解析当前工作目录失败。"))
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
