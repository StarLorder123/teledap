//! Breakpoint tool handlers: set_breakpoints, set_function_breakpoints.

use dap_types::types::{FunctionBreakpoint, Source, SourceBreakpoint};
use debug_session::DebugSession;
use mcp_protocol::CallToolResult;
use serde::Deserialize;

use crate::error::BridgeError;

fn text_result(value: &impl serde::Serialize) -> Result<CallToolResult, BridgeError> {
    Ok(CallToolResult::success_json(value)?)
}

// ── set_breakpoints ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BpItem {
    line: u64,
    #[serde(default)]
    condition: Option<String>,
    #[serde(default)]
    log_message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetBreakpointsParams {
    source_path: String,
    breakpoints: Vec<BpItem>,
}

pub async fn handle_set_breakpoints(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: SetBreakpointsParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "set_breakpoints".into(),
            message: e.to_string(),
        })?;

    // Resolve source path through the path mapper
    let resolved = session
        .resolve_path(&p.source_path)
        .await
        .unwrap_or_else(|| p.source_path.clone());

    let source = Source {
        name: Some(resolved.clone()),
        path: Some(resolved),
        ..Default::default()
    };

    let breakpoints: Vec<SourceBreakpoint> = p
        .breakpoints
        .into_iter()
        .map(|b| SourceBreakpoint {
            line: b.line,
            column: None,
            condition: b.condition,
            hit_condition: None,
            log_message: b.log_message,
            mode: None,
        })
        .collect();

    let args = dap_types::requests::SetBreakpointsArguments {
        source,
        breakpoints: Some(breakpoints),
        lines: None,
        source_modified: None,
    };

    let resp = session.set_breakpoints(args).await?;
    text_result(&resp)
}

// ── set_function_breakpoints ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetFunctionBreakpointsParams {
    names: Vec<String>,
    #[serde(default)]
    condition: Option<String>,
    #[serde(default)]
    hit_condition: Option<String>,
}

pub async fn handle_set_function_breakpoints(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: SetFunctionBreakpointsParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "set_function_breakpoints".into(),
            message: e.to_string(),
        })?;

    let breakpoints: Vec<FunctionBreakpoint> = p
        .names
        .into_iter()
        .map(|name| FunctionBreakpoint {
            name,
            condition: p.condition.clone(),
            hit_condition: p.hit_condition.clone(),
        })
        .collect();

    let args = dap_types::requests::SetFunctionBreakpointsArguments { breakpoints };

    let resp = session.set_function_breakpoints(args).await?;
    text_result(&resp)
}
