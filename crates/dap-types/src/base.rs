//! Base protocol types: ProtocolMessage, Request, Response, Event, ErrorResponse, and Cancel.
//!
//! These types correspond to §Base Protocol in the DAP specification.

use serde::{Deserialize, Serialize};

use crate::types::Message;

// ── ProtocolMessage ────────────────────────────────────────────────

/// The top-level protocol message, tagged by `type` field.
///
/// On the wire this is always inside a `Content-Length: N\r\n\r\n{json}` frame,
/// but the framing is handled by the codec layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProtocolMessage {
    #[serde(rename = "request")]
    Request(Request),
    #[serde(rename = "response")]
    Response(Response),
    #[serde(rename = "event")]
    Event(Event),
}

impl ProtocolMessage {
    /// Returns the `seq` number regardless of variant.
    pub fn seq(&self) -> u64 {
        match self {
            ProtocolMessage::Request(r) => r.seq,
            ProtocolMessage::Response(r) => r.seq,
            ProtocolMessage::Event(e) => e.seq,
        }
    }

    /// Returns `true` if this message is a request.
    pub fn is_request(&self) -> bool {
        matches!(self, ProtocolMessage::Request(_))
    }

    /// Returns `true` if this message is a response.
    pub fn is_response(&self) -> bool {
        matches!(self, ProtocolMessage::Response(_))
    }

    /// Returns `true` if this message is an event.
    pub fn is_event(&self) -> bool {
        matches!(self, ProtocolMessage::Event(_))
    }

    /// Returns the command name for requests and responses, or `None` for events.
    pub fn command(&self) -> Option<&str> {
        match self {
            ProtocolMessage::Request(r) => Some(&r.command),
            ProtocolMessage::Response(r) => Some(&r.command),
            ProtocolMessage::Event(_) => None,
        }
    }
}

// ── Request ────────────────────────────────────────────────────────

/// A client-to-adapter or adapter-to-client request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Sequence number of the message.
    pub seq: u64,
    /// The command to execute.
    pub command: String,
    /// Object containing arguments for the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

impl Request {
    /// Create a new request with the given sequence number and command.
    pub fn new(seq: u64, command: impl Into<String>) -> Self {
        Request {
            seq,
            command: command.into(),
            arguments: None,
        }
    }

    /// Set the arguments for this request.
    pub fn with_arguments(mut self, args: impl Serialize) -> Result<Self, serde_json::Error> {
        self.arguments = Some(serde_json::to_value(args)?);
        Ok(self)
    }
}

// ── Response ───────────────────────────────────────────────────────

/// A response to a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Sequence number of the message.
    pub seq: u64,
    /// Sequence number of the corresponding request.
    pub request_seq: u64,
    /// Outcome of the request.
    pub success: bool,
    /// The command that was requested.
    pub command: String,
    /// Raw error in short form if `success` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Contains request result if success is true, or error details if false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

impl Response {
    /// Create a successful response.
    pub fn success(seq: u64, request_seq: u64, command: impl Into<String>) -> Self {
        Response {
            seq,
            request_seq,
            success: true,
            command: command.into(),
            message: None,
            body: None,
        }
    }

    /// Create an error response.
    pub fn error(
        seq: u64,
        request_seq: u64,
        command: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Response {
            seq,
            request_seq,
            success: false,
            command: command.into(),
            message: Some(message.into()),
            body: None,
        }
    }

    /// Set the body for this response.
    pub fn with_body(mut self, body: impl Serialize) -> Result<Self, serde_json::Error> {
        self.body = Some(serde_json::to_value(body)?);
        Ok(self)
    }
}

// ── Event ──────────────────────────────────────────────────────────

/// A debug adapter initiated event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// Sequence number of the message.
    pub seq: u64,
    /// Type of event.
    pub event: String,
    /// Event-specific information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

impl Event {
    /// Create a new event with the given sequence number and event type.
    pub fn new(seq: u64, event: impl Into<String>) -> Self {
        Event {
            seq,
            event: event.into(),
            body: None,
        }
    }

    /// Set the body for this event.
    pub fn with_body(mut self, body: impl Serialize) -> Result<Self, serde_json::Error> {
        self.body = Some(serde_json::to_value(body)?);
        Ok(self)
    }
}

// ── ErrorResponse ──────────────────────────────────────────────────

/// Body of a response when `success` is false.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponseBody {
    /// A structured error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Message>,
}

// ── Cancel ─────────────────────────────────────────────────────────

/// Arguments for the `cancel` request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelArguments {
    /// The ID (attribute `seq`) of the request to cancel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<u64>,
    /// The ID (attribute `progressId`) of the progress to cancel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_request() {
        let req = Request::new(1, "initialize")
            .with_arguments(serde_json::json!({"adapterID": "test"}))
            .unwrap();
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["seq"], 1);
        assert_eq!(json["command"], "initialize");
        assert_eq!(json["arguments"]["adapterID"], "test");
    }

    #[test]
    fn test_deserialize_protocol_message_request() {
        let json = r#"{"type":"request","seq":1,"command":"threads"}"#;
        let msg: ProtocolMessage = serde_json::from_str(json).unwrap();
        assert!(msg.is_request());
        assert_eq!(msg.seq(), 1);
        assert_eq!(msg.command().unwrap(), "threads");
    }

    #[test]
    fn test_deserialize_protocol_message_response() {
        let json = r#"{"type":"response","seq":2,"request_seq":1,"success":true,"command":"threads","body":{"threads":[{"id":1,"name":"main"}]}}"#;
        let msg: ProtocolMessage = serde_json::from_str(json).unwrap();
        assert!(msg.is_response());
        assert_eq!(msg.seq(), 2);
    }

    #[test]
    fn test_deserialize_protocol_message_event() {
        let json = r#"{"type":"event","seq":0,"event":"stopped","body":{"reason":"breakpoint","threadId":1}}"#;
        let msg: ProtocolMessage = serde_json::from_str(json).unwrap();
        assert!(msg.is_event());
        assert_eq!(msg.seq(), 0);
    }

    #[test]
    fn test_serialize_protocol_message() {
        let msg = ProtocolMessage::Request(Request::new(1, "threads"));
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"request""#));
        assert!(json.contains(r#""seq":1"#));
        assert!(json.contains(r#""command":"threads""#));
    }

    #[test]
    fn test_response_success_helpers() {
        let resp = Response::success(2, 1, "continue");
        assert!(resp.success);
        assert_eq!(resp.request_seq, 1);
        assert_eq!(resp.command, "continue");
        assert!(resp.message.is_none());
    }

    #[test]
    fn test_response_error_helpers() {
        let resp = Response::error(2, 1, "evaluate", "notStopped");
        assert!(!resp.success);
        assert_eq!(resp.message.as_deref(), Some("notStopped"));
    }

    #[test]
    fn test_cancel_arguments() {
        let args = CancelArguments {
            request_id: Some(5),
            progress_id: None,
        };
        let json = serde_json::to_string(&args).unwrap();
        assert!(json.contains("requestId"));
        assert!(json.contains("5"));
    }

    #[test]
    fn test_event_with_body() {
        let event = Event::new(0, "stopped")
            .with_body(serde_json::json!({"reason": "breakpoint"}))
            .unwrap();
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "stopped");
        assert_eq!(json["body"]["reason"], "breakpoint");
    }
}
