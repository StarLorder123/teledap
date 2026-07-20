//! DapClient — manages a debug adapter child process and provides high-level DAP operations.
//!
//! # Architecture
//!
//! ```text
//!                     ┌─────────────────────────────┐
//!  DapClient          │   debug adapter process      │
//!                     │                              │
//!  send_request() ───►│ stdin  (DAP requests)        │
//!  events ◄───────────│ stdout (responses/events)    │
//!  diagnostics ◄──────│ stderr (diagnostics)         │
//!                     └─────────────────────────────┘
//! ```
//!
//! A background task continuously reads framed DAP messages from stdout
//! and routes them:
//! - **Responses** → dispatched via `oneshot` channel to the waiting `send_request` call
//! - **Events** → published to an `mpsc` channel for the consumer to poll

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use dap_codec::DapCodec;
use dap_trace::TraceHandle;
use dap_types::base::{Event, ProtocolMessage, Request, Response};
use dap_types::requests::DapRequest;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::codec::FramedRead;

use crate::error::DapClientError;

/// Default maximum frame size: 4 MiB.
pub const DEFAULT_MAX_FRAME_SIZE: usize = 4 * 1024 * 1024;

/// The type of debug adapter, controlling behavioral differences
/// (e.g. whether the `launch` response is deferred).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterKind {
    /// codelldb: defers `launch` response until after `configurationDone`.
    Codelldb,
    /// GDB built-in DAP mode (`gdb -i dap`): responds to `launch` promptly.
    Gdb,
}

/// Configuration for spawning a debug adapter process.
#[derive(Debug, Clone)]
pub struct AdapterConfig {
    /// Path to the debug adapter binary (e.g. `"codelldb"`, `"gdb"`).
    pub path: String,
    /// The type of adapter (controls behavioral branching).
    pub kind: AdapterKind,
    /// Command-line arguments to pass to the adapter (e.g. `["-i", "dap"]` for GDB).
    pub args: Vec<String>,
}

/// Manages a debug adapter process and provides typed DAP request/response communication.
pub struct DapClient {
    /// Debug adapter child process handle.
    child: Mutex<Option<Child>>,

    /// Stdin writer for sending DAP requests.
    stdin: Mutex<Option<ChildStdin>>,

    /// Monotonically increasing sequence counter for DAP requests.
    seq: AtomicU64,

    /// Pending request waiters, keyed by request `seq`.
    /// Shared via Arc so the background reader task and `send_request()` use the same map.
    #[allow(clippy::type_complexity)]
    pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Response, DapClientError>>>>>,

    /// Sender side of the event channel (background reader → consumers).
    event_tx: mpsc::UnboundedSender<Event>,

    /// Receiver side of the event channel.
    event_rx: Mutex<mpsc::UnboundedReceiver<Event>>,

    /// Maximum allowed frame size for the codec.
    max_frame_size: usize,

    /// Optional trace handle for recording debug session interactions.
    trace: Option<TraceHandle>,

    /// The kind of adapter currently running (set by `start()`).
    adapter_kind: Mutex<Option<AdapterKind>>,

    /// Optional path to the liblldb shared library.
    /// When set, `start()` prepends its parent directory to the `PATH`
    /// environment variable of the spawned adapter process.
    lib_lldb_path: Mutex<Option<String>>,
}

