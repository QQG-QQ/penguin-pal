use serde_json::{json, Value};
use std::time::Duration;
use tauri::AppHandle;

use crate::control::{
    errors::{ControlError, ControlResult},
    logging,
    types::UiSelector,
};

use super::{
    common::{run_powershell_json, INPUT_PREAMBLE, UIA_PREAMBLE},
    selector,
};

pub fn find_element(app: &AppHandle, selector_value: &Value) -> ControlResult<Value> {
    let selector = selector::parse_selector(selector_value)?;
    run_uia(app, "find_element", &selector, None, build_find_element_script(), Duration::from_secs(5))
}

pub fn click_element(app: &AppHandle, selector_value: &Value) -> ControlResult<Value> {
    let selector = selector::parse_selector(selector_value)?;
    let selector_json = selector::selector_to_value(&selector)?;
    let _ = logging::append_log(
        app,
        "click_element",
        "request",
        selector_json.to_string(),
    );
    let mut result = run_uia(
        app,
        "click_element",
        &selector,
        None,
        &format!(
            r#"{INPUT_PREAMBLE}
{CLICK_HELPER}
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$selectorSummary = [pscustomobject]@{{
  windowTitle = [string]$payload.selector.windowTitle
  automationId = [string]$payload.selector.automationId
  name = [string]$payload.selector.name
  controlType = [string]$payload.selector.controlType
  className = [string]$payload.selector.className
  matchMode = [string]$payload.selector.matchMode
}}
$window = Get-WindowElement $payload.selector
$windowSummary = [pscustomobject]@{{
  title = [string]$window.Current.Name
  automationId = [string]$window.Current.AutomationId
  controlType = [string](Get-ControlTypeName $window)
  className = [string]$window.Current.ClassName
}}
try {{
  $window.SetFocus()
  Start-Sleep -Milliseconds 120
}} catch {{}}
try {{
  $found = Find-ElementCore $payload.selector
}} catch {{
  $debug = [pscustomobject]@{{
    selector = $selectorSummary
    window = $windowSummary
    stage = 'find_after_focus'
    error = $_.Exception.Message
  }}
  throw ('未找到匹配的 UI 元素。 debug=' + ($debug | ConvertTo-Json -Compress -Depth 6))
}}
$element = $found.element
$summary = Convert-ElementSummary $element $found.windowTitle
$supportedPatterns = New-Object System.Collections.Generic.List[string]
$attemptErrors = New-Object System.Collections.Generic.List[string]
$strategy = $null

function Try-GetPattern($element, $pattern, [string]$name, $supportedPatterns) {{
  try {{
    if ($null -eq $pattern) {{
      return $null
    }}
    $instance = $element.GetCurrentPattern($pattern)
    if ($null -ne $instance) {{
      $supportedPatterns.Add($name) | Out-Null
      return $instance
    }}
  }} catch {{}}
  return $null
}}

function Resolve-PatternIdentifier([string]$typeName) {{
  $qualifiedName = 'System.Windows.Automation.' + $typeName + ', UIAutomationClient'
  $type = [System.Type]::GetType($qualifiedName, $false)
  if ($null -eq $type) {{
    return $null
  }}
  $property = $type.GetProperty('Pattern', [System.Reflection.BindingFlags] 'Public, Static')
  if ($null -eq $property) {{
    return $null
  }}
  return $property.GetValue($null, $null)
}}

function Try-GetPatternByTypeName($element, [string]$typeName, [string]$label, $supportedPatterns) {{
  $pattern = Resolve-PatternIdentifier $typeName
  return Try-GetPattern $element $pattern $label $supportedPatterns
}}

function Try-ClickStrategy([string]$name, [scriptblock]$Action, $attemptErrors) {{
  try {{
    & $Action
    return $true
  }} catch {{
    $attemptErrors.Add(($name + ': ' + $_.Exception.Message)) | Out-Null
    return $false
  }}
}}

$invokePattern = Try-GetPatternByTypeName $element 'InvokePattern' 'InvokePattern' $supportedPatterns
$selectionItemPattern = Try-GetPatternByTypeName $element 'SelectionItemPattern' 'SelectionItemPattern' $supportedPatterns
$legacyPattern = Try-GetPatternByTypeName $element 'LegacyIAccessiblePattern' 'LegacyIAccessiblePattern' $supportedPatterns
$expandCollapsePattern = Try-GetPatternByTypeName $element 'ExpandCollapsePattern' 'ExpandCollapsePattern' $supportedPatterns
$togglePattern = Try-GetPatternByTypeName $element 'TogglePattern' 'TogglePattern' $supportedPatterns

if ($summary.controlType -in @('MenuBarItem', 'MenuItem')) {{
  if ($null -ne $expandCollapsePattern -and (Try-ClickStrategy 'ExpandCollapsePattern.Expand' {{ $expandCollapsePattern.Expand() }} $attemptErrors)) {{
    $strategy = 'ExpandCollapsePattern.Expand'
  }}
}}

if ($null -eq $strategy -and $null -ne $invokePattern) {{
  if (Try-ClickStrategy 'InvokePattern.Invoke' {{ $invokePattern.Invoke() }} $attemptErrors) {{
    $strategy = 'InvokePattern.Invoke'
  }}
}}

if ($null -eq $strategy -and $null -ne $selectionItemPattern) {{
  if (Try-ClickStrategy 'SelectionItemPattern.Select' {{ $selectionItemPattern.Select() }} $attemptErrors) {{
    $strategy = 'SelectionItemPattern.Select'
  }}
}}

if ($null -eq $strategy -and $null -ne $legacyPattern) {{
  if (Try-ClickStrategy 'LegacyIAccessiblePattern.DoDefaultAction' {{ $legacyPattern.DoDefaultAction() }} $attemptErrors) {{
    $strategy = 'LegacyIAccessiblePattern.DoDefaultAction'
  }}
}}

if ($null -eq $strategy -and $null -ne $togglePattern) {{
  if (Try-ClickStrategy 'TogglePattern.Toggle' {{ $togglePattern.Toggle() }} $attemptErrors) {{
    $strategy = 'TogglePattern.Toggle'
  }}
}}

if ($null -eq $strategy) {{
  if (Try-ClickStrategy 'SetFocus+CenterClick' {{
    $element.SetFocus()
    Start-Sleep -Milliseconds 80
    Click-ElementCenter $element
  }} $attemptErrors) {{
    $strategy = 'SetFocus+CenterClick'
  }}
}}

if ($null -eq $strategy) {{
  $debug = [pscustomobject]@{{
    selector = $selectorSummary
    window = $windowSummary
    controlType = $summary.controlType
    supportedPatterns = @($supportedPatterns.ToArray())
    strategy = $null
    attemptErrors = @($attemptErrors.ToArray())
  }}
  throw ('click_element 执行失败。 debug=' + ($debug | ConvertTo-Json -Compress -Depth 6))
}}
[pscustomobject]@{{
  strategy = $strategy
  element = $summary
  debug = [pscustomobject]@{{
    selector = $selectorSummary
    window = $windowSummary
    controlType = $summary.controlType
    supportedPatterns = @($supportedPatterns.ToArray())
    strategy = $strategy
    attemptErrors = @($attemptErrors.ToArray())
  }}
}} | ConvertTo-Json -Compress -Depth 6
"#,
        ),
        Duration::from_secs(5),
    )?;

    if let Some(debug) = result.get("debug").cloned() {
        let _ = logging::append_log(app, "click_element", "debug", debug.to_string());
        if let Some(object) = result.as_object_mut() {
            object.remove("debug");
        }
    }

    Ok(result)
}

