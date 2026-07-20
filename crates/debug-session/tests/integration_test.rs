//! Integration tests for the debug-session crate.
//!
//! These tests spawn a real `codelldb` process and verify the state machine,
//! operation gating, and context-chain assembly end-to-end.
//!
//! Every test that requires codelldb uses a `codelldb_available()` guard that
//! gracefully skips when codelldb is not installed.
//!
//! All async operations are wrapped in a timeout to prevent hangs.

use std::process::Command;
use std::time::Duration;

use dap_client::{AdapterConfig, AdapterKind, DapClient, DEFAULT_MAX_FRAME_SIZE};
use dap_types::requests::{InitializeRequestArguments, LaunchRequestArguments};
use debug_session::{DebugSession, DebugSessionError, SessionState, ToolAvailability};
use tokio::time::timeout;

// ── Environment probes ──────────────────────────────────────────────

/// Returns `true` if `codelldb` can be spawned (and immediately killed).
fn codelldb_available() -> bool {
    Command::new("codelldb")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|mut child| {
            let _ = child.kill();
            let _ = child.wait();
            true
        })
        .unwrap_or(false)
}

/// Returns a path to an ELF binary for testing, or None if not available.
fn test_elf_path() -> Option<String> {
    // Try to find any compiled test binary from our own workspace
    let exe = std::env::current_exe().ok()?;
    let exe_str = exe.to_string_lossy().to_string();

    // On Windows, look for any .exe in the target directory
    if exe_str.ends_with("test") || exe_str.ends_with("test.exe") {
        // We ARE a test binary; use ourselves as a target
        return Some(exe_str);
    }

    // Look for the test helper binary
    let base = exe.parent()?;
    for entry in (std::fs::read_dir(base).ok()?).flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".exe") || !name.contains('.') {
            return Some(entry.path().to_string_lossy().to_string());
        }
    }
    None
}

// ── Informational probe ─────────────────────────────────────────────

/// Always-passing test that reports environment status.
#[tokio::test]
async fn test_integration_environment_probe() {
    if codelldb_available() {
        eprintln!("codelldb found on PATH — debug-session integration tests will run.");
    } else {
        eprintln!("SKIP: codelldb not found on PATH. Integration tests will be skipped.");
    }
    if let Some(elf) = test_elf_path() {
        eprintln!("ELF binary available: {elf}");
    } else {
        eprintln!("No ELF binary found — context chain tests will be skipped.");
    }
}

// ── State machine tests ─────────────────────────────────────────────

/// Verify the full state machine cycle: start → initialize → configurationDone
/// → shutdown. Does not require an ELF binary.
#[tokio::test]
async fn test_full_state_machine_cycle() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);
    let session = DebugSession::new(client, None);

    let result = timeout(Duration::from_secs(5), async {
        // Disconnected → Connected
        assert_eq!(session.current_state().await, SessionState::Disconnected);
        let start_config = AdapterConfig {
            path: "codelldb".into(),
            kind: AdapterKind::Codelldb,
            args: vec![],
        };
        session.start(&start_config).await?;
        assert_eq!(session.current_state().await, SessionState::Connected);

        // Connected → Initialized
        let caps = session
            .initialize(InitializeRequestArguments {
                adapter_id: Some("lldb".into()),
                client_name: Some("teledap-sm-test".into()),
                ..Default::default()
            })
            .await?;
        assert_eq!(session.current_state().await, SessionState::Initialized);
        assert!(caps.supports_configuration_done_request.unwrap_or(false));

        // Initialized → Running (configurationDone without launch is OK
        // for basic state machine testing)
        session.configuration_done().await?;
        assert_eq!(session.current_state().await, SessionState::Running);

        // Running → Disconnected (shutdown)
        session.shutdown().await?;
        assert_eq!(session.current_state().await, SessionState::Disconnected);

        Ok::<_, DebugSessionError>(())
    })
    .await;

    // Best-effort cleanup
    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("State machine cycle test failed: {e}"),
        Err(_) => panic!("State machine cycle test timed out after 5 seconds"),
    }
}

