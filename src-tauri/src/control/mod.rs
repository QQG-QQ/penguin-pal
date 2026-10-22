//! Internal desktop tools for the optional legacy assistant.
//! No HTTP listener, external control API, or reaction lease service is provided.
#![allow(unused)]

pub mod errors;
pub mod files;
pub mod installer;
pub mod logging;
pub mod pending;
pub mod policy;
pub mod registry;
pub mod router;
pub mod shell;
pub mod system_registry;
pub mod types;
pub mod windows;

use self::types::PendingControlRequest;
use std::{collections::BTreeMap, sync::Mutex};

pub struct ControlServiceState {
    pending_requests: Mutex<BTreeMap<String, PendingControlRequest>>,
}

impl ControlServiceState {
    pub fn new() -> Self {
        Self {
            pending_requests: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn pending_requests(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, PendingControlRequest>>, String> {
        self.pending_requests
            .lock()
            .map_err(|_| "控制工具待确认状态锁定失败".to_string())
    }
}
