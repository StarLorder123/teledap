//! Integration tests for the DAP client.
//!
//! These tests spawn a real `codelldb` process and verify end-to-end DAP
//! communication: handshake, request/response dispatch, event streaming, and
//! process lifecycle management.
//!
//! Every test that requires codelldb uses a `codelldb_available()` guard that
//! gracefully skips when codelldb is not installed. This means `cargo test
//! --workspace` will never fail due to a missing codelldb binary.
//!
//! All async operations are wrapped in a 2-second timeout to prevent the test
//! suite from hanging indefinitely if codelldb stalls.

use std::process::Command;
use std::time::Duration;

use dap_client::{DapClient, DapClientError, DEFAULT_MAX_FRAME_SIZE};
use dap_types::requests::{
    ConfigurationDoneRequest, DisconnectArguments, DisconnectRequest, InitializeRequest,
    InitializeRequestArguments, NoArguments,
};
use tokio::time::timeout;

// ── Environment probe ──────────────────────────────────────────────

/// Returns `true` if `codelldb` can be spawned (and immediately killed).
///
/// This is the canary for all integration tests. On systems without codelldb
/// installed, every test that requires a live process will print a skip message
/// and return early instead of failing.
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

// ── Tests ──────────────────────────────────────────────────────────

/// Always-passing informational test that reports whether codelldb is
/// available on this machine.
#[tokio::test]
async fn test_integration_environment_probe() {
    if codelldb_available() {
        eprintln!("codelldb found on PATH — integration tests will run.");
    } else {
        eprintln!("SKIP: codelldb not found on PATH. Integration tests will be skipped.");
    }
    // This test always passes; it exists to surface the probe result in test
    // output so the developer knows whether integration tests are actually
    // exercising anything.
}

/// End-to-end initialize handshake with a real codelldb process.
///
/// Spawns codelldb, sends an `initialize` request, asserts that the returned
/// capabilities are non-trivial, then disconnects and shuts down cleanly.
#[tokio::test]
async fn test_integration_initialize_handshake() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);

    let result = timeout(Duration::from_secs(2), async {
        client.start("codelldb").await?;

        // Send initialize request
        let caps = client
            .send_request::<InitializeRequest>(InitializeRequestArguments {
                adapter_id: Some("codelldb".into()),
                client_name: Some("teledap-test".into()),
                ..Default::default()
            })
            .await?;

        // codelldb always returns at least one of these capabilities
        let has_caps = caps.supports_configuration_done_request.is_some()
            || caps.supports_function_breakpoints.is_some()
            || caps.supports_conditional_breakpoints.is_some();
        assert!(has_caps, "Initialize should return non-empty capabilities");

        // Clean disconnect — best effort (don't fail the test if this errors)
        let _ = client
            .send_request::<DisconnectRequest>(DisconnectArguments {
                terminate_debuggee: Some(false),
                ..Default::default()
            })
            .await;

        Ok::<_, DapClientError>(())
    })
    .await;

    // Always attempt shutdown regardless of test outcome
    let _ = client.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Initialize handshake failed: {}", e),
        Err(_elapsed) => panic!("Initialize handshake timed out after 2 seconds"),
    }
}

/// Verify that `shutdown()` reliably kills the child process and that the
/// client correctly reports its own state.
///
/// After shutdown:
/// - `is_running()` returns `false`.
/// - `send_request()` returns `Err(DapClientError::NotConnected)`.
/// - Calling `shutdown()` a second time is idempotent (no panic).
#[tokio::test]
async fn test_integration_process_cleanup() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);

    let result = timeout(Duration::from_secs(2), async {
        client.start("codelldb").await?;
        assert!(
            client.is_running().await,
            "Client should report running after start"
        );

        // Complete initialize so shutdown has a clean disconnect path
        let _ = client
            .send_request::<InitializeRequest>(InitializeRequestArguments {
                adapter_id: Some("codelldb".into()),
                ..Default::default()
            })
            .await;

        // First shutdown
        client.shutdown().await?;

        // Verify state after shutdown
        assert!(
            !client.is_running().await,
            "Client should report not running after shutdown"
        );

        // Sending a request after shutdown must fail
        let post_shutdown = client
            .send_request::<InitializeRequest>(InitializeRequestArguments::default())
            .await;
        assert!(
            post_shutdown.is_err(),
            "send_request after shutdown should return Err"
        );

        // Double-shutdown must be idempotent (no panic)
        client.shutdown().await?;

        Ok::<_, DapClientError>(())
    })
    .await;

    // Guard: ensure cleanup even if the timeout fired mid-test
    let _ = client.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Process cleanup test failed: {}", e),
        Err(_) => panic!("Process cleanup test timed out after 2 seconds"),
    }
}

