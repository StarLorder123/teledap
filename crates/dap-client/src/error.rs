//! Error types for the DAP client.

/// Errors that can occur during DAP client operations.
#[derive(Debug, thiserror::Error)]
pub enum DapClientError {
    /// Failed to spawn the codelldb child process.
    #[error("Failed to spawn process: {0}")]
    SpawnFailed(String),

    /// The codelldb process exited unexpectedly.
    #[error("Process exited unexpectedly: {0}")]
    ProcessExited(String),

    /// An operation was attempted without an active connection.
    #[error("Not connected: {0}")]
    NotConnected(String),

    /// A DAP request returned `success: false`.
    #[error("DAP request '{command}' failed: {message}")]
    DapRequestFailed { command: String, message: String },

    /// A DAP protocol-level error (e.g. bad frame, missing header).
    #[error("DAP protocol error: {0}")]
    DapProtocol(String),

    /// A timeout while waiting for a response.
    #[error("Timeout: {0}")]
    Timeout(String),

    /// IO error from the underlying transport.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl DapClientError {
    /// Create a `DapRequestFailed` error from a response.
    pub fn request_failed(command: impl Into<String>, message: impl Into<String>) -> Self {
        DapClientError::DapRequestFailed {
            command: command.into(),
            message: message.into(),
        }
    }

    /// Returns true if this error is a `NotConnected` error.
    pub fn is_not_connected(&self) -> bool {
        matches!(self, DapClientError::NotConnected(_))
    }
}
