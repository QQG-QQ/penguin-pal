use crate::app_state::{now_millis, AuditEntry, AUDIT_LIMIT};

pub fn record(action: &str, outcome: &str, detail: impl Into<String>, risk_level: u8) -> AuditEntry {
    AuditEntry {
        id: format!("audit-{}", now_millis()),
        action: action.to_string(),
        outcome: outcome.to_string(),
        detail: detail.into(),
        created_at: now_millis(),
        risk_level,
    }
}

pub fn push_entry(entries: &mut Vec<AuditEntry>, entry: AuditEntry) {
    entries.insert(0, entry);
    if entries.len() > AUDIT_LIMIT {
        entries.truncate(AUDIT_LIMIT);
    }
}
