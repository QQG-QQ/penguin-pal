use serde_json::json;
use std::time::Duration;
use tauri::AppHandle;

use crate::control::errors::{ControlError, ControlResult};

use super::common::{run_powershell_json, INPUT_PREAMBLE, WINDOW_ENUM_PREAMBLE};

pub fn type_text(app: &AppHandle, text: &str) -> ControlResult<serde_json::Value> {
    let args = json!({ "text": text });
    run_powershell_json(
        app,
        "type_text",
        r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$text = [string]$payload.text
function Escape-SendKeys([string]$value) {
  return ($value -replace '([+^%~(){}\[\]])', '{$1}')
}
[System.Windows.Forms.SendKeys]::SendWait((Escape-SendKeys $text))
[pscustomobject]@{ typedLength = $text.Length } | ConvertTo-Json -Compress -Depth 3
"#,
        Some(&args),
        Duration::from_secs(3),
    )
}

pub fn send_hotkey(app: &AppHandle, keys: &[String]) -> ControlResult<serde_json::Value> {
    let sequence = build_hotkey_sequence(keys)?;
    let args = json!({
        "keys": keys,
        "sequence": sequence,
    });
    run_powershell_json(
        app,
        "send_hotkey",
        r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$sequence = [string]$payload.sequence
[System.Windows.Forms.SendKeys]::SendWait($sequence)
[pscustomobject]@{ sequence = $sequence } | ConvertTo-Json -Compress -Depth 3
"#,
        Some(&args),
        Duration::from_secs(3),
    )
}

pub fn click_at(
    app: &AppHandle,
    x: i64,
    y: i64,
    button: &str,
) -> ControlResult<serde_json::Value> {
    let args = json!({
        "x": x,
        "y": y,
        "button": button,
    });

    let script = format!(
        r#"{WINDOW_ENUM_PREAMBLE}
{INPUT_PREAMBLE}
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$hwnd = [PenguinPalWinApi]::GetForegroundWindow()
if ($hwnd -eq [IntPtr]::Zero) {{ throw '当前没有活动窗口。' }}
$rect = New-Object PenguinPalWinApi+RECT
[void][PenguinPalWinApi]::GetWindowRect($hwnd, [ref]$rect)
$windowWidth = [Math]::Max(0, $rect.Right - $rect.Left)
$windowHeight = [Math]::Max(0, $rect.Bottom - $rect.Top)
if ([int]$payload.x -lt 0 -or [int]$payload.y -lt 0 -or [int]$payload.x -gt $windowWidth -or [int]$payload.y -gt $windowHeight) {{
  throw '点击坐标超出活动窗口范围。'
}}
$screenX = $rect.Left + [int]$payload.x
$screenY = $rect.Top + [int]$payload.y
[void][PenguinPalInputApi]::SetCursorPos($screenX, $screenY)
Start-Sleep -Milliseconds 40
$button = [string]$payload.button
switch ($button) {{
  'right' {{
    [PenguinPalInputApi]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
    [PenguinPalInputApi]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
  }}
  'double' {{
    1..2 | ForEach-Object {{
      [PenguinPalInputApi]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
      [PenguinPalInputApi]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
      Start-Sleep -Milliseconds 50
    }}
  }}
  default {{
    [PenguinPalInputApi]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    [PenguinPalInputApi]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
  }}
}}
[pscustomobject]@{{ screenX = $screenX; screenY = $screenY; button = $button }} | ConvertTo-Json -Compress -Depth 4
"#
    );

    run_powershell_json(app, "click_at", &script, Some(&args), Duration::from_secs(3))
}

