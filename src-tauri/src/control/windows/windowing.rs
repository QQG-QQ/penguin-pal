use serde_json::{json, Value};
use std::time::Duration;
use tauri::AppHandle;

use crate::control::errors::ControlResult;

use super::common::{run_powershell_json, WINDOW_ENUM_PREAMBLE};

pub fn list_windows(app: &AppHandle) -> ControlResult<Value> {
    let script = format!(
        r#"{WINDOW_ENUM_PREAMBLE}
$active = [PenguinPalWinApi]::GetForegroundWindow()
$items = New-Object System.Collections.Generic.List[object]
[PenguinPalWinApi]::EnumWindows({{
  param($hWnd, $lParam)
  if (-not [PenguinPalWinApi]::IsWindowVisible($hWnd)) {{ return $true }}
  $title = Get-WindowTitle $hWnd
  if ([string]::IsNullOrWhiteSpace($title)) {{ return $true }}
  $rect = New-Object PenguinPalWinApi+RECT
  [void][PenguinPalWinApi]::GetWindowRect($hWnd, [ref]$rect)
  $items.Add([pscustomobject]@{{
    handle = $hWnd.ToInt64()
    title = $title
    isActive = ($hWnd.ToInt64() -eq $active.ToInt64())
    bounds = [pscustomobject]@{{
      left = $rect.Left
      top = $rect.Top
      width = [Math]::Max(0, $rect.Right - $rect.Left)
      height = [Math]::Max(0, $rect.Bottom - $rect.Top)
    }}
  }}) | Out-Null
  return $true
}}, [IntPtr]::Zero) | Out-Null
$items | ConvertTo-Json -Compress -Depth 6
"#
    );

    run_powershell_json(app, "list_windows", &script, None, Duration::from_secs(3))
}

pub fn focus_window(app: &AppHandle, title: &str, match_mode: &str) -> ControlResult<Value> {
    let args = json!({
        "title": title,
        "match": match_mode,
    });

    let script = format!(
        r#"{WINDOW_ENUM_PREAMBLE}
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$needle = [string]$payload.title
$matchMode = [string]$payload.match
if ([string]::IsNullOrWhiteSpace($matchMode)) {{ $matchMode = 'contains' }}
$matched = $null
[PenguinPalWinApi]::EnumWindows({{
  param($hWnd, $lParam)
  if (-not [PenguinPalWinApi]::IsWindowVisible($hWnd)) {{ return $true }}
  $title = Get-WindowTitle $hWnd
  if ([string]::IsNullOrWhiteSpace($title)) {{ return $true }}
  $normalizedTitle = $title.ToLowerInvariant()
  $normalizedNeedle = $needle.ToLowerInvariant()
  $isMatch = $false
  switch ($matchMode) {{
    'exact' {{ $isMatch = ($normalizedTitle -eq $normalizedNeedle) }}
    'prefix' {{ $isMatch = $normalizedTitle.StartsWith($normalizedNeedle) }}
    default {{ $isMatch = $normalizedTitle.Contains($normalizedNeedle) }}
  }}
  if (-not $isMatch) {{ return $true }}
  $matched = [pscustomobject]@{{ handle = $hWnd; title = $title }}
  return $false
}}, [IntPtr]::Zero) | Out-Null
if ($null -eq $matched) {{ throw '未找到匹配窗口。' }}
[void][PenguinPalWinApi]::ShowWindow($matched.handle, 9)
[void][PenguinPalWinApi]::SetForegroundWindow($matched.handle)
[pscustomobject]@{{ handle = $matched.handle.ToInt64(); title = $matched.title }} | ConvertTo-Json -Compress -Depth 4
"#
    );

    run_powershell_json(app, "focus_window", &script, Some(&args), Duration::from_secs(3))
}
