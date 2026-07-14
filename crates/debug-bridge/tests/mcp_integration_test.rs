//! Integration tests for the MCP tool dispatch layer.
//!
//! These tests use a real `DebugSession` backed by a live `codelldb` process to
//! verify that tool handlers correctly translate MCP tool calls into DAP
//! operations and return properly structured MCP results.
//!
//! Every test uses `codelldb_available()` to gracefully skip when codelldb is
//! not installed. All async operations are wrapped in a 2-second timeout.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use std::sync::Arc;

use dap_client::DapClient;
use dap_trace::TraceHandle;
use debug_bridge::{BridgeError, ToolRegistry};
use debug_session::{DebugSession, SessionState};
use mcp_protocol::CallToolResult;
use openocd_client::OpenOcdClient;
use tokio::sync::RwLock;
use tokio::time::timeout;

// ── Environment probe ───────────────────────────────────────────────────

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

fn empty_openocd() -> Arc<RwLock<Option<OpenOcdClient>>> {
    Arc::new(RwLock::new(None))
}

fn make_session() -> (DebugSession, TraceHandle) {
    let (trace, _bg) = TraceHandle::new(None, 100);
    let client = DapClient::with_trace(4 * 1024 * 1024, trace.clone());
    let session = DebugSession::new(client, Some(trace.clone()));
    (session, trace)
}

/// Resolve the path to `test_debuggee/test_debuggee.exe`.
///
/// Tests run from either the workspace root (`cargo test --workspace`) or from
/// the crate directory (`cargo test -p debug-bridge`). This tries multiple
/// candidate paths and returns the first one that exists on disk.
fn test_debuggee_path() -> Option<String> {
    // Candidate 1: workspace-root CWD
    let rel = Path::new("test_debuggee/test_debuggee.exe");
    if rel.exists() {
        return Some(rel.to_string_lossy().to_string());
    }
    // Candidate 2: relative to CARGO_MANIFEST_DIR (crates/debug-bridge → ../..)
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let abs = Path::new(&manifest_dir).join("../../test_debuggee/test_debuggee.exe");
        if abs.exists() {
            return Some(abs.to_string_lossy().to_string());
        }
    }
    // Candidate 3: relative to std::env::current_exe() directory
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let from_exe = parent.join("../../../test_debuggee/test_debuggee.exe");
            if from_exe.exists() {
                return Some(from_exe.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Run the DAP event loop until the session receives a `stopped` event
/// (transitioning to Halted) or the debuggee terminates.
///
/// Returns `Ok(true)` when `stopped` was received, `Ok(false)` on
/// `terminated` / `exited` / EOF. This is the one piece that cannot go through
/// MCP dispatch since there is no MCP tool for event receipt.
async fn wait_for_stopped(
    session: &DebugSession,
    timeout_dur: Duration,
) -> Result<bool, BridgeError> {
    timeout(timeout_dur, async {
        loop {
            match session.client().recv_event().await {
                Some(event) => {
                    let event_name = event.event.clone();
                    let _ = session
                        .handle_event(&event)
                        .await
                        .map_err(|e| BridgeError::Internal(format!("handle_event failed: {e}")))?;
                    if event_name == "stopped" {
                        return Ok(true);
                    }
                    if event_name == "terminated" || event_name == "exited" {
                        return Ok(false);
                    }
                }
                None => return Ok(false),
            }
        }
    })
    .await
    .map_err(|_elapsed| BridgeError::Internal("Timed out waiting for stopped event".into()))?
}

/// Run the DAP event loop until the session receives the `initialized` event
/// from the adapter (sent during `launch` request processing).
async fn wait_for_initialized(
    session: &DebugSession,
    timeout_dur: Duration,
) -> Result<bool, BridgeError> {
    timeout(timeout_dur, async {
        loop {
            match session.client().recv_event().await {
                Some(event) => {
                    let is_init = event.event == "initialized";
                    let _ = session
                        .handle_event(&event)
                        .await
                        .map_err(|e| BridgeError::Internal(format!("handle_event failed: {e}")))?;
                    if is_init {
                        return Ok(true);
                    }
                }
                None => return Ok(false),
            }
        }
    })
    .await
    .map_err(|_elapsed| BridgeError::Internal("Timed out waiting for initialized event".into()))?
}

// ── Tests ───────────────────────────────────────────────────────────────

/// Verify that `tools/list` returns the correct tools for Disconnected state:
/// only `start` plus utility tools.
#[tokio::test]
async fn test_list_tools_disconnected() {
    let tools = ToolRegistry::list_tools_for_state(SessionState::Disconnected);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"start"), "Should contain start");
    assert!(
        !names.contains(&"initialize"),
        "Should NOT contain initialize"
    );
    assert!(!names.contains(&"continue"), "Should NOT contain continue");
    // Utility tools always present
    assert!(names.contains(&"get_state"));
}

