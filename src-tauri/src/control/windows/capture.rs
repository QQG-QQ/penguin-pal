use serde_json::json;
use std::{fs, time::Duration};
use tauri::{AppHandle, Manager};

use crate::control::errors::{ControlError, ControlResult};

use super::common::{run_powershell_json, WINDOW_ENUM_PREAMBLE};

pub fn capture_active_window(app: &AppHandle) -> ControlResult<serde_json::Value> {
    let capture_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| ControlError::backend("backend_exec_failed", "无法解析截图目录。", Some(error.to_string())))?
        .join("captures");
    fs::create_dir_all(&capture_dir)
        .map_err(|error| ControlError::backend("backend_exec_failed", "无法创建截图目录。", Some(error.to_string())))?;

    let args = json!({
        "dir": capture_dir.to_string_lossy().to_string(),
    });

    let script = format!(
        r#"{WINDOW_ENUM_PREAMBLE}
Add-Type -AssemblyName System.Drawing
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$dir = [string]$payload.dir
$hwnd = [PenguinPalWinApi]::GetForegroundWindow()
if ($hwnd -eq [IntPtr]::Zero) {{ throw '当前没有活动窗口。' }}
$title = Get-WindowTitle $hwnd
$rect = New-Object PenguinPalWinApi+RECT
[void][PenguinPalWinApi]::GetWindowRect($hwnd, [ref]$rect)
$width = [Math]::Max(0, $rect.Right - $rect.Left)
$height = [Math]::Max(0, $rect.Bottom - $rect.Top)
if ($width -le 0 -or $height -le 0) {{ throw '活动窗口尺寸无效，无法截图。' }}
[void][System.IO.Directory]::CreateDirectory($dir)
$fileName = 'active-window-' + [DateTime]::Now.ToString('yyyyMMdd-HHmmssfff') + '.png'
$path = Join-Path $dir $fileName
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
$bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
[pscustomobject]@{{
  title = $title
  path = $path
  width = $width
  height = $height
}} | ConvertTo-Json -Compress -Depth 4
"#
    );

    run_powershell_json(
        app,
        "capture_active_window",
        &script,
        Some(&args),
        Duration::from_secs(5),
    )
}
