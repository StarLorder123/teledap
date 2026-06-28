//! OpenOCD Tcl RPC driver.
//!
//! Manages a TCP connection to OpenOCD's Tcl RPC server (default port 6666)
//! and provides methods for hardware control: reset, flash, registers, memory.
//!
//! # Protocol
//!
//! The OpenOCD Tcl RPC protocol is a simple text-based protocol:
//! - Commands are Tcl strings terminated by `\x1a` (Ctrl+Z / SUB character)
//! - Responses are Tcl strings terminated by `\x1a`
//! - The TCP connection is persistent (stateful)

use crate::audit_tracker::{AuditLogger, LogDirection, LogSource};
use crate::error::DriverError;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

/// The OpenOCD Tcl RPC protocol terminator byte (Ctrl+Z / SUB).
const TCL_TERMINATOR: u8 = 0x1a;

/// Manages a TCP connection to OpenOCD's Tcl RPC server.
///
/// All commands are sent as Tcl strings terminated by `\x1a`.
/// Responses are read until `\x1a` is received.
pub struct OpenOcdDriver {
    /// TCP stream to OpenOCD Tcl RPC port.
    stream: Mutex<Option<TcpStream>>,

    /// Audit logger for command tracing.
    audit: Arc<AuditLogger>,

    /// Hostname of the connected OpenOCD server.
    host: Mutex<String>,

    /// Port of the connected OpenOCD server.
    port: Mutex<u16>,
}

impl OpenOcdDriver {
    /// Creates a new `OpenOcdDriver` with no active connection.
    pub fn new(audit: Arc<AuditLogger>) -> Self {
        Self {
            stream: Mutex::new(None),
            audit,
            host: Mutex::new(String::new()),
            port: Mutex::new(0),
        }
    }

    /// Returns true if connected to an OpenOCD Tcl RPC server.
    pub async fn is_connected(&self) -> bool {
        self.stream.lock().await.is_some()
    }

    // ── Connection Lifecycle ──────────────────────────────────────

    /// Connect to an OpenOCD Tcl RPC server.
    ///
    /// Enables TCP_NODELAY for minimum latency on hardware commands.
    pub async fn connect(
        &self,
        host: &str,
        port: u16,
    ) -> Result<(), DriverError> {
        if self.is_connected().await {
            return Err(DriverError::TcpConnect(format!(
                "Already connected to {}:{}",
                self.host.lock().await,
                self.port.lock().await
            )));
        }

        let stream = TcpStream::connect(format!("{}:{}", host, port))
            .await
            .map_err(|e| {
                DriverError::TcpConnect(format!(
                    "Failed to connect to {}:{}: {}",
                    host, port, e
                ))
            })?;

        // Disable Nagle's algorithm for minimum latency
        stream
            .set_nodelay(true)
            .map_err(|e| DriverError::TcpConnect(e.to_string()))?;

        *self.host.lock().await = host.to_string();
        *self.port.lock().await = port;
        *self.stream.lock().await = Some(stream);

        self.audit.log(
            LogSource::Internal,
            LogDirection::Outbound,
            "openocd_connected",
            Some(serde_json::json!({"host": host, "port": port})),
            None,
            None,
        );

        tracing::info!("OpenOCD connected: {}:{}", host, port);
        Ok(())
    }

    /// Close the TCP connection to OpenOCD.
    pub async fn disconnect(&self) -> Result<(), DriverError> {
        let mut stream_guard = self.stream.lock().await;
        if let Some(stream) = stream_guard.take() {
            drop(stream); // TCP connection closed on drop
            self.audit.log(
                LogSource::Internal,
                LogDirection::Outbound,
                "openocd_disconnected",
                None,
                None,
                None,
            );
            tracing::info!("OpenOCD disconnected.");
        }
        Ok(())
    }

    // ── Low-Level Command ─────────────────────────────────────────

