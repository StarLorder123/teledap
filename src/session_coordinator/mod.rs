//! Core session coordinator.
//!
//! The `SessionCoordinator` is the central orchestrator of TeleDAP.
//! It owns the protocol drivers, enforces the debug session state machine,
//! and provides the tool dispatch interface used by the MCP layer.

pub mod coordinator;
pub mod state_machine;

pub use coordinator::SessionCoordinator;
// SessionState is accessed via state_machine::SessionState
