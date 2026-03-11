use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlRiskLevel {
    ReadOnly,
    WriteLow,
    WriteHigh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlToolArgSpec {
    pub name: String,
    pub required: bool,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlToolDefinition {
    pub name: String,
    pub title: String,
    pub summary: String,
    pub minimum_permission_level: u8,
    pub risk_level: ControlRiskLevel,
    pub requires_confirmation: bool,
    pub args: Vec<ControlToolArgSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvokeRequest {
    pub tool: String,
    #[serde(default = "empty_json_object")]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingControlRequest {
    pub id: String,
    pub tool: String,
    pub title: String,
    pub prompt: String,
    pub preview: Value,
    pub args: Value,
    pub created_at: u64,
    pub expires_at: u64,
    pub minimum_permission_level: u8,
    pub risk_level: ControlRiskLevel,
}

pub type ControlPendingRequest = PendingControlRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiSelector {
    pub window_title: Option<String>,
    pub automation_id: Option<String>,
    pub name: Option<String>,
    pub control_type: Option<String>,
    pub class_name: Option<String>,
    #[serde(default = "default_match_mode")]
    pub match_mode: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvokeResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_request: Option<PendingControlRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ControlErrorPayload>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlServiceStatus {
    pub running: bool,
    pub base_url: Option<String>,
    pub tool_count: usize,
    pub message: String,
}

pub fn empty_json_object() -> Value {
    Value::Object(serde_json::Map::new())
}

fn default_match_mode() -> String {
    "contains".to_string()
}
