//! Managed debug session with state machine, context-chain assembly,
//! and C++ variable expansion.
//!
//! This crate builds on [`dap_client`] to provide:
//!
//! - **State machine**: `Disconnected → Connected → Initialized → Running ↔ Halted`
//! - **Operation gating**: every operation validates the current session state
//! - **Context-chain assembly**: structured snapshot of threads, frames, scopes, variables
//! - **Variable expansion**: recursive expansion with depth limiting and paging
//! - **Path mapping**: bidirectional AI relative ↔ system absolute path translation
//! - **Variable handle cache**: thread-safe variable name → handle mapping with auto-invalidation
//!
//! # Example
//!
//! ```ignore
//! use debug_session::{DebugSession, SessionState};
//! use dap_client::{AdapterConfig, AdapterKind};
//!
//! let client = DapClient::new(4 * 1024 * 1024);
//! let session = DebugSession::new(client, None);
//!
//! session.start(&AdapterConfig {
//!     path: "/usr/bin/codelldb".into(),
//!     kind: AdapterKind::Codelldb,
//!     args: vec![],
//! }).await?;
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

pub mod cache;
pub mod context;
pub mod error;
pub mod gating;
pub mod mapping;
pub mod session;
pub mod state;
pub mod variables;

// Re-export key types for convenience
pub use cache::{VariableHandleCache, VariableHandleEntry};
pub use context::{ContextChain, FrameContext, ScopeContext, ThreadContext};
pub use dap_client::{AdapterConfig, AdapterKind};
pub use error::DebugSessionError;
pub use gating::ToolAvailability;
pub use mapping::PathMapper;
pub use session::DebugSession;
pub use state::{HaltState, SessionState};
pub use variables::{ExpandedVariable, ExpansionConfig, VariableExpander};
