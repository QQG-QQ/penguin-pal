use std::{sync::Mutex, time::Duration};

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager, State};

use crate::{
    agent::{
        screen_context::{self},
        types::{AgentTaskProgress, AgentTaskStatus},
    },
    app_state::RuntimeState,
    control::{
        router as control_router,
        types::{ControlPendingRequest, ToolInvokeRequest, ToolInvokeResponse},
        windows::common::run_powershell_json,
    },
};

use super::{
    assertions::{self, AssertionContext},
    history,
    registry,
    retry,
    types::{
        FailureItem, TestAssertion, TestCase, TestCaseResult, TestCaseStatus, TestFailureStage,
        TestRunReport, TestRunRequest, TestRunState, TestRunStatus, TestRunSummary, TestStep,
    },
};

#[derive(Debug, Clone)]
pub struct HarnessExecutionResult {
    pub reply_text: String,
    pub report: TestRunReport,
    pub pending_request: Option<ControlPendingRequest>,
    pub task: Option<AgentTaskProgress>,
}

#[derive(Default)]
struct CaseContext {
    vars: Map<String, Value>,
    screen_context: Option<Value>,
    last_result: Option<Value>,
}

pub async fn execute_request(
    app: &AppHandle,
    request: TestRunRequest,
) -> Result<HarnessExecutionResult, String> {
    if has_active_run(app)? {
        return Ok(blocked_report(
            request,
            "当前还有一个未完成的测试任务，请先确认或取消。".to_string(),
        ));
    }

    let mut selected_cases = resolve_selected_cases(app, &request)?;
    if request.max_cases > 0 && selected_cases.len() > request.max_cases {
        selected_cases.truncate(request.max_cases);
    }
    if selected_cases.is_empty() {
        return Ok(blocked_report(
            request,
            "没有匹配到可执行的测试 case。".to_string(),
        ));
    }

    let now = crate::app_state::now_millis();
    let report = TestRunReport {
        run_id: format!("test-run-{now}"),
        title: request.title.clone(),
        selector: request.selection.clone(),
        dynamic_cases: request.dynamic_cases.clone(),
        started_at: now,
        finished_at: None,
        status: TestRunStatus::Running,
        summary: TestRunSummary {
            total: selected_cases.len(),
            passed: 0,
            failed: 0,
            blocked: 0,
            skipped: 0,
            rerun_count: 0,
        },
        case_results: vec![],
        failure_items: vec![],
        recent_failed_summary: history::recent_failed_summary(app).unwrap_or_default(),
    };

    let mut state = TestRunState {
        report,
        selected_cases,
        current_case_index: 0,
        waiting_pending_id: None,
        waiting_case_index: None,
        allow_supplementary_rerun: request.allow_supplementary_rerun,
    };

    continue_run(app, &mut state, None).await
}

pub async fn confirm_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<ToolInvokeResponse>, String> {
    let Some(mut run) = take_run_waiting_on_pending(app, pending_id)? else {
        return Ok(None);
    };

    let confirmed = control_router::confirm(app, pending_id).map_err(|error| error.to_string())?;
    let waiting_index = run
        .waiting_case_index
        .ok_or_else(|| "测试运行状态异常：缺少等待确认的 case。".to_string())?;
    let current_case = run
        .selected_cases
        .get(waiting_index)
        .cloned()
        .ok_or_else(|| "测试运行状态异常：待确认 case 不存在。".to_string())?;
    let current_result = run
        .report
        .case_results
        .get_mut(waiting_index)
        .ok_or_else(|| "测试运行状态异常：待确认结果不存在。".to_string())?;

    let confirmed_payload = confirmed.result.clone().unwrap_or_else(|| json!({}));
    if let Some(last_step) = current_result.step_results.last_mut() {
        last_step.status = TestCaseStatus::Passed;
        last_step.payload = Some(confirmed_payload);
        last_step.detail = Some("高风险步骤已确认并执行。".to_string());
    }
    current_result.status = TestCaseStatus::Passed;
    run.report.status = TestRunStatus::Running;
    run.waiting_case_index = None;
    run.waiting_pending_id = None;
    run.current_case_index = waiting_index + 1;

    let result = continue_run(app, &mut run, Some(current_case)).await?;
    Ok(Some(to_tool_response(result)))
}

