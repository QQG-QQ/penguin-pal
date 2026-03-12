use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::control::{
    errors::{ControlError, ControlResult},
};

use super::common::{run_powershell_json, UIA_PREAMBLE, WINDOW_ENUM_PREAMBLE};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowUiBounds {
    pub left: i64,
    pub top: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowUiElementSummary {
    pub role: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub automation_id: Option<String>,
    #[serde(default)]
    pub class_name: Option<String>,
    pub is_enabled: bool,
    pub is_offscreen: bool,
    #[serde(default)]
    pub value_preview: Option<String>,
    #[serde(default)]
    pub bounds: Option<WindowUiBounds>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowUiDescription {
    pub window_title: String,
    #[serde(default)]
    pub window_class_name: Option<String>,
    #[serde(default)]
    pub focus_hint: Option<String>,
    #[serde(default)]
    pub visible_elements: Vec<WindowUiElementSummary>,
}

pub fn describe_active_window_ui(app: &AppHandle) -> ControlResult<WindowUiDescription> {
    let script = format!(
        r#"{WINDOW_ENUM_PREAMBLE}
{UIA_PREAMBLE}
$foreground = [PenguinPalWinApi]::GetForegroundWindow()
if ($foreground -eq [IntPtr]::Zero) {{ throw '当前没有活动窗口。' }}
$activeTitle = Normalize-WindowText (Get-WindowTitle $foreground)
if ([string]::IsNullOrWhiteSpace($activeTitle)) {{ throw '当前活动窗口标题为空。' }}
$selector = [pscustomobject]@{{
  windowTitle = $activeTitle
  automationId = ''
  name = ''
  controlType = ''
  className = ''
  matchMode = 'exact'
}}
$window = Get-WindowElement $selector
$allowedRoles = @('Button', 'Edit', 'Document', 'MenuItem', 'TabItem', 'ListItem', 'ComboBox', 'Hyperlink', 'CheckBox', 'RadioButton')
$seen = New-Object 'System.Collections.Generic.HashSet[string]'
$items = New-Object 'System.Collections.Generic.List[object]'
$focusedName = $null
try {{
  $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
  if ($null -ne $focused) {{
    $focusedName = [string]$focused.Current.Name
  }}
}} catch {{}}
$nodes = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
for ($i = 0; $i -lt $nodes.Count; $i++) {{
  $node = $nodes.Item($i)
  $role = [string](Get-ControlTypeName $node)
  if ([string]::IsNullOrWhiteSpace($role) -or ($allowedRoles -notcontains $role)) {{ continue }}
  if ([bool]$node.Current.IsOffscreen) {{ continue }}
  $rect = $node.Current.BoundingRectangle
  if ($rect.Width -lt 2 -or $rect.Height -lt 2) {{ continue }}
  $name = [string]$node.Current.Name
  $automationId = [string]$node.Current.AutomationId
  $className = [string]$node.Current.ClassName
  if ([string]::IsNullOrWhiteSpace($name) -and [string]::IsNullOrWhiteSpace($automationId) -and [string]::IsNullOrWhiteSpace($className)) {{
    continue
  }}
  $signature = ((Normalize-Text $role) + '|' + (Normalize-Text $name) + '|' + (Normalize-Text $automationId) + '|' + (Normalize-Text $className))
  if (-not $seen.Add($signature)) {{ continue }}
  $valuePreview = $null
  try {{
    $valuePattern = $node.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    if ($null -ne $valuePattern) {{
      $candidate = [string]$valuePattern.Current.Value
      if (-not [string]::IsNullOrWhiteSpace($candidate)) {{
        $valuePreview = $candidate.Trim()
        if ($valuePreview.Length -gt 64) {{
          $valuePreview = $valuePreview.Substring(0, 64) + '…'
        }}
      }}
    }}
  }} catch {{}}
  $items.Add([pscustomobject]@{{
    role = $role
    name = if ([string]::IsNullOrWhiteSpace($name)) {{ $null }} else {{ $name }}
    automationId = if ([string]::IsNullOrWhiteSpace($automationId)) {{ $null }} else {{ $automationId }}
    className = if ([string]::IsNullOrWhiteSpace($className)) {{ $null }} else {{ $className }}
    isEnabled = [bool]$node.Current.IsEnabled
    isOffscreen = [bool]$node.Current.IsOffscreen
    valuePreview = $valuePreview
    bounds = [pscustomobject]@{{
      left = [int]$rect.Left
      top = [int]$rect.Top
      width = [int][Math]::Max(0, $rect.Width)
      height = [int][Math]::Max(0, $rect.Height)
    }}
  }}) | Out-Null
  if ($items.Count -ge 16) {{ break }}
}}
[pscustomobject]@{{
  windowTitle = [string]$window.Current.Name
  windowClassName = if ([string]::IsNullOrWhiteSpace([string]$window.Current.ClassName)) {{ $null }} else {{ [string]$window.Current.ClassName }}
  focusHint = if ([string]::IsNullOrWhiteSpace($focusedName)) {{ $null }} else {{ $focusedName }}
  visibleElements = @($items.ToArray())
}} | ConvertTo-Json -Compress -Depth 6
"#
    );

    let payload = run_powershell_json(
        app,
        "describe_window_ui",
        &script,
        None,
        Duration::from_secs(5),
    )?;

    serde_json::from_value(payload).map_err(|error| {
        ControlError::backend(
            "invalid_backend_response",
            "describe_window_ui 返回结构无效。",
            Some(error.to_string()),
        )
    })
}
