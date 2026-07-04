//! Types for debug session trace entries.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Identifies which subsystem produced a trace entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceSource {
    /// A DAP request was sent to codelldb.
    DapRequest,
    /// A DAP response was received from codelldb.
    DapResponse,
    /// An unsolicited DAP event was received (e.g., "stopped", "output").
    DapEvent,
    /// An MCP tool was called by the AI client (Phase 3).
    McpTrigger,
    /// A Tcl command was sent to OpenOCD (future).
    OpenOcdTx,
    /// A Tcl response was received from OpenOCD (future).
    OpenOcdRx,
    /// Internal lifecycle events (state transitions, connections, etc.).
    Internal,
}

/// Direction of data flow for a trace entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDirection {
    /// Data flowing into TeleDAP (from hardware or AI).
    Inbound,
    /// Data flowing out of TeleDAP (to hardware or AI).
    Outbound,
}

/// A single trace entry recording one debug interaction.
///
/// Captures every operation with microsecond-precision timestamps,
/// enabling full reproducibility and debugging of AI-hardware interactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    /// ISO 8601 timestamp with microsecond precision.
    pub timestamp: DateTime<Utc>,

    /// Which subsystem produced this entry.
    pub source: TraceSource,

    /// Direction of data flow.
    pub direction: TraceDirection,

    /// Human-readable command or event name.
    pub command: String,

    /// Optional structured payload (tool arguments, DAP body, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,

    /// Result summary, truncated to ~500 characters in practice.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_entry_serialization() {
        let entry = TraceEntry {
            timestamp: Utc::now(),
            source: TraceSource::DapRequest,
            direction: TraceDirection::Outbound,
            command: "initialize".into(),
            payload: Some(serde_json::json!({"adapterID": "codelldb"})),
            result: None,
            duration_us: None,
            session_id: "test-session".into(),
            seq: 1,
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("initialize"));
        assert!(json.contains("dap_request"));
        assert!(json.contains("outbound"));

        let back: TraceEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.command, "initialize");
        assert_eq!(back.source, TraceSource::DapRequest);
        assert_eq!(back.direction, TraceDirection::Outbound);
        assert_eq!(back.seq, 1);
    }

    #[test]
    fn test_optional_fields_omitted() {
        let entry = TraceEntry {
            timestamp: Utc::now(),
            source: TraceSource::DapEvent,
            direction: TraceDirection::Inbound,
            command: "initialized".into(),
            payload: None,
            result: None,
            duration_us: None,
            session_id: "s".into(),
            seq: 0,
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("payload"));
        assert!(!json.contains("result"));
        assert!(!json.contains("duration_us"));
    }

    #[test]
    fn test_source_serde_roundtrip() {
        let sources = vec![
            (TraceSource::DapRequest, r#""dap_request""#),
            (TraceSource::DapResponse, r#""dap_response""#),
            (TraceSource::DapEvent, r#""dap_event""#),
            (TraceSource::McpTrigger, r#""mcp_trigger""#),
            (TraceSource::OpenOcdTx, r#""open_ocd_tx""#),
            (TraceSource::OpenOcdRx, r#""open_ocd_rx""#),
            (TraceSource::Internal, r#""internal""#),
        ];
        for (src, expected) in sources {
            let json = serde_json::to_string(&src).unwrap();
            assert_eq!(json, expected);
            let back: TraceSource = serde_json::from_str(&json).unwrap();
            assert_eq!(back, src);
        }
    }

    #[test]
    fn test_direction_serde_roundtrip() {
        let json = serde_json::to_string(&TraceDirection::Inbound).unwrap();
        assert_eq!(json, r#""inbound""#);
        let back: TraceDirection = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TraceDirection::Inbound);

        let json = serde_json::to_string(&TraceDirection::Outbound).unwrap();
        assert_eq!(json, r#""outbound""#);
        let back: TraceDirection = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TraceDirection::Outbound);
    }
}
