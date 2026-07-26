//! HTTP/SSE MCP server entry point.
//!
//! Exposes the same debug tools as the stdio MCP server, but over HTTP/SSE so
//! multiple clients can share a single `DebugSession`/`OpenOcdClient`.
//!
//! Endpoints:
//!   GET  /sse              — Subscribe to server-sent events. The first event
//!                            is `endpoint` and contains the POST URL for this
//!                            client session.
//!   POST /message          — Send a JSON-RPC request/notification. Responses
//!                            are delivered asynchronously over the SSE stream.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Router,
};
use dap_client::DapClient;
use dap_trace::TraceHandle;
use debug_session::DebugSession;
use mcp_protocol::transport::McpServer;
use openocd_client::OpenOcdClient;
use serde::Deserialize;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::config::Config;
use crate::mcp_router::{JsonRpcMessage, McpRouter};
use crate::server::{cleanup_session, spawn_dap_event_loop};

/// A single MCP client SSE connection.
struct ClientConnection {
    sender: mpsc::UnboundedSender<String>,
    router: RwLock<McpRouter>,
}

/// Session state shared across all connected MCP clients.
pub struct SharedSession {
    session: Arc<DebugSession>,
    openocd: Arc<RwLock<Option<OpenOcdClient>>>,
    trace: TraceHandle,
    clients: RwLock<HashMap<String, ClientConnection>>,
}

impl SharedSession {
    fn new(
        session: Arc<DebugSession>,
        openocd: Arc<RwLock<Option<OpenOcdClient>>>,
        trace: TraceHandle,
    ) -> Self {
        Self {
            session,
            openocd,
            trace,
            clients: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new SSE client and return its session id.
    async fn register_client(&self) -> (String, mpsc::UnboundedReceiver<String>) {
        let session_id = Uuid::new_v4().to_string();
        let (tx, rx) = mpsc::unbounded_channel();
        let conn = ClientConnection {
            sender: tx,
            router: RwLock::new(McpRouter::new()),
        };
        self.clients.write().await.insert(session_id.clone(), conn);
        (session_id, rx)
    }

    /// Broadcast a JSON-RPC message to all connected clients, pruning dead ones.
    async fn broadcast_all(&self, msg: JsonRpcMessage) {
        let json = match serde_json::to_string(&msg) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to serialize broadcast message: {e}");
                return;
            }
        };

        let mut clients = self.clients.write().await;
        let mut dead = Vec::new();
        for (id, conn) in clients.iter() {
            if conn.sender.send(json.clone()).is_err() {
                dead.push(id.clone());
            }
        }
        for id in dead {
            clients.remove(&id);
        }
    }

    /// Send a JSON-RPC message to a specific client session.
    async fn send_to(&self, session_id: &str, msg: JsonRpcMessage) {
        let json = match serde_json::to_string(&msg) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to serialize response message: {e}");
                return;
            }
        };

        let mut clients = self.clients.write().await;
        if let Some(conn) = clients.get(session_id) {
            if conn.sender.send(json).is_err() {
                clients.remove(session_id);
            }
        }
    }
}

#[derive(Deserialize)]
struct MessageQuery {
    session_id: String,
}