pub fn get_element_text(app: &AppHandle, selector_value: &Value) -> ControlResult<Value> {
    let selector = selector::parse_selector(selector_value)?;
    run_uia(
        app,
        "get_element_text",
        &selector,
        None,
        r#"
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$found = Find-ElementCore $payload.selector
$element = $found.element
$text = $null
try {
  $valuePattern = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
  $text = $valuePattern.Current.Value
} catch {}
if ([string]::IsNullOrWhiteSpace($text)) {
  try {
    $textPattern = $element.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern)
    $text = $textPattern.DocumentRange.GetText(-1)
  } catch {}
}
if ([string]::IsNullOrWhiteSpace($text)) {
  $text = [string]$element.Current.Name
}
[pscustomobject]@{
  text = [string]$text
  element = (Convert-ElementSummary $element $found.windowTitle)
} | ConvertTo-Json -Compress -Depth 6
"#,
        Duration::from_secs(5),
    )
}

pub fn set_element_value(
    app: &AppHandle,
    selector_value: &Value,
    text: &str,
) -> ControlResult<Value> {
    let selector = selector::parse_selector(selector_value)?;
    let payload = Some(json!({ "text": text }));
    run_uia(
        app,
        "set_element_value",
        &selector,
        payload,
        r#"
Add-Type -AssemblyName System.Windows.Forms
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$found = Find-ElementCore $payload.selector
$element = $found.element
$text = [string]$payload.text
function Escape-SendKeys([string]$value) {
  return ($value -replace '([+^%~(){}\[\]])', '{$1}')
}
$usedFallback = $false
try {
  $valuePattern = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
  $valuePattern.SetValue($text)
} catch {
  $usedFallback = $true
  $element.SetFocus()
  Start-Sleep -Milliseconds 80
  [System.Windows.Forms.SendKeys]::SendWait('^a')
  Start-Sleep -Milliseconds 40
  [System.Windows.Forms.SendKeys]::SendWait((Escape-SendKeys $text))
}
[pscustomobject]@{
  usedFallback = $usedFallback
  textLength = $text.Length
  element = (Convert-ElementSummary $element $found.windowTitle)
} | ConvertTo-Json -Compress -Depth 6
"#,
        Duration::from_secs(5),
    )
}