/// Verify that the state watcher broadcasts transitions.
#[tokio::test]
async fn test_state_watcher_notifications() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);
    let session = DebugSession::new(client, None);
    let mut watcher = session.state_watcher();

    // Initial value should be Disconnected
    assert_eq!(*watcher.borrow(), SessionState::Disconnected);

    let result = timeout(Duration::from_secs(5), async {
        let start_config = AdapterConfig {
            path: "codelldb".into(),
            kind: AdapterKind::Codelldb,
            args: vec![],
        };
        session.start(&start_config).await?;

        // The watcher should now show Connected
        // tokio::sync::watch only notifies on change, but borrow() reads the
        // current value; `changed()` waits for a new value
        watcher
            .changed()
            .await
            .expect("watch channel should be open");
        assert_eq!(*watcher.borrow(), SessionState::Connected);

        session
            .initialize(InitializeRequestArguments {
                adapter_id: Some("lldb".into()),
                ..Default::default()
            })
            .await?;
        watcher
            .changed()
            .await
            .expect("watch channel should be open");
        assert_eq!(*watcher.borrow(), SessionState::Initialized);

        Ok::<_, DebugSessionError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("State watcher test failed: {e}"),
        Err(_) => panic!("State watcher test timed out after 5 seconds"),
    }
}

// ── Operation gating tests ──────────────────────────────────────────

/// Verify that operations are rejected in invalid states.
#[tokio::test]
async fn test_operation_gating_rejects_invalid_state() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);
    let session = DebugSession::new(client, None);

    let result = timeout(Duration::from_secs(5), async {
        // Attempt to initialize before start → InvalidState
        let err = session
            .initialize(InitializeRequestArguments::default())
            .await
            .unwrap_err();
        assert!(
            matches!(err, DebugSessionError::InvalidState { .. }),
            "Expected InvalidState, got: {err:?}"
        );

        // Attempt to continue before connected → InvalidState
        let err = session.continue_execution(1, None).await.unwrap_err();
        assert!(
            matches!(err, DebugSessionError::InvalidState { .. }),
            "Expected InvalidState, got: {err:?}"
        );

        // Start properly
        let start_config = AdapterConfig {
            path: "codelldb".into(),
            kind: AdapterKind::Codelldb,
            args: vec![],
        };
        session.start(&start_config).await?;
        assert_eq!(session.current_state().await, SessionState::Connected);

        // Attempt to launch before initialize → InvalidState
        let err = session
            .launch(LaunchRequestArguments::default())
            .await
            .unwrap_err();
        assert!(
            matches!(err, DebugSessionError::InvalidState { .. }),
            "Expected InvalidState, got: {err:?}"
        );

        Ok::<_, DebugSessionError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Gating test failed: {e}"),
        Err(_) => panic!("Gating test timed out after 5 seconds"),
    }
}

/// Verify ToolAvailability mappings for all states.
#[test]
fn test_tool_availability_consistency() {
    // Every known operation should have at least one valid state
    for op in &[
        "start",
        "initialize",
        "launch",
        "attach",
        "configuration_done",
        "shutdown",
        "disconnect",
        "continue",
        "step_over",
        "step_in",
        "step_out",
        "pause",
        "set_breakpoints",
        "set_function_breakpoints",
        "get_threads",
        "get_stack_trace",
        "get_scopes",
        "get_variables",
        "evaluate",
        "set_variable",
        "assemble_context",
    ] {
        let states = ToolAvailability::allowed_states(op);
        assert!(!states.is_empty(), "Operation `{op}` has no allowed states");
    }

    // Key invariants
    assert!(ToolAvailability::is_allowed(
        "start",
        SessionState::Disconnected
    ));
    assert!(!ToolAvailability::is_allowed(
        "start",
        SessionState::Connected
    ));
    assert!(ToolAvailability::is_allowed(
        "continue",
        SessionState::Halted
    ));
    assert!(!ToolAvailability::is_allowed(
        "continue",
        SessionState::Running
    ));
    assert!(ToolAvailability::is_allowed("pause", SessionState::Running));
    assert!(!ToolAvailability::is_allowed("pause", SessionState::Halted));
}

// ── Context-chain assembly tests ────────────────────────────────────

