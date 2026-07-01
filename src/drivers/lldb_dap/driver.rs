//! DapDriver — manages a codelldb process and provides high-level DAP operations.

use crate::audit_tracker::{AuditLogger, LogDirection, LogSource};
use crate::error::DriverError;
use super::frame_decoder::{DapCodec, DapMessage};
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;
use tokio_util::codec::{Encoder, FramedRead};

/// Manages a codelldb process and provides high-level DAP operations.
///
/// # Architecture
///
/// ```text
///                     ┌─────────────────────────┐
///  DapDriver          │   codelldb process       │
///                     │                          │
///  send_request() ───►│ stdin  (DAP requests)    │
///  events ◄───────────│ stdout (responses/events)│
///  tracing ◄──────────│ stderr (diagnostics)     │
///                     └─────────────────────────┘
/// ```
///
/// A background task continuously reads framed DAP messages from stdout
/// and publishes them to an internal channel. `send_request` waits for
/// the matching response by sequence number.
pub struct DapDriver {
    /// codelldb child process handle.
    child: Mutex<Option<Child>>,

    /// Stdin writer for sending DAP requests.
    stdin: Mutex<Option<ChildStdin>>,

    /// Monotonically increasing sequence counter for DAP requests.
    seq: AtomicU64,

    /// Audit logger for command tracing.
    audit: Arc<AuditLogger>,

    /// Maximum allowed Content-Length for inbound frames.
    max_frame_size: usize,

    /// Sender side of the event channel (background reader → consumers).
    event_tx: tokio::sync::mpsc::UnboundedSender<DapMessage>,

    /// Receiver side of the event channel.
    event_rx: Mutex<tokio::sync::mpsc::UnboundedReceiver<DapMessage>>,
}