pub async fn cancel_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<ToolInvokeResponse>, String> {
    let Some(mut run) = take_run_waiting_on_pending(app, pending_id)? else {
        return Ok(None);
    };

    let _ = control_router::cancel(app, pending_id).map_err(|error| error.to_string())?;
    run.waiting_case_index = None;
    run.waiting_pending_id = None;
    run.report.status = TestRunStatus::Cancelled;
    run.report.finished_at = Some(crate::app_state::now_millis());
    update_summary(&mut run.report);
    history::persist_report(app, &run.report)?;
    replace_active_run(app, None)?;

    Ok(Some(ToolInvokeResponse {
        status: "success".to_string(),
        result: Some(json!({
            "cancelled": true,
            "task": progress_from_report(&run.report, None, Some("测试任务已取消。".to_string())),
        })),
        message: Some(format!("测试任务“{}”已取消。", run.report.title)),
        pending_request: None,
        error: None,
    }))
}

async fn continue_run(
    app: &AppHandle,
    state: &mut TestRunState,
    resumed_case: Option<TestCase>,
) -> Result<HarnessExecutionResult, String> {
    let mut case_context = CaseContext::default();

    if let Some(case) = resumed_case {
        execute_case(app, state, &case, &mut case_context, true).await?;
    }

    while state.current_case_index < state.selected_cases.len() {
        let case = state.selected_cases[state.current_case_index].clone();
        if let Some(result) =
            execute_case(app, state, &case, &mut CaseContext::default(), false).await?
        {
            return Ok(result);
        }
        state.current_case_index += 1;
    }

    if state.allow_supplementary_rerun {
        rerun_failed_cases(app, state).await?;
    }

    finalize_report(app, state)
}