/// Run the HTTP/SSE MCP server on the given port.
pub async fn run(port: u16, config: Option<Config>) {
    info!("TeleDAP HTTP/SSE MCP server starting on port {port}...");

    // ── Session setup ─────────────────────────────────────────────────
    let (trace, _bg) = TraceHandle::new(None, 10_000);
    let client = DapClient::with_trace(4 * 1024 * 1024, trace.clone());
    let session = Arc::new(DebugSession::new(client, Some(trace.clone())));
    let openocd: Arc<RwLock<Option<OpenOcdClient>>> = Arc::new(RwLock::new(None));
    let shared = Arc::new(SharedSession::new(
        Arc::clone(&session),
        Arc::clone(&openocd),
        trace.clone(),
    ));

    // ── Auto-setup from config file (before background tasks) ─────────
    // Auto-setup must consume the initialized event directly, so it runs
    // before the background event loop is spawned.
    if let Some(ref cfg) = config {
        if cfg.options.auto_start {
            if let Err(e) = crate::config::auto_configure(&session, &openocd, cfg).await {
                error!("Auto-setup failed: {e}");
                cleanup_session(&session, &openocd).await;
                info!("HTTP/SSE server shutting down due to configuration error.");
                return;
            }
            info!("Auto-setup complete — session is ready.");
        } else {
            // Register path mappings even when auto_start is false
            for dir in &cfg.path_mapping.base_dirs {
                session.register_base_dir(dir).await;
            }
            for (alias, abs_path) in &cfg.path_mapping.aliases {
                session.register_path_alias(alias, abs_path).await;
            }
            info!("Config path mappings registered (auto_start is off).");
        }
    }

    // ── Background tasks ──────────────────────────────────────────────
    spawn_dap_event_loop(Arc::clone(&session));
    spawn_state_change_broadcaster(Arc::clone(&shared));

    // ── Axum app ──────────────────────────────────────────────────────
    let app = Router::new()
        .route("/sse", get(sse_handler))
        .route("/message", post(message_handler))
        .with_state(Arc::clone(&shared));

    let listener = match tokio::net::TcpListener::bind(("0.0.0.0", port)).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind HTTP server to port {port}: {e}");
            return;
        }
    };

    info!("TeleDAP HTTP/SSE MCP server listening on http://0.0.0.0:{port}");

    if let Err(e) = axum::serve(listener, app).await {
        error!("HTTP server error: {e}");
    }

    // ── Cleanup ──────────────────────────────────────────────────────
    cleanup_session(&session, &openocd).await;
    info!("HTTP/SSE MCP server shutting down.");
}

/// SSE endpoint: opens a new client session and streams JSON-RPC messages.
async fn sse_handler(
    State(shared): State<Arc<SharedSession>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let (session_id, rx) = shared.register_client().await;
    info!("New SSE client session: {session_id}");

    let endpoint = format!("/message?session_id={session_id}");
    let stream = UnboundedReceiverStream::new(rx)
        .map(|msg| Ok::<_, Infallible>(Event::default().event("message").data(msg)));

    let initial = Event::default().event("endpoint").data(endpoint);
    let stream = tokio_stream::once(Ok(initial)).chain(stream);

    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Message endpoint: accepts a JSON-RPC message and routes it asynchronously.
async fn message_handler(
    State(shared): State<Arc<SharedSession>>,
    Query(query): Query<MessageQuery>,
    body: String,
) -> StatusCode {
    let msg = match McpServer::parse_incoming(&body) {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to parse incoming MCP message: {e}");
            // Per MCP spec, parse errors are sent over the SSE stream if we
            // can identify the client; otherwise we just return 400.
            let err = JsonRpcMessage::Error {
                id: None,
                code: mcp_protocol::PARSE_ERROR,
                message: e.to_string(),
            };
            shared.send_to(&query.session_id, err).await;
            return StatusCode::ACCEPTED;
        }
    };

    // Route through this client's McpRouter.
    let response = {
        let clients = shared.clients.read().await;
        let conn = match clients.get(&query.session_id) {
            Some(c) => c,
            None => return StatusCode::NOT_FOUND,
        };
        let mut router = conn.router.write().await;
        router
            .handle_message(msg, &shared.session, &shared.openocd, &shared.trace)
            .await
    };

    if let Some(response) = response {
        shared.send_to(&query.session_id, response).await;
    }

    StatusCode::ACCEPTED
}

