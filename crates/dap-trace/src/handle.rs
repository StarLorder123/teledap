//! TraceHandle — lightweight, Clone + Send + Sync handle for recording trace entries.
//!
//! All methods are synchronous and non-blocking. The actual I/O is handled
//! by a background tokio task spawned when the handle is created.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::entry::{TraceDirection, TraceEntry, TraceSource};
use crate::logger::spawn_logger;

/// Default ring buffer capacity: keep the last 10,000 entries in memory.
pub const DEFAULT_RING_SIZE: usize = 10_000;

/// Maximum length of result strings stored in trace entries.
pub const MAX_RESULT_LEN: usize = 500;

/// A lightweight, cloneable handle for recording debug session traces.
///
/// # Example
///
/// ```rust,no_run
/// use dap_trace::TraceHandle;
///
/// let (trace, _handle) = TraceHandle::new(None, 1000);
///
/// // Record a DAP request
/// trace.trace_request("setBreakpoints", Some(serde_json::json!({
///     "source": {"path": "/src/main.cpp"}
/// })));
///
/// // Later, query recent entries
/// let recent = trace.recent(10);
/// ```
#[derive(Clone)]
pub struct TraceHandle {
    /// Sender side of the log channel. Cloning this is cheap (mpsc sender clone).
    tx: mpsc::UnboundedSender<TraceEntry>,

    /// Thread-safe ring buffer of recent entries.
    ring: Arc<RwLock<VecDeque<TraceEntry>>>,

    /// Session correlation ID (UUID v4).
    session_id: Arc<String>,
}

impl TraceHandle {
    /// Creates a new `TraceHandle` and spawns the background consumer task.
    ///
    /// # Arguments
    /// * `log_dir` — Optional directory for `.jsonl` file output.
    /// * `max_ring_size` — Maximum entries in the in-memory ring buffer.
    ///
    /// # Returns
    /// `(TraceHandle, JoinHandle<()>)` — The handle is cloneable and can be
    /// shared across subsystems. The `JoinHandle` can be awaited for graceful
    /// shutdown when all handles are dropped.
    pub fn new(
        log_dir: Option<PathBuf>,
        max_ring_size: usize,
    ) -> (Self, JoinHandle<()>) {
        let (tx, ring, session_id, handle) = spawn_logger(log_dir, max_ring_size);

        (
            TraceHandle {
                tx,
                ring,
                session_id: Arc::new(session_id),
            },
            handle,
        )
    }

    // ── Core trace method ─────────────────────────────────────────

    /// Record a trace entry. Non-blocking — always returns immediately.
    ///
    /// The entry is sent over an unbounded mpsc channel to the background logger
    /// task, where it is appended to the ring buffer and optionally written to
    /// a JSONL file.
    pub fn trace(&self, entry: TraceEntry) {
        // Unbounded send — never fails unless all receivers are gone (shutdown)
        let _ = self.tx.send(entry);
    }

    // ── Convenience methods ───────────────────────────────────────

    /// Record a DAP request being sent to the debug adapter.
    pub fn trace_request(&self, command: &str, payload: Option<serde_json::Value>) {
        self.trace(TraceEntry {
            timestamp: Utc::now(),
            source: TraceSource::DapRequest,
            direction: TraceDirection::Outbound,
            command: command.to_string(),
            payload,
            result: None,
            duration_us: None,
            session_id: self.session_id().to_string(),
            seq: 0, // seq is assigned by the background task
        });
    }

    /// Record a DAP response received from the debug adapter.
    pub fn trace_response(
        &self,
        command: &str,
        result: Option<String>,
        duration_us: Option<i64>,
    ) {
        let truncated = result.map(|s| truncate(&s, MAX_RESULT_LEN));
        self.trace(TraceEntry {
            timestamp: Utc::now(),
            source: TraceSource::DapResponse,
            direction: TraceDirection::Inbound,
            command: command.to_string(),
            payload: None,
            result: truncated,
            duration_us,
            session_id: self.session_id().to_string(),
            seq: 0,
        });
    }

    /// Record a DAP event received from the debug adapter.
    pub fn trace_event(&self, event: &str, body: Option<serde_json::Value>) {
        self.trace(TraceEntry {
            timestamp: Utc::now(),
            source: TraceSource::DapEvent,
            direction: TraceDirection::Inbound,
            command: event.to_string(),
            payload: body,
            result: None,
            duration_us: None,
            session_id: self.session_id().to_string(),
            seq: 0,
        });
    }

