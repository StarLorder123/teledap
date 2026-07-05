//! Managed debug session with state machine, context-chain assembly,
//! and C++ variable expansion.
//!
//! This crate builds on [`dap_client`] to provide:
//!
//! - **State machine**: `Disconnected → Connected → Initialized → Running ↔ Halted`
//! - **Operation gating**: every operation validates the current session state
//! - **Context-chain assembly**: structured snapshot of threads, frames, scopes, variables
//! - **Variable expansion**: recursive expansion with depth limiting and paging
//!
//! # Example
//!
//! ```ignore
//! use debug_session::{DebugSession, SessionState};
//!
//! let client = DapClient::new(4 * 1024 * 1024);
//! let session = DebugSession::new(client, None);
//!
//! session.start("/usr/bin/codelldb").await?;
//! session.initialize(InitializeRequestArguments { ... }).await?;
//! session.launch(LaunchRequestArguments::with_program("app.elf")).await?;
//! session.configuration_done().await?;
//!
//! // Event loop
//! while let Some(event) = session.client().recv_event().await {
//!     if !session.handle_event(&event).await? {
//!         // Handle output events, etc.
//!     }
//!     if session.current_state().await == SessionState::Disconnected {
//!         break;
//!     }
//! }
//! ```

pub mod context;
pub mod error;
pub mod gating;
pub mod session;
pub mod state;
pub mod variables;

// Re-export key types for convenience
pub use context::{ContextChain, FrameContext, ScopeContext, ThreadContext};
pub use error::DebugSessionError;
pub use gating::ToolAvailability;
pub use session::DebugSession;
pub use state::{HaltState, SessionState};
pub use variables::{ExpandedVariable, ExpansionConfig, VariableExpander};
