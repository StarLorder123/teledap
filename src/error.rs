use crate::session_coordinator::state_machine::SessionState;

/// Top-level error type for TeleDAP operations.
/// These errors cross the session boundary and may be exposed to the MCP client.
#[derive(Debug, thiserror::Error)]
pub enum TeleDapError {
    #[error("Session in wrong state: current={current:?}, expected={expected}")]
    InvalidState {
        current: SessionState,
        expected: String,
    },

    #[error("Tool '{0}' is not available in the current session state ({1:?})")]
    ToolUnavailable(String, SessionState),

    #[error("Unknown tool: {0}")]
    UnknownTool(String),

    #[error("Missing required parameter: {0}")]
    MissingParameter(String),

    #[error("Invalid parameter '{name}': {reason}")]
    InvalidParameter { name: String, reason: String },

    #[error("Driver error: {0}")]
    Driver(#[from] DriverError),

    #[error("Communication error: {0}")]
    Communication(String),

    #[error("Not connected: {0}")]
    NotConnected(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Shutdown in progress")]
    ShuttingDown,
}

/// Low-level errors from protocol drivers (DAP, OpenOCD).
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("Process spawn failed: {0}")]
    SpawnFailed(String),

    #[error("Process exited unexpectedly: {0}")]
    ProcessExited(String),

    #[error("Not connected: {0}")]
    NotConnected(String),

    #[error("TCP connection failed: {0}")]
    TcpConnect(String),

    #[error("TCP connection lost")]
    TcpDisconnected,

    #[error("DAP protocol error: {0}")]
    DapProtocol(String),

    #[error("DAP request failed: {command} — {message}")]
    DapRequestFailed { command: String, message: String },

    #[error("OpenOCD error: {0}")]
    OpenOcd(String),

    #[error("Frame decode error: {0}")]
    FrameDecode(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