impl DapClient {
    /// Creates a new `DapClient` with no running process.
    ///
    /// Call `start()` to spawn a debug adapter and begin communication.
    pub fn new(max_frame_size: usize) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            seq: AtomicU64::new(1),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            event_rx: Mutex::new(event_rx),
            max_frame_size,
            trace: None,
            adapter_kind: Mutex::new(None),
            lib_lldb_path: Mutex::new(None),
        }
    }

    /// Creates a new `DapClient` with debug session tracing enabled.
    ///
    /// When a `TraceHandle` is provided, all DAP requests, responses, and events
    /// are automatically recorded to the trace.
    pub fn with_trace(max_frame_size: usize, trace: TraceHandle) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            seq: AtomicU64::new(1),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            event_rx: Mutex::new(event_rx),
            max_frame_size,
            trace: Some(trace),
            adapter_kind: Mutex::new(None),
            lib_lldb_path: Mutex::new(None),
        }
    }

    /// Returns true if the debug adapter process has been started.
    pub async fn is_running(&self) -> bool {
        self.child.lock().await.is_some()
    }

    /// Returns the adapter kind, if the adapter has been started.
    pub async fn adapter_kind(&self) -> Option<AdapterKind> {
        *self.adapter_kind.lock().await
    }

    /// Set the path to the liblldb shared library.
    ///
    /// Call this before `start()` to make the DLL available to codelldb
    /// at adapter-spawn time (prepended to the child process PATH).
    pub async fn set_lib_lldb_path(&self, path: Option<String>) {
        *self.lib_lldb_path.lock().await = path;
    }

    /// Returns the currently configured liblldb path, if any.
    pub async fn lib_lldb_path(&self) -> Option<String> {
        self.lib_lldb_path.lock().await.clone()
    }

    // ── Process Lifecycle ─────────────────────────────────────────

    /// Spawns a debug adapter as a child process with piped stdio.
    ///
    /// A background task is launched to continuously read and frame DAP messages
    /// from stdout. Responses are routed to the matching `send_request` waiter via
    /// `request_seq`, and events are published to the event channel.
    ///
    /// A second background task consumes stderr line-by-line to prevent
    /// pipe buffer deadlock with verbose adapters.
    pub async fn start(&self, config: &AdapterConfig) -> Result<(), DapClientError> {
        if self.is_running().await {
            return Err(DapClientError::SpawnFailed(
                "adapter is already running".into(),
            ));
        }

        let mut cmd = Command::new(&config.path);
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // ── liblldb PATH injection ──────────────────────────────────
        // On Windows, codelldb needs liblldb.dll in its DLL search path.
        // Prepending the DLL's parent directory to PATH is the most
        // portable way to make it findable without registry changes.
        if let Some(ref lldb_path) = *self.lib_lldb_path.lock().await {
            if let Some(parent) = std::path::Path::new(lldb_path).parent() {
                let separator = if cfg!(windows) { ";" } else { ":" };
                let existing_path = std::env::var("PATH").unwrap_or_default();
                let new_path = format!("{}{}{}", parent.display(), separator, existing_path);
                cmd.env("PATH", &new_path);
                tracing::info!(
                    liblldb_dir = %parent.display(),
                    "prepended liblldb directory to adapter PATH"
                );
            } else {
                tracing::warn!(
                    liblldb_path = %lldb_path,
                    "cannot extract parent directory from liblldb path; skipping PATH injection"
                );
            }
        }

        if !config.args.is_empty() {
            cmd.args(&config.args);
        }

        let mut child = cmd.spawn().map_err(|e| {
            DapClientError::SpawnFailed(format!("Failed to spawn '{}': {}", config.path, e))
        })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DapClientError::SpawnFailed("adapter stdout is not available".into()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| DapClientError::SpawnFailed("adapter stdin is not available".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| DapClientError::SpawnFailed("adapter stderr is not available".into()))?;

        // Share the pending_requests map with the background reader task
        let pending_clone = self.pending_requests.clone();
        let event_tx = self.event_tx.clone();
        let max_frame = self.max_frame_size;
        let trace_opt = self.trace.clone();

        // Spawn background stdout reader
        tokio::spawn(async move {
            let mut framed = FramedRead::new(stdout, DapCodec::new(max_frame));
            while let Some(result) = framed.next().await {
                match result {
                    Ok(ProtocolMessage::Response(response)) => {
                        let request_seq = response.request_seq;
                        let mut pending_map = pending_clone.lock().await;
                        // Trace the response
                        if let Some(ref t) = trace_opt {
                            let result = response.body.as_ref().map(|b| b.to_string());
                            t.trace_response(&response.command, result, None);
                        }
                        if let Some(sender) = pending_map.remove(&request_seq) {
                            let _ = sender.send(Ok(response));
                        } else {
                            tracing::warn!(
                                request_seq = request_seq,
                                "Received response with no pending request"
                            );
                        }
                    }
                    Ok(ProtocolMessage::Event(event)) => {
                        tracing::trace!(
                            event_type = %event.event,
                            seq = event.seq,
                            "DAP event received"
                        );
                        // Trace the event (best-effort)
                        if let Some(ref t) = trace_opt {
                            t.trace_event(&event.event, event.body.clone());
                        }
                        // Ignore send error — event channel may be closed during shutdown
                        let _ = event_tx.send(event);
                    }
                    Ok(ProtocolMessage::Request(_)) => {
                        // Reverse requests are not currently handled by TeleDAP
                        tracing::warn!("Unexpected request from debug adapter");
                    }
                    Err(e) => {
                        tracing::error!("DAP frame decode error: {}", e);
                        break;
                    }
                }
            }
            tracing::info!("adapter stdout reader exited.");
        });

        // Spawn background stderr consumer to prevent pipe buffer deadlock
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "adapter_stderr", "{}", line);
            }
            tracing::debug!("adapter stderr reader exited.");
        });

        *self.stdin.lock().await = Some(stdin);
        *self.child.lock().await = Some(child);
        *self.adapter_kind.lock().await = Some(config.kind);

        tracing::info!(path = %config.path, kind = ?config.kind, "adapter started");
        if let Some(ref t) = self.trace {
            t.trace_internal(
                "adapter_started",
                Some(serde_json::json!({
                    "path": config.path,
                    "kind": format!("{:?}", config.kind),
                })),
            );
        }
        Ok(())
    }

    // ── Request / Response ────────────────────────────────────────

    /// Send a DAP request and wait for the matching response.
    ///
    /// The request body is constructed from the `DapRequest` trait:
    /// - `Req::COMMAND` becomes the `command` field
    /// - `arguments` is serialized into the `arguments` field
    ///
    /// On success, the response body is deserialized into `Req::Response`.
    pub async fn send_request<Req>(
        &self,
        arguments: Req::Arguments,
    ) -> Result<Req::Response, DapClientError>
    where
        Req: DapRequest,
        Req::Arguments: Serialize,
        Req::Response: DeserializeOwned,
    {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);

        // Serialize arguments once for both the request and the trace
        let args_value = serde_json::to_value(arguments)?;

        // Trace the outgoing request (clone args_value — cheap for small JSON)
        let start = Instant::now();
        if let Some(ref t) = self.trace {
            t.trace_request(Req::COMMAND, Some(args_value.clone()));
        }

        let request = Request {
            seq,
            command: Req::COMMAND.to_string(),
            arguments: Some(args_value),
        };

        let msg = ProtocolMessage::Request(request);

        // Set up the oneshot channel BEFORE sending, to avoid race with response
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(seq, tx);
        }

        // Encode and write to stdin
        {
            let mut stdin_guard = self.stdin.lock().await;
            let stdin = stdin_guard
                .as_mut()
                .ok_or_else(|| DapClientError::NotConnected("adapter not started".into()))?;

            let buf = dap_codec::encode_to_bytes(&msg)
                .map_err(|e| DapClientError::DapProtocol(e.to_string()))?;
            stdin.write_all(&buf).await?;
            stdin.flush().await?;
        }

        tracing::debug!(command = Req::COMMAND, seq = seq, "DAP request sent");

        // Wait for matching response
        let elapsed = start.elapsed();
        match rx.await {
            Ok(Ok(response)) => {
                if let Some(ref t) = self.trace {
                    let result = response.body.as_ref().map(|b| b.to_string());
                    t.trace_response(Req::COMMAND, result, Some(elapsed.as_micros() as i64));
                }
                if !response.success {
                    return Err(DapClientError::request_failed(
                        Req::COMMAND,
                        response.message.unwrap_or_else(|| "unknown error".into()),
                    ));
                }
                match response.body {
                    Some(body) => {
                        let result: Req::Response = serde_json::from_value(body)?;
                        Ok(result)
                    }
                    None => {
                        // Use serde_json::Value::Null as Default equivalent
                        let result: Req::Response =
                            serde_json::from_value(serde_json::Value::Null)?;
                        Ok(result)
                    }
                }
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                // Sender was dropped — clean up pending entry
                let mut pending = self.pending_requests.lock().await;
                pending.remove(&seq);
                Err(DapClientError::ProcessExited(
                    "adapter stdout stream closed before response".into(),
                ))
            }
        }
    }

    /// Send a DAP request without waiting for a response (fire-and-forget).
    ///
    /// This is useful for requests like `configurationDone` where the response
    /// has no meaningful body, or for adapters (like codelldb) that defer
    /// their response until after `configurationDone`.
    pub async fn send_request_nb<Req>(
        &self,
        arguments: Req::Arguments,
    ) -> Result<(), DapClientError>
    where
        Req: DapRequest,
        Req::Arguments: Serialize,
    {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);

        let request = Request {
            seq,
            command: Req::COMMAND.to_string(),
            arguments: Some(serde_json::to_value(arguments)?),
        };

        let msg = ProtocolMessage::Request(request);

        let mut stdin_guard = self.stdin.lock().await;
        let stdin = stdin_guard
            .as_mut()
            .ok_or_else(|| DapClientError::NotConnected("adapter not started".into()))?;

        let buf = dap_codec::encode_to_bytes(&msg)
            .map_err(|e| DapClientError::DapProtocol(e.to_string()))?;
        stdin.write_all(&buf).await?;
        stdin.flush().await?;

        tracing::debug!(
            command = Req::COMMAND,
            seq = seq,
            "DAP request sent (non-blocking)"
        );
        Ok(())
    }

    // ── Event Stream ──────────────────────────────────────────────

    /// Receive the next DAP event from the debug adapter.
    ///
    /// Returns `None` if the event channel has been closed (e.g. process exited).
    pub async fn recv_event(&self) -> Option<Event> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await
    }

    /// Try to receive an event without blocking.
    pub fn try_recv_event(&self) -> Option<Event> {
        // We can't use try_lock on an async Mutex, but we can poll
        // For non-blocking use, the caller should use `recv_event` with a timeout
        None
    }

    /// Drain any pending events from the channel.
    pub async fn drain_events(&self) {
        let mut rx = self.event_rx.lock().await;
        while rx.try_recv().is_ok() {
            // Keep draining
        }
    }

    // ── Shutdown ──────────────────────────────────────────────────

    /// Disconnect from the debuggee and shut down the debug adapter.
    ///
    /// Sends a `disconnect` request (best-effort), then kills the child process.
    pub async fn shutdown(&self) -> Result<(), DapClientError> {
        // Best-effort disconnect
        let _ = self
            .send_request::<dap_types::requests::DisconnectRequest>(
                dap_types::requests::DisconnectArguments {
                    terminate_debuggee: Some(false),
                    ..Default::default()
                },
            )
            .await;

        let mut child_guard = self.child.lock().await;
        if let Some(ref mut child) = *child_guard {
            let _ = child.kill().await;
        }
        *child_guard = None;
        *self.adapter_kind.lock().await = None;

        tracing::info!("debug adapter shut down.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_new() {
        let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);
        // No tokio runtime needed for construction
        assert_eq!(client.seq.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_set_lib_lldb_path() {
        let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);

        // Initially None
        assert_eq!(client.lib_lldb_path().await, None);

        // Set and verify
        client
            .set_lib_lldb_path(Some("C:\\LLVM\\bin\\liblldb.dll".into()))
            .await;
        assert_eq!(
            client.lib_lldb_path().await.as_deref(),
            Some("C:\\LLVM\\bin\\liblldb.dll")
        );

        // Clear and verify
        client.set_lib_lldb_path(None).await;
        assert_eq!(client.lib_lldb_path().await, None);
    }
}
