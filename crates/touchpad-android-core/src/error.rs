use std::path::PathBuf;
use thiserror::Error;

/// Errors returned by [`crate::AndroidEngine`] operations.
#[derive(Debug, Error)]
pub enum AndroidEngineError {
    /// Persistent configuration storage failure.
    #[error("storage error: {0}")]
    Storage(#[from] touchpad_storage::StorageError),

    /// Network or filesystem I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization or deserialization failure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Caller-provided input failed validation.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Referenced saved computer ID does not exist.
    #[error("computer not found: {0}")]
    ComputerNotFound(String),

    /// Target host IP/port failed socket address resolution.
    #[error("invalid address for '{target}': {reason}")]
    InvalidAddress { target: String, reason: String },

    /// Engine has already been shut down.
    #[error("android engine has been shut down")]
    Shutdown,
}

impl AndroidEngineError {
    #[allow(dead_code)]
    pub(crate) fn storage_path_hint(&self) -> Option<&PathBuf> {
        match self {
            Self::Storage(touchpad_storage::StorageError::Io { path, .. }) => Some(path),
            _ => None,
        }
    }
}
