use std::process::Command;

use serde_json::{json, Value};
use tauri::AppHandle;

use super::errors::{ControlError, ControlResult};

pub fn query_registry_key(_app: &AppHandle, path: &str) -> ControlResult<Value> {
    let path = normalize_path(path)?;
    ensure_read_path_allowed(&path)?;
    let output = run_reg(&["query", &path])?;
    Ok(json!({
        "path": path,
        "stdout": output,
    }))
}

pub fn read_registry_value(_app: &AppHandle, path: &str, name: &str) -> ControlResult<Value> {
    let path = normalize_path(path)?;
    let name = normalize_name(name)?;
    ensure_read_path_allowed(&path)?;
    let output = run_reg(&["query", &path, "/v", &name])?;
    let parsed = parse_query_value_line(&output, &name);
    Ok(json!({
        "path": path,
        "name": name,
        "valueType": parsed.as_ref().and_then(|item| item.get("valueType")).cloned().unwrap_or(Value::Null),
        "value": parsed.as_ref().and_then(|item| item.get("value")).cloned().unwrap_or(Value::Null),
        "stdout": output,
    }))
}

pub fn write_registry_value(
    _app: &AppHandle,
    path: &str,
    name: &str,
    value_type: &str,
    value: &str,
) -> ControlResult<Value> {
    let path = normalize_path(path)?;
    let name = normalize_name(name)?;
    let value_type = normalize_value_type(value_type)?;
    ensure_write_path_allowed(&path)?;
    let output = run_reg(&["add", &path, "/v", &name, "/t", &value_type, "/d", value, "/f"])?;
    Ok(json!({
        "path": path,
        "name": name,
        "valueType": value_type,
        "value": value,
        "stdout": output,
        "written": true,
    }))
}

pub fn delete_registry_value(_app: &AppHandle, path: &str, name: &str) -> ControlResult<Value> {
    let path = normalize_path(path)?;
    let name = normalize_name(name)?;
    ensure_write_path_allowed(&path)?;
    let output = run_reg(&["delete", &path, "/v", &name, "/f"])?;
    Ok(json!({
        "path": path,
        "name": name,
        "stdout": output,
        "deleted": true,
    }))
}

fn run_reg(args: &[&str]) -> ControlResult<String> {
    let mut cmd = Command::new("reg");
    cmd.args(args);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd
        .output()
        .map_err(|error| {
            ControlError::backend(
                "registry_command_failed",
                "启动注册表命令失败。",
                Some(error.to_string()),
            )
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() {
        return Err(ControlError::backend(
            "registry_operation_failed",
            "注册表命令执行失败。",
            Some(format!("exitCode={:?} stderr={stderr}", output.status.code())),
        ));
    }
    Ok(stdout)
}

fn normalize_path(path: &str) -> ControlResult<String> {
    let next = path.trim().trim_matches('\\').to_string();
    if next.is_empty() {
        return Err(ControlError::invalid_argument("注册表 path 不能为空。"));
    }
    Ok(next)
}

fn normalize_name(name: &str) -> ControlResult<String> {
    let next = name.trim().to_string();
    if next.is_empty() {
        return Err(ControlError::invalid_argument("注册表 value name 不能为空。"));
    }
    Ok(next)
}

fn normalize_value_type(value_type: &str) -> ControlResult<String> {
    let next = value_type.trim().to_uppercase();
    if !["REG_SZ", "REG_EXPAND_SZ", "REG_DWORD", "REG_QWORD"].contains(&next.as_str()) {
        return Err(ControlError::invalid_argument(
            "第一版 write_registry_value 只允许 REG_SZ / REG_EXPAND_SZ / REG_DWORD / REG_QWORD。",
        ));
    }
    Ok(next)
}

fn ensure_read_path_allowed(path: &str) -> ControlResult<()> {
    let upper = path.to_uppercase();
    if upper.starts_with("HKCU\\")
        || upper.starts_with("HKEY_CURRENT_USER\\")
        || upper.starts_with("HKLM\\")
        || upper.starts_with("HKEY_LOCAL_MACHINE\\")
        || upper.starts_with("HKCR\\")
        || upper.starts_with("HKEY_CLASSES_ROOT\\")
        || upper.starts_with("HKU\\")
        || upper.starts_with("HKEY_USERS\\")
    {
        Ok(())
    } else {
        Err(ControlError::invalid_argument(
            "第一版只允许读取 HKCU / HKLM / HKCR / HKU 路径。",
        ))
    }
}

fn ensure_write_path_allowed(path: &str) -> ControlResult<()> {
    let upper = path.to_uppercase();
    if upper.starts_with("HKCU\\SOFTWARE\\")
        || upper.starts_with("HKEY_CURRENT_USER\\SOFTWARE\\")
        || upper.starts_with("HKCU\\ENVIRONMENT")
        || upper.starts_with("HKEY_CURRENT_USER\\ENVIRONMENT")
    {
        Ok(())
    } else {
        Err(ControlError::invalid_argument(
            "第一版注册表写入/删除只允许 HKCU\\\\Software\\\\... 或 HKCU\\\\Environment。",
        ))
    }
}

fn parse_query_value_line(output: &str, target_name: &str) -> Option<Value> {
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with(target_name) {
            return None;
        }
        let parts = trimmed.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 3 {
            return None;
        }
        let value_type = parts.get(1).copied().unwrap_or_default().to_string();
        let value = parts.iter().skip(2).copied().collect::<Vec<_>>().join(" ");
        Some(json!({
            "valueType": value_type,
            "value": value,
        }))
    })
}