    /// Send a raw Tcl command and wait for the response.
    ///
    /// Both the command and response are terminated by `\x1a`.
    /// This is the low-level primitive used by all high-level methods.
    pub async fn send_command(
        &self,
        cmd: &str,
    ) -> Result<String, DriverError> {
        let start = Instant::now();

        // Build wire format: <cmd>\x1a
        let mut wire = cmd.as_bytes().to_vec();
        wire.push(TCL_TERMINATOR);

        // Send (lock held only for the send)
        {
            let mut stream_guard = self.stream.lock().await;
            let stream = stream_guard
                .as_mut()
                .ok_or(DriverError::NotConnected(
                    "OpenOCD not connected".into(),
                ))?;
            stream.write_all(&wire).await?;
            stream.flush().await?;
        }

        self.audit.log(
            LogSource::OpenOcdTx,
            LogDirection::Outbound,
            cmd,
            None,
            None,
            None,
        );

        // Receive: read until \x1a
        let response = {
            let mut buffer = Vec::with_capacity(8192);
            let mut stream_guard = self.stream.lock().await;
            let stream = stream_guard
                .as_mut()
                .ok_or(DriverError::TcpDisconnected)?;

            let mut read_buf = [0u8; 1024];
            loop {
                let n = stream.read(&mut read_buf).await?;
                if n == 0 {
                    return Err(DriverError::TcpDisconnected);
                }
                buffer.extend_from_slice(&read_buf[..n]);

                // Check for terminator
                if buffer.contains(&TCL_TERMINATOR) {
                    break;
                }
            }

            // Strip trailing \x1a and any trailing whitespace
            let term_pos = buffer
                .iter()
                .position(|&b| b == TCL_TERMINATOR)
                .unwrap();
            let response_bytes = &buffer[..term_pos];
            String::from_utf8_lossy(response_bytes)
                .trim()
                .to_string()
        };

        let duration_us = start.elapsed().as_micros() as i64;

        self.audit.log(
            LogSource::OpenOcdRx,
            LogDirection::Inbound,
            cmd,
            None,
            Some(response.clone()),
            Some(duration_us),
        );

        tracing::trace!(
            "OpenOCD cmd: '{}' -> '{}' ({}µs)",
            cmd,
            &response[..response.len().min(100)],
            duration_us
        );

        Ok(response)
    }

    // ── High-Level Hardware Operations ────────────────────────────

    /// Reset the target and halt at the reset vector.
    pub async fn reset_halt(&self) -> Result<(), DriverError> {
        let resp = self.send_command("reset halt").await?;
        tracing::info!("reset halt: {}", resp);
        Ok(())
    }

    /// Reset and run the target (no halt).
    pub async fn reset_run(&self) -> Result<(), DriverError> {
        let resp = self.send_command("reset run").await?;
        tracing::info!("reset run: {}", resp);
        Ok(())
    }

    /// Halt the target immediately.
    pub async fn halt(&self) -> Result<(), DriverError> {
        let resp = self.send_command("halt").await?;
        tracing::info!("halt: {}", resp);
        Ok(())
    }

    /// Resume (continue) execution.
    pub async fn resume(&self) -> Result<(), DriverError> {
        let resp = self.send_command("resume").await?;
        tracing::info!("resume: {}", resp);
        Ok(())
    }

    /// Erase a flash memory region.
    pub async fn flash_erase(
        &self,
        address: u32,
        length: u32,
    ) -> Result<(), DriverError> {
        let cmd = format!("flash erase_address 0x{:x} {}", address, length);
        let resp = self.send_command(&cmd).await?;
        tracing::info!("flash erase 0x{:x}+{}: {}", address, length, resp);
        Ok(())
    }

    /// Write binary data to flash memory.
    ///
    /// `data` is hex-encoded before sending over the Tcl wire.
    pub async fn flash_write(
        &self,
        address: u32,
        data: &[u8],
    ) -> Result<(), DriverError> {
        let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
        let cmd = format!(
            "flash write_image erase {{{}}} 0x{:x}",
            hex, address
        );
        let resp = self.send_command(&cmd).await?;
        tracing::info!(
            "flash write 0x{:x} ({} bytes): {}",
            address,
            data.len(),
            resp
        );
        Ok(())
    }

    /// Read a 32-bit value from a peripheral register.
    ///
    /// `reg_name` can be a human-readable register name (e.g., "GPIOA_ODR")
    /// or a hex address (e.g., "0x40020014").
    pub async fn read_register(
        &self,
        reg_name: &str,
    ) -> Result<u32, DriverError> {
        let cmd = format!("ocd_mdw {}", reg_name);
        let resp = self.send_command(&cmd).await?;
        parse_hex_u32(&resp).ok_or_else(|| {
            DriverError::OpenOcd(format!(
                "Could not parse register value from: {}",
                resp
            ))
        })
    }

