use std::{env, path::PathBuf, process::Command};
use tauri::{AppHandle, Manager};

pub fn execute_action(app: &AppHandle, action_id: &str) -> Result<String, String> {
    match action_id {
        "show_window" | "focus_window" => {
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| "未找到主窗口".to_string())?;
            window.show().map_err(|error| error.to_string())?;
            window.set_focus().map_err(|error| error.to_string())?;
            Ok("主窗口已显示并聚焦。".to_string())
        }
        "hide_window" => {
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| "未找到主窗口".to_string())?;
            window.hide().map_err(|error| error.to_string())?;
            Ok("主窗口已隐藏，仍可通过托盘恢复。".to_string())
        }
        "open_notepad" => launch_windows_binary("notepad.exe", &[], "已启动记事本。"),
        "open_calculator" => launch_windows_binary("calc.exe", &[], "已启动计算器。"),
        "open_downloads" => {
            let downloads = downloads_directory()?;
            let path = downloads.to_string_lossy().to_string();
            launch_windows_binary("explorer.exe", &[path.as_str()], "已打开下载目录。")
        }
        _ => Err("未知或未授权的桌面动作。".to_string()),
    }
}

#[cfg(target_os = "windows")]
fn launch_windows_binary(binary: &str, args: &[&str], success_message: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    Command::new(binary)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(success_message.to_string())
}

#[cfg(not(target_os = "windows"))]
fn launch_windows_binary(
    _binary: &str,
    _args: &[&str],
    _success_message: &str,
) -> Result<String, String> {
    Err("当前系统不是 Windows，仅保留白名单动作接口，不执行实际系统调用。".to_string())
}

fn downloads_directory() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        let profile = env::var("USERPROFILE").map_err(|error| error.to_string())?;
        return Ok(PathBuf::from(profile).join("Downloads"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err("当前系统不是 Windows，无法解析 Downloads 目录。".to_string())
    }
}