/// Full DAP session lifecycle without needing an ELF binary.
///
/// Sequence: start → initialize → wait for `initialized` event →
/// configurationDone → disconnect → shutdown.
///
/// The `initialized` event is treated leniently — some codelldb versions may
/// not emit it without a launch configuration, which is acceptable.
#[tokio::test]
async fn test_integration_full_lifecycle() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);

    let result = timeout(Duration::from_secs(2), async {
        client.start("codelldb").await?;

        // Step 1: Initialize
        let caps = client
            .send_request::<InitializeRequest>(InitializeRequestArguments {
                adapter_id: Some("codelldb".into()),
                client_name: Some("teledap-lifecycle-test".into()),
                ..Default::default()
            })
            .await?;
        let cap_count = serde_json::to_value(&caps)
            .ok()
            .and_then(|v| v.as_object().map(|o| o.len()))
            .unwrap_or(0);
        eprintln!("Initialize OK, {} capability fields present.", cap_count);

        // Step 2: Wait for initialized event (lenient — may not arrive
        // without a launch config)
        let initialized_received = timeout(Duration::from_millis(1500), async {
            loop {
                match client.recv_event().await {
                    Some(event) if event.event == "initialized" => break true,
                    Some(_) => continue,
                    None => break false,
                }
            }
        })
        .await
        .unwrap_or(false);

        eprintln!(
            "Initialized event: {}",
            if initialized_received {
                "received"
            } else {
                "NOT received (may be OK for some codelldb versions)"
            }
        );

        // Step 3: Configuration done (only if initialized was received)
        if initialized_received {
            client
                .send_request::<ConfigurationDoneRequest>(NoArguments {})
                .await?;
            eprintln!("ConfigurationDone OK");
        }

        // Step 4: Disconnect
        client
            .send_request::<DisconnectRequest>(DisconnectArguments {
                terminate_debuggee: Some(false),
                ..Default::default()
            })
            .await?;
        eprintln!("Disconnect OK");

        Ok::<_, DapClientError>(())
    })
    .await;

    // Always shutdown
    let _ = client.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Full lifecycle test failed: {}", e),
        Err(_) => panic!("Full lifecycle test timed out after 2 seconds"),
    }
}

/// CL-I06: Verify that calling start() a second time is rejected.
///
/// The client must guard against double-start because spawning a second
/// codelldb process while one is already running would leak the first process.
#[tokio::test]
async fn test_integration_duplicate_start_rejection() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);

    let result = timeout(Duration::from_secs(2), async {
        // First start succeeds
        client.start("codelldb").await?;
        assert!(
            client.is_running().await,
            "Client should be running after first start"
        );

        // Second start must fail with SpawnFailed
        let err = client.start("codelldb").await.unwrap_err();
        assert!(
            matches!(err, DapClientError::SpawnFailed(_)),
            "Expected SpawnFailed, got: {:?}",
            err
        );
        let msg = err.to_string();
        assert!(
            msg.contains("already running"),
            "Error message should mention 'already running': {}",
            msg
        );

        Ok::<_, DapClientError>(())
    })
    .await;

    let _ = client.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("Duplicate start rejection test failed: {e}"),
        Err(_elapsed) => panic!("Duplicate start rejection test timed out after 2 seconds"),
    }
}

/// CL-I07: Verify that send_request_nb sends a request without blocking.
///
/// The non-blocking send is important for fire-and-forget requests like
/// `configurationDone` where the response has no meaningful body, and we
/// don't want to waste a oneshot channel.
#[tokio::test]
async fn test_integration_send_request_nb() {
    if !codelldb_available() {
        eprintln!("SKIP: codelldb not found on PATH.");
        return;
    }

    let client = DapClient::new(DEFAULT_MAX_FRAME_SIZE);

    let result = timeout(Duration::from_secs(2), async {
        client.start("codelldb").await?;

        // Initialize first so the adapter is ready to accept configurationDone
        let _caps = client
            .send_request::<InitializeRequest>(InitializeRequestArguments {
                adapter_id: Some("codelldb".into()),
                client_name: Some("teledap-nb-test".into()),
                ..Default::default()
            })
            .await?;

        // send_request_nb should return Ok(()) immediately without blocking
        client
            .send_request_nb::<ConfigurationDoneRequest>(NoArguments {})
            .await?;

        Ok::<_, DapClientError>(())
    })
    .await;

    let _ = client.shutdown().await;

    match result {
        Ok(Ok(())) => { /* success */ }
        Ok(Err(e)) => panic!("send_request_nb test failed: {e}"),
        Err(_elapsed) => panic!("send_request_nb test timed out after 2 seconds"),
    }
}