async fn execute_case(
    app: &AppHandle,
    state: &mut TestRunState,
    case: &TestCase,
    case_context: &mut CaseContext,
    resume_existing: bool,
) -> Result<Option<HarnessExecutionResult>, String> {
    let mut case_result = if resume_existing {
        state
            .report
            .case_results
            .get(state.current_case_index)
            .cloned()
            .ok_or_else(|| "测试运行状态异常：缺少当前 case 结果。".to_string())?
    } else {
        TestCaseResult {
            case_id: case.id.clone(),
            title: case.title.clone(),
            suite: case.suite.clone(),
            feature: case.feature.clone(),
            status: TestCaseStatus::Passed,
            started_at: crate::app_state::now_millis(),
            finished_at: crate::app_state::now_millis(),
            destructive_level: case.destructive_level.clone(),
            test_target_policy: case.test_target_policy.clone(),
            step_results: vec![],
            failure_reason: None,
            failure_stage: None,
            probes_used: 0,
            rerun_count: 0,
        }
    };

    for precondition in &case.preconditions {
        validate_precondition(case_context, precondition)?;
    }

    let start_step = if resume_existing {
        case_result.step_results.len()
    } else {
        0
    };

    for (index, step) in case.steps.iter().enumerate().skip(start_step) {
        match execute_step(app, case_context, step).await {
            Ok(StepOutcome::Done { payload, detail }) => {
                case_result.step_results.push(super::types::TestStepResult {
                    index: index + 1,
                    summary: step_summary(step),
                    status: TestCaseStatus::Passed,
                    tool: step_tool(step),
                    detail: Some(detail),
                    payload: payload.clone(),
                });
                case_context.last_result = payload;
            }
            Ok(StepOutcome::Pending { pending_request }) => {
                case_result.status = TestCaseStatus::WaitingConfirmation;
                case_result.step_results.push(super::types::TestStepResult {
                    index: index + 1,
                    summary: step_summary(step),
                    status: TestCaseStatus::WaitingConfirmation,
                    tool: step_tool(step),
                    detail: Some("步骤等待人工确认。".to_string()),
                    payload: None,
                });
                upsert_case_result(state, case_result.clone());
                state.report.status = TestRunStatus::WaitingConfirmation;
                state.waiting_case_index = Some(state.current_case_index);
                state.waiting_pending_id = Some(pending_request.id.clone());
                replace_active_run(app, Some(state.clone()))?;
                return Ok(Some(pending_report(
                    state,
                    pending_request,
                    case,
                    index + 1,
                )));
            }
            Err((stage, reason)) => {
                case_result.status = TestCaseStatus::Failed;
                case_result.finished_at = crate::app_state::now_millis();
                case_result.failure_reason = Some(reason.clone());
                case_result.failure_stage = Some(stage.clone());
                upsert_case_result(state, case_result.clone());
                state.report.failure_items.push(FailureItem {
                    case_id: case.id.clone(),
                    case_title: case.title.clone(),
                    failure_stage: stage,
                    step_index: index + 1,
                    step_name: step_summary(step),
                    reason,
                    rerunnable: true,
                });
                return Ok(None);
            }
        }
    }

    for (index, assertion) in case.assertions.iter().enumerate() {
        let resolved_params = match resolve_args(&assertion.params, &case_context.vars) {
            Ok(params) => params,
            Err(reason) => {
                case_result.status = TestCaseStatus::Failed;
                case_result.finished_at = crate::app_state::now_millis();
                case_result.failure_reason = Some(reason.clone());
                case_result.failure_stage = Some(TestFailureStage::Assertion);
                upsert_case_result(state, case_result.clone());
                state.report.failure_items.push(FailureItem {
                    case_id: case.id.clone(),
                    case_title: case.title.clone(),
                    failure_stage: TestFailureStage::Assertion,
                    step_index: case.steps.len() + index + 1,
                    step_name: assertion.summary.clone(),
                    reason,
                    rerunnable: true,
                });
                return Ok(None);
            }
        };
        let resolved_assertion = TestAssertion {
            kind: assertion.kind.clone(),
            params: resolved_params,
            summary: assertion.summary.clone(),
        };
        if let Err((stage, reason)) = assertions::evaluate(
            &resolved_assertion,
            &AssertionContext {
                vars: case_context.vars.clone(),
                screen_context: case_context.screen_context.clone(),
                last_result: case_context.last_result.clone(),
            },
        ) {
            case_result.status = TestCaseStatus::Failed;
            case_result.finished_at = crate::app_state::now_millis();
            case_result.failure_reason = Some(reason.clone());
            case_result.failure_stage = Some(stage.clone());
            upsert_case_result(state, case_result.clone());
            let failure = FailureItem {
                case_id: case.id.clone(),
                case_title: case.title.clone(),
                failure_stage: stage,
                step_index: case.steps.len() + index + 1,
                step_name: assertion.summary.clone(),
                reason,
                rerunnable: true,
            };
            state.report.failure_items.push(failure);
            return Ok(None);
        }
    }

    case_result.status = TestCaseStatus::Passed;
    case_result.finished_at = crate::app_state::now_millis();
    upsert_case_result(state, case_result);
    Ok(None)
}

