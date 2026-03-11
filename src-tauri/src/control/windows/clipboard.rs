use std::time::Duration;
use tauri::AppHandle;

use crate::control::errors::ControlResult;

use super::common::run_powershell_json;

pub fn read_clipboard(app: &AppHandle) -> ControlResult<serde_json::Value> {
    run_powershell_json(
        app,
        "read_clipboard",
        r#"
$ErrorActionPreference = 'Stop'
$text = Get-Clipboard -Raw
if ($null -eq $text) { $text = '' }
[string]$text = $text
if ($text.Length -gt 8192) { $text = $text.Substring(0, 8192) }
[pscustomobject]@{ text = [string]$text } | ConvertTo-Json -Compress -Depth 3
"#,
        None,
        Duration::from_secs(3),
    )
}
