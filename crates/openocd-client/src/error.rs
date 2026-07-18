//! Error types for the OpenOCD client.

/// Errors that can occur during OpenOCD client operations.
#[derive(Debug, thiserror::Error)]
pub enum OpenOcdClientError {
    /// Failed to spawn the OpenOCD child process.
    #[error("Failed to spawn OpenOCD: {0}")]
    SpawnFailed(String),

    /// The OpenOCD process is already running.
    #[error("OpenOCD is already running")]
    AlreadyRunning,

    /// An operation was attempted without an active OpenOCD process.
    #[error("OpenOCD is not running: {0}")]
    NotConnected(String),

    /// IO error from the underlying transport or file.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A Tcl command timed out waiting for response.
    #[error("Command timed out after {timeout_ms}ms: {command}")]
    Timeout { command: String, timeout_ms: u64 },

    /// The OpenOCD process exited unexpectedly.
    #[error("OpenOCD process exited: {0}")]
    ProcessExited(String),
}
