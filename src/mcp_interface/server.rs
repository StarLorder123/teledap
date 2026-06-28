//! `TeleDapServer` — MCP protocol adapter implementing `rmcp::ServerHandler`.

use super::protocol_types;
use crate::session_coordinator::SessionCoordinator;
use rmcp::{
    handler::server::ServerHandler,
    model::{
        CallToolRequestParams, CallToolResult, Content, Implementation,
        ListToolsResult, ServerCapabilities, ServerInfo,
        Tool,
    },
    service::RequestContext,
    ErrorData as McpError, RoleServer,
};
use std::sync::Arc;

/// The MCP server implementation for TeleDAP.
///
/// Wraps a `SessionCoordinator` and implements the `ServerHandler`
/// trait from `rmcp`, providing the bridge between MCP JSON-RPC
/// protocol messages and TeleDAP's internal debug session logic.
#[derive(Clone)]
pub struct TeleDapServer {
    coordinator: Arc<SessionCoordinator>,
    server_info: ServerInfo,
}

impl TeleDapServer {
    /// Creates a new `TeleDapServer`.
    pub fn new(coordinator: Arc<SessionCoordinator>) -> Self {
        let mut server_info = ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .build(),
        );
        server_info.server_info = Implementation::new(
            "TeleDAP",
            env!("CARGO_PKG_VERSION"),
        );
        server_info.instructions = Some(
            "TeleDAP — Embedded hardware debug bridge for AI assistants.\n\n\
             Use auto_launch with an ELF file path to start a debug session. \
             Then use set_breakpoint, continue_execution, and step tools to \
             control execution. Use get_stack_trace, get_variables, and evaluate \
             to inspect program state. Use read_register/write_register for \
             hardware peripheral access. Use get_debug_logs to review operation \
             history."
                .into(),
        );

        Self {
            coordinator,
            server_info,
        }
    }
}

impl ServerHandler for TeleDapServer {
    fn get_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let state = self.coordinator.current_state().await;
        let mut tools: Vec<Tool> = Vec::new();

        use crate::session_coordinator::state_machine::SessionState;

        // ── Always Available ──
        tools.push(Tool::new(
            "get_status",
            "Get the current TeleDAP session status",
            protocol_types::get_status_schema(),
        ));
        tools.push(Tool::new(
            "get_debug_logs",
            "Query recent operation history from the audit log",
            protocol_types::get_debug_logs_schema(),
        ));
        tools.push(Tool::new(
            "shutdown",
            "Gracefully shutdown all connections and exit",
            protocol_types::shutdown_schema(),
        ));

        // ── State-Dependent Tools ──
        match state {
            SessionState::Disconnected => {
                tools.push(Tool::new(
                    "auto_launch",
                    "Auto-launch debug session: connect OpenOCD + start CodeLLDB + load ELF",
                    protocol_types::auto_launch_schema(),
                ));
                tools.push(Tool::new(
                    "connect_openocd",
                    "Connect to OpenOCD Tcl RPC server",
                    protocol_types::connect_openocd_schema(),
                ));
            }
            SessionState::Initialized => {
                add_openocd_tools(&mut tools);
            }
            SessionState::Attached | SessionState::Halted
            | SessionState::Running =>
            {
                add_openocd_tools(&mut tools);

                if state == SessionState::Halted {
                    add_dap_stopped_tools(&mut tools);
                }

                if state == SessionState::Running {
                    tools.push(Tool::new(
                        "halt",
                        "Halt target execution",
                        protocol_types::halt_schema(),
                    ));
                }

                tools.push(Tool::new(
                    "continue_execution",
                    "Continue target execution",
                    protocol_types::continue_execution_schema(),
                ));

                tools.push(Tool::new(
                    "reset_halt",
                    "Reset target and halt at reset vector",
                    protocol_types::reset_halt_schema(),
                ));
            }
        }

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tool_name = request.name.to_string();
        let args_map = request.arguments.unwrap_or_default();
        let args = serde_json::Value::Object(args_map);

        tracing::info!("MCP tool call: {}", tool_name);

        match self.coordinator.execute_tool(&tool_name, &args).await {
            Ok(result_text) => {
                Ok(CallToolResult::success(vec![Content::text(result_text)]))
            }
            Err(e) => {
                tracing::error!(
                    "Tool '{}' failed: {}",
                    tool_name,
                    e
                );
                Ok(CallToolResult::error(vec![Content::text(format!(
                    "Error: {}",
                    e
                ))]))
            }
        }
    }
}

// ── Tool Set Helpers ─────────────────────────────────────────────

fn add_openocd_tools(tools: &mut Vec<Tool>) {
    tools.push(Tool::new(
        "reset_halt",
        "Reset target and halt at reset vector",
        protocol_types::reset_halt_schema(),
    ));
    tools.push(Tool::new(
        "flash_erase",
        "Erase flash memory region",
        protocol_types::flash_erase_schema(),
    ));
    tools.push(Tool::new(
        "flash_write",
        "Write binary data to flash (hex-encoded)",
        protocol_types::flash_write_schema(),
    ));
    tools.push(Tool::new(
        "read_register",
        "Read a 32-bit peripheral register",
        protocol_types::read_register_schema(),
    ));
    tools.push(Tool::new(
        "write_register",
        "Write a 32-bit peripheral register",
        protocol_types::write_register_schema(),
    ));
    tools.push(Tool::new(
        "read_memory",
        "Read memory region (hex dump)",
        protocol_types::read_memory_schema(),
    ));
    tools.push(Tool::new(
        "write_memory",
        "Write memory region (hex-encoded)",
        protocol_types::write_memory_schema(),
    ));
}

fn add_dap_stopped_tools(tools: &mut Vec<Tool>) {
    tools.push(Tool::new(
        "set_breakpoint",
        "Set breakpoint at file:line",
        protocol_types::set_breakpoint_schema(),
    ));
    tools.push(Tool::new(
        "step_in",
        "Step into function call",
        protocol_types::step_schema(),
    ));
    tools.push(Tool::new(
        "step_over",
        "Step over current line",
        protocol_types::step_schema(),
    ));
    tools.push(Tool::new(
        "step_out",
        "Step out of current function",
        protocol_types::step_schema(),
    ));
    tools.push(Tool::new(
        "get_stack_trace",
        "Get call stack frames",
        protocol_types::stack_trace_schema(),
    ));
    tools.push(Tool::new(
        "get_variables",
        "Get local variables in frame",
        protocol_types::variables_schema(),
    ));
    tools.push(Tool::new(
        "evaluate",
        "Evaluate C/C++ expression",
        protocol_types::evaluate_schema(),
    ));
}
