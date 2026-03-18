use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use tauri::AppHandle;

use crate::control::{
    errors::{ControlError, ControlResult},
    logging,
};

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub const POWERSHELL_UTF8_PREAMBLE: &str = r#"
$ErrorActionPreference = 'Stop'
$utf8 = New-Object System.Text.UTF8Encoding($false)
[Console]::InputEncoding = $utf8
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8
"#;

pub const WINDOW_ENUM_PREAMBLE: &str = r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PenguinPalWinApi {
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int maxCount);
  [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int cmdShow);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
"@
function Normalize-WindowText([object]$Value) {
  if ($null -eq $Value) { return '' }
  return ([string]$Value).Trim()
}
function Get-WindowTitle([IntPtr]$Handle) {
  $len = [PenguinPalWinApi]::GetWindowTextLength($Handle)
  if ($len -le 0) { return $null }
  $builder = New-Object System.Text.StringBuilder ($len + 1)
  [void][PenguinPalWinApi]::GetWindowText($Handle, $builder, $builder.Capacity)
  return (Normalize-WindowText $builder.ToString())
}
function New-WindowSummary([IntPtr]$Handle, [IntPtr]$ActiveHandle) {
  $title = Get-WindowTitle $Handle
  if ([string]::IsNullOrWhiteSpace($title)) { return $null }
  $rect = New-Object PenguinPalWinApi+RECT
  [void][PenguinPalWinApi]::GetWindowRect($Handle, [ref]$rect)
  return [pscustomobject]@{
    handle = $Handle.ToInt64()
    title = [string]$title
    isActive = ($Handle.ToInt64() -eq $ActiveHandle.ToInt64())
    bounds = [pscustomobject]@{
      left = $rect.Left
      top = $rect.Top
      width = [Math]::Max(0, $rect.Right - $rect.Left)
      height = [Math]::Max(0, $rect.Bottom - $rect.Top)
    }
  }
}
function Get-VisibleWindowSummaries() {
  $active = [PenguinPalWinApi]::GetForegroundWindow()
  $items = New-Object System.Collections.Generic.List[object]
  [PenguinPalWinApi]::EnumWindows({
    param($hWnd, $lParam)
    if (-not [PenguinPalWinApi]::IsWindowVisible($hWnd)) { return $true }
    $summary = New-WindowSummary $hWnd $active
    if ($null -ne $summary) {
      $items.Add($summary) | Out-Null
    }
    return $true
  }, [IntPtr]::Zero) | Out-Null
  return @($items.ToArray())
}
function Normalize-MatchMode([object]$Value) {
  $mode = (Normalize-WindowText $Value).ToLowerInvariant()
  if ([string]::IsNullOrWhiteSpace($mode)) { return 'contains' }
  switch ($mode) {
    'exact' { return 'exact' }
    'prefix' { return 'prefix' }
    default { return 'contains' }
  }
}
function Find-MatchingWindowSummary($Windows, [string]$Needle, [string]$MatchMode) {
  $normalizedNeedle = (Normalize-WindowText $Needle).ToLowerInvariant()
  if ([string]::IsNullOrWhiteSpace($normalizedNeedle)) { return $null }
  $mode = Normalize-MatchMode $MatchMode
  foreach ($window in @($Windows)) {
    if ($null -eq $window) { continue }
    $windowTitle = (Normalize-WindowText $window.title).ToLowerInvariant()
    if ([string]::IsNullOrWhiteSpace($windowTitle)) { continue }
    $isMatch = $false
    switch ($mode) {
      'exact' { $isMatch = ($windowTitle -eq $normalizedNeedle) }
      'prefix' { $isMatch = $windowTitle.StartsWith($normalizedNeedle) }
      default { $isMatch = $windowTitle.Contains($normalizedNeedle) }
    }
    if ($isMatch) {
      return $window
    }
  }
  return $null
}
"#;

pub const INPUT_PREAMBLE: &str = r#"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class PenguinPalInputApi {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extraInfo);
}
"@
"#;

