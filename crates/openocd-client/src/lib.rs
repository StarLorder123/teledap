//! OpenOCD process management client.
//!
//! Provides `OpenOcdClient` for spawning and controlling an OpenOCD GDB server
//! process, sending Tcl commands, and capturing stdout/stderr output to log files.
//!
//! This crate is independent of the DAP/codelldb debugging layer — OpenOCD
//! management is an optional extension composed alongside `DebugSession` in
//! the main binary, not embedded within it.

pub mod client;
pub mod error;

pub use client::OpenOcdClient;
pub use error::OpenOcdClientError;
