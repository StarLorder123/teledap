//! Error types for the debug session layer.

/// Errors that can occur during debug session operations.
#[derive(Debug, thiserror::Error)]
pub enum DebugSessionError {
    /// Wrapped error from the underlying DAP client.
    #[error("DAP client error: {0}")]
    DapClient(#[from] dap_client::DapClientError),

    /// An operation was attempted in an invalid session state.
    #[error(
        "invalid state for `{operation}`: current = {current}, requires one of [{}]",
        required.iter().map(|s| format!("{}", s)).collect::<Vec<_>>().join(", ")
    )]
    InvalidState {
        operation: String,
        current: SessionState,
        required: Vec<SessionState>,
    },

    /// The context cache is stale and must be refreshed.
    #[error("context cache is stale for area: {0:?}")]
    StaleContext(Option<String>),

    /// A variable expansion depth limit was reached.
    #[error("variable expansion depth limit reached (max {0})")]
    ExpansionDepthExceeded(usize),

    /// An operation timed out.
    #[error("operation timed out: {0}")]
    Timeout(String),
}

use crate::state::SessionState;
