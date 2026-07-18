//! OpenOCD management tool handlers.
//!
//! These handlers are independent of the DAP debug session — OpenOCD is an
//! optional extension composed alongside `DebugSession`, not embedded within it.

use std::sync::Arc;

use debug_session::DebugSession;
use mcp_protocol::CallToolResult;
use openocd_client::OpenOcdClient;
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::error::BridgeError;

fn text_result(value: &impl serde::Serialize) -> Result<CallToolResult, BridgeError> {
    Ok(CallToolResult::success_json(value)?)
}

fn ok_result(msg: impl Into<String>) -> Result<CallToolResult, BridgeError> {
    Ok(CallToolResult::success(msg))
}

/// Helper: get a reference to the running OpenOCD client, or return an error.
fn require_openocd(
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
) -> Result<OpenOcdClientRef<'_>, BridgeError> {
    let guard = openocd
        .try_read()
        .map_err(|_| BridgeError::Internal("OpenOCD is busy, try again later.".into()))?;
    if guard.is_none() {
        return Err(BridgeError::Internal(
            "OpenOCD is not running. Call openocd_start first.".into(),
        ));
    }
    Ok(OpenOcdClientRef { _guard: guard })
}

/// RAII wrapper to hold the read lock while accessing the client.
struct OpenOcdClientRef<'a> {
    _guard: tokio::sync::RwLockReadGuard<'a, Option<OpenOcdClient>>,
}

impl OpenOcdClientRef<'_> {
    fn client(&self) -> &OpenOcdClient {
        self._guard.as_ref().unwrap()
    }
}

// ── Handler params ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenocdStartParams {
    openocd_path: String,
    config_files: Vec<String>,
    #[serde(default)]
    extra_args: Option<Vec<String>>,
    #[serde(default)]
    log_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenocdOutputParams {
    #[serde(default = "default_lines")]
    lines: usize,
    #[serde(default)]
    include_stderr: bool,
}

fn default_lines() -> usize {
    100
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenocdSendParams {
    command: String,
    #[serde(default = "default_timeout")]
    timeout_ms: u64,
}

fn default_timeout() -> u64 {
    5000
}

// ── Handlers ───────────────────────────────────────────────────────────

pub async fn handle_openocd_start(
    _session: &DebugSession,
    params: serde_json::Value,
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
) -> Result<CallToolResult, BridgeError> {
    let p: OpenocdStartParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "openocd_start".into(),
            message: e.to_string(),
        })?;

    // Check if already running
    {
        let guard = openocd.read().await;
        if guard.is_some() {
            return Err(BridgeError::Internal(
                "OpenOCD is already running. Call openocd_stop first to restart.".into(),
            ));
        }
    }

    let client = OpenOcdClient::new();
    client
        .start(
            &p.openocd_path,
            &p.config_files,
            &p.extra_args.unwrap_or_default(),
            p.log_dir.as_deref(),
        )
        .await
        .map_err(|e| BridgeError::Internal(format!("Failed to start OpenOCD: {e}")))?;

    *openocd.write().await = Some(client);

    let log_info = p
        .log_dir
        .as_ref()
        .map(|d| format!(" Output logged to {d}"))
        .unwrap_or_default();
    ok_result(format!(
        "OpenOCD started with config(s): {}.{}",
        p.config_files.join(", "),
        log_info
    ))
}

pub async fn handle_openocd_stop(
    _session: &DebugSession,
    _params: serde_json::Value,
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
) -> Result<CallToolResult, BridgeError> {
    let mut guard = openocd.write().await;
    let client = guard
        .take()
        .ok_or_else(|| BridgeError::Internal("OpenOCD is not running.".into()))?;

    client
        .shutdown()
        .await
        .map_err(|e| BridgeError::Internal(format!("Failed to stop OpenOCD: {e}")))?;

    ok_result("OpenOCD stopped.")
}

pub async fn handle_openocd_status(
    _session: &DebugSession,
    _params: serde_json::Value,
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
) -> Result<CallToolResult, BridgeError> {
    let ocd = require_openocd(openocd)?;
    let status = ocd.client().status().await;
    text_result(&status)
}

pub async fn handle_openocd_output(
    _session: &DebugSession,
    params: serde_json::Value,
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
) -> Result<CallToolResult, BridgeError> {
    let p: OpenocdOutputParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "openocd_output".into(),
            message: e.to_string(),
        })?;

    let ocd = require_openocd(openocd)?;

    let stdout_lines = ocd
        .client()
        .read_output(p.lines)
        .await
        .map_err(|e| BridgeError::Internal(format!("Failed to read output: {e}")))?;

    let mut result = serde_json::json!({
        "stdout_lines": stdout_lines,
    });

    if p.include_stderr {
        let stderr_lines = ocd
            .client()
            .read_errors(p.lines)
            .await
            .map_err(|e| BridgeError::Internal(format!("Failed to read errors: {e}")))?;
        result["stderr_lines"] = serde_json::json!(stderr_lines);
    }

    text_result(&result)
}

pub async fn handle_openocd_send(
    _session: &DebugSession,
    params: serde_json::Value,
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
) -> Result<CallToolResult, BridgeError> {
    let p: OpenocdSendParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "openocd_send".into(),
            message: e.to_string(),
        })?;

    let ocd = require_openocd(openocd)?;

    let response = ocd
        .client()
        .send_command(&p.command, p.timeout_ms)
        .await
        .map_err(|e| BridgeError::Internal(format!("Command failed: {e}")))?;

    text_result(&serde_json::json!({
        "command": p.command,
        "response": response,
    }))
}
