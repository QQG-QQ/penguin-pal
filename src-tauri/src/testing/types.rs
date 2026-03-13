use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::control::types::ControlRiskLevel;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TestDestructiveLevel {
    None,
    Draft,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TestTargetPolicy {
    ReadOnlyCurrentContext,
    NamedWindowRequired,
    ActiveWindowRequired,
    ExplicitUserTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TestFailureStage {
    Selection,
    Preconditions,
    StepExecute,
    Assertion,
    Probe,
    Confirmation,
    History,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TestCaseStatus {
    Passed,
    Failed,
    Blocked,
    Skipped,
    WaitingConfirmation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TestRunStatus {
    Running,
    Passed,
    Failed,
    Blocked,
    WaitingConfirmation,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestPrecondition {
    pub kind: String,
    #[serde(default = "empty_json_object")]
    pub params: Value,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum TestStep {
    ControlInvoke {
        tool: String,
        #[serde(default = "empty_json_object")]
        args: Value,
        summary: String,
    },
    SeedClipboardText {
        text: String,
        summary: String,
    },
    CaptureScreenContext {
        summary: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestAssertion {
    pub kind: String,
    #[serde(default = "empty_json_object")]
    pub params: Value,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCase {
    pub id: String,
    pub title: String,
    pub suite: String,
    pub feature: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub max_probes: usize,
    pub destructive_level: TestDestructiveLevel,
    pub test_target_policy: TestTargetPolicy,
    pub risk_level: ControlRiskLevel,
    #[serde(default)]
    pub preconditions: Vec<TestPrecondition>,
    #[serde(default)]
    pub steps: Vec<TestStep>,
    #[serde(default)]
    pub assertions: Vec<TestAssertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TestSelection {
    #[serde(default)]
    pub suite: Option<String>,
    #[serde(default)]
    pub feature: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub case_ids: Vec<String>,
    #[serde(default)]
    pub rerun_failed_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunRequest {
    pub title: String,
    pub selection: TestSelection,
    #[serde(default)]
    pub dynamic_cases: Vec<TestCase>,
    pub max_cases: usize,
    pub allow_supplementary_rerun: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestStepResult {
    pub index: usize,
    pub summary: String,
    pub status: TestCaseStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureItem {
    pub case_id: String,
    pub case_title: String,
    pub failure_stage: TestFailureStage,
    pub step_index: usize,
    pub step_name: String,
    pub reason: String,
    pub rerunnable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCaseResult {
    pub case_id: String,
    pub title: String,
    pub suite: String,
    pub feature: String,
    pub status: TestCaseStatus,
    pub started_at: u64,
    pub finished_at: u64,
    pub destructive_level: TestDestructiveLevel,
    pub test_target_policy: TestTargetPolicy,
    #[serde(default)]
    pub step_results: Vec<TestStepResult>,
    #[serde(default)]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub failure_stage: Option<TestFailureStage>,
    #[serde(default)]
    pub probes_used: usize,
    #[serde(default)]
    pub rerun_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TestRunSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub blocked: usize,
    pub skipped: usize,
    pub rerun_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunReport {
    pub run_id: String,
    pub title: String,
    pub selector: TestSelection,
    #[serde(default)]
    pub dynamic_cases: Vec<TestCase>,
    pub started_at: u64,
    #[serde(default)]
    pub finished_at: Option<u64>,
    pub status: TestRunStatus,
    pub summary: TestRunSummary,
    #[serde(default)]
    pub case_results: Vec<TestCaseResult>,
    #[serde(default)]
    pub failure_items: Vec<FailureItem>,
    #[serde(default)]
    pub recent_failed_summary: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRunIndexEntry {
    pub run_id: String,
    pub title: String,
    pub started_at: u64,
    pub status: TestRunStatus,
    pub summary: TestRunSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentFailureSummary {
    pub case_id: String,
    pub case_title: String,
    pub failure_stage: TestFailureStage,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestHistoryIndex {
    pub date: String,
    #[serde(default)]
    pub recent_runs: Vec<TestRunIndexEntry>,
    #[serde(default)]
    pub recent_failed_items: Vec<RecentFailureSummary>,
}

#[derive(Debug, Clone)]
pub struct TestRunState {
    pub report: TestRunReport,
    pub selected_cases: Vec<TestCase>,
    pub shared_vars: serde_json::Map<String, Value>,
    pub current_case_index: usize,
    pub waiting_pending_id: Option<String>,
    pub waiting_case_index: Option<usize>,
    pub allow_supplementary_rerun: bool,
}

fn empty_json_object() -> Value {
    Value::Object(serde_json::Map::new())
}
