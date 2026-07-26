//! Shared MCP JSON-RPC dispatch logic.
//!
//! `McpRouter` is independent of transport (stdio or HTTP/SSE). It takes an
//! `IncomingMessage`, routes it to the debug-bridge, and returns a JSON value
//! suitable for sending back to the client. Notifications return `None`.

use std::sync::Arc;

use dap_trace::TraceHandle;
use debug_bridge::ToolRegistry;
use debug_session::DebugSession;
use mcp_protocol::{
    CallToolResult, ImplementationInfo, IncomingMessage, InitializeParams, InitializeResult,
    ServerCapabilities, ToolsCapability, INTERNAL_ERROR, METHOD_NOT_FOUND,
};
use openocd_client::OpenOcdClient;
use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// A JSON-RPC 2.0 message ready to be serialized and sent to the client.
#[derive(Debug, Clone)]
pub enum JsonRpcMessage {
    /// A successful JSON-RPC response.
    Response { id: u64, result: serde_json::Value },
    /// A JSON-RPC error response.
    Error {
        id: Option<u64>,
        code: i32,
        message: String,
    },
    /// A server-initiated JSON-RPC notification.
    Notification {
        method: String,
        params: Option<serde_json::Value>,
    },
}

impl Serialize for JsonRpcMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            JsonRpcMessage::Response { id, result } => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            })
            .serialize(serializer),
            JsonRpcMessage::Error { id, code, message } => {
                let id = id
                    .map(|v| serde_json::json!(v))
                    .unwrap_or(serde_json::Value::Null);
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": code,
                        "message": message,
                    },
                })
                .serialize(serializer)
            }
            JsonRpcMessage::Notification { method, params } => {
                let mut obj = serde_json::Map::new();
                obj.insert("jsonrpc".into(), "2.0".into());
                obj.insert("method".into(), method.clone().into());
                if let Some(p) = params {
                    obj.insert("params".into(), p.clone());
                }
                serde_json::Value::Object(obj).serialize(serializer)
            }
        }
    }
}

/// Stateless per-client MCP message router.
///
/// Tracks whether this client has completed the MCP `initialize` handshake.
/// The underlying debug session is shared across all clients.
pub struct McpRouter {
    initialized: bool,
}

impl McpRouter {
    pub fn new() -> Self {
        Self { initialized: false }
    }

    /// Route a single incoming MCP message.
    ///
    /// Returns `Some` for JSON-RPC request responses, `None` for notifications.
    pub async fn handle_message(
        &mut self,
        msg: IncomingMessage,
        session: &DebugSession,
        openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
        trace: &TraceHandle,
    ) -> Option<JsonRpcMessage> {
        match msg {
            IncomingMessage::Request { id, method, params } => {
                if !self.initialized && method != "initialize" {
                    return Some(JsonRpcMessage::Error {
                        id: Some(id),
                        code: INTERNAL_ERROR,
                        message: "Server not initialized".into(),
                    });
                }

                match method.as_str() {
                    "initialize" => match self.handle_initialize(id, params, session).await {
                        Ok(result) => {
                            self.initialized = true;
                            Some(JsonRpcMessage::Response { id, result })
                        }
                        Err(e) => Some(JsonRpcMessage::Error {
                            id: Some(id),
                            code: INTERNAL_ERROR,
                            message: e,
                        }),
                    },
                    "tools/list" => {
                        let state = session.current_state().await;
                        let tools = ToolRegistry::list_tools_for_state(state);
                        let result = serde_json::to_value(mcp_protocol::ListToolsResult { tools })
                            .unwrap_or(serde_json::Value::Null);
                        Some(JsonRpcMessage::Response { id, result })
                    }
                    "tools/call" => {
                        info!("Tool call request");
                        Some(
                            self.handle_tool_call(id, params, session, trace, openocd)
                                .await,
                        )
                    }
                    _ => Some(JsonRpcMessage::Error {
                        id: Some(id),
                        code: METHOD_NOT_FOUND,
                        message: format!("Unknown method: {method}"),
                    }),
                }
            }
            IncomingMessage::Notification { method, params: _ } => {
                match method.as_str() {
                    "notifications/initialized" => {
                        info!("MCP client initialization complete.");
                    }
                    _ => {
                        debug!("Unhandled notification: {method}");
                    }
                }
                None
            }
        }
    }

