//! Integration tests for the MCP tool dispatch layer.
//!
//! These tests use a real `DebugSession` backed by a live `codelldb` process to
//! verify that tool handlers correctly translate MCP tool calls into DAP
//! operations and return properly structured MCP results.
//!
//! Every test uses `codelldb_available()` to gracefully skip when codelldb is
//! not installed. All async operations are wrapped in a 2-second timeout.

use std::process::Command;
use std::time::Duration;

use dap_client::DapClient;
use dap_trace::TraceHandle;
use debug_bridge::{BridgeError, ToolRegistry};
use debug_session::{DebugSession, SessionState};
use mcp_protocol::CallToolResult;
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

fn make_session() -> (DebugSession, TraceHandle) {
    let (trace, _bg) = TraceHandle::new(None, 100);
    let client = DapClient::with_trace(4 * 1024 * 1024, trace.clone());
    let session = DebugSession::new(client, Some(trace.clone()));
    (session, trace)
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
        let result =
            ToolRegistry::dispatch("shutdown", &session, serde_json::json!({}), Some(&trace))
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
        let result =
            ToolRegistry::dispatch("get_state", &session, serde_json::json!({}), Some(&trace))
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
        )
        .await?;
        assert!(!result.is_error);

        // register_base_dir works in any state
        let result = ToolRegistry::dispatch(
            "register_base_dir",
            &session,
            serde_json::json!({"dir": "/tmp"}),
            Some(&trace),
        )
        .await?;
        assert!(!result.is_error);

        // search_variables works in any state (returns empty)
        let result = ToolRegistry::dispatch(
            "search_variables",
            &session,
            serde_json::json!({"query": "x"}),
            Some(&trace),
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
