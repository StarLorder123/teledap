//! DAP wire-format codec implementing tokio's `Decoder` and `Encoder` traits.
//!
//! DAP uses LSP-style framing over stdio: `Content-Length: <N>\r\n\r\n<JSON body>`.
//! This codec handles partial reads, multiple frames in one buffer, and oversized
//! frame rejection.

use bytes::{Buf, BytesMut};
use dap_types::base::ProtocolMessage;
use serde::Serialize;
use tokio_util::codec::{Decoder, Encoder};

/// A tokio codec for the DAP wire format.
///
/// # Example (with FramedRead)
///
/// ```rust
/// use dap_codec::DapCodec;
/// use tokio_util::codec::FramedRead;
/// use bytes::BytesMut;
///
/// let codec = DapCodec::new(4 * 1024 * 1024);
/// // let framed = FramedRead::new(stdio, codec);
/// ```
pub struct DapCodec {
    /// Maximum allowed Content-Length value. Frames exceeding this are
    /// rejected with an `InvalidData` error to prevent memory exhaustion.
    max_frame_size: usize,
}

impl DapCodec {
    /// Create a new DapCodec with the given maximum frame size in bytes.
    ///
    /// The default recommended value is 4 MiB for debug adapters.
    pub fn new(max_frame_size: usize) -> Self {
        Self { max_frame_size }
    }

    /// Returns the maximum frame size configured for this codec.
    pub fn max_frame_size(&self) -> usize {
        self.max_frame_size
    }
}

impl Default for DapCodec {
    fn default() -> Self {
        Self::new(4 * 1024 * 1024) // 4 MiB
    }
}

impl Decoder for DapCodec {
    type Item = ProtocolMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // 1. Find the header terminator: "\r\n\r\n"
        let header_end = match find_subsequence(src, b"\r\n\r\n") {
            Some(pos) => pos,
            None => {
                // Safety: if no header terminator but buffer is growing
                // beyond a reasonable header size, bail out.
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
        let header_str = std::str::from_utf8(&src[..header_end])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // 3. Extract Content-Length value
        let content_length = header_str
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("Content-Length:")
                    .and_then(|v| v.trim().parse::<usize>().ok())
            })
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Missing Content-Length header in: {:?}", header_str),
                )
            })?;

        // 4. Check frame size limit
        if content_length > self.max_frame_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Content-Length {} exceeds max allowed {}",
                    content_length, self.max_frame_size
                ),
            ));
        }

        // 5. Check if the full body is available
        let body_start = header_end + 4; // +4 for "\r\n\r\n"
        let total_needed = body_start + content_length;

        if src.len() < total_needed {
            // Body not complete yet — reserve space and wait for more data
            src.reserve(total_needed - src.len());
            return Ok(None);
        }

        // 6. Extract and parse the JSON body
        let body_bytes = &src[body_start..total_needed];
        let message: ProtocolMessage = serde_json::from_slice(body_bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // 7. Advance the buffer past the consumed frame
        src.advance(total_needed);

        tracing::trace!(
            seq = message.seq(),
            msg_type = ?if message.is_request() { "request" } else if message.is_response() { "response" } else { "event" },
            body_len = content_length,
            "DAP frame decoded"
        );

        Ok(Some(message))
    }
}

impl Encoder<ProtocolMessage> for DapCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: ProtocolMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        encode_json(&item, dst)
    }
}

/// Convenience: encode a raw `serde_json::Value` directly as a DAP frame.
impl Encoder<serde_json::Value> for DapCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: serde_json::Value, dst: &mut BytesMut) -> Result<(), Self::Error> {
        encode_json(&item, dst)
    }
}