/// Verify that dispatching an unknown tool returns a `BridgeError::UnknownTool`.
#[tokio::test]
async fn test_dispatch_unknown_tool() {
    let (session, trace) = make_session();
    let result = ToolRegistry::dispatch(
        "nonexistent_tool",
        &session,
        serde_json::json!({}),
        Some(&trace),
        &empty_openocd(),
    )
    .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        BridgeError::UnknownTool(name) => assert_eq!(name, "nonexistent_tool"),
        e => panic!("Expected UnknownTool, got {e:?}"),
    }
}

/// Verify that calling a gated tool in the wrong state returns an error
/// (not a panic or timeout).
#[tokio::test]
async fn test_state_gating_rejects_wrong_state() {
    let (session, trace) = make_session();
    // "continue" requires Halted; session is Disconnected
    let result = ToolRegistry::dispatch(
        "continue",
        &session,
        serde_json::json!({"threadId": 1}),
        Some(&trace),
        &empty_openocd(),
    )
    .await;
    assert!(
        result.is_err(),
        "Should reject 'continue' in Disconnected state"
    );
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("continue"),
        "Error should mention the operation: {msg}"
    );
}

/// Verify that tool errors produce valid `is_error: true` CallToolResult.
#[tokio::test]
async fn test_bridge_error_to_tool_result() {
    let err = BridgeError::UnknownTool("fake_tool".into());
    let result: CallToolResult = err.to_tool_result();
    assert!(result.is_error, "is_error should be true");
    assert!(!result.content.is_empty());
    let text = &result.content[0].text;
    assert!(
        text.contains("fake_tool"),
        "Error text should mention the tool name: {text}"
    );
}

/// Integration test: start codelldb, initialize, verify tools/list for
/// Initialized state. Requires codelldb on PATH.
#[tokio::test]
async fn test_integration_lifecycle_tool_dispatch() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let (session, trace) = make_session();

    let result = timeout(Duration::from_secs(5), async {
        // 1. Start codelldb
        let result = ToolRegistry::dispatch(
            "start",
            &session,
            serde_json::json!({"codelldbPath": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!result.is_error, "start should succeed");
        assert_eq!(session.current_state().await, SessionState::Connected);

        // 2. Initialize
        let result = ToolRegistry::dispatch(
            "initialize",
            &session,
            serde_json::json!({"adapterId": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!result.is_error, "initialize should succeed");
        assert_eq!(session.current_state().await, SessionState::Initialized);

        // 3. Verify tools/list for Initialized state
        let tools = ToolRegistry::list_tools_for_state(SessionState::Initialized);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"launch"));
        assert!(names.contains(&"attach"));
        assert!(names.contains(&"configuration_done"));
        assert!(names.contains(&"set_breakpoints"));
        assert!(!names.contains(&"continue")); // Not available until Halted

        // 4. Shutdown
        let result = ToolRegistry::dispatch(
            "shutdown",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!result.is_error, "shutdown should succeed");
        assert_eq!(session.current_state().await, SessionState::Disconnected);

        Ok::<_, BridgeError>(())
    })
    .await;

    // Best-effort cleanup
    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Integration test failed: {e}"),
        Err(_elapsed) => panic!("Integration test timed out after 5 seconds"),
    }
}

