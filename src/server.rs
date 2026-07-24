//! MCP server loop — reads JSON-RPC 2.0 from stdin, dispatches tool calls
//! to the debug-bridge, writes responses to stdout.
//!
//! This is the entry point used when TeledAP is spawned by an AI client
//! (e.g. Claude Desktop) that communicates via MCP over stdio.

use std::sync::Arc;

use dap_client::DapClient;
use dap_trace::TraceHandle;
use debug_session::{DebugSession, SessionState};
use mcp_protocol::{McpServer, PARSE_ERROR};
use openocd_client::OpenOcdClient;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use crate::mcp_router::{JsonRpcMessage, McpRouter};

pub async fn run() {
    info!("TeleDAP MCP server starting...");

    // ── Session setup ─────────────────────────────────────────────────
    let (trace, _bg) = TraceHandle::new(None, 10_000);
    let client = DapClient::with_trace(4 * 1024 * 1024, trace.clone());
    let session = Arc::new(DebugSession::new(client, Some(trace.clone())));

    // OpenOCD is an optional extension — not started by default.
    let openocd: Arc<RwLock<Option<OpenOcdClient>>> = Arc::new(RwLock::new(None));

    // ── Background DAP event handler ──────────────────────────────────
    spawn_dap_event_loop(Arc::clone(&session));

    // ── MCP server loop ──────────────────────────────────────────────
    let mut server = McpServer::new();
    let mut router = McpRouter::new();

    while let Some(msg_result) = server.next_message().await {
        match msg_result {
            Err(e) => {
                error!("Read error: {e}");
                let _ = server.send_error(None, PARSE_ERROR, &e.to_string()).await;
                // Per MCP spec, continue after parse errors
                continue;
            }
            Ok(msg) => {
                if let Some(response) = router.handle_message(msg, &session, &openocd, &trace).await
                {
                    if let Err(e) = send_json_rpc_message(&mut server, response).await {
                        error!("Failed to send response: {e}");
                    }
                }
            }
        }
    }

    // ── Cleanup ──────────────────────────────────────────────────────
    cleanup_session(&session, &openocd).await;
    info!("MCP server shutting down.");
}

/// Spawn the background task that reads DAP events from the adapter and feeds
/// them through the session state machine.
pub fn spawn_dap_event_loop(session: Arc<DebugSession>) {
    tokio::spawn(async move {
        while let Some(event) = session.client().recv_event().await {
            let event_name = event.event.clone();
            match session.handle_event(&event).await {
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
                info!("Debuggee {event_name} received");
            }
        }
        info!("DAP event stream ended.");
    });
}

/// Clean up the debug session and OpenOCD client before shutdown.
pub async fn cleanup_session(session: &DebugSession, openocd: &Arc<RwLock<Option<OpenOcdClient>>>) {
    // Shut down OpenOCD first (if it was started), then the debug adapter.
    if let Some(ref ocd) = *openocd.read().await {
        info!("Shutting down OpenOCD...");
        let _ = ocd.shutdown().await;
    }
    if session.current_state().await != SessionState::Disconnected {
        let _ = session.shutdown().await;
    }
}

/// Write a JSON-RPC message to the stdio transport.
async fn send_json_rpc_message(
    server: &mut McpServer,
    msg: JsonRpcMessage,
) -> Result<(), Box<dyn std::error::Error>> {
    match msg {
        JsonRpcMessage::Response { id, result } => server.send_response(id, &result).await?,
        JsonRpcMessage::Error { id, code, message } => {
            server.send_error(id, code, &message).await?
        }
        JsonRpcMessage::Notification { method, params } => {
            // Notifications over stdio are written as JSON-RPC notifications.
            let value = if let Some(params) = params {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": method,
                    "params": params,
                })
            } else {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": method,
                })
            };
            server.write_raw(&value).await?;
        }
    }
    Ok(())
}
