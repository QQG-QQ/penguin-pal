use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde_json::{json, Value};

use crate::app_state::now_millis;

use super::types::{ControlPendingRequest, ControlToolDefinition};

pub const CONTROL_CONFIRMATION_TTL_MS: u64 = 30_000;

pub fn build_pending_request(
    definition: &ControlToolDefinition,
    args: Value,
    prompt: String,
    preview: Value,
) -> ControlPendingRequest {
    let created_at = now_millis();
    ControlPendingRequest {
        id: random_pending_id(created_at),
        tool: definition.name.clone(),
        title: format!("待确认：{}", definition.title),
        prompt,
        preview,
        args,
        created_at,
        expires_at: created_at + CONTROL_CONFIRMATION_TTL_MS,
        minimum_permission_level: definition.minimum_permission_level,
        risk_level: definition.risk_level.clone(),
    }
}

pub fn cleanup_expired_pending(
    pending_requests: &mut Vec<ControlPendingRequest>,
) -> Vec<ControlPendingRequest> {
    let now = now_millis();
    let mut expired = vec![];
    pending_requests.retain(|item| {
        let keep = item.expires_at > now;
        if !keep {
            expired.push(item.clone());
        }
        keep
    });
    expired
}

pub fn cancel_pending(
    pending_requests: &mut Vec<ControlPendingRequest>,
    id: &str,
) -> Option<ControlPendingRequest> {
    let cancelled = pending_requests.iter().find(|item| item.id == id).cloned();
    pending_requests.retain(|item| item.id != id);
    cancelled
}

pub fn default_preview(message: &str) -> Value {
    json!({ "summary": message })
}

fn random_pending_id(created_at: u64) -> String {
    let suffix: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();
    format!("control-{}-{}", created_at, suffix)
}
