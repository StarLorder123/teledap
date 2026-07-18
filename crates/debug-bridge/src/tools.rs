//! MCP tool definitions: names, titles, descriptions, and JSON input schemas.
//!
//! Defines 24 tools: 20 gated debug operations (matching `ToolAvailability`
//! operations) plus 4 utility tools with no state gating.
//!
//! Each tool's `inputSchema` is built from `JsonSchema` / `PropertySchema`
//! helpers from `mcp-protocol`.

use mcp_protocol::{JsonSchema, PropertySchema, Tool};

// ── Helpers ──────────────────────────────────────────────────────────────

fn object_schema() -> JsonSchema {
    JsonSchema::new_object()
}

fn string(desc: &str) -> PropertySchema {
    PropertySchema::string(desc)
}

fn integer(desc: &str) -> PropertySchema {
    PropertySchema::integer(desc)
}

fn boolean(desc: &str) -> PropertySchema {
    PropertySchema::boolean(desc)
}

fn array_of(desc: &str, item_type: &str) -> PropertySchema {
    PropertySchema::array_of(desc, item_type)
}

fn object_of(desc: &str, item_type: &str) -> PropertySchema {
    PropertySchema::object_of(desc, item_type)
}

// ── Breakpoint item schema ───────────────────────────────────────────────

// ── Tool list ────────────────────────────────────────────────────────────