impl DapDriver {
    /// Creates a new `DapDriver` with no running process.
    ///
    /// Call `start()` to spawn codelldb and begin communication.
    pub fn new(audit: Arc<AuditLogger>, max_frame_size: usize) -> Self {
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            seq: AtomicU64::new(1),
            audit,
            max_frame_size,
            event_tx,
            event_rx: Mutex::new(event_rx),
        }
    }

    /// Returns true if the codelldb process has been started.
    pub async fn is_running(&self) -> bool {
        self.child.lock().await.is_some()
    }

    // ── Process Lifecycle ─────────────────────────────────────────

    /// Spawns codelldb as a child process with piped stdio.
    ///
    /// A background task is launched to continuously read and frame
    /// DAP messages from stdout. Messages are logged to the audit
    /// trail and published to the internal event channel.
    pub async fn start(&self, codelldb_path: &str) -> Result<(), DriverError> {
        if self.is_running().await {
            return Err(DriverError::SpawnFailed(
                "codelldb is already running".into(),
            ));
        }

        let mut child = Command::new(codelldb_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                DriverError::SpawnFailed(format!(
                    "Failed to spawn '{}': {}",
                    codelldb_path, e
                ))
            })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            DriverError::SpawnFailed("codelldb stdout is not available".into())
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            DriverError::SpawnFailed("codelldb stdin is not available".into())
        })?;

        // Spawn background reader for stdout
        let audit = self.audit.clone();
        let event_tx = self.event_tx.clone();
        let max_frame = self.max_frame_size;
        tokio::spawn(async move {
            let mut framed = FramedRead::new(stdout, DapCodec::new(max_frame));
            while let Some(result) = framed.next().await {
                match result {
                    Ok(msg) => {
                        let label = msg
                            .command
                            .as_deref()
                            .or(msg.event.as_deref())
                            .unwrap_or("unknown");

                        let source = if msg.msg_type == "event" {
                            LogSource::DapEvent
                        } else {
                            LogSource::DapResponse
                        };

                        audit.log(
                            source,
                            LogDirection::Inbound,
                            label,
                            Some(msg.body.clone()),
                            None,
                            None,
                        );

                        let _ = event_tx.send(msg);
                    }
                    Err(e) => {
                        tracing::error!("DAP frame decode error: {}", e);
                        break;
                    }
                }
            }
            tracing::info!("codelldb stdout reader exited.");
        });

        *self.stdin.lock().await = Some(stdin);
        *self.child.lock().await = Some(child);

        self.audit.log(
            LogSource::Internal,
            LogDirection::Outbound,
            "codelldb_started",
            Some(json!({"path": codelldb_path})),
            None,
            None,
        );

        tracing::info!("codelldb started: {}", codelldb_path);
        Ok(())
    }

    // ── Request / Response ────────────────────────────────────────

    /// Sends a DAP request and waits for the matching response.
    pub async fn send_request(
        &self,
        command: &str,
        arguments: Option<Value>,
    ) -> Result<Value, DriverError> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let args_for_wire = arguments.clone().unwrap_or(json!({}));
        let request = json!({
            "type": "request",
            "seq": seq,
            "command": command,
            "arguments": args_for_wire
        });

        // Encode and send via stdin
        {
            let mut stdin_guard = self.stdin.lock().await;
            let stdin = stdin_guard.as_mut().ok_or_else(|| {
                DriverError::NotConnected("codelldb not started".into())
            })?;

            let mut codec = DapCodec::new(self.max_frame_size);
            let mut buf = bytes::BytesMut::new();
            codec
                .encode(request.clone(), &mut buf)
                .map_err(|e: std::io::Error| {
                    DriverError::DapProtocol(e.to_string())
                })?;
            stdin.write_all(&buf).await?;
            stdin.flush().await?;
        }

        self.audit.log(
            LogSource::DapRequest,
            LogDirection::Outbound,
            command,
            arguments,
            None,
            None,
        );

        tracing::debug!("DAP request sent: {} (seq={})", command, seq);

        // Wait for matching response
        let mut rx = self.event_rx.lock().await;
        loop {
            match rx.recv().await {
                Some(msg)
                    if msg.msg_type == "response"
                        && msg.seq == seq =>
                {
                    if !msg.body.get("success").and_then(|v| v.as_bool()).unwrap_or(true) {
                        let error_msg = msg
                            .body
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown error");
                        return Err(DriverError::DapRequestFailed {
                            command: command.to_string(),
                            message: error_msg.to_string(),
                        });
                    }
                    return Ok(msg.body);
                }
                Some(_msg) => {
                    continue;
                }
                None => {
                    return Err(DriverError::ProcessExited(
                        "codelldb stdout stream closed".into(),
                    ));
                }
            }
        }
    }

    /// Sends a DAP request without waiting for a response.
    pub async fn send_notification(
        &self,
        command: &str,
        arguments: Option<Value>,
    ) -> Result<(), DriverError> {
        let notification = json!({
            "type": "event",
            "seq": 0,
            "event": command,
            "body": arguments.unwrap_or(json!({}))
        });

        let mut stdin_guard = self.stdin.lock().await;
        let stdin = stdin_guard
            .as_mut()
            .ok_or(DriverError::NotConnected("codelldb not started".into()))?;

        let mut codec = DapCodec::new(self.max_frame_size);
        let mut buf = bytes::BytesMut::new();
        codec
            .encode(notification, &mut buf)
            .map_err(|e: std::io::Error| {
                DriverError::DapProtocol(e.to_string())
            })?;
        stdin.write_all(&buf).await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Drain any pending events from the channel.
    pub async fn drain_events(&self) {
        let mut rx = self.event_rx.lock().await;
        while let Ok(msg) = rx.try_recv() {
            tracing::trace!(
                "Drained event: type={}, command={:?}, event={:?}",
                msg.msg_type,
                msg.command,
                msg.event
            );
        }
    }

    // ── High-Level DAP Operations ─────────────────────────────────

    /// Initialize the DAP session (handshake).
    pub async fn initialize(&self) -> Result<(), DriverError> {
        let _resp = self
            .send_request(
                "initialize",
                Some(json!({
                    "adapterID": "codelldb",
                    "columnsStartAt1": true,
                    "linesStartAt1": true,
                    "pathFormat": "path"
                })),
            )
            .await?;
        tracing::info!("DAP initialize completed");
        Ok(())
    }

    /// Launch the target binary, optionally connecting to a GDB server.
    ///
    /// When `gdb_remote` is `None`, the binary is launched locally on the host
    /// (no hardware required). Set `stop_on_entry` to `true` for local debugging
    /// to halt at the program entry point.
    pub async fn launch(
        &self,
        elf_path: &str,
        gdb_remote: Option<&str>,
        stop_on_entry: bool,
    ) -> Result<(), DriverError> {
        let mut args = json!({
            "program": elf_path,
            "stopOnEntry": stop_on_entry,
        });

        if let Some(remote) = gdb_remote {
            args["customLaunchSetupCommands"] = json!([{
                "text": format!("gdb-remote {}", remote)
            }]);
        }

        let _resp = self.send_request("launch", Some(args)).await?;
        tracing::info!("DAP launch: {} (gdb-remote: {:?}, stopOnEntry: {})", elf_path, gdb_remote, stop_on_entry);
        Ok(())
    }

    /// Set a breakpoint at a specific source file and line.
    pub async fn set_breakpoint(
        &self,
        file: &str,
        line: u32,
    ) -> Result<Value, DriverError> {
        self.send_request(
            "setBreakpoints",
            Some(json!({
                "source": { "path": file },
                "breakpoints": [{ "line": line }]
            })),
        )
        .await
    }

    /// Continue execution of the target.
    pub async fn continue_execution(
        &self,
        thread_id: Option<u64>,
    ) -> Result<(), DriverError> {
        let _resp = self
            .send_request(
                "continue",
                Some(json!({ "threadId": thread_id.unwrap_or(0) })),
            )
            .await?;
        Ok(())
    }

    /// Pause (halt) execution of the target.
    pub async fn pause(&self, thread_id: u64) -> Result<(), DriverError> {
        let _resp = self
            .send_request("pause", Some(json!({ "threadId": thread_id })))
            .await?;
        Ok(())
    }

    /// Step the target by the given granularity ("in", "over", "out").
    pub async fn step(
        &self,
        granularity: &str,
        thread_id: u64,
    ) -> Result<(), DriverError> {
        let cmd = match granularity {
            "in" => "stepIn",
            "out" => "stepOut",
            _ => "next",
        };
        let _resp = self
            .send_request(cmd, Some(json!({ "threadId": thread_id })))
            .await?;
        Ok(())
    }

    /// Get the call stack for a thread.
    pub async fn stack_trace(
        &self,
        thread_id: u64,
    ) -> Result<Value, DriverError> {
        self.send_request(
            "stackTrace",
            Some(json!({ "threadId": thread_id, "levels": 20 })),
        )
        .await
    }

    /// Get the list of threads.
    pub async fn threads(&self) -> Result<Value, DriverError> {
        self.send_request("threads", None).await
    }

    /// Get scopes for a stack frame.
    pub async fn scopes(&self, frame_id: u64) -> Result<Value, DriverError> {
        self.send_request("scopes", Some(json!({ "frameId": frame_id }))).await
    }

    /// Get variables within a scope or complex variable.
    pub async fn variables(
        &self,
        variables_reference: u64,
    ) -> Result<Value, DriverError> {
        self.send_request(
            "variables",
            Some(json!({ "variablesReference": variables_reference })),
        )
        .await
    }

    /// Evaluate an expression in the debuggee.
    pub async fn evaluate(
        &self,
        expr: &str,
        frame_id: Option<u64>,
    ) -> Result<Value, DriverError> {
        let mut args = json!({ "expression": expr });
        if let Some(fid) = frame_id {
            args["frameId"] = json!(fid);
        }
        self.send_request("evaluate", Some(args)).await
    }

    /// Disconnect from the debuggee and shut down codelldb.
    pub async fn shutdown(&self) -> Result<(), DriverError> {
        let _ = self
            .send_request(
                "disconnect",
                Some(json!({ "terminateDebuggee": false })),
            )
            .await;

        let mut child_guard = self.child.lock().await;
        if let Some(ref mut child) = *child_guard {
            let _ = child.kill().await;
        }
        *child_guard = None;

        self.audit.log(
            LogSource::Internal,
            LogDirection::Outbound,
            "codelldb_shutdown",
            None,
            None,
            None,
        );

        tracing::info!("codelldb shut down.");
        Ok(())
    }
}
