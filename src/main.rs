mod audit_tracker;
mod drivers;
mod error;
mod mcp_interface;
mod session_coordinator;

use audit_tracker::AuditLogger;
use clap::Parser;
use mcp_interface::TeleDapServer;
use rmcp::ServiceExt;
use session_coordinator::SessionCoordinator;
use std::path::PathBuf;
use std::sync::Arc;

/// TeleDAP — MCP Server for embedded hardware debugging.
///
/// Bridges AI assistants (LLMs) to CodeLLDB (DAP protocol) and OpenOCD (Tcl RPC),
/// providing a clean, declarative tool interface for hardware debugging.
#[derive(Parser, Debug)]
#[command(name = "teledap", version, about = "MCP Server for embedded hardware debugging")]
struct Cli {
    /// Path to codelldb executable
    #[arg(long, default_value = "codelldb")]
    codelldb_path: String,

    /// OpenOCD Tcl RPC host
    #[arg(long, default_value = "127.0.0.1")]
    openocd_host: String,

    /// OpenOCD Tcl RPC TCP port
    #[arg(long, default_value_t = 6666)]
    openocd_tcl_port: u16,

    /// OpenOCD GDB server TCP port (for gdb-remote connections)
    #[arg(long, default_value_t = 3333)]
    openocd_gdb_port: u16,

    /// Directory for audit log JSONL files
    #[arg(long)]
    log_dir: Option<PathBuf>,

    /// Max ring buffer entries for in-memory audit log
    #[arg(long, default_value_t = 500)]
    ring_size: usize,

    /// Max DAP frame size in bytes (safety limit)
    #[arg(long, default_value_t = 4_194_304)]
    max_dap_frame: usize,

    /// Enable verbose tracing output
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing subscriber — writes to stderr to avoid
    // interfering with MCP JSON-RPC on stdout.
    let env_filter = if cli.verbose {
        "teledap=trace,rmcp=debug"
    } else {
        "teledap=info,rmcp=warn"
    };
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .init();

    tracing::info!(
        "TeleDAP v{} starting up...",
        env!("CARGO_PKG_VERSION")
    );
    tracing::info!("CodeLLDB path: {}", cli.codelldb_path);
    tracing::info!(
        "OpenOCD: {}:{} (Tcl), {}:{} (GDB)",
        cli.openocd_host,
        cli.openocd_tcl_port,
        cli.openocd_host,
        cli.openocd_gdb_port
    );

    // ── Phase 1: Initialize Audit Logger ─────────────────────────
    let (audit, _audit_handle) =
        AuditLogger::new(cli.log_dir.clone(), cli.ring_size);
    tracing::info!(
        "Audit logger initialized (ring size: {})",
        cli.ring_size
    );
    if let Some(ref dir) = cli.log_dir {
        tracing::info!("Audit log directory: {:?}", dir);
    }

    // ── Phase 3: Initialize Session Coordinator ──────────────────
    let coordinator = Arc::new(SessionCoordinator::new(
        audit.clone(),
        cli.openocd_host,
        cli.openocd_tcl_port,
        cli.openocd_gdb_port,
        cli.codelldb_path,
        cli.max_dap_frame,
    ));
    tracing::info!("Session coordinator initialized.");

    // ── Phase 4: Build MCP Server ────────────────────────────────
    let server = TeleDapServer::new(coordinator.clone());
    tracing::info!("MCP server created.");

    // ── Phase 5: Graceful Shutdown ───────────────────────────────
    let coordinator_shutdown = coordinator.clone();
    let audit_shutdown = audit.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Received Ctrl+C, shutting down...");
        let _ = coordinator_shutdown.shutdown().await;
        // Drop the audit logger Arc to signal the background task
        drop(audit_shutdown);
    });

    // ── Start MCP stdio transport ────────────────────────────────
    tracing::info!("Starting MCP stdio transport...");
    let (stdin, stdout) = rmcp::transport::io::stdio();

    match server.serve((stdin, stdout)).await {
        Ok(running_service) => {
            let _ = running_service.waiting().await;
        }
        Err(e) => {
            tracing::error!("MCP server error: {}", e);
            return Err(anyhow::anyhow!("MCP server error: {}", e));
        }
    }

    tracing::info!("TeleDAP exited.");
    Ok(())
}