    /// Write a 32-bit value to a peripheral register.
    pub async fn write_register(
        &self,
        reg_name: &str,
        value: u32,
    ) -> Result<(), DriverError> {
        let cmd = format!("ocd_mww {} 0x{:x}", reg_name, value);
        let resp = self.send_command(&cmd).await?;
        tracing::info!(
            "write register {} = 0x{:x}: {}",
            reg_name,
            value,
            resp
        );
        Ok(())
    }

    /// Read a block of memory.
    ///
    /// Returns the raw bytes read from the target.
    pub async fn read_memory(
        &self,
        address: u32,
        length: usize,
    ) -> Result<Vec<u8>, DriverError> {
        let cmd = format!("ocd_read_custom 0x{:x} {}", address, length);
        let resp = self.send_command(&cmd).await?;
        parse_hex_bytes(&resp).ok_or_else(|| {
            DriverError::OpenOcd(format!(
                "Could not parse memory from: {}",
                resp
            ))
        })
    }

    /// Write binary data to memory.
    pub async fn write_memory(
        &self,
        address: u32,
        data: &[u8],
    ) -> Result<(), DriverError> {
        let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
        let cmd = format!(
            "ocd_write_custom 0x{:x} {{{}}}",
            address, hex
        );
        let resp = self.send_command(&cmd).await?;
        tracing::info!(
            "write memory 0x{:x} ({} bytes): {}",
            address,
            data.len(),
            resp
        );
        Ok(())
    }
}

// ── Response Parsing Helpers ─────────────────────────────────────

/// Parse a hex u32 from an OpenOCD response string.
///
/// Handles prefixes like `0x` and leading/trailing whitespace.
fn parse_hex_u32(s: &str) -> Option<u32> {
    let hex_str = s
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X");
    u32::from_str_radix(hex_str, 16).ok()
}

/// Parse hex bytes from an OpenOCD response string.
///
/// Extracts all hex digit pairs and converts them to bytes.
/// Handles mixed formats (whitespace, prefixes, etc.).
fn parse_hex_bytes(s: &str) -> Option<Vec<u8>> {
    // Strip common prefixes: "0x", "0X"
    let trimmed = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    let hex_str: String = trimmed
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();

    if hex_str.len() % 2 != 0 {
        return None;
    }

    let bytes: Option<Vec<u8>> = (0..hex_str.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16).ok())
        .collect();

    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_u32_simple() {
        assert_eq!(parse_hex_u32("0xDEADBEEF"), Some(0xDEADBEEF));
    }

    #[test]
    fn test_parse_hex_u32_no_prefix() {
        assert_eq!(parse_hex_u32("A5A5"), Some(0xA5A5));
    }

    #[test]
    fn test_parse_hex_u32_whitespace() {
        assert_eq!(parse_hex_u32("  0x42  "), Some(0x42));
    }

    #[test]
    fn test_parse_hex_u32_zero() {
        assert_eq!(parse_hex_u32("0x0"), Some(0));
    }

    #[test]
    fn test_parse_hex_u32_max() {
        assert_eq!(parse_hex_u32("0xFFFFFFFF"), Some(0xFFFFFFFF));
    }

    #[test]
    fn test_parse_hex_u32_overflow() {
        // More than 32 bits
        assert_eq!(parse_hex_u32("0x1FFFFFFFF"), None);
    }

    #[test]
    fn test_parse_hex_u32_invalid() {
        assert_eq!(parse_hex_u32("xyz"), None);
    }

    #[test]
    fn test_parse_hex_bytes_simple() {
        let bytes = parse_hex_bytes("DEADBEEF").unwrap();
        assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_parse_hex_bytes_with_whitespace() {
        let bytes = parse_hex_bytes("DE AD BE EF").unwrap();
        assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_parse_hex_bytes_with_prefix() {
        let bytes = parse_hex_bytes("0xCAFE").unwrap();
        assert_eq!(bytes, vec![0xCA, 0xFE]);
    }

    #[test]
    fn test_parse_hex_bytes_odd_length() {
        assert_eq!(parse_hex_bytes("ABC"), None);
    }

    #[test]
    fn test_parse_hex_bytes_empty() {
        let bytes = parse_hex_bytes("").unwrap();
        assert!(bytes.is_empty());
    }

    #[test]
    fn test_parse_hex_bytes_mixed_case() {
        let bytes = parse_hex_bytes("aBcD").unwrap();
        assert_eq!(bytes, vec![0xAB, 0xCD]);
    }
}
