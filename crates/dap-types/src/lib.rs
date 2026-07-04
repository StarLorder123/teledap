//! Complete DAP (Debug Adapter Protocol) type definitions.
//!
//! This crate provides Rust types for all 103 types defined in the
//! [DAP specification](https://microsoft.github.io/debug-adapter-protocol/specification),
//! organized into the following modules:
//!
//! - [`base`] — ProtocolMessage, Request, Response, Event, ErrorResponse, Cancel
//! - [`enums`] — All string enums and type aliases
//! - [`types`] — Data types: Source, StackFrame, Scope, Variable, Breakpoint, etc.
//! - [`capabilities`] — The Capabilities struct returned by `initialize`
//! - [`events`] — All 17 event body types
//! - [`requests`] — All 42 request/response pairs with the [`DapRequest`] trait
//! - [`reverse_requests`] — Reverse requests: RunInTerminal, StartDebugging
//!
//! # Example
//!
//! ```rust
//! use dap_types::base::{ProtocolMessage, Request};
//! use dap_types::requests::SetBreakpointsArguments;
//! use dap_types::types::{Source, SourceBreakpoint};
//!
//! // Deserialize from JSON
//! let msg: ProtocolMessage = serde_json::from_str(
//!     r#"{"type":"request","seq":1,"command":"threads"}"#
//! ).unwrap();
//! assert!(msg.is_request());
//!
//! // Construct a typed breakpoint request
//! let args = SetBreakpointsArguments {
//!     source: Source {
//!         path: Some("/src/main.rs".into()),
//!         ..Default::default()
//!     },
//!     breakpoints: Some(vec![SourceBreakpoint {
//!         line: 42,
//!         column: None,
//!         condition: None,
//!         hit_condition: None,
//!         log_message: None,
//!         mode: None,
//!     }]),
//!     lines: None,
//!     source_modified: None,
//! };
//! ```

pub mod base;
pub mod capabilities;
pub mod enums;
pub mod events;
pub mod requests;
pub mod reverse_requests;
pub mod types;
