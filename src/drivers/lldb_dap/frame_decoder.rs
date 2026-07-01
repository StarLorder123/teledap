use bytes::{Buf, BytesMut};
use serde_json::Value;
use tokio_util::codec::{Decoder, Encoder};

/// A single DAP protocol message, decoded from the wire.
#[derive(Debug, Clone)]
pub struct DapMessage {
    /// Monotonic sequence number from the JSON body (adapter-assigned).
    pub seq: u64,
    /// For responses: the `request_seq` that matches the original request's `seq`.
    /// Not present for requests or events.
    pub request_seq: Option<u64>,
    /// "request", "response", or "event"
    pub msg_type: String,
    /// Command name for requests/responses.
    pub command: Option<String>,
    /// Event name for events.
    pub event: Option<String>,
    /// The full JSON body.
    pub body: Value,
}

/// Tokio codec for the DAP (Debug Adapter Protocol) wire format.
///
/// DAP uses LSP-style framing: `Content-Length: <N>\r\n\r\n<JSON body>`.
/// This codec handles:
/// - Partial headers (returns `Ok(None)`, waiting for more data)
/// - Incomplete bodies (returns `Ok(None)`, waiting for more bytes)
/// - Multiple complete messages in a single buffer
/// - Oversized frames (returns `Err` with InvalidData)
pub struct DapCodec {
    /// Maximum allowed Content-Length value. Frames exceeding this are
    /// rejected to prevent memory exhaustion attacks.
    max_frame_size: usize,
}

impl DapCodec {
    /// Create a new DapCodec with the given maximum frame size in bytes.
    pub fn new(max_frame_size: usize) -> Self {
        Self { max_frame_size }
    }
}

impl Decoder for DapCodec {
    type Item = DapMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // 1. Find the header terminator: "\r\n\r\n"
        let header_end = match find_subsequence(src, b"\r\n\r\n") {
            Some(pos) => pos,
            None => {
                // Safety check: if no header terminator but buffer is growing
                // beyond a reasonable header size, bail out
                if src.len() > 4096 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "DAP header exceeds 4 KiB without terminator",
                    ));
                }
                return Ok(None);
            }
        };

        // 2. Parse the header string
        let header_str =
            std::str::from_utf8(&src[..header_end]).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e)
            })?;

        // 3. Extract Content-Length value
        let content_length = header_str
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("Content-Length:")
                    .map(|v| v.trim().parse::<usize>().ok())
                    .flatten()
            })
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Missing Content-Length header in: {:?}",
                        header_str
                    ),
                )
            })?;

        if content_length > self.max_frame_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Content-Length {} exceeds max allowed {}",
                    content_length, self.max_frame_size
                ),
            ));
        }

        // 4. Check if the full body is available
        let body_start = header_end + 4; // +4 for "\r\n\r\n"
        let total_needed = body_start + content_length;

        if src.len() < total_needed {
            // Body not complete yet — reserve space and wait
            src.reserve(total_needed - src.len());
            return Ok(None);
        }

        // 5. Extract and parse the JSON body
        let body_bytes = &src[body_start..total_needed];
        let body: Value = serde_json::from_slice(body_bytes).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;

        // 6. Advance the buffer past the consumed frame
        src.advance(total_needed);

        // 7. Extract message type metadata from JSON
        let msg_type = body
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let command = body
            .get("command")
            .and_then(|v| v.as_str())
            .map(String::from);

        let event = body
            .get("event")
            .and_then(|v| v.as_str())
            .map(String::from);

        let seq = body
            .get("seq")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let request_seq = body
            .get("request_seq")
            .and_then(|v| v.as_u64());

        tracing::trace!(
            "DAP decode: type={}, seq={}, req_seq={:?}, body_len={}",
            msg_type,
            seq,
            request_seq,
            content_length
        );

        Ok(Some(DapMessage {
            seq,
            request_seq,
            msg_type,
            command,
            event,
            body,
        }))
    }
}

/// Encode a DapMessage as `Content-Length: <N>\r\n\r\n<JSON>`.
impl Encoder<DapMessage> for DapCodec {
    type Error = std::io::Error;

    fn encode(
        &mut self,
        item: DapMessage,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        encode_json_value(&item.body, dst)
    }
}

/// Convenience: encode a raw `serde_json::Value` directly.
impl Encoder<Value> for DapCodec {
    type Error = std::io::Error;

    fn encode(
        &mut self,
        item: Value,
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        encode_json_value(&item, dst)
    }
}

/// Shared encoding logic: JSON → bytes, then prepend Content-Length header.
fn encode_json_value(
    value: &Value,
    dst: &mut BytesMut,
) -> Result<(), std::io::Error> {
    let body =
        serde_json::to_vec(value).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    dst.reserve(header.len() + body.len());
    dst.extend_from_slice(header.as_bytes());
    dst.extend_from_slice(&body);
    Ok(())
}

