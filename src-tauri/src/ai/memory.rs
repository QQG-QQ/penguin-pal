use crate::app_state::{ChatMessage, HISTORY_LIMIT};

pub fn trim_history(messages: &mut Vec<ChatMessage>) {
    if messages.len() > HISTORY_LIMIT {
        let extra = messages.len() - HISTORY_LIMIT;
        messages.drain(0..extra);
    }
}

pub fn context_window(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let start = messages.len().saturating_sub(12);
    messages[start..].to_vec()
}
