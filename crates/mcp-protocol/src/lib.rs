//! MCP (Model Context Protocol) — JSON-RPC 2.0 types and stdio transport.
//!
//! This crate provides the protocol layer for TeleDAP's AI client interface.
//! It handles line-delimited JSON-RPC 2.0 over stdin/stdout (the MCP transport
//! differs from DAP's Content-Length framing — do not use `dap-codec` here).
//!
//! # Modules
//!
//! - `types` — JSON-RPC 2.0 and MCP type definitions
//! - `transport` — `McpServer` for line-delimited stdin/stdout I/O
//! - `error` — `McpError` for transport and protocol errors

pub mod error;
pub mod transport;
pub mod types;

pub use error::McpError;
pub use transport::McpServer;
pub use types::*;
