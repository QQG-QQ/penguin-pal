use serde_json::{json, Value};
use std::time::Duration;
use tauri::AppHandle;

use crate::control::{
    errors::ControlResult,
    logging,
};

use super::common::{run_powershell_json, WINDOW_ENUM_PREAMBLE};

pub fn list_windows(app: &AppHandle) -> ControlResult<Value> {
    let script = format!(
        r#"{WINDOW_ENUM_PREAMBLE}
Get-VisibleWindowSummaries | ConvertTo-Json -Compress -Depth 6
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
$needle = Normalize-WindowText $payload.title
$matchMode = Normalize-MatchMode $payload.match
$windows = @(Get-VisibleWindowSummaries)
$candidateTitles = @($windows | ForEach-Object {{ Normalize-WindowText $_.title }})
$matched = Find-MatchingWindowSummary $windows $needle $matchMode
$debug = [pscustomobject]@{{
  requestTitle = [string]$needle
  requestMatch = [string]$matchMode
  candidateCount = @($candidateTitles).Count
  candidateTitles = $candidateTitles
  matchedType = if ($null -eq $matched) {{ 'null' }} else {{ $matched.GetType().FullName }}
  matchedHandle = if ($null -eq $matched) {{ $null }} else {{ [int64]$matched.handle }}
  matchedTitle = if ($null -eq $matched) {{ $null }} else {{ [string](Normalize-WindowText $matched.title) }}
}}
if ($null -eq $matched) {{
  throw ('未找到匹配窗口。 debug=' + ($debug | ConvertTo-Json -Compress -Depth 6))
}}
$handle = [IntPtr]([int64]$matched.handle)
[void][PenguinPalWinApi]::ShowWindow($handle, 9)
[void][PenguinPalWinApi]::SetForegroundWindow($handle)
[pscustomobject]@{{
  handle = [int64]$matched.handle
  title = [string](Normalize-WindowText $matched.title)
  debug = $debug
}} | ConvertTo-Json -Compress -Depth 6
"#
    );

    let _ = logging::append_log(
        app,
        "focus_window",
        "request",
        format!("title={title} match={match_mode}"),
    );
    let mut result = run_powershell_json(
        app,
        "focus_window",
        &script,
        Some(&args),
        Duration::from_secs(3),
    )?;

    if let Some(debug) = result.get("debug").cloned() {
        let _ = logging::append_log(app, "focus_window", "debug", debug.to_string());
        if let Some(object) = result.as_object_mut() {
            object.remove("debug");
        }
    }

    Ok(result)
}