pub fn wait_for_element(
    app: &AppHandle,
    selector_value: &Value,
    timeout_ms: i64,
) -> ControlResult<Value> {
    let selector = selector::parse_selector(selector_value)?;
    let clamped_timeout = timeout_ms.clamp(500, 10_000);
    let payload = Some(json!({ "timeoutMs": clamped_timeout }));
    run_uia(
        app,
        "wait_for_element",
        &selector,
        payload,
        r#"
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$deadline = [DateTime]::UtcNow.AddMilliseconds([int]$payload.timeoutMs)
while ([DateTime]::UtcNow -lt $deadline) {
  try {
    $found = Find-ElementCore $payload.selector
    (Convert-ElementSummary $found.element $found.windowTitle) | ConvertTo-Json -Compress -Depth 6
    exit 0
  } catch {}
  Start-Sleep -Milliseconds 250
}
throw '在超时时间内未找到匹配的 UI 元素。'
"#,
        Duration::from_millis((clamped_timeout + 1500) as u64),
    )
}

fn run_uia(
    app: &AppHandle,
    tool: &str,
    selector: &UiSelector,
    extra_payload: Option<Value>,
    body: &str,
    timeout: Duration,
) -> ControlResult<Value> {
    let selector_json = selector::selector_to_value(selector)?;
    let mut root = json!({ "selector": selector_json });
    if let Some(extra) = extra_payload {
        if let (Some(root_map), Some(extra_map)) = (root.as_object_mut(), extra.as_object()) {
            for (key, value) in extra_map {
                root_map.insert(key.clone(), value.clone());
            }
        }
    }

    let script = format!("{UIA_PREAMBLE}\n{body}");
    run_powershell_json(app, tool, &script, Some(&root), timeout).map_err(|error| {
        if let Some(detail) = error.payload().detail.as_deref() {
            let _ = logging::append_log(app, tool, "error_detail", detail);
        }
        if error
            .payload()
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("在超时时间内未找到匹配的 UI 元素"))
        {
            ControlError::timeout("在超时时间内未找到匹配的 UI 元素。")
        } else if error.payload().code == "backend_exec_failed"
            && error
                .payload()
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("未找到匹配"))
        {
            ControlError::not_found("element_not_found", "未找到匹配的 UI 元素。")
        } else if error
            .payload()
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("未找到匹配窗口"))
        {
            ControlError::not_found("window_not_found", "未找到匹配窗口。")
        } else {
            error
        }
    })
}

fn build_find_element_script() -> &'static str {
    r#"
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$found = Find-ElementCore $payload.selector
Convert-ElementSummary $found.element $found.windowTitle | ConvertTo-Json -Compress -Depth 6
"#
}

const CLICK_HELPER: &str = r#"
function Click-ElementCenter($element) {
  $rect = $element.Current.BoundingRectangle
  if ($null -eq $rect -or $rect.Width -le 0 -or $rect.Height -le 0) {
    throw '目标元素没有可点击的边界矩形。'
  }
  $x = [int]($rect.Left + ($rect.Width / 2))
  $y = [int]($rect.Top + ($rect.Height / 2))
  [void][PenguinPalInputApi]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 40
  [PenguinPalInputApi]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
  [PenguinPalInputApi]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
}
"#;
