//! TeleDAP — MCP Server for AI-assisted embedded hardware debugging.
//!
//! This binary is the Phase 2 verification entry point: it uses `DebugSession`
//! to drive a managed debug session with state tracking, operation gating,
//! and context-chain assembly.
//!
//! Usage:
//! ```bash
//! cargo run -- --codelldb-path /path/to/codelldb [--elf-path /path/to/elf]
//! ```

use std::process;

use clap::Parser;
use dap_client::DapClient;
use dap_trace::TraceHandle;
use dap_types::events::{ExitedEventBody, OutputEventBody, StoppedEventBody};
use dap_types::requests::{
    InitializeRequestArguments, LaunchRequestArguments, SetBreakpointsArguments,
};
use dap_types::types::{Source, SourceBreakpoint};
use debug_session::{DebugSession, SessionState};
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

pub async fn run() {
    let args = Args::parse();

    tracing::info!("TeleDAP Phase 2 — managed debug session");
    tracing::info!("Starting codelldb from: {}", args.codelldb_path);

    // Set up debug trace
    let log_dir = args.log_dir.map(std::path::PathBuf::from);
    let (trace, _trace_bg) = TraceHandle::new(log_dir, 10_000);
    let client = DapClient::with_trace(4 * 1024 * 1024, trace.clone());
    let session = DebugSession::new(client, Some(trace));

    // ── 1. Start codelldb ─────────────────────────────────────────
    if let Err(e) = session.start(&args.codelldb_path).await {
        tracing::error!("Failed to start codelldb: {e}");
        process::exit(1);
    }
    tracing::info!(
        "codelldb process started (state: {:?})",
        SessionState::Connected
    );

    // ── 2. Initialize handshake ───────────────────────────────────
    let caps = match session
        .initialize(InitializeRequestArguments {
            adapter_id: Some("codelldb".into()),
            lines_start_at1: Some(true),
            columns_start_at1: Some(true),
            path_format: Some("path".into()),
            ..Default::default()
        })
        .await
    {
        Ok(caps) => {
            tracing::info!(
                "DAP initialize completed successfully (state: {:?})",
                SessionState::Initialized
            );
            caps
        }
        Err(e) => {
            tracing::error!("DAP initialize failed: {e}");
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
        match session.client().recv_event().await {
            Some(event) => {
                tracing::debug!("Received event: {}", event.event);

                // Handle state-affecting events
                let _ = session.handle_event(&event).await;

                if event.event == "initialized" {
                    tracing::info!("Adapter is ready for configuration.");
                    break;
                }

                // Handle output passthrough events
                if event.event == "output" {
                    if let Some(body) = &event.body {
                        if let Ok(output) = serde_json::from_value::<OutputEventBody>(body.clone())
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
                serde_json::json!([{"text": format!("gdb-remote {remote}")}]);
        }

        let launch_args = LaunchRequestArguments {
            no_debug: None,
            __restart: None,
            extra: launch_extra,
        };

        match session.launch(launch_args).await {
            Ok(_) => tracing::info!("Launch successful"),
            Err(e) => tracing::error!("Launch failed: {e}"),
        }

        // ── 5. Set breakpoint at main ──────────────────────────────
        tracing::info!("Setting breakpoint at main...");
        let bp_result = session
            .set_breakpoints(SetBreakpointsArguments {
                source: Source {
                    name: Some(args.elf_path.clone()),
                    path: Some(args.elf_path.clone()),
                    ..Default::default()
                },
                breakpoints: Some(vec![SourceBreakpoint {
                    line: 0,
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
            Err(e) => tracing::error!("setBreakpoints failed: {e}"),
        }

        // ── 6. Configuration done ───────────────────────────────────
        match session.configuration_done().await {
            Ok(_) => tracing::info!(
                "Configuration done (state: {:?})",
                session.current_state().await
            ),
            Err(e) => tracing::error!("configurationDone failed: {e}"),
        }
    }

    // ── 7. Main event loop ────────────────────────────────────────
    tracing::info!("Entering event loop (Ctrl+C to exit)...");

    loop {
        match session.client().recv_event().await {
            Some(event) => {
                let event_name = event.event.clone();

                // Let the state machine process this event
                let handled = match session.handle_event(&event).await {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::error!("Error handling event '{event_name}': {e}");
                        break;
                    }
                };

                // Handle events that the state machine doesn't fully process
                match event_name.as_str() {
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

                        // Query threads using the session's gated methods
                        match session.get_threads().await {
                            Ok(threads) => {
                                tracing::info!(
                                    "Threads: {}",
                                    threads
                                        .iter()
                                        .map(|t| format!("#{} {}", t.id, t.name))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );

                                // Query stack trace for the first thread
                                if !threads.is_empty() {
                                    let thread_id = body
                                        .as_ref()
                                        .and_then(|b| b.thread_id)
                                        .unwrap_or(threads[0].id);

                                    match session.get_stack_trace(thread_id, None, Some(10)).await {
                                        Ok(frames) => {
                                            tracing::info!("Stack frames: {}", frames.len());
                                            for frame in &frames {
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
                                            if let Some(top_frame) = frames.first() {
                                                match session.get_scopes(top_frame.id).await {
                                                    Ok(scopes) => {
                                                        for scope in &scopes {
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
                                                        tracing::error!("scopes failed: {e}")
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => tracing::error!("stackTrace failed: {e}"),
                                    }
                                }
                            }
                            Err(e) => tracing::error!("threads failed: {e}"),
                        }
                    }

                    "output" => {
                        let body = event.body.as_ref().and_then(|b| {
                            serde_json::from_value::<OutputEventBody>(b.clone()).ok()
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
                            serde_json::from_value::<ExitedEventBody>(b.clone()).ok()
                        });
                        tracing::info!(
                            "Debuggee exited with code: {:?}",
                            body.map(|b| b.exit_code)
                        );
                        break;
                    }

                    _ => {
                        if !handled {
                            tracing::debug!("Unhandled event: {event_name} (seq={})", event.seq);
                        }
                    }
                }

                // Check if we've been disconnected
                if session.current_state().await == SessionState::Disconnected {
                    tracing::info!("Session disconnected.");
                    break;
                }
            }
            None => {
                tracing::info!("Event stream closed.");
                break;
            }
        }
    }

    // ── 8. Shutdown ───────────────────────────────────────────────
    tracing::info!(
        "Shutting down (state: {:?})...",
        session.current_state().await
    );
    if let Err(e) = session.shutdown().await {
        tracing::error!("Shutdown error: {e}");
    }
    tracing::info!("TeleDAP Phase 2 verification complete.");
}
