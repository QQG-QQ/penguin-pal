use std::collections::BTreeMap;

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
    pending_requests: &mut BTreeMap<String, ControlPendingRequest>,
) -> Vec<ControlPendingRequest> {
    let now = now_millis();
    let expired_ids = pending_requests
        .iter()
        .filter(|(_, item)| item.expires_at <= now)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();

    let mut expired = vec![];
    for id in expired_ids {
        if let Some(item) = pending_requests.remove(&id) {
            expired.push(item);
        }
    }

    expired
}

pub fn insert_pending(
    pending_requests: &mut BTreeMap<String, ControlPendingRequest>,
    request: ControlPendingRequest,
) -> usize {
    let duplicated_tool_ids = pending_requests
        .iter()
        .filter(|(_, item)| item.tool == request.tool)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();

    for id in duplicated_tool_ids {
        pending_requests.remove(&id);
    }

    pending_requests.insert(request.id.clone(), request);
    pending_requests.len()
}

pub fn list_pending(pending_requests: &BTreeMap<String, ControlPendingRequest>) -> Vec<ControlPendingRequest> {
    let mut items = pending_requests.values().cloned().collect::<Vec<_>>();
    items.sort_by_key(|item| item.created_at);
    items
}

pub fn take_pending(
    pending_requests: &mut BTreeMap<String, ControlPendingRequest>,
    id: &str,
) -> Option<ControlPendingRequest> {
    pending_requests.remove(id)
}

pub fn cancel_pending(
    pending_requests: &mut BTreeMap<String, ControlPendingRequest>,
    id: &str,
) -> Option<ControlPendingRequest> {
    take_pending(pending_requests, id)
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
