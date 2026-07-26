//! TeleDAP — MCP Server for AI-assisted embedded hardware debugging.
//!
//! Auto-detects the execution mode:
//! - `--http`                       → HTTP/SSE MCP server mode
//! - `--cli`                        → Phase 2 verification CLI mode
//! - stdin is a pipe (e.g. spawned by Claude Desktop) → MCP server mode over stdio
//! - stdin is a terminal            → Phase 2 verification CLI mode
//!
//! In MCP mode, all tracing output is written to stderr to keep stdout
//! clean for the MCP JSON-RPC protocol.

mod cli;
mod http_server;
mod mcp_router;
mod server;

use clap::Parser;
use std::io::IsTerminal;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    let args = cli::Args::parse();
    let force_cli = args.cli;
    let is_http = args.http;

    // Tracing always goes to stderr (stdout is the MCP protocol channel)
    let filter = if is_http {
        "info" // HTTP mode: info level to stderr
    } else if !force_cli && !std::io::stdin().is_terminal() {
        "warn" // MCP stdio mode: only warnings/errors to stderr
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

    if is_http {
        tracing::info!("Starting in HTTP/SSE MCP server mode on port {}", args.port);
        http_server::run(args.port).await;
    } else if force_cli || std::io::stdin().is_terminal() {
        tracing::info!("Starting in CLI verification mode");
        cli::run(args).await;
    } else {
        tracing::info!("Starting in MCP stdio server mode");
        server::run().await;
    }
}