    /// Handle the MCP `initialize` request.
    async fn handle_initialize(
        &self,
        id: u64,
        params: Option<serde_json::Value>,
        session: &DebugSession,
    ) -> Result<serde_json::Value, String> {
        let _ = id;
        if let Some(ref p) = params {
            if let Ok(init_params) = serde_json::from_value::<InitializeParams>(p.clone()) {
                session.set_lib_lldb_path(init_params.liblldb_path).await;
            }
        }

        let result = InitializeResult {
            protocol_version: "2025-11-25".into(),
            capabilities: ServerCapabilities {
                tools: ToolsCapability { list_changed: true },
            },
            server_info: ImplementationInfo {
                name: "teleDAP".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
        };
        serde_json::to_value(result).map_err(|e| e.to_string())
    }

    /// Handle the MCP `tools/call` request.
    async fn handle_tool_call(
        &self,
        id: u64,
        params: Option<serde_json::Value>,
        session: &DebugSession,
        trace: &TraceHandle,
        openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
    ) -> JsonRpcMessage {
        let params = match params {
            Some(p) => p,
            None => {
                return JsonRpcMessage::Error {
                    id: Some(id),
                    code: INTERNAL_ERROR,
                    message: "Missing params in tools/call".into(),
                }
            }
        };

        let name = match params
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
        {
            Some(n) => n,
            None => {
                return JsonRpcMessage::Error {
                    id: Some(id),
                    code: INTERNAL_ERROR,
                    message: "Missing 'name' field in tools/call params".into(),
                }
            }
        };

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        info!("Tool call: {name}");

        match ToolRegistry::dispatch(&name, session, arguments, Some(trace), openocd).await {
            Ok(result) => match serde_json::to_value(result) {
                Ok(result) => JsonRpcMessage::Response { id, result },
                Err(e) => JsonRpcMessage::Error {
                    id: Some(id),
                    code: INTERNAL_ERROR,
                    message: e.to_string(),
                },
            },
            Err(e) => {
                let error_result: CallToolResult = e.to_tool_result();
                match serde_json::to_value(error_result) {
                    Ok(result) => JsonRpcMessage::Response { id, result },
                    Err(ser_err) => JsonRpcMessage::Error {
                        id: Some(id),
                        code: INTERNAL_ERROR,
                        message: ser_err.to_string(),
                    },
                }
            }
        }
    }
}

impl Default for McpRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dap_client::DapClient;
    use mcp_protocol::PARSE_ERROR;

    fn make_session() -> (
        Arc<DebugSession>,
        Arc<RwLock<Option<OpenOcdClient>>>,
        TraceHandle,
    ) {
        let (trace, _bg) = TraceHandle::new(None, 100);
        let client = DapClient::with_trace(1024, trace.clone());
        let session = Arc::new(DebugSession::new(client, Some(trace.clone())));
        let openocd = Arc::new(RwLock::new(None));
        (session, openocd, trace)
    }

    #[tokio::test]
    async fn test_initialize_sets_initialized() {
        let (session, openocd, trace) = make_session();
        let mut router = McpRouter::new();
        let msg = IncomingMessage::Request {
            id: 1,
            method: "initialize".into(),
            params: Some(serde_json::json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1.0" },
            })),
        };
        let resp = router
            .handle_message(msg, &session, &openocd, &trace)
            .await
            .expect("expected response");
        match resp {
            JsonRpcMessage::Response { id, result } => {
                assert_eq!(id, 1);
                assert!(result.get("protocolVersion").is_some());
                assert!(result.get("capabilities").is_some());
            }
            other => panic!("expected Response, got {other:?}"),
        }
        assert!(router.initialized);
    }

    #[tokio::test]
    async fn test_pre_initialize_tools_list_rejected() {
        let (session, openocd, trace) = make_session();
        let mut router = McpRouter::new();
        let msg = IncomingMessage::Request {
            id: 2,
            method: "tools/list".into(),
            params: Some(serde_json::json!({})),
        };
        let resp = router.handle_message(msg, &session, &openocd, &trace).await;
        match resp {
            Some(JsonRpcMessage::Error { id, code, .. }) => {
                assert_eq!(id, Some(2));
                assert_eq!(code, INTERNAL_ERROR);
            }
            other => panic!("expected error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_post_initialize_tools_list_returns_tools() {
        let (session, openocd, trace) = make_session();
        let mut router = McpRouter::new();

        // Initialize first
        let init = IncomingMessage::Request {
            id: 1,
            method: "initialize".into(),
            params: Some(serde_json::json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1.0" },
            })),
        };
        router
            .handle_message(init, &session, &openocd, &trace)
            .await;

        let msg = IncomingMessage::Request {
            id: 2,
            method: "tools/list".into(),
            params: Some(serde_json::json!({})),
        };
        let resp = router.handle_message(msg, &session, &openocd, &trace).await;
        match resp {
            Some(JsonRpcMessage::Response { id, result }) => {
                assert_eq!(id, 2);
                let tools = result.get("tools").and_then(|v| v.as_array()).unwrap();
                // In Disconnected state, only start + utility + openocd tools are available.
                assert!(!tools.is_empty());
            }
            other => panic!("expected response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let (session, openocd, trace) = make_session();
        let mut router = McpRouter::new();
        // Must initialize first to avoid "not initialized" error.
        let init = IncomingMessage::Request {
            id: 0,
            method: "initialize".into(),
            params: Some(serde_json::json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1.0" },
            })),
        };
        router
            .handle_message(init, &session, &openocd, &trace)
            .await;

        let msg = IncomingMessage::Request {
            id: 3,
            method: "unknown/method".into(),
            params: None,
        };
        let resp = router.handle_message(msg, &session, &openocd, &trace).await;
        match resp {
            Some(JsonRpcMessage::Error { id, code, .. }) => {
                assert_eq!(id, Some(3));
                assert_eq!(code, METHOD_NOT_FOUND);
            }
            other => panic!("expected error, got {other:?}"),
        }
    }

    #[test]
    fn test_json_rpc_message_serialization() {
        let resp = JsonRpcMessage::Response {
            id: 7,
            result: serde_json::json!({"ok": true}),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":7"));
        assert!(json.contains("\"ok\":true"));

        let err = JsonRpcMessage::Error {
            id: Some(8),
            code: PARSE_ERROR,
            message: "bad".into(),
        };
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"id\":8"));
        assert!(json.contains("\"code\":-32700"));

        let notif = JsonRpcMessage::Notification {
            method: "notifications/tools/list_changed".into(),
            params: None,
        };
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains("\"method\":\"notifications/tools/list_changed\""));
        assert!(!json.contains("\"id\""));
    }
}
