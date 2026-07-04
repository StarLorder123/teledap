//! TeleDAP — MCP Server for AI-assisted embedded hardware debugging.
//!
//! This binary is the Phase 1 verification entry point: it spawns codelldb,
//! completes the DAP initialize handshake, and demonstrates a basic debug session.
//!
//! Usage:
//! ```bash
//! cargo run -- --codelldb-path /path/to/codelldb [--elf-path /path/to/elf]
//! ```

use std::process;

use clap::Parser;
use dap_client::DapClient;
use dap_trace::TraceHandle;
use dap_types::events::StoppedEventBody;
use dap_types::requests::*;
use dap_types::types::{Source, SourceBreakpoint};
use tracing_subscriber::EnvFilter;

/// TeleDAP — Debug Adapter Protocol client for AI-driven debugging.
#[derive(Parser, Debug)]
#[command(name = "teledap", version, about)]
struct Args {
    /// Path to the codelldb binary.
    #[arg(long, default_value = "codelldb")]
    codelldb_path: String,

    /// Path to the ELF binary to debug.
    #[arg(long, default_value = "")]
    elf_path: String,

    /// Remote GDB server address (for remote debugging).
    #[arg(long)]
    gdb_remote: Option<String>,

    /// Enable verbose logging.
    #[arg(short, long, default_value = "false")]
    verbose: bool,

    /// Directory for debug trace JSONL output.
    #[arg(long)]
    log_dir: Option<String>,
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    let filter = if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();

    let args = Args::parse();

    tracing::info!("TeleDAP Phase 1 — DAP protocol verification");
    tracing::info!("Starting codelldb from: {}", args.codelldb_path);

    // Set up debug trace
    let log_dir = args.log_dir.map(std::path::PathBuf::from);
    let (trace, _trace_bg) = TraceHandle::new(log_dir, 10_000);
    let client = DapClient::with_trace(4 * 1024 * 1024, trace);

    // ── 1. Start codelldb ─────────────────────────────────────────
    if let Err(e) = client.start(&args.codelldb_path).await {
        tracing::error!("Failed to start codelldb: {}", e);
        process::exit(1);
    }
    tracing::info!("codelldb process started");

    // ── 2. Initialize handshake ───────────────────────────────────
    let caps = match client
        .send_request::<InitializeRequest>(InitializeRequestArguments {
            adapter_id: Some("codelldb".into()),
            lines_start_at1: Some(true),
            columns_start_at1: Some(true),
            path_format: Some("path".into()),
            ..Default::default()
        })
        .await
    {
        Ok(caps) => {
            tracing::info!("DAP initialize completed successfully");
            caps
        }
        Err(e) => {
            tracing::error!("DAP initialize failed: {}", e);
            process::exit(1);
        }
    };

    tracing::info!("Adapter capabilities:");
    tracing::info!(
        "  ConfigurationDone: {:?}",
        caps.supports_configuration_done_request
    );
    tracing::info!(
        "  Function breakpoints: {:?}",
        caps.supports_function_breakpoints
    );
    tracing::info!(
        "  Conditional breakpoints: {:?}",
        caps.supports_conditional_breakpoints
    );
    tracing::info!(
        "  Evaluate for hovers: {:?}",
        caps.supports_evaluate_for_hovers
    );
    tracing::info!("  Step back: {:?}", caps.supports_step_back);

    // ── 3. Wait for initialized event ─────────────────────────────
    tracing::info!("Waiting for 'initialized' event...");
    loop {
        match client.recv_event().await {
            Some(event) => {
                tracing::info!("Received event: {}", event.event);
                if event.event == "initialized" {
                    tracing::info!("Adapter is ready for configuration.");
                    break;
                }
                if event.event == "output" {
                    if let Some(body) = &event.body {
                        if let Ok(output) = serde_json::from_value::<
                            dap_types::events::OutputEventBody,
                        >(body.clone())
                        {
                            tracing::info!("[stdout] {}", output.output.trim_end());
                        }
                    }
                }
            }
            None => {
                tracing::error!("Event stream closed before initialized event");
                process::exit(1);
            }
        }
    }

    // ── 4. Launch debuggee (if elf_path provided) ──────────────────
    if !args.elf_path.is_empty() {
        tracing::info!("Launching debuggee: {}", args.elf_path);

        let mut launch_extra = serde_json::json!({
            "program": args.elf_path,
            "stopOnEntry": false,
        });

        if let Some(ref remote) = args.gdb_remote {
            launch_extra["customLaunchSetupCommands"] =
                serde_json::json!([{"text": format!("gdb-remote {}", remote)}]);
        }

        let launch_args = LaunchRequestArguments {
            no_debug: None,
            __restart: None,
            extra: launch_extra,
        };

        match client.send_request::<LaunchRequest>(launch_args).await {
            Ok(_) => tracing::info!("Launch successful"),
            Err(e) => tracing::error!("Launch failed: {}", e),
        }

        // ── 5. Set breakpoint at main ──────────────────────────────
        tracing::info!("Setting breakpoint at main...");
        let bp_result = client
            .send_request::<SetBreakpointsRequest>(SetBreakpointsArguments {
                source: Source {
                    name: Some(args.elf_path.clone()),
                    path: Some(args.elf_path.clone()),
                    ..Default::default()
                },
                breakpoints: Some(vec![SourceBreakpoint {
                    line: 0, // Function-name based breakpoints are adapter-specific
                    column: None,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                    mode: None,
                }]),
                lines: None,
                source_modified: None,
            })
            .await;

        match bp_result {
            Ok(resp) => {
                tracing::info!("Breakpoints set: {} total", resp.breakpoints.len());
                for bp in &resp.breakpoints {
                    tracing::info!(
                        "  BP id={:?}, verified={}, line={:?}",
                        bp.id,
                        bp.verified,
                        bp.line
                    );
                }
            }
            Err(e) => tracing::error!("setBreakpoints failed: {}", e),
        }

        // ── 6. Configuration done ───────────────────────────────────
        match client
            .send_request::<ConfigurationDoneRequest>(NoArguments {})
            .await
        {
            Ok(_) => tracing::info!("Configuration done"),
            Err(e) => tracing::error!("configurationDone failed: {}", e),
        }
    }