/// Shared encoding logic: serialize to JSON, prepend Content-Length header.
fn encode_json<T: Serialize + ?Sized>(value: &T, dst: &mut BytesMut) -> Result<(), std::io::Error> {
    let body = serde_json::to_vec(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    dst.reserve(header.len() + body.len());
    dst.extend_from_slice(header.as_bytes());
    dst.extend_from_slice(&body);
    Ok(())
}

/// Encode a DAP message into a new `BytesMut`.
pub fn encode_to_bytes(message: &ProtocolMessage) -> Result<BytesMut, std::io::Error> {
    let mut buf = BytesMut::new();
    let mut codec = DapCodec::default();
    codec.encode(message.clone(), &mut buf)?;
    Ok(buf)
}

/// Find a byte subsequence in a slice. Returns the starting index.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use dap_types::base::{Event, Request};

    fn make_codec() -> DapCodec {
        DapCodec::new(4 * 1024 * 1024)
    }

    /// Build a properly-framed DAP wire message from JSON bytes.
    fn make_wire(json: &[u8]) -> BytesMut {
        let header = format!("Content-Length: {}\r\n\r\n", json.len());
        let mut buf = BytesMut::new();
        buf.extend_from_slice(header.as_bytes());
        buf.extend_from_slice(json);
        buf
    }

    /// Build a wire message from a serde_json::Value.
    fn make_wire_value(body: serde_json::Value) -> BytesMut {
        let body_bytes = serde_json::to_vec(&body).unwrap();
        make_wire(&body_bytes)
    }

    // ── Decoding ────────────────────────────────────────────────

    #[test]
    fn test_decode_single_request() {
        let body =
            serde_json::json!({"type":"request","seq":1,"command":"initialize","arguments":{}});
        let mut buf = make_wire_value(body);
        let mut codec = make_codec();

        let msg = codec.decode(&mut buf).unwrap().unwrap();
        assert!(msg.is_request());
        assert_eq!(msg.seq(), 1);
        assert_eq!(msg.command(), Some("initialize"));
        assert!(buf.is_empty());
    }

    #[test]
    fn test_decode_response() {
        let body = serde_json::json!({"type":"response","seq":5,"request_seq":1,"success":true,"command":"threads"});
        let mut buf = make_wire_value(body);
        let mut codec = make_codec();

        let msg = codec.decode(&mut buf).unwrap().unwrap();
        assert!(msg.is_response());
        assert_eq!(msg.seq(), 5);
        if let ProtocolMessage::Response(r) = &msg {
            assert!(r.success);
            assert_eq!(r.request_seq, 1);
        }
    }

    #[test]
    fn test_decode_event() {
        let body = serde_json::json!({"type":"event","seq":0,"event":"stopped","body":{"reason":"breakpoint","threadId":1}});
        let mut buf = make_wire_value(body);
        let mut codec = make_codec();

        let msg = codec.decode(&mut buf).unwrap().unwrap();
        assert!(msg.is_event());
        assert_eq!(msg.seq(), 0);
    }

    #[test]
    fn test_partial_header_returns_none() {
        let wire = b"Content-Leng";
        let mut buf = BytesMut::from(&wire[..]);
        let mut codec = make_codec();

        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert!(!buf.is_empty()); // Buffer preserved
    }

    #[test]
    fn test_partial_body_returns_none() {
        // Content-Length says 100 but only ~20 bytes of body
        let wire = b"Content-Length: 100\r\n\r\n{\"type\":\"request\"";
        let mut buf = BytesMut::from(&wire[..]);
        let mut codec = make_codec();

        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert!(!buf.is_empty()); // Buffer preserved
    }

    #[test]
    fn test_multiple_frames_in_one_buffer() {
        let body1 = serde_json::json!({"type":"request","seq":1,"command":"threads"});
        let body2 = serde_json::json!({"type":"response","seq":2,"request_seq":1,"success":true,"command":"threads"});

        let mut combined = make_wire_value(body1);
        let frame2 = make_wire_value(body2);
        combined.extend_from_slice(&frame2);

        let mut codec = make_codec();

        let msg1 = codec.decode(&mut combined).unwrap().unwrap();
        assert!(msg1.is_request());

        let msg2 = codec.decode(&mut combined).unwrap().unwrap();
        assert!(msg2.is_response());

        assert!(combined.is_empty());
    }

    #[test]
    fn test_oversized_frame_rejected() {
        let mut codec = DapCodec::new(10);
        let wire = b"Content-Length: 100\r\n\r\n{\"type\":\"request\"}";
        let mut buf = BytesMut::from(&wire[..]);

        let result = codec.decode(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_content_length_header() {
        let wire = b"X-Other: 42\r\n\r\n{\"type\":\"request\"}";
        let mut buf = BytesMut::from(&wire[..]);
        let mut codec = make_codec();

        let result = codec.decode(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_header_exceeds_4kib_rejected() {
        let mut buf = BytesMut::new();
        // Write 5000 bytes of junk without \r\n\r\n
        buf.extend_from_slice(&[b'A'; 5000]);
        let mut codec = make_codec();

        let result = codec.decode(&mut buf);
        assert!(result.is_err());
    }

    // ── Encoding ────────────────────────────────────────────────

    #[test]
    fn test_encode_request() {
        let msg = ProtocolMessage::Request(Request {
            seq: 1,
            command: "continue".into(),
            arguments: Some(serde_json::json!({"threadId": 1})),
        });

        let mut codec = make_codec();
        let mut dst = BytesMut::new();
        codec.encode(msg, &mut dst).unwrap();

        let output = std::str::from_utf8(&dst).unwrap();
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains("\r\n\r\n"));
        assert!(output.contains("\"command\":\"continue\""));
        assert!(output.contains("\"threadId\":1"));
    }

    #[test]
    fn test_encode_value() {
        let mut codec = make_codec();
        let value = serde_json::json!({"type":"event","seq":0,"event":"initialized"});
        let mut dst = BytesMut::new();
        codec.encode(value, &mut dst).unwrap();

        let output = std::str::from_utf8(&dst).unwrap();
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains("\"event\":\"initialized\""));
    }

    #[test]
    fn test_decode_roundtrip() {
        let original = ProtocolMessage::Request(Request {
            seq: 42,
            command: "evaluate".into(),
            arguments: Some(serde_json::json!({"expression": "x + 1", "frameId": 0})),
        });

        let mut codec = make_codec();
        let mut encoded = BytesMut::new();
        codec.encode(original.clone(), &mut encoded).unwrap();

        let decoded = codec.decode(&mut encoded).unwrap().unwrap();
        assert_eq!(decoded.seq(), 42);
        assert!(decoded.is_request());
        assert_eq!(decoded.command(), Some("evaluate"));
    }

    #[test]
    fn test_encode_to_bytes_helper() {
        let msg = ProtocolMessage::Event(Event {
            seq: 0,
            event: "initialized".into(),
            body: None,
        });

        let buf = encode_to_bytes(&msg).unwrap();
        let output = std::str::from_utf8(&buf).unwrap();
        assert!(output.starts_with("Content-Length: "));
        assert!(output.contains("\"event\":\"initialized\""));
    }

    #[test]
    fn test_empty_body_event() {
        let body = serde_json::json!({"type":"event","seq":0,"event":"initialized"});
        let mut buf = make_wire_value(body);
        let mut codec = make_codec();

        let msg = codec.decode(&mut buf).unwrap().unwrap();
        assert!(msg.is_event());
    }
}
