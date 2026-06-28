use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Identifies which subsystem produced a log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    /// An MCP tool was called by the LLM.
    McpTrigger,
    /// A DAP request was sent to codelldb.
    DapRequest,
    /// A DAP response was received from codelldb.
    DapResponse,
    /// An unsolicited DAP event was received (e.g., "stopped", "output").
    DapEvent,
    /// A Tcl command was sent to OpenOCD.
    OpenOcdTx,
    /// A Tcl response was received from OpenOCD.
    OpenOcdRx,
    /// Internal lifecycle events (state transitions, connections, etc.).
    Internal,
}

/// Direction of data flow for a log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogDirection {
    /// Data flowing into TeleDAP (from hardware or LLM).
    Inbound,
    /// Data flowing out of TeleDAP (to hardware or LLM).
    Outbound,
}

/// A single normalized audit log entry.
///
/// Captures every operation with microsecond-precision timestamps,
/// enabling full reproducibility and debugging of AI-hardware interactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    /// ISO 8601 timestamp with microsecond precision.
    pub timestamp: DateTime<Utc>,

    /// Which subsystem produced this entry.
    pub source: LogSource,

    /// Direction of data flow.
    pub direction: LogDirection,

    /// Human-readable command or event name.
    pub command: String,

    /// Optional structured payload (tool arguments, DAP body, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,

    /// Truncated result summary (max 500 chars in practice).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,

    /// Wall-clock duration in microseconds for request-response pairs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<i64>,

    /// Session correlation ID (persists for entire server lifetime).
    pub session_id: String,

    /// Monotonic sequence number within this session.
    pub seq: u64,
}