async fn rerun_failed_cases(app: &AppHandle, state: &mut TestRunState) -> Result<(), String> {
    let failed_cases = state
        .report
        .failure_items
        .clone()
        .into_iter()
        .filter_map(|failure| {
            let case = state
                .selected_cases
                .iter()
                .find(|item| item.id == failure.case_id)?
                .clone();
            if retry::should_rerun_failure(&case, &failure) {
                Some((case, failure))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if failed_cases.is_empty() {
        return Ok(());
    }

    state.report.failure_items.clear();

    for (case, failure) in failed_cases {
        let probes = retry::supplementary_probes(&case, &failure);
        let mut probe_context = CaseContext::default();
        for probe in &probes {
            let _ = execute_step(app, &mut probe_context, probe).await;
        }

        if let Some(existing) = state
            .report
            .case_results
            .iter_mut()
            .find(|result| result.case_id == case.id)
        {
            existing.probes_used += probes.len();
            existing.rerun_count += 1;
        }
        state.report.summary.rerun_count += 1;

        if let Some(position) = state.selected_cases.iter().position(|item| item.id == case.id) {
            state.current_case_index = position;
            let _ = execute_case(app, state, &case, &mut CaseContext::default(), false).await?;
        }
    }

    Ok(())
}

enum StepOutcome {
    Done {
        payload: Option<Value>,
        detail: String,
    },
    Pending {
        pending_request: ControlPendingRequest,
    },
}

async fn execute_step(
    app: &AppHandle,
    case_context: &mut CaseContext,
    step: &TestStep,
) -> Result<StepOutcome, (TestFailureStage, String)> {
    match step {
        TestStep::ControlInvoke { tool, args, .. } => {
            let resolved_args = resolve_args(args, &case_context.vars)
                .map_err(|error| (TestFailureStage::StepExecute, error))?;
            let response = control_router::invoke(
                app,
                ToolInvokeRequest {
                    tool: tool.clone(),
                    args: resolved_args,
                },
            )
            .map_err(|error| (TestFailureStage::StepExecute, error.payload().message))?;

            if response.status == "pending_confirmation" {
                let pending_request = response.pending_request.ok_or_else(|| {
                    (
                        TestFailureStage::StepExecute,
                        "控制层返回了待确认状态，但缺少 pendingRequest。".to_string(),
                    )
                })?;
                return Ok(StepOutcome::Pending { pending_request });
            }

            let payload = response.result.clone();
            refresh_dynamic_vars(case_context, tool, payload.clone());
            Ok(StepOutcome::Done {
                payload,
                detail: response
                    .message
                    .unwrap_or_else(|| format!("{} 已执行。", tool)),
            })
        }
        TestStep::SeedClipboardText { text, .. } => {
            let payload = seed_clipboard_text(app, text)
                .map_err(|error| (TestFailureStage::StepExecute, error))?;
            Ok(StepOutcome::Done {
                payload: Some(payload),
                detail: format!("已写入测试剪贴板文本（{} 字符）。", text.chars().count()),
            })
        }
        TestStep::CaptureScreenContext { .. } => {
            let screen_context = capture_screen_context(app)
                .await
                .map_err(|error| (TestFailureStage::StepExecute, error))?;
            case_context.screen_context = Some(screen_context.clone());
            refresh_dynamic_vars_from_screen_context(case_context, &screen_context);
            Ok(StepOutcome::Done {
                payload: Some(screen_context),
                detail: "已采集当前 screen context。".to_string(),
            })
        }
    }
}

async fn capture_screen_context(app: &AppHandle) -> Result<Value, String> {
    let (provider, api_key, oauth_access_token, vision_channel, vision_api_key) = {
        let state: tauri::State<'_, Mutex<RuntimeState>> = app.state();
        let runtime = state.lock().map_err(|_| "助手状态锁定失败".to_string())?;
        (
            runtime.provider.clone(),
            runtime.api_key.clone(),
            runtime.oauth_access_token.clone(),
            runtime.vision_channel.clone(),
            runtime.vision_api_key.clone(),
        )
    };

    let context = screen_context::describe_current_screen(app, &vision_channel, vision_api_key).await;
    let mut value = serde_json::to_value(context).map_err(|error| error.to_string())?;
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "providerHint".to_string(),
            json!({
                "kind": provider.kind.label(),
                "allowNetwork": provider.allow_network,
                "apiKeyLoaded": api_key.as_ref().is_some_and(|value| !value.trim().is_empty()),
                "oauthLoaded": oauth_access_token.as_ref().is_some_and(|value| !value.trim().is_empty()),
            }),
        );
    }
    Ok(value)
}

fn validate_precondition(
    case_context: &CaseContext,
    precondition: &super::types::TestPrecondition,
) -> Result<(), String> {
    match precondition.kind.as_str() {
        "requires_window_var" => {
            let var_name = precondition
                .params
                .get("var")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if case_context.vars.contains_key(var_name) {
                Ok(())
            } else {
                Err(format!("缺少目标窗口变量：{var_name}"))
            }
        }
        _ => Ok(()),
    }
}

fn refresh_dynamic_vars(case_context: &mut CaseContext, tool: &str, payload: Option<Value>) {
    let Some(payload) = payload else {
        return;
    };

    if tool != "list_windows" {
        return;
    }

    let windows = payload
        .get("windows")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| payload.as_array().cloned())
        .unwrap_or_default();
    if let Some(title) = find_window_title(&windows, &["chrome", "edge", "firefox", "浏览器"]) {
        case_context
            .vars
            .insert("browserWindow".to_string(), Value::String(title));
    }
    if let Some(title) = find_window_title(&windows, &["notepad", "记事本"]) {
        case_context
            .vars
            .insert("notepadWindow".to_string(), Value::String(title));
    }
    if let Some(title) = find_window_title(&windows, &["微信"]) {
        case_context
            .vars
            .insert("wechatWindow".to_string(), Value::String(title));
    }
}

