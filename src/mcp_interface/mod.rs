//! MCP protocol adapter layer.
//!
//! Implements the `rmcp::ServerHandler` trait, routing `tools/list` and
//! `tools/call` requests to the `SessionCoordinator`.

pub mod protocol_types;
pub mod server;

pub use server::TeleDapServer;