    /// Record an internal lifecycle event.
    pub fn trace_internal(&self, message: &str, payload: Option<serde_json::Value>) {
        self.trace(TraceEntry {
            timestamp: Utc::now(),
            source: TraceSource::Internal,
            direction: TraceDirection::Outbound,
            command: message.to_string(),
            payload,
            result: None,
            duration_us: None,
            session_id: self.session_id().to_string(),
            seq: 0,
        });
    }

    // ── Query ─────────────────────────────────────────────────────

    /// Query recent entries from the in-memory ring buffer.
    ///
    /// Returns entries in reverse chronological order (most recent first),
    /// up to `count` entries.
    pub fn recent(&self, count: usize) -> Vec<TraceEntry> {
        let ring = self.ring.read();
        ring.iter().rev().take(count).cloned().collect()
    }

    /// Returns the session correlation ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Returns the number of entries currently in the ring buffer.
    pub fn len(&self) -> usize {
        self.ring.read().len()
    }

    /// Returns true if the ring buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.ring.read().is_empty()
    }
}

/// Truncate a string to `max_len` characters, appending "…" if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{TraceDirection, TraceSource};

    #[tokio::test]
    async fn test_trace_request() {
        let (handle, _bg) = TraceHandle::new(None, 10);

        handle.trace_request("initialize", Some(serde_json::json!({"adapterID": "test"})));

        tokio::task::yield_now().await;

        let recent = handle.recent(1);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].command, "initialize");
        assert_eq!(recent[0].source, TraceSource::DapRequest);
        assert_eq!(recent[0].direction, TraceDirection::Outbound);
    }

    #[tokio::test]
    async fn test_trace_response() {
        let (handle, _bg) = TraceHandle::new(None, 10);

        handle.trace_response("threads", Some("{\"threads\":[...]}".into()), Some(1500));

        tokio::task::yield_now().await;

        let recent = handle.recent(1);
        assert_eq!(recent[0].source, TraceSource::DapResponse);
        assert_eq!(recent[0].direction, TraceDirection::Inbound);
        assert_eq!(recent[0].duration_us, Some(1500));
    }

    #[tokio::test]
    async fn test_trace_event() {
        let (handle, _bg) = TraceHandle::new(None, 10);

        handle.trace_event("stopped", Some(serde_json::json!({"reason": "breakpoint"})));

        tokio::task::yield_now().await;

        let recent = handle.recent(1);
        assert_eq!(recent[0].source, TraceSource::DapEvent);
        assert_eq!(recent[0].command, "stopped");
    }

    #[tokio::test]
    async fn test_trace_internal() {
        let (handle, _bg) = TraceHandle::new(None, 10);

        handle.trace_internal("codelldb_started", Some(serde_json::json!({"path": "/bin/codelldb"})));

        tokio::task::yield_now().await;

        let recent = handle.recent(1);
        assert_eq!(recent[0].source, TraceSource::Internal);
        assert_eq!(recent[0].command, "codelldb_started");
    }

    #[tokio::test]
    async fn test_clone_handle() {
        let (h1, _bg) = TraceHandle::new(None, 10);
        let h2 = h1.clone();

        h1.trace_request("from_h1", None);
        h2.trace_event("from_h2", None);

        tokio::task::yield_now().await;

        // Both handles share the same ring buffer
        assert_eq!(h1.len(), 2);
        assert_eq!(h2.len(), 2);
        assert_eq!(h1.session_id(), h2.session_id());
    }

    #[tokio::test]
    async fn test_non_blocking_no_log_dir() {
        let (handle, _bg) = TraceHandle::new(None, 100);

        // Should not block even with many rapid calls
        for i in 0..1000 {
            handle.trace_request(&format!("cmd_{}", i), None);
        }

        // The call should return immediately — no assertion needed
    }

    #[test]
    fn test_truncate() {
        let s = "a".repeat(600);
        let t = truncate(&s, 500);
        // "…" is 3 bytes in UTF-8, so total = 500 + 3 = 503 bytes
        assert_eq!(t.len(), 503);
        assert!(t.ends_with('…'));

        let s = "short";
        let t = truncate(s, 500);
        assert_eq!(t, "short");
    }
}