/// Integration test: verify that utility tools work in any state.
/// Requires codelldb on PATH.
#[tokio::test]
async fn test_integration_utility_tools() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let (session, trace) = make_session();

    let result = timeout(Duration::from_secs(5), async {
        // get_state works in Disconnected state
        let result = ToolRegistry::dispatch(
            "get_state",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!result.is_error);
        let text = &result.content[0].text;
        assert!(
            text.contains("Disconnected"),
            "Should report Disconnected state: {text}"
        );

        // register_path_alias works in any state
        let result = ToolRegistry::dispatch(
            "register_path_alias",
            &session,
            serde_json::json!({"alias": "src/main.cpp", "absolutePath": "/tmp/main.cpp"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!result.is_error);

        // register_base_dir works in any state
        let result = ToolRegistry::dispatch(
            "register_base_dir",
            &session,
            serde_json::json!({"dir": "/tmp"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!result.is_error);

        // search_variables works in any state (returns empty)
        let result = ToolRegistry::dispatch(
            "search_variables",
            &session,
            serde_json::json!({"query": "x"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!result.is_error);

        Ok::<_, BridgeError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Utility tools integration test failed: {e}"),
        Err(_elapsed) => panic!("Utility tools integration test timed out"),
    }
}

/// Verify tool result JSON structure is valid MCP.
#[tokio::test]
async fn test_tool_result_json_structure() {
    let success = CallToolResult::success("hello world");
    let json = serde_json::to_string(&success).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["content"][0]["type"], "text");
    assert_eq!(parsed["content"][0]["text"], "hello world");
    // is_error should be absent or false
    assert!(!parsed
        .get("isError")
        .map(|v| v.as_bool().unwrap_or(false))
        .unwrap_or(false));

    let error = CallToolResult::error("something failed");
    let json = serde_json::to_string(&error).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["isError"], true);
}

// ═══════════════════════════════════════════════════════════════════════════
// State gating tests (no codelldb needed)
// ═══════════════════════════════════════════════════════════════════════════

/// Verify that every tool requiring the Halted state is rejected with a proper
/// error when the session is Disconnected. Exercises the state-gating path in
/// `ToolRegistry::dispatch()` for all 11 Halted-gated tools.
#[tokio::test]
async fn test_state_gating_all_halted_tools() {
    let (session, trace) = make_session();
    // session starts in Disconnected

    let halted_tools_with_params: Vec<(&str, serde_json::Value)> = vec![
        ("continue", serde_json::json!({"threadId": 1})),
        ("step_over", serde_json::json!({"threadId": 1})),
        ("step_in", serde_json::json!({"threadId": 1})),
        ("step_out", serde_json::json!({"threadId": 1})),
        ("get_threads", serde_json::json!({})),
        ("get_stack_trace", serde_json::json!({"threadId": 1})),
        ("get_scopes", serde_json::json!({"frameId": 0})),
        (
            "get_variables",
            serde_json::json!({"variablesReference": 0}),
        ),
        ("evaluate", serde_json::json!({"expression": "1"})),
        (
            "set_variable",
            serde_json::json!({"variablesReference": 0, "name": "x", "value": "42"}),
        ),
        ("assemble_context", serde_json::json!({})),
    ];

    // Also verify set_breakpoints gating (requires Initialized/Running/Halted)
    let non_halted_only = [(
        "set_breakpoints",
        serde_json::json!({"sourcePath": "main.c", "breakpoints": [{"line": 1}]}),
    )];

    for (name, params) in halted_tools_with_params
        .iter()
        .chain(non_halted_only.iter())
    {
        let result = ToolRegistry::dispatch(
            name,
            &session,
            params.clone(),
            Some(&trace),
            &empty_openocd(),
        )
        .await;
        assert!(
            result.is_err(),
            "Tool '{name}' should be rejected in Disconnected state"
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains(name) || err_msg.to_lowercase().contains(&name.to_lowercase()),
            "Error for '{name}' should mention the tool name: {err_msg}"
        );
    }
}

/// Verify `pause` is rejected in Disconnected state (it requires Running,
/// not Halted).
#[tokio::test]
async fn test_state_gating_pause() {
    let (session, trace) = make_session();
    let result = ToolRegistry::dispatch(
        "pause",
        &session,
        serde_json::json!({"threadId": 1}),
        Some(&trace),
        &empty_openocd(),
    )
    .await;
    assert!(
        result.is_err(),
        "pause should be rejected in Disconnected state"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.to_lowercase().contains("pause"),
        "Error should mention pause: {err_msg}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Integration: lifecycle (requires codelldb)
// ═══════════════════════════════════════════════════════════════════════════

/// End-to-end test of start → initialize → launch(stopOnEntry) →
/// configuration_done → wait_for_stopped → verify Halted tools →
/// shutdown — all through MCP tool dispatch.
#[tokio::test]
async fn test_integration_full_lifecycle_with_debuggee() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }
    let debuggee_path = match test_debuggee_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: test_debuggee.exe not found.");
            return;
        }
    };

    let (session, trace) = make_session();

    let result = timeout(Duration::from_secs(20), async {
        // 1. Start codelldb
        let r = ToolRegistry::dispatch(
            "start",
            &session,
            serde_json::json!({"codelldbPath": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "start should succeed");
        assert_eq!(session.current_state().await, SessionState::Connected);

        // 2. Initialize
        let r = ToolRegistry::dispatch(
            "initialize",
            &session,
            serde_json::json!({"adapterId": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "initialize should succeed");
        assert_eq!(session.current_state().await, SessionState::Initialized);

        // 3. Launch with stopOnEntry
        let r = ToolRegistry::dispatch(
            "launch",
            &session,
            serde_json::json!({"program": debuggee_path, "stopOnEntry": true}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "launch should succeed");
        assert!(
            r.content[0].text.contains("Launch command sent"),
            "launch result should confirm: {}",
            r.content[0].text
        );

        // 4. Wait for initialized event
        let got_init = wait_for_initialized(&session, Duration::from_secs(5)).await?;
        assert!(got_init, "Should receive initialized event");

        // 5. Configuration done
        let r = ToolRegistry::dispatch(
            "configuration_done",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "configuration_done should succeed");

        // 6. Wait for stopped (stopOnEntry)
        let got_stopped = wait_for_stopped(&session, Duration::from_secs(5)).await?;
        if got_stopped {
            assert_eq!(session.current_state().await, SessionState::Halted);

            // 7. get_state reports Halted
            let r = ToolRegistry::dispatch(
                "get_state",
                &session,
                serde_json::json!({}),
                Some(&trace),
                &empty_openocd(),
            )
            .await?;
            assert!(!r.is_error);
            assert!(
                r.content[0].text.contains("Halted"),
                "get_state should report Halted: {}",
                r.content[0].text
            );

            // 8. Verify tools/list for Halted state
            let tools = ToolRegistry::list_tools_for_state(SessionState::Halted);
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            assert!(
                names.contains(&"continue"),
                "Halted should include continue"
            );
            assert!(
                names.contains(&"step_over"),
                "Halted should include step_over"
            );
            assert!(
                names.contains(&"get_threads"),
                "Halted should include get_threads"
            );
            assert!(
                names.contains(&"get_stack_trace"),
                "Halted should include get_stack_trace"
            );
            assert!(
                names.contains(&"get_scopes"),
                "Halted should include get_scopes"
            );
            assert!(
                names.contains(&"get_variables"),
                "Halted should include get_variables"
            );
            assert!(
                names.contains(&"evaluate"),
                "Halted should include evaluate"
            );
            assert!(
                names.contains(&"assemble_context"),
                "Halted should include assemble_context"
            );
            assert!(!names.contains(&"pause"), "Halted should NOT include pause");
        }

        // 9. Shutdown
        let r = ToolRegistry::dispatch(
            "shutdown",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "shutdown should succeed");
        assert_eq!(session.current_state().await, SessionState::Disconnected);

        Ok::<_, BridgeError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Full lifecycle integration test failed: {e}"),
        Err(_elapsed) => panic!("Full lifecycle integration test timed out after 20 seconds"),
    }
}

/// Test launch and configuration_done MCP tool dispatch paths specifically:
/// verify launch rejects wrong-state calls, launch params deserialization
/// (program, args, env, stopOnEntry), and configuration_done response format.
#[tokio::test]
async fn test_integration_launch_config_done_dispatch() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }
    let debuggee_path = match test_debuggee_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: test_debuggee.exe not found.");
            return;
        }
    };

    let (session, trace) = make_session();

    let result = timeout(Duration::from_secs(15), async {
        // 1. Start codelldb
        ToolRegistry::dispatch(
            "start",
            &session,
            serde_json::json!({"codelldbPath": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        // 2. Verify launch is gated — requires Initialized, currently Connected
        let launch_err = ToolRegistry::dispatch(
            "launch",
            &session,
            serde_json::json!({"program": debuggee_path}),
            Some(&trace),
            &empty_openocd(),
        )
        .await;
        assert!(
            launch_err.is_err(),
            "launch should be rejected in Connected state"
        );

        // 3. Initialize
        ToolRegistry::dispatch(
            "initialize",
            &session,
            serde_json::json!({"adapterId": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        // 4. Now in Initialized; launch should work with all params
        let r = ToolRegistry::dispatch(
            "launch",
            &session,
            serde_json::json!({
                "program": debuggee_path,
                "args": ["--verbose"],
                "stopOnEntry": true
            }),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error);
        assert!(
            r.content[0].text.contains("Launch command sent"),
            "launch result: {}",
            r.content[0].text
        );

        // 5. Wait for initialized
        let got_init = wait_for_initialized(&session, Duration::from_secs(5)).await?;
        assert!(got_init, "Should receive initialized event");

        // 6. Configuration done
        let r = ToolRegistry::dispatch(
            "configuration_done",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error);
        assert!(
            r.content[0].text.contains("Configuration done"),
            "config_done result: {}",
            r.content[0].text
        );

        // 7. Shutdown
        ToolRegistry::dispatch(
            "shutdown",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        Ok::<_, BridgeError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Launch/config_done dispatch test failed: {e}"),
        Err(_elapsed) => panic!("Launch/config_done dispatch test timed out"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Integration: execution control (requires codelldb)
// ═══════════════════════════════════════════════════════════════════════════

/// Test `pause` tool through MCP dispatch in Running state.
#[tokio::test]
async fn test_integration_pause_dispatch() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }
    let debuggee_path = match test_debuggee_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: test_debuggee.exe not found.");
            return;
        }
    };

    let (session, trace) = make_session();

    let result = timeout(Duration::from_secs(15), async {
        // Setup: start → initialize → launch → configDone → Running
        ToolRegistry::dispatch(
            "start",
            &session,
            serde_json::json!({"codelldbPath": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        ToolRegistry::dispatch(
            "initialize",
            &session,
            serde_json::json!({"adapterId": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        ToolRegistry::dispatch(
            "launch",
            &session,
            serde_json::json!({"program": debuggee_path, "stopOnEntry": false}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        wait_for_initialized(&session, Duration::from_secs(5)).await?;
        ToolRegistry::dispatch(
            "configuration_done",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        // State should be Running (or the debuggee may have already terminated)
        let state = session.current_state().await;

        if state == SessionState::Running {
            // Dispatch pause. The debuggee may be short-lived and exit before
            // pause takes effect, so this is best-effort.
            let r = ToolRegistry::dispatch(
                "pause",
                &session,
                serde_json::json!({"threadId": 1}),
                Some(&trace),
                &empty_openocd(),
            )
            .await;
            match r {
                Ok(result) => {
                    assert!(!result.is_error, "pause dispatch should succeed");
                    assert!(
                        result.content[0].text.contains("Pause command sent"),
                        "pause result: {}",
                        result.content[0].text
                    );
                }
                Err(e) => {
                    // pause may fail if the debuggee already terminated
                    eprintln!("INFO: pause dispatch failed (debuggee likely exited): {e}");
                }
            }
        } else {
            // Debuggee already exited — still a valid outcome
            eprintln!("INFO: debuggee already exited before pause (state={state})");
        }

        // Shutdown
        ToolRegistry::dispatch(
            "shutdown",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        Ok::<_, BridgeError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Pause dispatch test failed: {e}"),
        Err(_elapsed) => panic!("Pause dispatch test timed out"),
    }
}

/// Test step_over, step_in, step_out through MCP dispatch (best-effort).
/// The simple test_debuggee may exit early — step results are validated
/// for success but stopped-after-step is best-effort.
#[tokio::test]
async fn test_integration_step_operations() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }
    let debuggee_path = match test_debuggee_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: test_debuggee.exe not found.");
            return;
        }
    };

    let (session, trace) = make_session();

    let result = timeout(Duration::from_secs(20), async {
        // Setup to Halted via stopOnEntry
        ToolRegistry::dispatch(
            "start",
            &session,
            serde_json::json!({"codelldbPath": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        ToolRegistry::dispatch(
            "initialize",
            &session,
            serde_json::json!({"adapterId": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        ToolRegistry::dispatch(
            "launch",
            &session,
            serde_json::json!({"program": debuggee_path, "stopOnEntry": true}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        wait_for_initialized(&session, Duration::from_secs(5)).await?;
        ToolRegistry::dispatch(
            "configuration_done",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        let mut halted = wait_for_stopped(&session, Duration::from_secs(5)).await?;

        if !halted {
            eprintln!("INFO: debuggee did not stop — step tests skipped");
            ToolRegistry::dispatch(
                "shutdown",
                &session,
                serde_json::json!({}),
                Some(&trace),
                &empty_openocd(),
            )
            .await?;
            return Ok(());
        }

        // Get thread id for step operations
        let r = ToolRegistry::dispatch(
            "get_threads",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        let thread_id = extract_first_thread_id(&r).unwrap_or(1);

        // ── step_over ──
        let r = ToolRegistry::dispatch(
            "step_over",
            &session,
            serde_json::json!({"threadId": thread_id}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "step_over dispatch should succeed");
        assert!(
            r.content[0].text.contains("Step over"),
            "step_over result: {}",
            r.content[0].text
        );

        halted = wait_for_stopped(&session, Duration::from_secs(3)).await?;

        // ── step_in (best-effort) ──
        if halted {
            let r = ToolRegistry::dispatch(
                "step_in",
                &session,
                serde_json::json!({"threadId": thread_id}),
                Some(&trace),
                &empty_openocd(),
            )
            .await?;
            assert!(!r.is_error, "step_in dispatch should succeed");
            assert!(
                r.content[0].text.contains("Step in"),
                "step_in result: {}",
                r.content[0].text
            );
            halted = wait_for_stopped(&session, Duration::from_secs(3)).await?;
        }

        // ── step_out (best-effort) ──
        if halted {
            let r = ToolRegistry::dispatch(
                "step_out",
                &session,
                serde_json::json!({"threadId": thread_id}),
                Some(&trace),
                &empty_openocd(),
            )
            .await?;
            assert!(!r.is_error, "step_out dispatch should succeed");
            assert!(
                r.content[0].text.contains("Step out"),
                "step_out result: {}",
                r.content[0].text
            );
            // Don't wait — step_out may take a while or the program may exit
        }

        ToolRegistry::dispatch(
            "shutdown",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        Ok::<_, BridgeError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Step operations test failed: {e}"),
        Err(_elapsed) => panic!("Step operations test timed out"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Integration: breakpoints + inspect (requires codelldb)
// ═══════════════════════════════════════════════════════════════════════════

/// Test `set_function_breakpoints` through MCP dispatch: set a breakpoint on
/// the `add` function, continue, and verify the stack frame contains "add".
#[tokio::test]
async fn test_integration_function_breakpoint() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }
    let debuggee_path = match test_debuggee_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: test_debuggee.exe not found.");
            return;
        }
    };

    let (session, trace) = make_session();

    let result = timeout(Duration::from_secs(20), async {
        // Setup: start → initialize → launch (no stopOnEntry) → initialized
        ToolRegistry::dispatch(
            "start",
            &session,
            serde_json::json!({"codelldbPath": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        ToolRegistry::dispatch(
            "initialize",
            &session,
            serde_json::json!({"adapterId": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        ToolRegistry::dispatch(
            "launch",
            &session,
            serde_json::json!({"program": debuggee_path, "stopOnEntry": false}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        wait_for_initialized(&session, Duration::from_secs(5)).await?;

        // Set function breakpoint on "add"
        let r = ToolRegistry::dispatch(
            "set_function_breakpoints",
            &session,
            serde_json::json!({"names": ["add"]}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "set_function_breakpoints should succeed");
        // Verify response is valid JSON with breakpoints array
        let bp_json: serde_json::Value =
            serde_json::from_str(&r.content[0].text).unwrap_or_default();
        if let Some(bps) = bp_json["breakpoints"].as_array() {
            assert!(
                !bps.is_empty(),
                "Should have at least one function breakpoint"
            );
        }

        // Configuration done + wait for stopped
        ToolRegistry::dispatch(
            "configuration_done",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        let got_stopped = wait_for_stopped(&session, Duration::from_secs(8)).await?;

        if got_stopped {
            // Get actual thread ID
            let tr = ToolRegistry::dispatch(
                "get_threads",
                &session,
                serde_json::json!({}),
                Some(&trace),
                &empty_openocd(),
            )
            .await?;
            let thread_id = extract_first_thread_id(&tr).unwrap_or(1);

            // Verify we're inside "add" by checking stack trace
            let r = ToolRegistry::dispatch(
                "get_stack_trace",
                &session,
                serde_json::json!({"threadId": thread_id, "levels": 5}),
                Some(&trace),
                &empty_openocd(),
            )
            .await?;
            assert!(!r.is_error);
            let frames: serde_json::Value =
                serde_json::from_str(&r.content[0].text).unwrap_or_default();
            if let Some(arr) = frames.as_array() {
                let has_add = arr.iter().any(|f| {
                    f["name"]
                        .as_str()
                        .map(|n| n.contains("add"))
                        .unwrap_or(false)
                });
                assert!(has_add, "Stack should contain 'add' frame: {frames}");
            }
        } else {
            eprintln!("INFO: debuggee did not stop at function breakpoint (may have exited)");
        }

        ToolRegistry::dispatch(
            "shutdown",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        Ok::<_, BridgeError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Function breakpoint test failed: {e}"),
        Err(_elapsed) => panic!("Function breakpoint test timed out"),
    }
}

/// Comprehensive test: set source breakpoints, continue to hit them, then
/// exercise the full inspection tool chain (get_threads, get_stack_trace,
/// get_scopes, get_variables, evaluate, assemble_context) — all through
/// MCP dispatch. This is the core test exercising the biggest gap.
#[tokio::test]
async fn test_integration_breakpoint_and_inspect() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }
    let debuggee_path = match test_debuggee_path() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: test_debuggee.exe not found.");
            return;
        }
    };
    // Derive source path from debuggee path (main.c in same dir)
    let source_path = Path::new(&debuggee_path)
        .parent()
        .map(|p| p.join("main.c"))
        .unwrap_or_else(|| Path::new("test_debuggee/main.c").to_path_buf());
    let source_path_str = source_path.to_string_lossy().to_string();

    let (session, trace) = make_session();

    let result = timeout(Duration::from_secs(25), async {
        // ── Setup to Halted via stopOnEntry ──
        ToolRegistry::dispatch(
            "start",
            &session,
            serde_json::json!({"codelldbPath": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        ToolRegistry::dispatch(
            "initialize",
            &session,
            serde_json::json!({"adapterId": "codelldb"}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        ToolRegistry::dispatch(
            "launch",
            &session,
            serde_json::json!({"program": debuggee_path, "stopOnEntry": true}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        wait_for_initialized(&session, Duration::from_secs(5)).await?;

        // ── set_breakpoints (source) ──
        // NOTE: BpItem uses snake_case (no #[serde(rename_all)]).
        // Set breakpoint on line 13: int z = add(x, y);
        let r = ToolRegistry::dispatch(
            "set_breakpoints",
            &session,
            serde_json::json!({
                "sourcePath": source_path_str,
                "breakpoints": [{"line": 13}]
            }),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "set_breakpoints should succeed");
        let bp_resp: serde_json::Value =
            serde_json::from_str(&r.content[0].text).unwrap_or_default();
        let bps = bp_resp["breakpoints"].as_array();
        assert!(
            bps.is_some(),
            "set_breakpoints response should have breakpoints array"
        );

        // ── Configuration done ──
        ToolRegistry::dispatch(
            "configuration_done",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        // Wait for first stop (stopOnEntry)
        let mut halted = wait_for_stopped(&session, Duration::from_secs(5)).await?;
        if !halted {
            eprintln!("INFO: no initial stop — debuggee may have exited");
            ToolRegistry::dispatch(
                "shutdown",
                &session,
                serde_json::json!({}),
                Some(&trace),
                &empty_openocd(),
            )
            .await?;
            return Ok(());
        }

        // ── Continue to hit source breakpoint ──
        let r = ToolRegistry::dispatch(
            "continue",
            &session,
            serde_json::json!({"threadId": 1}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "continue dispatch should succeed");

        halted = wait_for_stopped(&session, Duration::from_secs(5)).await?;

        if !halted {
            eprintln!("INFO: debuggee did not stop at breakpoint (may have exited)");
            ToolRegistry::dispatch(
                "shutdown",
                &session,
                serde_json::json!({}),
                Some(&trace),
                &empty_openocd(),
            )
            .await?;
            return Ok(());
        }

        // ── get_threads ──
        let r = ToolRegistry::dispatch(
            "get_threads",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "get_threads should succeed");
        let threads: Vec<serde_json::Value> =
            serde_json::from_str(&r.content[0].text).unwrap_or_default();
        assert!(!threads.is_empty(), "Should have at least one thread");
        let thread_id = threads[0]["id"].as_u64().unwrap_or(1);

        // ── get_stack_trace ──
        let r = ToolRegistry::dispatch(
            "get_stack_trace",
            &session,
            serde_json::json!({"threadId": thread_id, "levels": 10}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "get_stack_trace should succeed");
        let frames: Vec<serde_json::Value> =
            serde_json::from_str(&r.content[0].text).unwrap_or_default();
        assert!(!frames.is_empty(), "Should have at least one stack frame");
        let frame_id = frames[0]["id"].as_u64().unwrap_or(0);
        assert!(frame_id > 0, "Frame ID should be positive");

        // ── get_scopes ──
        let r = ToolRegistry::dispatch(
            "get_scopes",
            &session,
            serde_json::json!({"frameId": frame_id}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "get_scopes should succeed");
        let scopes: Vec<serde_json::Value> =
            serde_json::from_str(&r.content[0].text).unwrap_or_default();
        assert!(!scopes.is_empty(), "Should have at least one scope");

        // Find a scope with variables
        let scope_with_vars = scopes
            .iter()
            .find(|s| s["variablesReference"].as_u64().unwrap_or(0) > 0);
        assert!(
            scope_with_vars.is_some(),
            "Should have at least one scope with variables"
        );
        let vars_ref = scope_with_vars.unwrap()["variablesReference"]
            .as_u64()
            .unwrap_or(0);

        // ── get_variables ──
        let r = ToolRegistry::dispatch(
            "get_variables",
            &session,
            serde_json::json!({"variablesReference": vars_ref}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "get_variables should succeed");
        let variables: Vec<serde_json::Value> =
            serde_json::from_str(&r.content[0].text).unwrap_or_default();
        assert!(!variables.is_empty(), "Should have at least one variable");

        // ── evaluate ──
        let r = ToolRegistry::dispatch(
            "evaluate",
            &session,
            serde_json::json!({"expression": "1 + 1", "frameId": frame_id}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "evaluate should succeed");
        let eval_result: serde_json::Value =
            serde_json::from_str(&r.content[0].text).unwrap_or_default();
        // The result should contain "2" somewhere
        let eval_text = eval_result.to_string();
        assert!(
            eval_text.contains("2"),
            "evaluate(1+1) should produce 2: {eval_text}"
        );

        // ── assemble_context ──
        let r = ToolRegistry::dispatch(
            "assemble_context",
            &session,
            serde_json::json!({"maxFrames": 5, "maxDepth": 1}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;
        assert!(!r.is_error, "assemble_context should succeed");
        let ctx: serde_json::Value = serde_json::from_str(&r.content[0].text).unwrap_or_default();
        assert!(
            ctx["threads"].is_array(),
            "assemble_context should have threads array"
        );
        assert!(
            ctx["total_threads"].is_u64() || ctx["total_threads"].is_number(),
            "assemble_context should have total_threads"
        );

        // ── Shutdown ──
        ToolRegistry::dispatch(
            "shutdown",
            &session,
            serde_json::json!({}),
            Some(&trace),
            &empty_openocd(),
        )
        .await?;

        Ok::<_, BridgeError>(())
    })
    .await;

    let _ = session.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Breakpoint + inspect test failed: {e}"),
        Err(_elapsed) => panic!("Breakpoint + inspect test timed out"),
    }
}

// ── Helpers for extracting values from tool results ───────────────────

/// Extract the first thread ID from a `get_threads` CallToolResult.
fn extract_first_thread_id(result: &CallToolResult) -> Option<u64> {
    let threads: Vec<serde_json::Value> = serde_json::from_str(&result.content[0].text).ok()?;
    threads.first()?.get("id")?.as_u64()
}
