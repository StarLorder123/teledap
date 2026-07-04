//! Non-blocking debug session trace recorder.
//!
//! This crate provides a trace layer that is **separate from application logging**:
//!
//! | Concern | `tracing` (app logs) | `dap-trace` (debug trace) |
//! |---------|---------------------|--------------------------|
//! | Audience | Developers / ops | Session analysis / AI replay |
//! | Content | Server health, errors, perf | Every DAP request, response, event |
//! | Lifetime | Process-level | Debug session (multiple per process) |
//! | Output | Console / log files | JSONL files + in-memory ring buffer |
//!
//! # Architecture
//!
//! ```text
//! DapClient ──► TraceHandle::trace_request() ──► mpsc channel ──► Logger (bg task)
//!                  .trace_response()                                  │
//!                  .trace_event()                                     ├─ Ring buffer
//!                                                                     └─ JSONL file
//! ```
//!
//! All `trace_*()` calls are synchronous and non-blocking — entries are pushed
//! to an unbounded channel and processed by a background tokio task.
//!
//! # Example
//!
//! ```rust,no_run
//! use dap_trace::TraceHandle;
//!
//! let (trace, _bg_handle) = TraceHandle::new(
//!     Some("./traces".into()),
//!     10_000,  // ring buffer capacity
//! );
//!
//! // Record throughout the debug session
//! trace.trace_request("launch", Some(serde_json::json!({"program": "/app"})));
//! trace.trace_event("stopped", Some(serde_json::json!({"reason": "breakpoint"})));
//!
//! // Query recent entries
//! for entry in trace.recent(10) {
//!     println!("[{}] {} {:?}", entry.seq, entry.command, entry.source);
//! }
//! ```

pub mod entry;
pub mod handle;
mod logger;

pub use entry::{TraceDirection, TraceEntry, TraceSource};
pub use handle::{TraceHandle, DEFAULT_RING_SIZE, MAX_RESULT_LEN};
