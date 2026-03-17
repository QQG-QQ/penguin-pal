//! Control Module - 桌面控制服务
//!
//! 提供 HTTP API 和桌面控制工具。
#![allow(unused)]

pub mod errors;
pub mod files;
pub mod http;
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

use std::{collections::BTreeMap, sync::Mutex};

use self::types::PendingControlRequest;

pub const CONTROL_PORT_RANGE: std::ops::RangeInclusive<u16> = 48_765..=48_775;

pub struct ControlServiceState {
    bind_address: Mutex<Option<String>>,
    pending_requests: Mutex<BTreeMap<String, PendingControlRequest>>,
}

impl ControlServiceState {
    pub fn new() -> Self {
        Self {
            bind_address: Mutex::new(None),
            pending_requests: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn set_bind_address(&self, address: String) -> Result<(), String> {
        let mut state = self
            .bind_address
            .lock()
            .map_err(|_| "控制服务地址状态锁定失败".to_string())?;
        *state = Some(address);
        Ok(())
    }

    pub fn bind_address(&self) -> Result<Option<String>, String> {
        self.bind_address
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "控制服务地址状态锁定失败".to_string())
    }

    pub fn pending_requests(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, PendingControlRequest>>, String> {
        self.pending_requests
            .lock()
            .map_err(|_| "控制服务待确认状态锁定失败".to_string())
    }
}
