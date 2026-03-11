use std::{error::Error, fmt};

use super::types::ControlErrorPayload;

pub type ControlResult<T> = Result<T, ControlError>;

#[derive(Debug, Clone)]
pub struct ControlError {
    payload: ControlErrorPayload,
}

impl ControlError {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
        detail: Option<String>,
        retryable: bool,
    ) -> Self {
        Self {
            payload: ControlErrorPayload {
                code: code.into(),
                message: message.into(),
                detail,
                retryable,
            },
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new("validation_error", message, None, false)
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::new("permission_denied", message, None, false)
    }

    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(code, message, None, false)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new("timeout", message, None, true)
    }

    pub fn backend(
        code: impl Into<String>,
        message: impl Into<String>,
        detail: Option<String>,
    ) -> Self {
        Self::new(code, message, detail, true)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", message, None, false)
    }

    pub fn payload(&self) -> ControlErrorPayload {
        self.payload.clone()
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.payload.message)
    }
}

impl Error for ControlError {}