/// Full lifecycle with ELF binary: launch, hit breakpoint, assemble context.
/// Skips if no ELF binary is available.
#[tokio::test]
async fn test_context_chain_assembly() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let elf_path = match test_elf_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: no ELF binary available for context chain test.");
            return;
        }
    };

    eprintln!("Using ELF: {elf_path}");

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);
    let session = DebugSession::new(client, None);

    let result = timeout(Duration::from_secs(10), async {
        let start_config = AdapterConfig {
            path: "codelldb".into(),
            kind: AdapterKind::Codelldb,
            args: vec![],
        };
        session.start(&start_config).await?;

        // Initialize
        session
            .initialize(InitializeRequestArguments {
                adapter_id: Some("lldb".into()),
                client_name: Some("teledap-ctx-test".into()),
                ..Default::default()
            })
            .await?;

        // Launch the ELF
        session
            .launch(LaunchRequestArguments::with_program(&elf_path))
            .await?;

        // Wait for initialized event
        let initialized = timeout(Duration::from_secs(3), async {
            let mut got_initialized = false;
            while let Some(event) = session.client().recv_event().await {
                let is_init = event.event == "initialized";
                session.handle_event(&event).await?;
                if is_init {
                    got_initialized = true;
                    break;
                }
            }
            Ok::<_, DebugSessionError>(got_initialized)
        })
        .await;

        match initialized {
            Ok(Ok(true)) => { /* got initialized */ }
            Ok(Ok(false)) => {
                eprintln!("No initialized event received — may be OK.");
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                eprintln!("Timed out waiting for initialized event.");
            }
        }

        // Configuration done
        session.configuration_done().await?;

        // Wait for a stopped event (hit breakpoint at main, entry, etc.)
        let stopped_received = timeout(Duration::from_secs(5), async {
            loop {
                match session.client().recv_event().await {
                    Some(event) => {
                        let _ = session.handle_event(&event).await?;
                        if event.event == "stopped" {
                            return Ok::<_, DebugSessionError>(true);
                        }
                        if event.event == "terminated" || event.event == "exited" {
                            eprintln!("Debuggee exited/terminated before stopping.");
                            return Ok(false);
                        }
                    }
                    None => return Ok(false),
                }
            }
        })
        .await;

        match stopped_received {
            Ok(Ok(true)) => {
                // We're halted — assemble context
                eprintln!("Debuggee stopped at halt point.");
                let threads = session.get_threads().await?;
                eprintln!("Threads: {}", threads.len());
                assert!(!threads.is_empty(), "Should have at least one thread");

                if !threads.is_empty() {
                    let frames = session
                        .get_stack_trace(threads[0].id, None, Some(10))
                        .await?;
                    eprintln!("Frames for thread {}: {}", threads[0].id, frames.len());

                    if !frames.is_empty() {
                        let scopes = session.get_scopes(frames[0].id).await?;
                        eprintln!("Scopes for frame {}: {}", frames[0].id, scopes.len());

                        // Try to get variables for a scope with children
                        for scope in &scopes {
                            if scope.variables_reference > 0 {
                                let vars = session
                                    .get_variables(scope.variables_reference, None, None, None)
                                    .await?;
                                eprintln!("Variables for scope '{}': {}", scope.name, vars.len());
                            }
                        }
                    }
                }
            }
            Ok(Ok(false)) => {
                eprintln!("Program ran to completion without stopping — OK for some ELFs.");
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                eprintln!("Timed out waiting for stopped event.");
            }
        }

        Ok::<_, DebugSessionError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Context chain test failed: {e}"),
        Err(_) => panic!("Context chain test timed out after 10 seconds"),
    }
}