/// Returns the complete list of 24 tools for `tools/list`.
pub fn all_tools() -> Vec<Tool> {
    vec![
        // ═══════════════════════════════════════════════════════════════
        // Lifecycle tools (6 gated)
        // ═══════════════════════════════════════════════════════════════
        Tool {
            name: "start".into(),
            title: "Start debug adapter".into(),
            description: "Spawn the debug adapter process. Must be called before any other operation. Supports codelldb and GDB (gdb -i dap).".into(),
            input_schema: object_schema()
                .with_required("adapterPath", string("Absolute path to the debug adapter binary (e.g. \"codelldb\" or \"gdb\"). Also accepts \"codelldbPath\" for backward compatibility."))
                .with_optional("adapterKind", string("Adapter type: \"codelldb\" (default) or \"gdb\". Controls behavioral differences like launch response handling."))
                .with_optional("adapterArgs", array_of("Command-line arguments for the adapter binary (e.g. [\"-i\", \"dap\"] for GDB)", "string")),
        },
        Tool {
            name: "initialize".into(),
            title: "Initialize DAP handshake".into(),
            description: "Perform the DAP initialize handshake with the debug adapter. Returns adapter capabilities. The default adapterId is derived from the adapter kind (\"lldb\" for codelldb, \"gdb\" for GDB).".into(),
            input_schema: object_schema()
                .with_optional("adapterId", string("Adapter identifier (default: runtime-derived from adapter kind)")),
        },
        Tool {
            name: "launch".into(),
            title: "Launch debuggee".into(),
            description: "Launch the target program for debugging. The program path is resolved through the path mapper if relative.".into(),
            input_schema: object_schema()
                .with_required("program", string("Path to the ELF binary to debug"))
                .with_optional("args", array_of("Command-line arguments for the debuggee", "string"))
                .with_optional("env", object_of("Environment variables for the debuggee", "string"))
                .with_optional("stopOnEntry", boolean("Stop at program entry point (default: false)"))
                .with_optional("gdbRemote", string("Remote GDB server address (e.g. \"localhost:3333\")")),
        },
        Tool {
            name: "attach".into(),
            title: "Attach to process".into(),
            description: "Attach the debugger to a running process by PID or custom configuration.".into(),
            input_schema: object_schema()
                .with_optional("pid", integer("Process ID to attach to"))
                .with_optional("extra", object_of("Additional attach configuration", "string")),
        },
        Tool {
            name: "configuration_done".into(),
            title: "Configuration done".into(),
            description: "Signal to the debug adapter that configuration is complete and execution may begin. Transitions state to Running.".into(),
            input_schema: object_schema(),
        },
        Tool {
            name: "shutdown".into(),
            title: "Shut down session".into(),
            description: "Disconnect from the debug adapter and terminate the adapter process. Cleans up all resources.".into(),
            input_schema: object_schema(),
        },

        // ═══════════════════════════════════════════════════════════════
        // Execution control tools (5 gated)
        // ═══════════════════════════════════════════════════════════════
        Tool {
            name: "continue".into(),
            title: "Continue execution".into(),
            description: "Resume execution of the debuggee. Only available when halted.".into(),
            input_schema: object_schema()
                .with_required("threadId", integer("Thread ID to continue"))
                .with_optional("singleThread", boolean("Continue only this thread (default: false)")),
        },
        Tool {
            name: "step_over".into(),
            title: "Step over".into(),
            description: "Execute the current source line, stepping over any function calls. Only available when halted.".into(),
            input_schema: object_schema()
                .with_required("threadId", integer("Thread ID to step"))
                .with_optional("singleThread", boolean("Step only this thread (default: false)")),
        },
        Tool {
            name: "step_in".into(),
            title: "Step into".into(),
            description: "Step into the function called at the current source line. Only available when halted.".into(),
            input_schema: object_schema()
                .with_required("threadId", integer("Thread ID to step"))
                .with_optional("singleThread", boolean("Step only this thread (default: false)"))
                .with_optional("targetId", integer("Specific step-in target ID")),
        },
        Tool {
            name: "step_out".into(),
            title: "Step out".into(),
            description: "Execute until the current function returns. Only available when halted.".into(),
            input_schema: object_schema()
                .with_required("threadId", integer("Thread ID to step"))
                .with_optional("singleThread", boolean("Step only this thread (default: false)")),
        },
        Tool {
            name: "pause".into(),
            title: "Pause execution".into(),
            description: "Pause a running debuggee. Only available when the debuggee is Running.".into(),
            input_schema: object_schema()
                .with_required("threadId", integer("Thread ID to pause")),
        },

        // ═══════════════════════════════════════════════════════════════
        // Breakpoint tools (2 gated)
        // ═══════════════════════════════════════════════════════════════
        Tool {
            name: "set_breakpoints".into(),
            title: "Set source breakpoints".into(),
            description: "Set breakpoints on source file lines. The source path is resolved through the path mapper if relative. Returns verified breakpoints.".into(),
            input_schema: object_schema()
                .with_required("sourcePath", string("Path to the source file (relative or absolute)"))
                .with_required("breakpoints", array_of("Breakpoint locations", "object")),
        },
        Tool {
            name: "set_function_breakpoints".into(),
            title: "Set function breakpoints".into(),
            description: "Set breakpoints on function names. The debugger will stop when any of the named functions are entered.".into(),
            input_schema: object_schema()
                .with_required("names", array_of("Function names to break on", "string"))
                .with_optional("condition", string("Optional conditional expression"))
                .with_optional("hitCondition", string("Optional hit count condition (e.g. \">5\")")),
        },

        // ═══════════════════════════════════════════════════════════════
        // Introspection tools (7 gated + 1 utility)
        // ═══════════════════════════════════════════════════════════════
        Tool {
            name: "get_threads".into(),
            title: "Get threads".into(),
            description: "List all threads in the debuggee. Only available when halted.".into(),
            input_schema: object_schema(),
        },
        Tool {
            name: "get_stack_trace".into(),
            title: "Get stack trace".into(),
            description: "Get the call stack for a specific thread. Returns stack frames with source locations. Only available when halted.".into(),
            input_schema: object_schema()
                .with_required("threadId", integer("Thread ID to query"))
                .with_optional("levels", integer("Maximum number of frames to return"))
                .with_optional("startFrame", integer("Frame index to start from (0-based)")),
        },
        Tool {
            name: "get_scopes".into(),
            title: "Get scopes".into(),
            description: "Get the variable scopes (locals, globals, registers) for a stack frame. Only available when halted.".into(),
            input_schema: object_schema()
                .with_required("frameId", integer("Stack frame ID from get_stack_trace")),
        },
        Tool {
            name: "get_variables".into(),
            title: "Get variables".into(),
            description: "Get variables within a scope. The returned variables may have nested variablesReference handles for further expansion. Only available when halted.".into(),
            input_schema: object_schema()
                .with_required("variablesReference", integer("Variables reference from a scope or variable"))
                .with_optional("filter", string("Variable filter: \"named\" or \"indexed\""))
                .with_optional("start", integer("Start index for paging (0-based)"))
                .with_optional("count", integer("Maximum number of variables to return")),
        },
        Tool {
            name: "evaluate".into(),
            title: "Evaluate expression".into(),
            description: "Evaluate an expression in the debuggee's context (e.g. \"ptr->field\", \"x + y\"). Only available when halted.".into(),
            input_schema: object_schema()
                .with_required("expression", string("C++ expression to evaluate"))
                .with_optional("frameId", integer("Stack frame ID for context"))
                .with_optional("context", string("Evaluation context: \"watch\", \"repl\", \"hover\", or \"clipboard\"")),
        },
        Tool {
            name: "set_variable".into(),
            title: "Set variable".into(),
            description: "Set a variable's value in the debuggee. Only available when halted.".into(),
            input_schema: object_schema()
                .with_required("variablesReference", integer("Parent variables reference"))
                .with_required("name", string("Variable name"))
                .with_required("value", string("New value as a string")),
        },
        Tool {
            name: "assemble_context".into(),
            title: "Assemble context".into(),
            description: "Build a full debug context chain: threads → frames → scopes → variables. Returns nested data structure with all levels expanded up to the configured depth. Only available when halted.".into(),
            input_schema: object_schema()
                .with_optional("threadId", integer("Specific thread ID (default: all threads)"))
                .with_optional("maxFrames", integer("Maximum frames per thread (default: 10)"))
                .with_optional("maxDepth", integer("Maximum variable expansion depth (default: 2)")),
        },
        Tool {
            name: "search_variables".into(),
            title: "Search variables".into(),
            description: "Search the variable handle cache by name (fuzzy, case-insensitive). Useful for finding variables across scopes and frames.".into(),
            input_schema: object_schema()
                .with_required("query", string("Search query (partial name matching)"))
                .with_optional("limit", integer("Maximum results to return (default: 20)")),
        },

        // ═══════════════════════════════════════════════════════════════
        // Utility tools (3, no state gating)
        // ═══════════════════════════════════════════════════════════════
        Tool {
            name: "get_state".into(),
            title: "Get session state".into(),
            description: "Return the current session state, halt details, and list of available tools for the current state. Always available.".into(),
            input_schema: object_schema(),
        },
        Tool {
            name: "register_path_alias".into(),
            title: "Register path alias".into(),
            description: "Register a mapping from an AI-relative source path to an absolute system path. This enables setting breakpoints using short paths like \"src/main.cpp\".".into(),
            input_schema: object_schema()
                .with_required("alias", string("AI-relative path prefix (e.g. \"src/main.cpp\")"))
                .with_required("absolutePath", string("Absolute system path (e.g. \"/home/user/project/src/main.cpp\")")),
        },
        Tool {
            name: "register_base_dir".into(),
            title: "Register base directory".into(),
            description: "Register a base directory for resolving relative source paths. Multiple base dirs can be registered; the first match wins.".into(),
            input_schema: object_schema()
                .with_required("dir", string("Absolute path to a project root directory")),
        },

        // ═══════════════════════════════════════════════════════════════
        // OpenOCD management tools (5 utility, no state gating)
        // ═══════════════════════════════════════════════════════════════
        Tool {
            name: "openocd_start".into(),
            title: "Start OpenOCD".into(),
            description: "Launch the OpenOCD GDB server process with the specified config files. Optionally capture stdout/stderr to log files. OpenOCD must be started before launch with gdbRemote for embedded debugging.".into(),
            input_schema: object_schema()
                .with_required("openocdPath", string("Absolute path to the OpenOCD binary (e.g. \"/usr/bin/openocd\")"))
                .with_required("configFiles", array_of("OpenOCD config files in order (e.g. [\"board/stm32f4discovery.cfg\"])", "string"))
                .with_optional("extraArgs", array_of("Additional CLI arguments for OpenOCD (e.g. [\"-d\", \"2\"])", "string"))
                .with_optional("logDir", string("Directory for stdout/stderr log files. If omitted, output is discarded.")),
        },
        Tool {
            name: "openocd_stop".into(),
            title: "Stop OpenOCD".into(),
            description: "Send the shutdown command to OpenOCD and terminate the process.".into(),
            input_schema: object_schema(),
        },
        Tool {
            name: "openocd_status".into(),
            title: "Get OpenOCD status".into(),
            description: "Returns whether OpenOCD is running, uptime, and log directory.".into(),
            input_schema: object_schema(),
        },
        Tool {
            name: "openocd_output".into(),
            title: "Read OpenOCD output".into(),
            description: "Read recent stdout (and optionally stderr) lines from the OpenOCD log files. Requires logDir to have been configured in openocd_start.".into(),
            input_schema: object_schema()
                .with_optional("lines", integer("Number of recent lines to return (default: 100)"))
                .with_optional("includeStderr", boolean("Also include stderr lines (default: false)")),
        },
        Tool {
            name: "openocd_send".into(),
            title: "Send OpenOCD command".into(),
            description: "Send a raw Tcl command to OpenOCD and wait for the response. Useful for custom commands like 'reset halt', 'flash write_image', etc.".into(),
            input_schema: object_schema()
                .with_required("command", string("Tcl command to send (e.g. \"reset halt\", \"mdw 0x08000000 16\")"))
                .with_optional("timeoutMs", integer("Response timeout in milliseconds (default: 5000)")),
        },
    ]
}

