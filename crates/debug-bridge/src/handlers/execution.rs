//! Execution control tool handlers: continue, step_over, step_in,
//! step_out, pause.

use debug_session::DebugSession;
use mcp_protocol::CallToolResult;
use serde::Deserialize;

use crate::error::BridgeError;

fn text_result(value: &impl serde::Serialize) -> Result<CallToolResult, BridgeError> {
    Ok(CallToolResult::success_json(value)?)
}

fn ok_result(msg: impl Into<String>) -> Result<CallToolResult, BridgeError> {
    Ok(CallToolResult::success(msg))
}

// ── continue ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContinueParams {
    thread_id: u64,
    #[serde(default)]
    single_thread: Option<bool>,
}

pub async fn handle_continue(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: ContinueParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "continue".into(),
            message: e.to_string(),
        })?;

    let result = session
        .continue_execution(p.thread_id, p.single_thread)
        .await?;
    text_result(&result)
}

// ── step_over ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepParams {
    thread_id: u64,
    #[serde(default)]
    single_thread: Option<bool>,
}

pub async fn handle_step_over(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: StepParams = serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
        tool: "step_over".into(),
        message: e.to_string(),
    })?;

    session.step_over(p.thread_id, p.single_thread).await?;
    ok_result("Step over executed. Waiting for stopped event.")
}

// ── step_in ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepInParams {
    thread_id: u64,
    #[serde(default)]
    single_thread: Option<bool>,
    #[serde(default)]
    target_id: Option<u64>,
}

pub async fn handle_step_in(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: StepInParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "step_in".into(),
            message: e.to_string(),
        })?;

    session
        .step_in(p.thread_id, p.single_thread, p.target_id)
        .await?;
    ok_result("Step in executed. Waiting for stopped event.")
}

// ── step_out ────────────────────────────────────────────────────────────

pub async fn handle_step_out(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: StepParams = serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
        tool: "step_out".into(),
        message: e.to_string(),
    })?;

    session.step_out(p.thread_id, p.single_thread).await?;
    ok_result("Step out executed. Waiting for stopped event.")
}

// ── pause ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PauseParams {
    thread_id: u64,
}

pub async fn handle_pause(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: PauseParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "pause".into(),
            message: e.to_string(),
        })?;

    session.pause(p.thread_id).await?;
    ok_result("Pause command sent. Waiting for stopped event.")
}