pub fn scroll_at(
    app: &AppHandle,
    delta: i64,
    steps: i64,
    x: Option<i64>,
    y: Option<i64>,
) -> ControlResult<serde_json::Value> {
    let args = json!({
        "delta": delta,
        "steps": steps,
        "x": x,
        "y": y,
    });

    let script = format!(
        r#"{WINDOW_ENUM_PREAMBLE}
{INPUT_PREAMBLE}
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$hwnd = [PenguinPalWinApi]::GetForegroundWindow()
if ($hwnd -eq [IntPtr]::Zero) {{ throw '当前没有活动窗口。' }}
$rect = New-Object PenguinPalWinApi+RECT
[void][PenguinPalWinApi]::GetWindowRect($hwnd, [ref]$rect)
$windowWidth = [Math]::Max(0, $rect.Right - $rect.Left)
$windowHeight = [Math]::Max(0, $rect.Bottom - $rect.Top)
$hasX = $null -ne $payload.x
$hasY = $null -ne $payload.y
if ($hasX -and $hasY) {{
  if ([int]$payload.x -lt 0 -or [int]$payload.y -lt 0 -or [int]$payload.x -gt $windowWidth -or [int]$payload.y -gt $windowHeight) {{
    throw '滚动坐标超出活动窗口范围。'
  }}
  $screenX = $rect.Left + [int]$payload.x
  $screenY = $rect.Top + [int]$payload.y
}} else {{
  $screenX = $rect.Left + [int]($windowWidth / 2)
  $screenY = $rect.Top + [int]($windowHeight / 2)
}}
[void][PenguinPalInputApi]::SetCursorPos($screenX, $screenY)
1..([int]$payload.steps) | ForEach-Object {{
  [PenguinPalInputApi]::mouse_event(0x0800, 0, 0, [uint32][int]$payload.delta, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 40
}}
[pscustomobject]@{{ delta = [int]$payload.delta; steps = [int]$payload.steps; screenX = $screenX; screenY = $screenY }} | ConvertTo-Json -Compress -Depth 4
"#
    );

    run_powershell_json(app, "scroll_at", &script, Some(&args), Duration::from_secs(3))
}

fn build_hotkey_sequence(keys: &[String]) -> ControlResult<String> {
    if keys.is_empty() {
        return Err(ControlError::invalid_argument("keys 不能为空。"));
    }

    let mut modifiers = String::new();
    let mut main_key = None::<String>;
    for key in keys {
        let normalized = key.trim().to_ascii_uppercase();
        let next_main_key = match normalized.as_str() {
            "CTRL" | "CONTROL" => {
                modifiers.push('^');
                None
            }
            "ALT" => {
                modifiers.push('%');
                None
            }
            "SHIFT" => {
                modifiers.push('+');
                None
            }
            "ENTER" | "RETURN" => Some("{ENTER}".to_string()),
            "TAB" => Some("{TAB}".to_string()),
            "ESC" | "ESCAPE" => Some("{ESC}".to_string()),
            "UP" => Some("{UP}".to_string()),
            "DOWN" => Some("{DOWN}".to_string()),
            "LEFT" => Some("{LEFT}".to_string()),
            "RIGHT" => Some("{RIGHT}".to_string()),
            "DELETE" => Some("{DELETE}".to_string()),
            "BACKSPACE" => Some("{BACKSPACE}".to_string()),
            _ => {
                if normalized.len() == 1 {
                    Some(normalized.to_ascii_lowercase())
                } else if normalized.starts_with('F') && normalized[1..].parse::<u8>().is_ok() {
                    Some(format!("{{{normalized}}}"))
                } else {
                    return Err(ControlError::invalid_argument(format!(
                        "不支持的热键项：{key}"
                    )));
                }
            }
        };

        if let Some(next_main_key) = next_main_key {
            if main_key.replace(next_main_key).is_some() {
                return Err(ControlError::invalid_argument(
                    "keys 目前只支持一个主键，修饰键请与单个主键组合使用。",
                ));
            }
        }
    }

    let Some(main_key) = main_key else {
        return Err(ControlError::invalid_argument(
            "keys 至少要包含一个非修饰键，例如 V 或 ENTER。",
        ));
    };

    Ok(format!("{modifiers}{main_key}"))
}
