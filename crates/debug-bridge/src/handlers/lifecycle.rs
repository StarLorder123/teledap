//! Lifecycle tool handlers: start, initialize, launch, attach,
//! configuration_done, shutdown, and utility tools (get_state,
//! register_path_alias, register_base_dir).

use dap_types::requests::{
    AttachRequestArguments, InitializeRequestArguments, LaunchRequestArguments,
};
use debug_session::{AdapterConfig, AdapterKind, DebugSession};
use mcp_protocol::CallToolResult;
use serde::Deserialize;

use crate::error::BridgeError;

/// Helper: return a successful CallToolResult with pretty-printed JSON.
fn text_result(value: &impl serde::Serialize) -> Result<CallToolResult, BridgeError> {
    Ok(CallToolResult::success_json(value)?)
}

fn ok_result(msg: impl Into<String>) -> Result<CallToolResult, BridgeError> {
    Ok(CallToolResult::success(msg))
}

// ── start ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartParams {
    /// Path to the debug adapter binary (canonical name).
    /// Also accepts "codelldbPath" for backward compatibility.
    #[serde(alias = "codelldbPath")]
    adapter_path: String,
    /// Adapter kind: "codelldb" (default) or "gdb".
    #[serde(default)]
    adapter_kind: Option<String>,
    /// Command-line arguments to pass to the adapter binary.
    #[serde(default)]
    adapter_args: Option<Vec<String>>,
}

pub async fn handle_start(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: StartParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "start".into(),
            message: e.to_string(),
        })?;

    let kind = match p.adapter_kind.as_deref() {
        Some("gdb") => AdapterKind::Gdb,
        _ => AdapterKind::Codelldb, // default and "codelldb"
    };

    let config = AdapterConfig {
        path: p.adapter_path,
        kind,
        args: p.adapter_args.unwrap_or_default(),
    };

    session.start(&config).await?;
    ok_result(format!(
        "Debug adapter process started (kind: {:?}). State: {:?}",
        kind,
        session.current_state().await
    ))
}

// ── initialize ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeParams {
    #[serde(default)]
    adapter_id: Option<String>,
}

pub async fn handle_initialize(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: InitializeParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "initialize".into(),
            message: e.to_string(),
        })?;

    // Derive default adapter_id from the adapter kind (Codelldb → "lldb", Gdb → "gdb")
    let default_id = match session.adapter_kind().await {
        Some(AdapterKind::Gdb) => "gdb",
        _ => "lldb", // Codelldb or unknown
    };

    let args = InitializeRequestArguments {
        client_id: Some("teledap".into()),
        client_name: Some("TeleDAP".into()),
        adapter_id: p.adapter_id.or_else(|| Some(default_id.into())),
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
    };

    let caps = session.initialize(args).await?;
    text_result(&caps)
}

// ── launch ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchParams {
    program: String,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default)]
    env: Option<serde_json::Value>,
    #[serde(default)]
    stop_on_entry: Option<bool>,
    #[serde(default)]
    gdb_remote: Option<String>,
}

pub async fn handle_launch(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: LaunchParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "launch".into(),
            message: e.to_string(),
        })?;

    // Resolve the program path through the path mapper
    let resolved = session
        .resolve_path(&p.program)
        .await
        .unwrap_or_else(|| p.program.clone());

    let mut launch_extra = serde_json::json!({
        "program": resolved,
        "stopOnEntry": p.stop_on_entry.unwrap_or(false),
    });

    if let Some(ref remote) = p.gdb_remote {
        match session.adapter_kind().await {
            Some(AdapterKind::Gdb) => {
                // GDB DAP uses `target` field for remote debugging
                launch_extra["target"] = serde_json::json!(format!("remote {remote}"));
            }
            _ => {
                // codelldb uses `processCreateCommands` for remote debugging
                launch_extra["processCreateCommands"] =
                    serde_json::json!([format!("gdb-remote {remote}")]);
            }
        }
    }
    if let Some(ref cargs) = p.args {
        launch_extra["args"] = serde_json::json!(cargs);
    }
    if let Some(ref env) = p.env {
        launch_extra["env"] = env.clone();
    }

    let args = LaunchRequestArguments {
        no_debug: None,
        __restart: None,
        extra: launch_extra,
    };

    session.launch(args).await?;
    ok_result(format!(
        "Launch command sent for: {}. Waiting for initialized event.",
        p.program
    ))
}

// ── attach ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachParams {
    #[serde(default)]
    pid: Option<u64>,
    #[serde(default)]
    extra: Option<serde_json::Value>,
}

pub async fn handle_attach(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: AttachParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "attach".into(),
            message: e.to_string(),
        })?;

    let extra = p.extra.unwrap_or_else(|| {
        let mut map = serde_json::Map::new();
        if let Some(pid) = p.pid {
            map.insert("pid".into(), serde_json::json!(pid));
        }
        serde_json::Value::Object(map)
    });

    let args = AttachRequestArguments {
        __restart: None,
        extra,
    };

    session.attach(args).await?;
    ok_result("Attach command sent.")
}

// ── configuration_done ──────────────────────────────────────────────────

pub async fn handle_configuration_done(
    session: &DebugSession,
    _params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    session.configuration_done().await?;
    ok_result(format!(
        "Configuration done. State: {:?}",
        session.current_state().await
    ))
}

// ── shutdown ────────────────────────────────────────────────────────────

pub async fn handle_shutdown(
    session: &DebugSession,
    _params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    session.shutdown().await?;
    ok_result("Session shut down. Debug adapter process terminated.")
}

// ── get_state (utility) ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetStateParams {
    #[serde(default)]
    detail: Option<String>,
}

pub async fn handle_get_state(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    use debug_session::ToolAvailability;

    let p: GetStateParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "get_state".into(),
            message: e.to_string(),
        })?;
    let simple = p.detail.as_deref() == Some("simple");

    let state = session.current_state().await;
    let available_tools: Vec<String> = ToolAvailability::operations_for_state(state)
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let all_available: Vec<String> = crate::ToolRegistry::list_tools_for_state(state)
        .into_iter()
        .map(|t| t.name)
        .collect();

    let info = if simple {
        serde_json::json!({
            "state": format!("{:?}", state),
            "available_operations": available_tools,
            "available_tools": all_available,
        })
    } else {
        serde_json::json!({
            "state": format!("{:?}", state),
            "available_operations": available_tools,
            "available_tools": all_available,
            "capabilities": session.capabilities().await,
        })
    };

    text_result(&info)
}

// ── register_path_alias (utility) ───────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterPathAliasParams {
    alias: String,
    absolute_path: String,
}

pub async fn handle_register_path_alias(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: RegisterPathAliasParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "register_path_alias".into(),
            message: e.to_string(),
        })?;

    session
        .register_path_alias(&p.alias, &p.absolute_path)
        .await;
    ok_result(format!(
        "Path alias registered: \"{}\" → \"{}\"",
        p.alias, p.absolute_path
    ))
}

// ── register_base_dir (utility) ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterBaseDirParams {
    dir: String,
}

pub async fn handle_register_base_dir(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: RegisterBaseDirParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "register_base_dir".into(),
            message: e.to_string(),
        })?;

    session.register_base_dir(&p.dir).await;
    ok_result(format!("Base directory registered: \"{}\"", p.dir))
}
