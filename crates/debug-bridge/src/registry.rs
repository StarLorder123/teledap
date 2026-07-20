//! Tool registry — state-aware dispatch of MCP tool calls to handlers.
//!
//! `ToolRegistry` has no internal state; it receives a `&DebugSession` at
//! dispatch time and validates the current session state before routing to
//! the appropriate handler module.

use std::sync::Arc;

use dap_trace::{TraceDirection, TraceEntry, TraceHandle, TraceSource};
use debug_session::{DebugSession, SessionState, ToolAvailability};
use mcp_protocol::{CallToolResult, Tool};
use openocd_client::OpenOcdClient;
use tokio::sync::RwLock;

use crate::error::BridgeError;
use crate::handlers;
use crate::tools;

/// Stateless tool dispatcher.
pub struct ToolRegistry;

impl ToolRegistry {
    /// Returns the complete list of all 29 tools.
    pub fn list_tools() -> Vec<Tool> {
        tools::all_tools()
    }

    /// Returns only the tools available in the given session state.
    ///
    /// Gated tools are filtered by `ToolAvailability`; utility tools
    /// (no state gating) are always included.
    pub fn list_tools_for_state(state: SessionState) -> Vec<Tool> {
        let ops: std::collections::HashSet<&str> = ToolAvailability::operations_for_state(state)
            .into_iter()
            .collect();

        tools::all_tools()
            .into_iter()
            .filter(|t| {
                tools::tool_operation(&t.name)
                    .map(|op| ops.contains(op))
                    .unwrap_or(true) // utility tools always pass
            })
            .collect()
    }

    /// Dispatch a tool call to the appropriate handler.
    ///
    /// Validates state gating, records an MCP trace entry, then routes to
    /// the handler module matching `name`. Tool execution errors are returned
    /// as `BridgeError`, which callers should convert to `is_error: true`
    /// tool results (NOT JSON-RPC errors).
    pub async fn dispatch(
        name: &str,
        session: &DebugSession,
        params: serde_json::Value,
        trace: Option<&TraceHandle>,
        openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
    ) -> Result<CallToolResult, BridgeError> {
        // ── State gating ─────────────────────────────────────────────
        if let Some(op) = tools::tool_operation(name) {
            let state = session.current_state().await;
            if !ToolAvailability::is_allowed(op, state) {
                return Err(BridgeError::Internal(
                    ToolAvailability::describe_requirements(op),
                ));
            }
        }

        // ── Trace the MCP trigger ─────────────────────────────────────
        if let Some(t) = trace {
            let entry = TraceEntry {
                timestamp: chrono::Utc::now(),
                source: TraceSource::McpTrigger,
                direction: TraceDirection::Inbound,
                command: name.to_string(),
                payload: Some(params.clone()),
                result: None,
                duration_us: None,
                session_id: t.session_id().to_string(),
                seq: 0,
            };
            t.trace(entry);
        }

        // ── Dispatch to handler ───────────────────────────────────────
        match name {
            // Lifecycle
            "start" => handlers::lifecycle::handle_start(session, params).await,
            "initialize" => handlers::lifecycle::handle_initialize(session, params).await,
            "launch" => handlers::lifecycle::handle_launch(session, params).await,
            "attach" => handlers::lifecycle::handle_attach(session, params).await,
            "configuration_done" => {
                handlers::lifecycle::handle_configuration_done(session, params).await
            }
            "shutdown" => handlers::lifecycle::handle_shutdown(session, params).await,

            // Execution
            "continue" => handlers::execution::handle_continue(session, params).await,
            "step_over" => handlers::execution::handle_step_over(session, params).await,
            "step_in" => handlers::execution::handle_step_in(session, params).await,
            "step_out" => handlers::execution::handle_step_out(session, params).await,
            "pause" => handlers::execution::handle_pause(session, params).await,

            // Breakpoints
            "set_breakpoints" => {
                handlers::breakpoint::handle_set_breakpoints(session, params).await
            }
            "set_function_breakpoints" => {
                handlers::breakpoint::handle_set_function_breakpoints(session, params).await
            }

            // Introspection
            "get_threads" => handlers::inspect::handle_get_threads(session, params).await,
            "get_stack_trace" => handlers::inspect::handle_get_stack_trace(session, params).await,
            "get_scopes" => handlers::inspect::handle_get_scopes(session, params).await,
            "get_variables" => handlers::inspect::handle_get_variables(session, params).await,
            "evaluate" => handlers::inspect::handle_evaluate(session, params).await,
            "set_variable" => handlers::inspect::handle_set_variable(session, params).await,
            "assemble_context" => handlers::inspect::handle_assemble_context(session, params).await,
            "search_variables" => handlers::inspect::handle_search_variables(session, params).await,

            // Utility
            "get_state" => handlers::lifecycle::handle_get_state(session, params).await,
            "register_path_alias" => {
                handlers::lifecycle::handle_register_path_alias(session, params).await
            }
            "register_base_dir" => {
                handlers::lifecycle::handle_register_base_dir(session, params).await
            }

            // OpenOCD management
            "openocd_start" => {
                handlers::openocd::handle_openocd_start(session, params, openocd).await
            }
            "openocd_stop" => {
                handlers::openocd::handle_openocd_stop(session, params, openocd).await
            }
            "openocd_status" => {
                handlers::openocd::handle_openocd_status(session, params, openocd).await
            }
            "openocd_output" => {
                handlers::openocd::handle_openocd_output(session, params, openocd).await
            }
            "openocd_send" => {
                handlers::openocd::handle_openocd_send(session, params, openocd).await
            }

            _ => Err(BridgeError::UnknownTool(name.to_string())),
        }
    }
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_tools_for_state_disconnected() {
        let tools = ToolRegistry::list_tools_for_state(SessionState::Disconnected);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        // start + 4 utility tools
        assert!(names.contains(&"start"));
        assert!(names.contains(&"get_state"));
        assert!(names.contains(&"register_path_alias"));
        assert!(names.contains(&"register_base_dir"));
        assert!(names.contains(&"search_variables"));
        // Should NOT contain initialize, continue, etc.
        assert!(!names.contains(&"initialize"));
        assert!(!names.contains(&"continue"));
        assert!(!names.contains(&"get_threads"));
    }