    // ── 7. Main event loop ────────────────────────────────────────
    tracing::info!("Entering event loop (Ctrl+C to exit)...");

    loop {
        match client.recv_event().await {
            Some(event) => {
                match event.event.as_str() {
                    "stopped" => {
                        let body: Option<StoppedEventBody> = event
                            .body
                            .as_ref()
                            .and_then(|b| serde_json::from_value(b.clone()).ok());
                        tracing::info!(
                            "STOPPED: reason={:?}, thread={:?}",
                            body.as_ref().map(|b| &b.reason),
                            body.as_ref().and_then(|b| b.thread_id)
                        );

                        // Query threads
                        if let Some(ref b) = body {
                            if let Some(thread_id) = b.thread_id {
                                let threads =
                                    client.send_request::<ThreadsRequest>(NoArguments {}).await;
                                match threads {
                                    Ok(resp) => {
                                        tracing::info!(
                                            "Threads: {}",
                                            resp.threads
                                                .iter()
                                                .map(|t| format!("#{} {}", t.id, t.name))
                                                .collect::<Vec<_>>()
                                                .join(", ")
                                        );
                                    }
                                    Err(e) => tracing::error!("threads failed: {}", e),
                                }

                                // Query stack trace
                                let stack = client
                                    .send_request::<StackTraceRequest>(StackTraceArguments {
                                        thread_id,
                                        start_frame: None,
                                        levels: Some(10),
                                        format: None,
                                    })
                                    .await;

                                match stack {
                                    Ok(resp) => {
                                        tracing::info!(
                                            "Stack frames: {} ({} total)",
                                            resp.stack_frames.len(),
                                            resp.total_frames.unwrap_or(0)
                                        );
                                        for frame in &resp.stack_frames {
                                            tracing::info!(
                                                "  #{} {} at {}:{}",
                                                frame.id,
                                                frame.name,
                                                frame
                                                    .source
                                                    .as_ref()
                                                    .and_then(|s| s.path.as_deref())
                                                    .unwrap_or("?"),
                                                frame.line
                                            );
                                        }

                                        // Query scopes for top frame
                                        if let Some(top_frame) = resp.stack_frames.first() {
                                            let scopes = client
                                                .send_request::<ScopesRequest>(ScopesArguments {
                                                    frame_id: top_frame.id,
                                                })
                                                .await;

                                            match scopes {
                                                Ok(resp) => {
                                                    for scope in &resp.scopes {
                                                        tracing::info!(
                                                            "  Scope '{}': variablesRef={}, named={}, indexed={}",
                                                            scope.name,
                                                            scope.variables_reference,
                                                            scope.named_variables.unwrap_or(0),
                                                            scope.indexed_variables.unwrap_or(0),
                                                        );
                                                    }
                                                }
                                                Err(e) => {
                                                    tracing::error!("scopes failed: {}", e)
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => tracing::error!("stackTrace failed: {}", e),
                                }
                            }
                        }
                    }

                    "output" => {
                        let body = event.body.as_ref().and_then(|b| {
                            serde_json::from_value::<dap_types::events::OutputEventBody>(b.clone())
                                .ok()
                        });
                        if let Some(ref b) = body {
                            tracing::info!(
                                "[{}] {}",
                                b.category
                                    .as_ref()
                                    .map(|c| match c {
                                        dap_types::enums::OutputCategory::Stderr => "stderr",
                                        _ => "stdout",
                                    })
                                    .unwrap_or("output"),
                                b.output.trim_end()
                            );
                        }
                    }

                    "terminated" => {
                        tracing::info!("Debug session terminated.");
                        break;
                    }

                    "exited" => {
                        let body = event.body.as_ref().and_then(|b| {
                            serde_json::from_value::<dap_types::events::ExitedEventBody>(b.clone())
                                .ok()
                        });
                        tracing::info!(
                            "Debuggee exited with code: {:?}",
                            body.map(|b| b.exit_code)
                        );
                        break;
                    }

                    _ => {
                        tracing::debug!("Event: {} (seq={})", event.event, event.seq);
                    }
                }
            }
            None => {
                tracing::info!("Event stream closed.");
                break;
            }
        }
    }

    // ── 8. Shutdown ───────────────────────────────────────────────
    tracing::info!("Shutting down...");
    if let Err(e) = client.shutdown().await {
        tracing::error!("Shutdown error: {}", e);
    }
    tracing::info!("TeleDAP Phase 1 verification complete.");
}