/// Map a tool name to its corresponding `ToolAvailability` operation name.
///
/// Returns `None` for utility tools that have no state gating.
pub fn tool_operation(name: &str) -> Option<&'static str> {
    match name {
        // ── Lifecycle ─────────────────────────────────────────────────
        "start" => Some("start"),
        "initialize" => Some("initialize"),
        "launch" => Some("launch"),
        "attach" => Some("attach"),
        "configuration_done" => Some("configuration_done"),
        "shutdown" => Some("shutdown"),
        // ── Execution ─────────────────────────────────────────────────
        "continue" => Some("continue"),
        "step_over" => Some("step_over"),
        "step_in" => Some("step_in"),
        "step_out" => Some("step_out"),
        "pause" => Some("pause"),
        // ── Breakpoints ───────────────────────────────────────────────
        "set_breakpoints" => Some("set_breakpoints"),
        "set_function_breakpoints" => Some("set_function_breakpoints"),
        // ── Introspection ─────────────────────────────────────────────
        "get_threads" => Some("get_threads"),
        "get_stack_trace" => Some("get_stack_trace"),
        "get_scopes" => Some("get_scopes"),
        "get_variables" => Some("get_variables"),
        "evaluate" => Some("evaluate"),
        "set_variable" => Some("set_variable"),
        "assemble_context" => Some("assemble_context"),
        // ── Utility (no gating) ───────────────────────────────────────
        "search_variables" => None,
        "get_state" => None,
        "register_path_alias" => None,
        "register_base_dir" => None,
        // ── OpenOCD (utility, no gating) ──────────────────────────────
        "openocd_start" => None,
        "openocd_stop" => None,
        "openocd_status" => None,
        "openocd_output" => None,
        "openocd_send" => None,
        _ => None,
    }
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_tools_count() {
        let tools = all_tools();
        assert_eq!(tools.len(), 29, "Should be exactly 29 tools");
    }

    #[test]
    fn test_every_tool_has_required_fields() {
        for tool in all_tools() {
            assert!(!tool.name.is_empty(), "Tool has empty name");
            assert!(
                !tool.title.is_empty(),
                "Tool '{}' has empty title",
                tool.name
            );
            assert!(
                !tool.description.is_empty(),
                "Tool '{}' has empty description",
                tool.name
            );
            assert_eq!(
                tool.input_schema.schema_type, "object",
                "Tool '{}' inputSchema is not type=object",
                tool.name
            );
        }
    }

    #[test]
    fn test_tool_names_are_unique() {
        let tools = all_tools();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        names.sort();
        let orig_len = names.len();
        names.dedup();
        assert_eq!(names.len(), orig_len, "Duplicate tool names found");
    }

    #[test]
    fn test_tool_operation_mapping_covers_all_gated_tools() {
        let gated_count = all_tools()
            .iter()
            .filter(|t| tool_operation(&t.name).is_some())
            .count();
        assert_eq!(gated_count, 20, "Should have exactly 20 gated tools");
    }

    #[test]
    fn test_utility_tools_have_no_operation() {
        for name in &[
            "get_state",
            "register_path_alias",
            "register_base_dir",
            "search_variables",
            "openocd_start",
            "openocd_stop",
            "openocd_status",
            "openocd_output",
            "openocd_send",
        ] {
            assert_eq!(
                tool_operation(name),
                None,
                "Utility tool '{name}' should have no gating operation"
            );
        }
    }

    #[test]
    fn test_unknown_tool_returns_none() {
        assert_eq!(tool_operation("nonexistent"), None);
    }

    #[test]
    fn test_all_gated_tools_exist_in_known_operations() {
        // Every tool with a gating operation should be recognized
        let tools = all_tools();
        for tool in &tools {
            if let Some(op) = tool_operation(&tool.name) {
                assert!(
                    !op.is_empty(),
                    "Tool '{}' maps to empty operation",
                    tool.name
                );
            }
        }
    }
}