    #[test]
    fn test_list_tools_for_state_halted() {
        let tools = ToolRegistry::list_tools_for_state(SessionState::Halted);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        // Execution tools
        assert!(names.contains(&"continue"));
        assert!(names.contains(&"step_over"));
        assert!(names.contains(&"step_in"));
        assert!(names.contains(&"step_out"));
        // Introspection tools
        assert!(names.contains(&"get_threads"));
        assert!(names.contains(&"get_stack_trace"));
        assert!(names.contains(&"get_scopes"));
        assert!(names.contains(&"get_variables"));
        assert!(names.contains(&"evaluate"));
        assert!(names.contains(&"set_variable"));
        assert!(names.contains(&"assemble_context"));
        // Utility tools always present
        assert!(names.contains(&"get_state"));
        // Should NOT contain pause (only in Running)
        assert!(!names.contains(&"pause"));
        // Should NOT contain start (only in Disconnected)
        assert!(!names.contains(&"start"));
    }

    #[test]
    fn test_list_tools_for_state_running() {
        let tools = ToolRegistry::list_tools_for_state(SessionState::Running);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"pause"));
        assert!(names.contains(&"set_breakpoints"));
        assert!(names.contains(&"shutdown"));
        assert!(names.contains(&"get_threads"));
        // Should NOT contain deep introspection tools while running
        assert!(!names.contains(&"evaluate"));
    }

    #[test]
    fn test_list_tools_always_includes_utility() {
        for state in &[
            SessionState::Disconnected,
            SessionState::Connected,
            SessionState::Initialized,
            SessionState::Running,
            SessionState::Halted,
        ] {
            let tools = ToolRegistry::list_tools_for_state(*state);
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            assert!(
                names.contains(&"get_state"),
                "get_state missing in state {state}"
            );
            assert!(
                names.contains(&"register_path_alias"),
                "register_path_alias missing in state {state}"
            );
            assert!(
                names.contains(&"register_base_dir"),
                "register_base_dir missing in state {state}"
            );
            assert!(
                names.contains(&"search_variables"),
                "search_variables missing in state {state}"
            );
            // OpenOCD tools are also utility — always present
            assert!(
                names.contains(&"openocd_start"),
                "openocd_start missing in state {state}"
            );
            assert!(
                names.contains(&"openocd_stop"),
                "openocd_stop missing in state {state}"
            );
            assert!(
                names.contains(&"openocd_status"),
                "openocd_status missing in state {state}"
            );
            assert!(
                names.contains(&"openocd_output"),
                "openocd_output missing in state {state}"
            );
            assert!(
                names.contains(&"openocd_send"),
                "openocd_send missing in state {state}"
            );
        }
    }
}
