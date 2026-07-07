//! MCP transport layer — line-delimited JSON-RPC 2.0 over stdin/stdout.
//!
//! MCP uses newline-delimited JSON, NOT Content-Length framing (which is
//! what DAP uses). Each message is exactly one line of valid JSON terminated
//! by `\n`. This is simpler than DAP framing — `tokio::io::BufReader::read_line`
//! is sufficient; no custom `Decoder` is needed.

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Stdin, Stdout};

use crate::error::McpError;
use crate::types::IncomingMessage;

/// The MCP server transport — reads line-delimited JSON-RPC from stdin,
/// writes responses to stdout.
pub struct McpServer {
    reader: BufReader<Stdin>,
    writer: Stdout,
}

impl McpServer {
    /// Create a new MCP server transport bound to system stdin/stdout.
    pub fn new() -> Self {
        McpServer {
            reader: BufReader::new(tokio::io::stdin()),
            writer: tokio::io::stdout(),
        }
    }

    /// Read the next message from stdin.
    ///
    /// Returns `None` when stdin is closed (EOF). Empty lines are skipped
    /// and the next line is read (they are protocol violations but we
    /// tolerate them to be robust against whitespace-only noise).
    pub async fn next_message(&mut self) -> Option<Result<IncomingMessage, McpError>> {
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line).await {
                Ok(0) => return None, // EOF
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        // Skip empty lines, retry
                        continue;
                    }
                    return Some(Self::parse_incoming(trimmed));
                }
                Err(e) => return Some(Err(McpError::Io(e))),
            }
        }
    }

    /// Parse a single JSON line into an IncomingMessage.
    fn parse_incoming(json_str: &str) -> Result<IncomingMessage, McpError> {
        let val: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| McpError::parse_error(e.to_string()))?;

        let obj = val
            .as_object()
            .ok_or_else(|| McpError::invalid_request("message is not a JSON object"))?;

        // Check for required "method" field
        let method = obj
            .get("method")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| McpError::invalid_request("missing 'method' field"))?;

        let params = obj.get("params").cloned();

        // Discriminate Request vs Notification by presence of "id"
        // Per JSON-RPC 2.0, id:null means Notification (no response expected)
        let id_value = obj.get("id");
        match id_value {
            Some(v) if v.is_null() => {
                // null id → Notification
                Ok(IncomingMessage::Notification { method, params })
            }
            Some(v) => match v.as_u64() {
                Some(id) => Ok(IncomingMessage::Request { id, method, params }),
                None => Err(McpError::invalid_request(
                    "only numeric (u64) ids are supported",
                )),
            },
            None => Ok(IncomingMessage::Notification { method, params }),
        }
    }

    /// Write a JSON-RPC success response and flush stdout.
    pub async fn send_response(
        &mut self,
        id: u64,
        result: &impl serde::Serialize,
    ) -> Result<(), McpError> {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        self.write_line(&response).await
    }

    /// Write a JSON-RPC error response and flush stdout.
    ///
    /// `id` should be `None` only for parse errors (where the request id
    /// couldn't be extracted).
    pub async fn send_error(
        &mut self,
        id: Option<u64>,
        code: i32,
        message: &str,
    ) -> Result<(), McpError> {
        let response = if let Some(id) = id {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": code,
                    "message": message,
                },
            })
        } else {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": serde_json::Value::Null,
                "error": {
                    "code": code,
                    "message": message,
                },
            })
        };
        self.write_line(&response).await
    }

    /// Internal: serialize a JSON value to a single line and write + flush.
    async fn write_line(&mut self, value: &serde_json::Value) -> Result<(), McpError> {
        let mut json = serde_json::to_string(value)?;
        json.push('\n');
        self.writer.write_all(json.as_bytes()).await?;
        self.writer.flush().await?;
        Ok(())
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_incoming_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let msg = McpServer::parse_incoming(json).unwrap();
        match msg {
            IncomingMessage::Request {
                id,
                method,
                params: _,
            } => {
                assert_eq!(id, 1);
                assert_eq!(method, "tools/list");
            }
            _ => panic!("Expected Request, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_incoming_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let msg = McpServer::parse_incoming(json).unwrap();
        match msg {
            IncomingMessage::Notification { method, params } => {
                assert_eq!(method, "notifications/initialized");
                assert!(params.is_none());
            }
            _ => panic!("Expected Notification, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_incoming_missing_method() {
        let json = r#"{"jsonrpc":"2.0","id":1}"#;
        let err = McpServer::parse_incoming(json).unwrap_err();
        assert!(err.to_string().contains("missing 'method'"));
    }

    #[test]
    fn test_parse_incoming_non_object() {
        let json = r#"["not", "an", "object"]"#;
        let err = McpServer::parse_incoming(json).unwrap_err();
        assert!(err.to_string().contains("not a JSON object"));
    }

    #[test]
    fn test_parse_incoming_invalid_json() {
        let json = r#"not json at all"#;
        let err = McpServer::parse_incoming(json).unwrap_err();
        assert!(err.to_string().contains("Parse error"));
    }

    #[test]
    fn test_parse_incoming_string_id_rejected() {
        let json = r#"{"jsonrpc":"2.0","id":"abc","method":"test"}"#;
        let err = McpServer::parse_incoming(json).unwrap_err();
        assert!(err.to_string().contains("numeric (u64) ids"));
    }

    #[test]
    fn test_parse_incoming_null_id_is_notification() {
        // Per JSON-RPC 2.0, id:null means notification — we treat as Notification
        let json = r#"{"jsonrpc":"2.0","id":null,"method":"test"}"#;
        let msg = McpServer::parse_incoming(json).unwrap();
        assert!(matches!(msg, IncomingMessage::Notification { .. }));
    }

    #[test]
    fn test_parse_incoming_request_no_params() {
        let json = r#"{"jsonrpc":"2.0","id":42,"method":"initialize"}"#;
        let msg = McpServer::parse_incoming(json).unwrap();
        match msg {
            IncomingMessage::Request { id, method, params } => {
                assert_eq!(id, 42);
                assert_eq!(method, "initialize");
                assert!(params.is_none());
            }
            _ => panic!("Expected Request"),
        }
    }
}
