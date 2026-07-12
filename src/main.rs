//! TeleDAP — MCP Server for AI-assisted embedded hardware debugging.
//!
//! Auto-detects the execution mode:
//! - When stdin is a pipe (e.g. spawned by Claude Desktop) → MCP server mode
//! - When stdin is a terminal → Phase 2 verification CLI mode
//!
//! In MCP mode, all tracing output is written to stderr to keep stdout
//! clean for the MCP JSON-RPC protocol.

mod cli;
mod server;

use std::io::IsTerminal;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Detect mode: pipe → MCP, terminal → CLI
    let is_mcp = !std::io::stdin().is_terminal();

    // Tracing always goes to stderr (stdout is the MCP protocol channel)
    let filter = if is_mcp {
        "warn" // MCP mode: only warnings/errors to stderr
    } else if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    if is_mcp {
        tracing::info!("Starting in MCP server mode");
        server::run().await;
    } else {
        tracing::info!("Starting in CLI verification mode");
        cli::run().await;
    }
}
