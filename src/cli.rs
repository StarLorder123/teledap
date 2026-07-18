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

    /// Path to the source file for setting breakpoints (e.g. test_debuggee/main.c).
    /// If not provided, inferred from elf_path by replacing .exe with .c.
    #[arg(long, default_value = "")]
    source_path: String,

    /// Comma-separated line numbers for breakpoints (e.g. "9,13,4").
    /// Defaults to "8,13,4" if empty and source_path is set.
    #[arg(long, default_value = "")]
    breakpoints: String,

    /// Force CLI mode (skips stdin terminal detection).
    #[arg(long, default_value = "false")]
    cli: bool,

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
            client_id: Some("teledap".into()),
            client_name: Some("TeleDAP".into()),
            adapter_id: Some("lldb".into()),
            locale: Some("en-US".into()),
            lines_start_at1: Some(true),
            columns_start_at1: Some(true),
            path_format: Some("path".into()),
            supports_variable_type: Some(true),
            supports_variable_paging: Some(false),
            supports_run_in_terminal_request: Some(false),
            supports_memory_references: Some(true),
            supports_progress_reporting: Some(true),
            supports_invalidated_event: Some(true),
            supports_memory_event: Some(true),
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

    // ── 3. Launch debuggee (if elf_path provided) ──────────────────
    // Note: codelldb sends the `initialized` event during launch request
    // processing, so we must send launch first (fire-and-forget), then wait
    // for the initialized event.
    if !args.elf_path.is_empty() {
        tracing::info!("Launching debuggee: {}", args.elf_path);

        let mut launch_extra = serde_json::json!({
            "program": args.elf_path,
            "stopOnEntry": false,
        });

        if let Some(ref remote) = args.gdb_remote {
            launch_extra["processCreateCommands"] =
                serde_json::json!([format!("gdb-remote {remote}")]);
        }

        let launch_args = LaunchRequestArguments {
            no_debug: None,
            __restart: None,
            extra: launch_extra,
        };

        // Verify state before sending launch (must be in Initialized)
        let state = session.current_state().await;
        if !debug_session::ToolAvailability::is_allowed("launch", state) {
            tracing::error!("Cannot launch in state {:?}; expected Initialized", state);
            process::exit(1);
        }

        session
            .client()
            .send_request_nb::<dap_types::requests::LaunchRequest>(launch_args)
            .await
            .unwrap_or_else(|e| tracing::error!("Launch send failed: {e}"));
        tracing::info!("Launch request sent (non-blocking).");

        // ── 4. Wait for initialized event ─────────────────────────────
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
                            if let Ok(output) =
                                serde_json::from_value::<OutputEventBody>(body.clone())
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

        // ── 5. Set breakpoints with real source path and line numbers ──
        let source_path = if !args.source_path.is_empty() {
            // Resolve to absolute path, stripping Windows \\?\ prefix
            let p = std::path::PathBuf::from(&args.source_path);
            if p.is_absolute() {
                p.display().to_string()
            } else if let Ok(cwd) = std::env::current_dir() {
                let abs = cwd.join(&p);
                // std::fs::canonicalize would add \\?\ prefix on Windows which
                // codelldb can't match against debug info. Use current_dir join instead.
                abs.display().to_string()
            } else {
                args.source_path.clone()
            }
        } else {
            // Infer source path from ELF path: look for main.c in the ELF's directory
            let elf_path = std::path::Path::new(&args.elf_path);
            let inferred = if let Some(parent) = elf_path.parent() {
                parent.join("main.c")
            } else {
                std::path::PathBuf::from("main.c")
            };
            if inferred.is_absolute() {
                inferred.display().to_string()
            } else if let Ok(cwd) = std::env::current_dir() {
                cwd.join(&inferred).display().to_string()
            } else {
                inferred.display().to_string()
            }
        };

        if source_path.is_empty() {
            tracing::warn!("No source path available; skipping breakpoints.");
        } else {
            let bp_lines: Vec<u64> = if !args.breakpoints.is_empty() {
                args.breakpoints
                    .split(',')
                    .filter_map(|s| s.trim().parse::<u64>().ok())
                    .collect()
            } else {
                // Default breakpoints at executable lines:
                //   line 9: printf("Hello...") — first executable line in main
                //   line 13: int z = add(x, y) — after x, y init, before add() call
                //   line 4: int result = a + b — inside add(), inspect params
                vec![9, 13, 4]
            };

            if bp_lines.is_empty() {
                tracing::warn!("No valid breakpoint line numbers; skipping breakpoints.");
            } else {
                tracing::info!(
                    "Setting {} breakpoint(s) in {} at lines: {:?}",
                    bp_lines.len(),
                    source_path,
                    bp_lines
                );

                let source_bps: Vec<SourceBreakpoint> = bp_lines
                    .iter()
                    .map(|&line| SourceBreakpoint {
                        line,
                        column: None,
                        condition: None,
                        hit_condition: None,
                        log_message: None,
                        mode: None,
                    })
                    .collect();

                let bp_result = session
                    .set_breakpoints(SetBreakpointsArguments {
                        source: Source {
                            name: Some(
                                std::path::Path::new(&source_path)
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| source_path.clone()),
                            ),
                            path: Some(source_path),
                            ..Default::default()
                        },
                        breakpoints: Some(source_bps),
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
            }
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
                            "STOPPED: reason={:?}, thread={:?}, hit_bp_ids={:?}",
                            body.as_ref().map(|b| &b.reason),
                            body.as_ref().and_then(|b| b.thread_id),
                            body.as_ref().and_then(|b| b.hit_breakpoint_ids.as_deref())
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

                                            // Query scopes and variables for top frame
                                            if let Some(top_frame) = frames.first() {
                                                match session.get_scopes(top_frame.id).await {
                                                    Ok(scopes) => {
                                                        for scope in &scopes {
                                                            let named =
                                                                scope.named_variables.unwrap_or(0);
                                                            let indexed = scope
                                                                .indexed_variables
                                                                .unwrap_or(0);
                                                            let total_vars = named + indexed;

                                                            // Only fetch variables for Local scope to keep
                                                            // output manageable; summarize other scopes
                                                            let is_local = scope.name == "Local";
                                                            if is_local {
                                                                tracing::info!(
                                                                    "  Scope '{}': variablesRef={}, total_vars={}",
                                                                    scope.name,
                                                                    scope.variables_reference,
                                                                    total_vars,
                                                                );
                                                            } else {
                                                                tracing::info!(
                                                                    "  Scope '{}': variablesRef={}, total_vars={} (skipped)",
                                                                    scope.name,
                                                                    scope.variables_reference,
                                                                    total_vars,
                                                                );
                                                            }

                                                            if is_local
                                                                && scope.variables_reference > 0
                                                            {
                                                                match session
                                                                    .get_variables(
                                                                        scope.variables_reference,
                                                                        None,
                                                                        None,
                                                                        None,
                                                                    )
                                                                    .await
                                                                {
                                                                    Ok(vars) => {
                                                                        tracing::info!(
                                                                            "    Variables ({}):",
                                                                            vars.len()
                                                                        );
                                                                        for v in &vars {
                                                                            let type_info = v
                                                                                .var_type
                                                                                .as_deref()
                                                                                .unwrap_or("?");
                                                                            tracing::info!(
                                                                                "      {} = {} (type: {})",
                                                                                v.name,
                                                                                v.value,
                                                                                type_info,
                                                                            );
                                                                        }
                                                                    }
                                                                    Err(e) => {
                                                                        tracing::error!(
                                                                            "    variables failed for scope '{}': {e}",
                                                                            scope.name
                                                                        )
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        tracing::error!("scopes failed: {e}")
                                                    }
                                                }
                                            }

                                            // Continue execution after inspection
                                            tracing::info!(
                                                "Continuing execution (thread_id={})...",
                                                thread_id
                                            );
                                            match session.continue_execution(thread_id, None).await
                                            {
                                                Ok(resp) => {
                                                    tracing::info!(
                                                        "  Continued: all_threads_continued={:?}",
                                                        resp.all_threads_continued
                                                    );
                                                }
                                                Err(e) => {
                                                    tracing::error!("continue failed: {e}");
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
    let state = session.current_state().await;
    tracing::info!("Shutting down (state: {:?})...", state);
    if state != SessionState::Disconnected {
        if let Err(e) = session.shutdown().await {
            tracing::error!("Shutdown error: {e}");
        }
    }
    tracing::info!("TeleDAP Phase 2 verification complete.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_default_args() {
        let args = Args::try_parse_from(["teledap"]).unwrap();
        assert_eq!(args.codelldb_path, "codelldb");
        assert_eq!(args.elf_path, "");
        assert!(args.gdb_remote.is_none());
        assert!(!args.verbose);
        assert!(args.log_dir.is_none());
    }

    #[test]
    fn test_custom_codelldb_path() {
        let args =
            Args::try_parse_from(["teledap", "--codelldb-path", "/usr/bin/codelldb"]).unwrap();
        assert_eq!(args.codelldb_path, "/usr/bin/codelldb");
    }

    #[test]
    fn test_elf_and_gdb_remote() {
        let args = Args::try_parse_from([
            "teledap",
            "--elf-path",
            "app.elf",
            "--gdb-remote",
            "localhost:3333",
        ])
        .unwrap();
        assert_eq!(args.elf_path, "app.elf");
        assert_eq!(args.gdb_remote.as_deref(), Some("localhost:3333"));
    }

    #[test]
    fn test_verbose_flag() {
        let args = Args::try_parse_from(["teledap", "-v"]).unwrap();
        assert!(args.verbose);
    }

    #[test]
    fn test_log_dir() {
        let args = Args::try_parse_from(["teledap", "--log-dir", "./traces"]).unwrap();
        assert_eq!(args.log_dir.as_deref(), Some("./traces"));
    }
}
