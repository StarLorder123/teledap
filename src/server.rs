//! MCP server loop — reads JSON-RPC 2.0 from stdin, dispatches tool calls
//! to the debug-bridge, writes responses to stdout.
//!
//! This is the entry point used when TeledAP is spawned by an AI client
//! (e.g. Claude Desktop) that communicates via MCP over stdio.

use std::sync::Arc;

use dap_client::DapClient;
use dap_trace::TraceHandle;
use debug_bridge::ToolRegistry;
use debug_session::{DebugSession, SessionState};
use mcp_protocol::{
    CallToolResult, ImplementationInfo, IncomingMessage, InitializeParams, InitializeResult,
    McpServer, ServerCapabilities, ToolsCapability, INTERNAL_ERROR, METHOD_NOT_FOUND, PARSE_ERROR,
};
use openocd_client::OpenOcdClient;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

pub async fn run() {
    info!("TeleDAP MCP server starting...");

    // ── Session setup ─────────────────────────────────────────────────
    let (trace, _bg) = TraceHandle::new(None, 10_000);
    let client = DapClient::with_trace(4 * 1024 * 1024, trace.clone());
    let session = Arc::new(DebugSession::new(client, Some(trace.clone())));

    // OpenOCD is an optional extension — not started by default.
    let openocd: Arc<RwLock<Option<OpenOcdClient>>> = Arc::new(RwLock::new(None));

    // ── Background DAP event handler ──────────────────────────────────
    //
    // This task continuously reads DAP events from the adapter's stdout and
    // feeds them through the session state machine so that state
    // transitions (Running <-> Halted) happen automatically while the
    // main loop is blocked on stdin for MCP requests.
    let session_bg = Arc::clone(&session);
    tokio::spawn(async move {
        while let Some(event) = session_bg.client().recv_event().await {
            let event_name = event.event.clone();
            match session_bg.handle_event(&event).await {
                Ok(handled) => {
                    if handled {
                        debug!("State-affecting event processed: {event_name}");
                    }
                }
                Err(e) => {
                    error!("Error handling event '{event_name}': {e}");
                }
            }

            // Log output from the debuggee
            if event_name == "output" {
                if let Some(ref body) = event.body {
                    if let Ok(output) =
                        serde_json::from_value::<dap_types::events::OutputEventBody>(body.clone())
                    {
                        debug!("[debuggee] {}", output.output.trim_end());
                    }
                }
            }

            if event_name == "terminated" || event_name == "exited" {
                info!("Debuggee {} received", event_name);
            }
        }
        info!("DAP event stream ended.");
    });

    // ── MCP server loop ──────────────────────────────────────────────
    let mut server = McpServer::new();
    let mut initialized = false;

    while let Some(msg_result) = server.next_message().await {
        match msg_result {
            Err(e) => {
                error!("Read error: {e}");
                let _ = server.send_error(None, PARSE_ERROR, &e.to_string()).await;
                // Per MCP spec, continue after parse errors
                continue;
            }
            Ok(msg) => match msg {
                IncomingMessage::Request { id, method, params } => {
                    if !initialized && method != "initialize" {
                        let _ = server
                            .send_error(Some(id), INTERNAL_ERROR, "Server not initialized")
                            .await;
                        continue;
                    }

                    match method.as_str() {
                        "initialize" => {
                            match handle_initialize(id, params, &session, &mut server).await {
                                Ok(_) => initialized = true,
                                Err(e) => {
                                    let _ = server.send_error(Some(id), INTERNAL_ERROR, &e).await;
                                }
                            }
                        }
                        "tools/list" => {
                            let state = session.current_state().await;
                            let tools = ToolRegistry::list_tools_for_state(state);
                            let result = mcp_protocol::ListToolsResult { tools };
                            let _ = server.send_response(id, &result).await;
                        }
                        "tools/call" => {
                            match handle_tool_call(
                                id,
                                &session,
                                params,
                                &trace,
                                &openocd,
                                &mut server,
                            )
                            .await
                            {
                                Ok(_) => {}
                                Err(e) => {
                                    let _ = server.send_error(Some(id), INTERNAL_ERROR, &e).await;
                                }
                            }
                        }
                        _ => {
                            let _ = server
                                .send_error(
                                    Some(id),
                                    METHOD_NOT_FOUND,
                                    &format!("Unknown method: {method}"),
                                )
                                .await;
                        }
                    }
                }
                IncomingMessage::Notification { method, params: _ } => match method.as_str() {
                    "notifications/initialized" => {
                        info!("MCP client initialization complete.");
                    }
                    _ => {
                        debug!("Unhandled notification: {method}");
                    }
                },
            },
        }
    }

    // ── Cleanup ──────────────────────────────────────────────────────
    info!("MCP server shutting down.");
    // Shut down OpenOCD first (if it was started), then the debug adapter.
    if let Some(ref ocd) = *openocd.read().await {
        info!("Shutting down OpenOCD...");
        let _ = ocd.shutdown().await;
    }
    if session.current_state().await != SessionState::Disconnected {
        let _ = session.shutdown().await;
    }
}

/// Handle the MCP `initialize` request — the first message in the handshake.
///
/// Parses optional `liblldbPath` from the client params and stores it on the
/// session so that the debug adapter can find `liblldb.dll` at spawn time.
async fn handle_initialize(
    id: u64,
    params: Option<serde_json::Value>,
    session: &DebugSession,
    server: &mut McpServer,
) -> Result<(), String> {
    // Extract optional liblldbPath from initialize params.
    // Silently ignore parse errors — the client may use an older MCP
    // schema without liblldbPath, which is backward-compatible.
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
    server
        .send_response(id, &result)
        .await
        .map_err(|e| e.to_string())
}

/// Handle the MCP `tools/call` request — extract name + arguments, dispatch
/// to the debug-bridge, and send the result as a JSON-RPC response.
///
/// Per the MCP spec, tool execution errors are returned as successful
/// JSON-RPC responses with `is_error: true`, not as JSON-RPC errors.
async fn handle_tool_call(
    id: u64,
    session: &DebugSession,
    params: Option<serde_json::Value>,
    trace: &TraceHandle,
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
    server: &mut McpServer,
) -> Result<(), String> {
    let params = params.ok_or_else(|| "Missing params in tools/call".to_string())?;

    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing 'name' field in tools/call params".to_string())?;

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    info!("Tool call: {name}");

    match ToolRegistry::dispatch(name, session, arguments, Some(trace), openocd).await {
        Ok(result) => server
            .send_response(id, &result)
            .await
            .map_err(|e| e.to_string()),
        Err(e) => {
            // Tool errors → is_error: true result (NOT JSON-RPC error)
            let error_result: CallToolResult = e.to_tool_result();
            server
                .send_response(id, &error_result)
                .await
                .map_err(|e| e.to_string())
        }
    }
}
