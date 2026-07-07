//! Error types for the debug-bridge layer.
//!
//! `BridgeError` converts tool execution failures into MCP `CallToolResult`
//! values with `is_error: true` — per the MCP spec, tool errors are returned
//! as successful JSON-RPC responses, not as JSON-RPC errors.

use mcp_protocol::CallToolResult;

/// Errors that can occur during tool dispatch or execution.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Unknown tool: {0}")]
    UnknownTool(String),

    #[error("Session error: {0}")]
    Session(#[from] debug_session::DebugSessionError),

    #[error("Invalid parameters for `{tool}`: {message}")]
    InvalidParams { tool: String, message: String },

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Internal(String),
}

impl BridgeError {
    /// Convert this error into an MCP `CallToolResult` with `is_error: true`.
    pub fn to_tool_result(&self) -> CallToolResult {
        CallToolResult::error(self.to_string())
    }
}