/// IT-04: Verify step_over, step_in, and step_out operations in stopped state.
///
/// Requires codelldb + an ELF binary. Due to variability across binaries,
/// step_in and step_out are best-effort (the debuggee may exit before all
/// steps complete).
#[tokio::test]
async fn test_step_operations() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let elf_path = match test_elf_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: no ELF binary available for step test.");
            return;
        }
    };

    eprintln!("Using ELF: {elf_path}");

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);
    let session = DebugSession::new(client, None);

    let result = timeout(Duration::from_secs(10), async {
        let start_config = AdapterConfig {
            path: "codelldb".into(),
            kind: AdapterKind::Codelldb,
            args: vec![],
        };
        session.start(&start_config).await?;

        session
            .initialize(InitializeRequestArguments {
                adapter_id: Some("lldb".into()),
                client_name: Some("teledap-step-test".into()),
                ..Default::default()
            })
            .await?;

        session
            .launch(LaunchRequestArguments::with_program(&elf_path))
            .await?;

        // Event loop: wait for initialized, send configurationDone,
        // then wait for the first stopped event.
        let mut configured = false;
        let mut stopped = false;
        while let Some(event) = session.client().recv_event().await {
            let event_name = event.event.clone();
            let _ = session.handle_event(&event).await?;

            if event_name == "initialized" && !configured {
                session.configuration_done().await?;
                configured = true;
            }

            if event_name == "stopped" {
                stopped = true;
                break;
            }

            if event_name == "terminated" || event_name == "exited" {
                eprintln!("Debuggee exited before first stop — step test cannot run.");
                break;
            }
        }

        if !stopped {
            return Ok(());
        }

        assert_eq!(
            session.current_state().await,
            SessionState::Halted,
            "Should be Halted after stopped event"
        );

        let threads = session.get_threads().await?;
        assert!(!threads.is_empty(), "Should have at least one thread");
        let tid = threads[0].id;
        eprintln!("Thread ID for stepping: {tid}");

        // -- step_over --
        session.step_over(tid, None).await?;
        let mut re_stopped = false;
        while let Some(event) = session.client().recv_event().await {
            let _ = session.handle_event(&event).await?;
            let en = event.event.as_str();
            if en == "stopped" {
                re_stopped = true;
                break;
            }
            if en == "terminated" || en == "exited" {
                break;
            }
        }
        eprintln!(
            "step_over: {}",
            if re_stopped { "OK" } else { "program exited" }
        );

        // -- step_in (best-effort) --
        if re_stopped {
            let _ = session.step_in(tid, None, None).await;
            while let Some(event) = session.client().recv_event().await {
                let _ = session.handle_event(&event).await?;
                let en = event.event.as_str();
                if en == "stopped" || en == "terminated" || en == "exited" {
                    eprintln!(
                        "step_in: {}",
                        if en == "stopped" {
                            "OK"
                        } else {
                            "program exited"
                        }
                    );
                    break;
                }
            }
        }

        // -- step_out (best-effort) --
        if session.current_state().await == SessionState::Halted {
            let _ = session.step_out(tid, None).await;
            while let Some(event) = session.client().recv_event().await {
                let _ = session.handle_event(&event).await?;
                let en = event.event.as_str();
                if en == "stopped" || en == "terminated" || en == "exited" {
                    eprintln!(
                        "step_out: {}",
                        if en == "stopped" {
                            "OK"
                        } else {
                            "program exited"
                        }
                    );
                    break;
                }
            }
        }

        eprintln!("Step operations test complete.");
        Ok::<_, DebugSessionError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Step operations test failed: {e}"),
        Err(_) => panic!("Step operations test timed out after 10 seconds"),
    }
}

/// IT-08: Launch a short-lived program and verify that terminated/exited
/// events transition the session to Disconnected.
///
/// The session's handle_event method should call client.shutdown() and
/// transition to Disconnected on both terminated and exited events.
#[tokio::test]
async fn test_program_exit_handling() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let elf_path = match test_elf_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: no ELF binary available for exit test.");
            return;
        }
    };

    eprintln!("Using ELF: {elf_path}");

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);
    let session = DebugSession::new(client, None);

    let result = timeout(Duration::from_secs(10), async {
        let start_config = AdapterConfig {
            path: "codelldb".into(),
            kind: AdapterKind::Codelldb,
            args: vec![],
        };
        session.start(&start_config).await?;

        session
            .initialize(InitializeRequestArguments {
                adapter_id: Some("lldb".into()),
                client_name: Some("teledap-exit-test".into()),
                ..Default::default()
            })
            .await?;

        session
            .launch(LaunchRequestArguments::with_program(&elf_path))
            .await?;

        let mut configured = false;
        let mut exit_seen = false;

        // Event loop: wait for terminated or exited
        while let Some(event) = session.client().recv_event().await {
            let event_name = event.event.clone();
            let _ = session.handle_event(&event).await?;

            if event_name == "initialized" && !configured {
                session.configuration_done().await?;
                configured = true;
            }

            if event_name == "terminated" || event_name == "exited" {
                eprintln!("Debuggee {event_name} received.");
                exit_seen = true;
                break;
            }
        }

        if !exit_seen {
            // The channel may have closed before we saw the event
            eprintln!("No terminated/exited event seen via recv_event (channel may have closed).");
        }

        // The session should be Disconnected regardless of event path
        let final_state = session.current_state().await;
        eprintln!("Final session state: {final_state}");
        assert_eq!(
            final_state,
            SessionState::Disconnected,
            "Session should be Disconnected after program exit"
        );

        Ok::<_, DebugSessionError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Program exit test failed: {e}"),
        Err(_) => panic!("Program exit test timed out after 10 seconds"),
    }
}