pub const UIA_PREAMBLE: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
function Normalize-Text([string]$value) {
  if ($null -eq $value) { return '' }
  return $value.ToLowerInvariant().Trim()
}
function Match-Field([string]$actual, [string]$expected, [string]$mode) {
  if ([string]::IsNullOrWhiteSpace($expected)) { return $true }
  $actualNorm = Normalize-Text $actual
  $expectedNorm = Normalize-Text $expected
  switch ($mode) {
    'exact' { return $actualNorm -eq $expectedNorm }
    'prefix' { return $actualNorm.StartsWith($expectedNorm) }
    default { return $actualNorm.Contains($expectedNorm) }
  }
}
function Get-ControlTypeName($element) {
  $programmatic = $element.Current.ControlType.ProgrammaticName
  if ([string]::IsNullOrWhiteSpace($programmatic)) { return '' }
  if ($programmatic.Contains('.')) { return $programmatic.Split('.')[-1] }
  return $programmatic
}
function Convert-ElementSummary($element, [string]$windowTitle) {
  $rect = $element.Current.BoundingRectangle
  return [pscustomobject]@{
    name = [string]$element.Current.Name
    automationId = [string]$element.Current.AutomationId
    controlType = [string](Get-ControlTypeName $element)
    className = [string]$element.Current.ClassName
    windowTitle = [string]$windowTitle
    bounds = [pscustomobject]@{
      left = [int]$rect.Left
      top = [int]$rect.Top
      width = [int][Math]::Max(0, $rect.Width)
      height = [int][Math]::Max(0, $rect.Height)
    }
  }
}
function Get-WindowElement($selector) {
  $root = [System.Windows.Automation.AutomationElement]::RootElement
  $windows = $root.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)
  for ($i = 0; $i -lt $windows.Count; $i++) {
    $window = $windows.Item($i)
    if (Match-Field $window.Current.Name $selector.windowTitle $selector.matchMode) {
      return $window
    }
  }
  throw '未找到匹配窗口。'
}
function Test-SelectorMatch($element, $selector) {
  if (-not (Match-Field $element.Current.AutomationId $selector.automationId 'exact')) { return $false }
  if (-not (Match-Field $element.Current.Name $selector.name $selector.matchMode)) { return $false }
  if (-not (Match-Field (Get-ControlTypeName $element) $selector.controlType 'exact')) { return $false }
  if (-not (Match-Field $element.Current.ClassName $selector.className $selector.matchMode)) { return $false }
  return $true
}
function Find-ElementCore($selector) {
  $window = Get-WindowElement $selector
  $windowTitle = [string]$window.Current.Name
  $hasElementFields = -not [string]::IsNullOrWhiteSpace($selector.automationId) -or
                      -not [string]::IsNullOrWhiteSpace($selector.name) -or
                      -not [string]::IsNullOrWhiteSpace($selector.controlType) -or
                      -not [string]::IsNullOrWhiteSpace($selector.className)
  if (-not $hasElementFields) {
    return [pscustomobject]@{
      element = $window
      windowTitle = $windowTitle
    }
  }

  $nodes = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  for ($i = 0; $i -lt $nodes.Count; $i++) {
    $node = $nodes.Item($i)
    if (Test-SelectorMatch $node $selector) {
      return [pscustomobject]@{
        element = $node
        windowTitle = $windowTitle
      }
    }
  }

  throw '未找到匹配的 UI 元素。'
}
"#;

pub fn run_powershell_json(
    app: &AppHandle,
    tool: &str,
    script: &str,
    args: Option<&Value>,
    timeout: Duration,
) -> ControlResult<Value> {
    let encoded = encode_powershell(&format!("{POWERSHELL_UTF8_PREAMBLE}\n{script}"));
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
            &encoded,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);

    if let Some(args) = args {
        let serialized = serde_json::to_string(args)
            .map_err(|error| ControlError::internal(format!("控制参数序列化失败：{error}")))?;
        command.env("PENGUINPAL_CONTROL_ARGS", serialized);
    }

    let start = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| ControlError::backend("backend_exec_failed", "PowerShell 控制脚本启动失败。", Some(error.to_string())))?;

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| ControlError::backend("backend_exec_failed", "控制脚本等待输出失败。", Some(error.to_string())))?;

                let elapsed_ms = start.elapsed().as_millis();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let _ = logging::append_log(
                    app,
                    tool,
                    if output.status.success() { "ok" } else { "backend_error" },
                    format!("durationMs={elapsed_ms} stdout={} stderr={}", truncate(&stdout), truncate(&stderr)),
                );

                if !output.status.success() {
                    let detail = if !stderr.is_empty() { stderr } else { stdout };
                    return Err(ControlError::backend(
                        "backend_exec_failed",
                        "控制脚本执行失败。",
                        if detail.is_empty() { None } else { Some(detail) },
                    ));
                }

                if stdout.is_empty() {
                    return Ok(json!({}));
                }

                return serde_json::from_str::<Value>(&stdout).map_err(|error| {
                    ControlError::backend(
                        "invalid_backend_response",
                        "控制脚本返回的不是合法 JSON。",
                        Some(error.to_string()),
                    )
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = logging::append_log(
                        app,
                        tool,
                        "timeout",
                        format!("timeoutMs={}", timeout.as_millis()),
                    );
                    return Err(ControlError::timeout(format!(
                        "{tool} 超时，{} ms 内未完成。",
                        timeout.as_millis()
                    )));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                return Err(ControlError::backend(
                    "backend_exec_failed",
                    "控制脚本状态查询失败。",
                    Some(error.to_string()),
                ))
            }
        }
    }
}

fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    STANDARD.encode(bytes)
}

fn truncate(value: &str) -> String {
    let mut preview: String = value.chars().take(180).collect();
    if value.chars().count() > 180 {
        preview.push_str("...");
    }
    preview.replace('\n', "\\n").replace('\r', "\\r")
}