fn refresh_dynamic_vars_from_screen_context(case_context: &mut CaseContext, screen_context: &Value) {
    let Some(active_window) = screen_context.get("activeWindow") else {
        return;
    };

    if let Some(title) = active_window.get("title").and_then(Value::as_str) {
        let title = title.trim();
        if !title.is_empty() {
            case_context
                .vars
                .insert("activeWindowTitle".to_string(), Value::String(title.to_string()));
        }
    }

    let width = active_window
        .get("bounds")
        .and_then(|value| value.get("width"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let height = active_window
        .get("bounds")
        .and_then(|value| value.get("height"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if let Some((x, y)) = safe_center_for_active_window(width, height) {
        case_context
            .vars
            .insert("activeWindowSafeCenterX".to_string(), Value::Number(x.into()));
        case_context
            .vars
            .insert("activeWindowSafeCenterY".to_string(), Value::Number(y.into()));
    }
}

fn find_window_title(windows: &[Value], candidates: &[&str]) -> Option<String> {
    windows.iter().find_map(|item| {
        let title = item.get("title").and_then(Value::as_str)?.trim();
        if title.is_empty() {
            return None;
        }
        if candidates.iter().any(|candidate| title.contains(candidate)) {
            Some(title.to_string())
        } else {
            None
        }
    })
}

fn resolve_args(value: &Value, vars: &Map<String, Value>) -> Result<Value, String> {
    match value {
        Value::String(text) if text.starts_with('$') => {
            let key = text.trim_start_matches('$');
            vars.get(key)
                .cloned()
                .ok_or_else(|| format!("缺少动态参数：${key}"))
        }
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_args(item, vars))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut next = Map::new();
            for (key, value) in map {
                next.insert(key.clone(), resolve_args(value, vars)?);
            }
            Ok(Value::Object(next))
        }
        other => Ok(other.clone()),
    }
}

fn resolve_selected_cases(app: &AppHandle, request: &TestRunRequest) -> Result<Vec<TestCase>, String> {
    let selection = &request.selection;
    let mut cases = if selection.rerun_failed_only {
        let (case_ids, failed_dynamic_cases) = history::load_last_failed_selection(app)?;
        if case_ids.is_empty() && failed_dynamic_cases.is_empty() {
            vec![]
        } else {
            let mut selection = selection.clone();
            selection.case_ids = case_ids;
            selection.rerun_failed_only = false;
            let mut cases = registry::select_cases(&selection);
            for dynamic_case in failed_dynamic_cases {
                if !cases.iter().any(|existing| existing.id == dynamic_case.id) {
                    cases.push(dynamic_case);
                }
            }
            cases
        }
    } else {
        registry::select_cases(selection)
    };

    for dynamic_case in &request.dynamic_cases {
        if !cases.iter().any(|existing| existing.id == dynamic_case.id) {
            cases.push(dynamic_case.clone());
        }
    }

    Ok(cases)
}

fn seed_clipboard_text(app: &AppHandle, text: &str) -> Result<Value, String> {
    let args = json!({ "text": text });
    run_powershell_json(
        app,
        "test_seed_clipboard",
        r#"
$ErrorActionPreference = 'Stop'
$payload = $env:PENGUINPAL_CONTROL_ARGS | ConvertFrom-Json
$text = [string]$payload.text
Set-Clipboard -Value $text
[pscustomobject]@{ text = $text; length = $text.Length } | ConvertTo-Json -Compress -Depth 3
"#,
        Some(&args),
        Duration::from_secs(3),
    )
    .map_err(|error| error.payload().message)
}

fn safe_center_for_active_window(width: i64, height: i64) -> Option<(i64, i64)> {
    if width < 200 || height < 200 {
        return None;
    }

    let min_x = 120_i64.min(width / 2);
    let max_x = (width - 120).max(min_x);
    let min_y = 160_i64.min(height / 2);
    let max_y = (height - 100).max(min_y);
    let x = (width / 2).clamp(min_x, max_x);
    let y = ((height * 2) / 3).clamp(min_y, max_y);
    Some((x, y))
}

fn finalize_report(
    app: &AppHandle,
    state: &mut TestRunState,
) -> Result<HarnessExecutionResult, String> {
    update_summary(&mut state.report);
    state.report.finished_at = Some(crate::app_state::now_millis());
    state.report.status = if state.report.failure_items.is_empty() {
        TestRunStatus::Passed
    } else {
        TestRunStatus::Failed
    };
    history::persist_report(app, &state.report)?;
    replace_active_run(app, None)?;

    let reply_text = render_report_text(&state.report);
    Ok(HarnessExecutionResult {
        reply_text,
        report: state.report.clone(),
        pending_request: None,
        task: Some(progress_from_report(
            &state.report,
            None,
            Some(history::status_message(&state.report).to_string()),
        )),
    })
}

fn render_report_text(report: &TestRunReport) -> String {
    let mut lines = vec![
        format!("测试任务“{}”已结束。", report.title),
        format!(
            "结果：总计 {}，通过 {}，失败 {}，阻止 {}，跳过 {}，重测 {}。",
            report.summary.total,
            report.summary.passed,
            report.summary.failed,
            report.summary.blocked,
            report.summary.skipped,
            report.summary.rerun_count
        ),
    ];

    if !report.failure_items.is_empty() {
        lines.push("失败项：".to_string());
        for item in report.failure_items.iter().take(5) {
            lines.push(format!(
                "- {} / {:?}：{}",
                item.case_title, item.failure_stage, item.reason
            ));
        }
    }

    if !report.recent_failed_summary.is_empty() {
        lines.push("最近失败摘要：".to_string());
        for item in report.recent_failed_summary.iter().take(3) {
            lines.push(format!("- {item}"));
        }
    }

    lines.join("\n")
}

fn blocked_report(request: TestRunRequest, reason: String) -> HarnessExecutionResult {
    let report = TestRunReport {
        run_id: format!("test-run-{}", crate::app_state::now_millis()),
        title: request.title,
        selector: request.selection,
        dynamic_cases: request.dynamic_cases,
        started_at: crate::app_state::now_millis(),
        finished_at: Some(crate::app_state::now_millis()),
        status: TestRunStatus::Blocked,
        summary: TestRunSummary {
            total: 0,
            passed: 0,
            failed: 0,
            blocked: 1,
            skipped: 0,
            rerun_count: 0,
        },
        case_results: vec![],
        failure_items: vec![FailureItem {
            case_id: "selection".to_string(),
            case_title: "测试选择".to_string(),
            failure_stage: TestFailureStage::Selection,
            step_index: 0,
            step_name: "选择测试集".to_string(),
            reason: reason.clone(),
            rerunnable: false,
        }],
        recent_failed_summary: vec![],
    };

    HarnessExecutionResult {
        reply_text: format!("这次测试未执行。\n\n原因：{reason}"),
        report: report.clone(),
        pending_request: None,
        task: Some(progress_from_report(&report, None, Some(reason))),
    }
}

fn pending_report(
    state: &TestRunState,
    pending_request: ControlPendingRequest,
    case: &TestCase,
    step_index: usize,
) -> HarnessExecutionResult {
    HarnessExecutionResult {
        reply_text: format!(
            "测试任务“{}”已执行到 case「{}」第 {} 步，当前等待确认。\n\n{}",
            state.report.title, case.title, step_index, pending_request.prompt
        ),
        report: state.report.clone(),
        pending_request: Some(pending_request),
        task: Some(progress_from_report(
            &state.report,
            Some(case.title.clone()),
            Some("测试步骤等待人工确认。".to_string()),
        )),
    }
}

fn progress_from_report(
    report: &TestRunReport,
    step_summary: Option<String>,
    detail: Option<String>,
) -> AgentTaskProgress {
    let status = match report.status {
        TestRunStatus::Running => AgentTaskStatus::Running,
        TestRunStatus::Passed => AgentTaskStatus::Completed,
        TestRunStatus::Failed | TestRunStatus::Blocked => AgentTaskStatus::Failed,
        TestRunStatus::WaitingConfirmation => AgentTaskStatus::WaitingConfirmation,
        TestRunStatus::Cancelled => AgentTaskStatus::Cancelled,
    };
    AgentTaskProgress {
        task_id: report.run_id.clone(),
        task_title: report.title.clone(),
        step_index: report.case_results.len().max(1),
        step_count: report.summary.total.max(1),
        status,
        step_summary,
        detail,
    }
}

fn update_summary(report: &mut TestRunReport) {
    report.summary.total = report.case_results.len().max(report.summary.total);
    report.summary.passed = report
        .case_results
        .iter()
        .filter(|item| matches!(item.status, TestCaseStatus::Passed))
        .count();
    report.summary.failed = report
        .case_results
        .iter()
        .filter(|item| matches!(item.status, TestCaseStatus::Failed))
        .count();
    report.summary.blocked = report
        .case_results
        .iter()
        .filter(|item| matches!(item.status, TestCaseStatus::Blocked | TestCaseStatus::WaitingConfirmation))
        .count();
    report.summary.skipped = report
        .case_results
        .iter()
        .filter(|item| matches!(item.status, TestCaseStatus::Skipped))
        .count();
}

fn upsert_case_result(state: &mut TestRunState, result: TestCaseResult) {
    if let Some(existing) = state
        .report
        .case_results
        .iter_mut()
        .find(|item| item.case_id == result.case_id)
    {
        *existing = result;
    } else {
        state.report.case_results.push(result);
    }
}

fn has_active_run(app: &AppHandle) -> Result<bool, String> {
    let state: State<'_, super::TestingState> = app.state();
    let run = state.active_run()?;
    Ok(run.is_some())
}

fn replace_active_run(app: &AppHandle, next: Option<TestRunState>) -> Result<(), String> {
    let state: State<'_, super::TestingState> = app.state();
    let mut run = state.active_run()?;
    *run = next;
    Ok(())
}

fn take_run_waiting_on_pending(
    app: &AppHandle,
    pending_id: &str,
) -> Result<Option<TestRunState>, String> {
    let state: State<'_, super::TestingState> = app.state();
    let mut run = state.active_run()?;
    let matches = run
        .as_ref()
        .and_then(|item| item.waiting_pending_id.as_ref())
        .is_some_and(|item| item == pending_id);
    if matches {
        Ok(run.take())
    } else {
        Ok(None)
    }
}

fn to_tool_response(result: HarnessExecutionResult) -> ToolInvokeResponse {
    ToolInvokeResponse {
        status: if result.pending_request.is_some() {
            "pending_confirmation".to_string()
        } else {
            "success".to_string()
        },
        result: Some(json!({
            "task": result.task,
            "report": result.report,
        })),
        message: Some(result.reply_text),
        pending_request: result.pending_request,
        error: None,
    }
}

fn step_summary(step: &TestStep) -> String {
    match step {
        TestStep::ControlInvoke { summary, .. } => summary.clone(),
        TestStep::SeedClipboardText { summary, .. } => summary.clone(),
        TestStep::CaptureScreenContext { summary } => summary.clone(),
    }
}

fn step_tool(step: &TestStep) -> Option<String> {
    match step {
        TestStep::ControlInvoke { tool, .. } => Some(tool.clone()),
        TestStep::SeedClipboardText { .. } => Some("seed_clipboard_text".to_string()),
        TestStep::CaptureScreenContext { .. } => None,
    }
}