/// Watch for debug-session state changes and notify all clients.
fn spawn_state_change_broadcaster(shared: Arc<SharedSession>) {
    tokio::spawn(async move {
        let mut rx = shared.session.state_watcher();
        let mut last_state = *rx.borrow();
        while rx.changed().await.is_ok() {
            let new_state = *rx.borrow();
            if new_state != last_state {
                last_state = new_state;
                debug!("State changed to {new_state:?}; broadcasting tools/list_changed");
                let notification = JsonRpcMessage::Notification {
                    method: "notifications/tools/list_changed".into(),
                    params: None,
                };
                shared.broadcast_all(notification).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shared_session_register_and_broadcast() {
        let (trace, _bg) = TraceHandle::new(None, 100);
        let client = DapClient::with_trace(1024, trace.clone());
        let session = Arc::new(DebugSession::new(client, Some(trace.clone())));
        let openocd = Arc::new(RwLock::new(None));
        let shared = Arc::new(SharedSession::new(session, openocd, trace));

        let (id1, mut rx1) = shared.register_client().await;
        let (id2, mut rx2) = shared.register_client().await;

        assert_ne!(id1, id2);

        let msg = JsonRpcMessage::Notification {
            method: "test".into(),
            params: None,
        };
        shared.broadcast_all(msg).await;

        let got1 = rx1.recv().await.expect("client 1 should receive");
        let got2 = rx2.recv().await.expect("client 2 should receive");
        assert!(got1.contains("test"));
        assert_eq!(got1, got2);
    }

    #[tokio::test]
    async fn test_http_sse_initialize_handshake() {
        use axum::body::{to_bytes, Body};
        use axum::http::{Method, Request, StatusCode};
        use tower::ServiceExt;

        let (trace, _bg) = TraceHandle::new(None, 100);
        let client = DapClient::with_trace(1024, trace.clone());
        let session = Arc::new(DebugSession::new(client, Some(trace.clone())));
        let openocd = Arc::new(RwLock::new(None));
        let shared = Arc::new(SharedSession::new(Arc::clone(&session), openocd, trace));

        let app = Router::new()
            .route("/sse", get(sse_handler))
            .route("/message", post(message_handler))
            .with_state(Arc::clone(&shared));

        // ── Connect SSE ─────────────────────────────────────────────────
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/sse")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body();
        let mut stream = body.into_data_stream();
        let endpoint_event = read_sse_event(&mut stream).await;
        assert!(endpoint_event.contains("event: endpoint"));
        let data_line = endpoint_event
            .lines()
            .find(|l| l.starts_with("data:"))
            .expect("endpoint event should have data line");
        let endpoint_url = data_line.trim_start_matches("data:").trim();
        let session_id = endpoint_url
            .split("session_id=")
            .nth(1)
            .expect("endpoint should contain session_id");

        // ── POST initialize ─────────────────────────────────────────────
        let init_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "1.0" },
            }
        })
        .to_string();

        let post_response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/message?session_id={}", session_id))
                    .header("content-type", "application/json")
                    .body(Body::from(init_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(post_response.status(), StatusCode::ACCEPTED);

        // ── Read initialize response from SSE stream ────────────────────
        let response_event = read_sse_event(&mut stream).await;
        assert!(response_event.contains("event: message"));
        let data_line = response_event
            .lines()
            .find(|l| l.starts_with("data:"))
            .expect("response event should have data line");
        let json: serde_json::Value =
            serde_json::from_str(data_line.trim_start_matches("data:").trim())
                .expect("response data should be valid JSON");
        assert_eq!(json.get("id").and_then(|v| v.as_u64()), Some(1));
        assert!(json.get("result").is_some());
        assert!(json.get("error").is_none());

        // Suppress unused import warning for to_bytes; keep import for future tests.
        let _ = to_bytes;
    }

    /// Read one SSE event (text up to a blank line) from a byte stream.
    async fn read_sse_event<S>(stream: &mut S) -> String
    where
        S: tokio_stream::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Unpin,
    {
        let mut buf = Vec::new();
        while let Some(Ok(chunk)) = stream.next().await {
            buf.extend_from_slice(&chunk);
            let text = String::from_utf8_lossy(&buf);
            if text.contains("\n\n") {
                break;
            }
        }
        String::from_utf8(buf).expect("SSE event should be UTF-8")
    }
}
