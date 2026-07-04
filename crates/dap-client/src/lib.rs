//! DAP client for managing a codelldb child process and typed DAP communication.
//!
//! # Example
//!
//! ```rust,no_run
//! use dap_client::DapClient;
//! use dap_types::requests::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = DapClient::new(4 * 1024 * 1024);
//!     client.start("codelldb").await?;
//!
//!     // Initialize the debug session
//!     let caps = client.send_request::<InitializeRequest>(
//!         InitializeRequestArguments {
//!             adapter_id: Some("codelldb".into()),
//!             ..Default::default()
//!         }
//!     ).await?;
//!
//!     println!("Debug adapter capabilities: {:?}", caps);
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod error;

pub use client::DapClient;
pub use client::DEFAULT_MAX_FRAME_SIZE;
pub use error::DapClientError;
