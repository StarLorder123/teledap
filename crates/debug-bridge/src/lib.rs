//! Debug Bridge — MCP tool handlers bridging AI tool calls to
//! `DebugSession` operations in the TeleDAP server.
//!
//! This crate defines 24 MCP tools (20 gated debug operations + 4 utility
//! tools), a `ToolRegistry` for state-aware dispatch, and typed handler
//! functions grouped by category (lifecycle, execution, breakpoints, inspect).
//!
//! # Architecture
//!
//! ```text
//! tools/call -> ToolRegistry::dispatch()
//!   -> validate state via ToolAvailability
//!   -> trace via TraceSource::McpTrigger
//!   -> route to handlers/{lifecycle,execution,breakpoint,inspect}.rs
//!     -> call DebugSession methods
//!     -> return CallToolResult
//! ```

pub mod error;
pub mod handlers;
pub mod registry;
pub mod tools;

pub use error::BridgeError;
pub use registry::ToolRegistry;
