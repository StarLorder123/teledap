//! Error types for the MCP protocol layer.

use std::io;

/// Errors that can occur during MCP transport or message parsing.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Stdin closed unexpectedly")]
    StdinClosed,
}

impl McpError {
    pub fn parse_error(msg: impl Into<String>) -> Self {
        McpError::Protocol(format!("Parse error: {}", msg.into()))
    }

    pub fn invalid_request(msg: impl Into<String>) -> Self {
        McpError::Protocol(format!("Invalid request: {}", msg.into()))
    }

    pub fn method_not_found(method: &str) -> Self {
        McpError::Protocol(format!("Method not found: {method}"))
    }
}