/// Find a byte subsequence in a slice. Returns the starting index.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_codec() -> DapCodec {
        DapCodec::new(4 * 1024 * 1024)
    }

    /// Helper: create a properly-framed DAP wire message.
    fn make_wire(body: Value) -> BytesMut {
        let body_bytes = serde_json::to_vec(&body).unwrap();
        let header = format!(
            "Content-Length: {}\r\n\r\n",
            body_bytes.len()
        );
        let mut buf = BytesMut::new();
        buf.extend_from_slice(header.as_bytes());
        buf.extend_from_slice(&body_bytes);
        buf
    }

    #[test]
    fn test_decode_single_request() {
        let body = json!({"type":"request","seq":1,"command":"initialize","arguments":{}});
        let mut buf = make_wire(body);
        let mut codec = make_codec();

        let msg = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(msg.msg_type, "request");
        assert_eq!(msg.seq, 1);
        assert_eq!(msg.command.as_deref(), Some("initialize"));
        assert!(buf.is_empty());
    }

    #[test]
    fn test_decode_response() {
        let body = json!({"type":"response","seq":5,"success":true});
        let mut buf = make_wire(body);
        let mut codec = make_codec();

        let msg = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(msg.msg_type, "response");
        assert_eq!(msg.seq, 5);
        assert_eq!(msg.body["success"], true);
    }

    #[test]
    fn test_decode_response_with_request_seq() {
        // DAP responses carry request_seq to match the original request.
        // This test ensures we extract it correctly — matching on seq alone
        // would fail when the adapter sends events between responses.
        let body = json!({"type":"response","seq":3,"request_seq":2,"command":"launch","success":true});
        let mut buf = make_wire(body);
        let mut codec = make_codec();

        let msg = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(msg.msg_type, "response");
        assert_eq!(msg.seq, 3);
        assert_eq!(msg.request_seq, Some(2));
        assert_eq!(msg.command.as_deref(), Some("launch"));
    }

    #[test]
    fn test_decode_event() {
        let body = json!({"type":"event","seq":0,"event":"stopped","body":{"reason":"breakpoint"}});
        let mut buf = make_wire(body);
        let mut codec = make_codec();

        let msg = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(msg.msg_type, "event");
        assert_eq!(msg.event.as_deref(), Some("stopped"));
        assert_eq!(msg.body["body"]["reason"], "breakpoint");
    }

    #[test]
    fn test_partial_header_returns_none() {
        let wire = b"Content-Leng";
        let mut buf = BytesMut::from(&wire[..]);
        let mut codec = make_codec();

        assert!(codec.decode(&mut buf).unwrap().is_none());
        // Buffer should be preserved for more data
        assert!(!buf.is_empty());
    }

    #[test]
    fn test_partial_body_returns_none() {
        // Content-Length says 100 but only 20 bytes of body
        let wire = b"Content-Length: 100\r\n\r\n{\"type\":\"req\"}";
        let mut buf = BytesMut::from(&wire[..]);
        let mut codec = make_codec();

        assert!(codec.decode(&mut buf).unwrap().is_none());
        // Buffer preserved
        assert!(!buf.is_empty());
    }

    #[test]
    fn test_multiple_frames_in_one_buffer() {
        let body1 = json!({"type":"req"});
        let body2 = json!({"type":"resp"});
        let mut combined = make_wire(body1);
        let frame2 = make_wire(body2);
        combined.extend_from_slice(&frame2);

        let mut codec = make_codec();

        let msg1 = codec.decode(&mut combined).unwrap().unwrap();
        assert_eq!(msg1.msg_type, "req");

        let msg2 = codec.decode(&mut combined).unwrap().unwrap();
        assert_eq!(msg2.msg_type, "resp");

        assert!(combined.is_empty());
    }

    #[test]
    fn test_oversized_frame_rejected() {
        // Create a codec with a tiny max frame size
        let mut codec = DapCodec::new(10);
        let wire = b"Content-Length: 100\r\n\r\n{\"type\":\"req\"}";
        let mut buf = BytesMut::from(&wire[..]);

        let result = codec.decode(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_content_length_header() {
        let wire = b"X-Other: 42\r\n\r\n{\"type\":\"req\"}";
        let mut buf = BytesMut::from(&wire[..]);
        let mut codec = make_codec();

        let result = codec.decode(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_value() {
        let mut codec = make_codec();
        let value = serde_json::json!({"type": "request", "seq": 1, "command": "continue"});
        let mut dst = BytesMut::new();

        codec.encode(value, &mut dst).unwrap();

        let output = String::from_utf8_lossy(&dst);
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains("\r\n\r\n"));
        assert!(output.contains("\"command\":\"continue\""));
    }

    #[test]
    fn test_decode_roundtrip() {
        let original = serde_json::json!({
            "type": "request",
            "seq": 42,
            "command": "evaluate",
            "arguments": {"expression": "x + 1"}
        });

        let mut codec = make_codec();
        let mut encoded = BytesMut::new();
        codec.encode(original.clone(), &mut encoded).unwrap();

        let decoded = codec.decode(&mut encoded).unwrap().unwrap();
        assert_eq!(decoded.seq, 42);
        assert_eq!(decoded.command.as_deref(), Some("evaluate"));
        assert_eq!(
            decoded.body["arguments"]["expression"],
            "x + 1"
        );
    }
}
