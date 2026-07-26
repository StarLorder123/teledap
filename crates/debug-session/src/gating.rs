//! Tool availability gating — maps operations to the session states
//! in which they are legal. This feeds into Phase 3 MCP tool registration.

use crate::state::SessionState;

/// Static mapping of operation names to the states in which they are allowed.
pub struct ToolAvailability;

impl ToolAvailability {
    /// Returns the states in which `operation` may be called.
    pub fn allowed_states(operation: &str) -> &'static [SessionState] {
        match operation {
            // ── Lifecycle ─────────────────────────────────────────
            "start" => &[SessionState::Disconnected],
            "initialize" => &[SessionState::Connected],
            "launch" => &[SessionState::Initialized],
            "attach" => &[SessionState::Initialized],
            "configuration_done" => &[SessionState::Initialized],
            "shutdown" | "disconnect" => &[
                SessionState::Connected,
                SessionState::Initialized,
                SessionState::Running,
                SessionState::Halted,
            ],

            // ── Execution control ─────────────────────────────────
            "continue" => &[SessionState::Halted],
            "step_over" => &[SessionState::Halted],
            "step_in" => &[SessionState::Halted],
            "step_out" => &[SessionState::Halted],
            "pause" => &[SessionState::Running],

            // ── Breakpoints ───────────────────────────────────────
            "set_breakpoints" => &[
                SessionState::Initialized,
                SessionState::Running,
                SessionState::Halted,
            ],
            "set_function_breakpoints" => &[
                SessionState::Initialized,
                SessionState::Running,
                SessionState::Halted,
            ],
            "list_breakpoints" => &[
                SessionState::Initialized,
                SessionState::Running,
                SessionState::Halted,
            ],

            // ── Introspection ─────────────────────────────────────
            "get_threads" => &[SessionState::Running, SessionState::Halted],
            "get_stack_trace" => &[SessionState::Halted],
            "get_scopes" => &[SessionState::Halted],
            "get_variables" => &[SessionState::Halted],
            "evaluate" => &[SessionState::Halted],
            "set_variable" => &[SessionState::Halted],
            "assemble_context" => &[SessionState::Halted],

            // Unknown operation
            _ => &[],
        }
    }

    /// Returns true if `operation` is allowed in `state`.
    pub fn is_allowed(operation: &str, state: SessionState) -> bool {
        Self::allowed_states(operation).contains(&state)
    }

    /// Returns a human-readable description of the state requirements for an operation.
    pub fn describe_requirements(operation: &str) -> String {
        let states = Self::allowed_states(operation);
        if states.is_empty() {
            format!("`{operation}` is not a recognized operation")
        } else {
            format!(
                "`{operation}` requires state(s): {}",
                states
                    .iter()
                    .map(|s| format!("{s}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }

    /// Get the list of all known operations available in a given state.
    pub fn operations_for_state(state: SessionState) -> Vec<&'static str> {
        ALL_OPERATIONS
            .iter()
            .filter(|op| Self::is_allowed(op, state))
            .copied()
            .collect()
    }
}

/// Every known operation (used by operations_for_state).
const ALL_OPERATIONS: &[&str] = &[
    "start",
    "initialize",
    "launch",
    "attach",
    "configuration_done",
    "shutdown",
    "disconnect",
    "continue",
    "step_over",
    "step_in",
    "step_out",
    "pause",
    "set_breakpoints",
    "set_function_breakpoints",
    "list_breakpoints",
    "get_threads",
    "get_stack_trace",
    "get_scopes",
    "get_variables",
    "evaluate",
    "set_variable",
    "assemble_context",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_operations_have_gating() {
        for op in ALL_OPERATIONS {
            let states = ToolAvailability::allowed_states(op);
            assert!(
                !states.is_empty(),
                "Operation `{op}` has no allowed states — add gating rules"
            );
        }
    }

    #[test]
    fn test_describe_requirements_unknown_op() {
        let desc = ToolAvailability::describe_requirements("nonexistent");
        assert!(desc.contains("not a recognized operation"));
    }

    #[test]
    fn test_describe_requirements_known_op() {
        let desc = ToolAvailability::describe_requirements("continue");
        assert!(desc.contains("continue"));
        assert!(desc.contains("Halted"));
    }

    #[test]
    fn test_disconnected_operations() {
        let ops = ToolAvailability::operations_for_state(SessionState::Disconnected);
        assert_eq!(ops, vec!["start"]);
    }

    #[test]
    fn test_connected_operations() {
        let ops = ToolAvailability::operations_for_state(SessionState::Connected);
        assert!(ops.contains(&"initialize"));
        assert!(ops.contains(&"shutdown"));
        assert!(ops.contains(&"disconnect"));
        // Cannot do execution ops
        assert!(!ops.contains(&"continue"));
        assert!(!ops.contains(&"step_over"));
    }

    #[test]
    fn test_initialized_operations() {
        let ops = ToolAvailability::operations_for_state(SessionState::Initialized);
        assert!(ops.contains(&"launch"));
        assert!(ops.contains(&"attach"));
        assert!(ops.contains(&"configuration_done"));
        assert!(ops.contains(&"set_breakpoints"));
        assert!(ops.contains(&"list_breakpoints"));
        assert!(ops.contains(&"shutdown"));
        // Cannot execute yet
        assert!(!ops.contains(&"continue"));
        assert!(!ops.contains(&"pause"));
    }

    #[test]
    fn test_running_operations() {
        let ops = ToolAvailability::operations_for_state(SessionState::Running);
        assert!(ops.contains(&"pause"));
        assert!(ops.contains(&"set_breakpoints"));
        assert!(ops.contains(&"list_breakpoints"));
        assert!(ops.contains(&"shutdown"));
        assert!(ops.contains(&"get_threads"));
        // Cannot introspect deeply while running
        assert!(!ops.contains(&"get_stack_trace"));
        assert!(!ops.contains(&"evaluate"));
    }

    #[test]
    fn test_halted_operations() {
        let ops = ToolAvailability::operations_for_state(SessionState::Halted);
        assert!(ops.contains(&"continue"));
        assert!(ops.contains(&"step_over"));
        assert!(ops.contains(&"step_in"));
        assert!(ops.contains(&"step_out"));
        assert!(ops.contains(&"get_threads"));
        assert!(ops.contains(&"get_stack_trace"));
        assert!(ops.contains(&"get_scopes"));
        assert!(ops.contains(&"get_variables"));
        assert!(ops.contains(&"evaluate"));
        assert!(ops.contains(&"set_variable"));
        assert!(ops.contains(&"assemble_context"));
        assert!(ops.contains(&"set_breakpoints"));
        assert!(ops.contains(&"list_breakpoints"));
        assert!(ops.contains(&"shutdown"));
        // Cannot pause when already halted
        assert!(!ops.contains(&"pause"));
    }

    #[test]
    fn test_is_allowed() {
        assert!(ToolAvailability::is_allowed(
            "start",
            SessionState::Disconnected
        ));
        assert!(!ToolAvailability::is_allowed(
            "start",
            SessionState::Connected
        ));
        assert!(ToolAvailability::is_allowed(
            "continue",
            SessionState::Halted
        ));
        assert!(!ToolAvailability::is_allowed(
            "continue",
            SessionState::Running
        ));
        assert!(ToolAvailability::is_allowed("pause", SessionState::Running));
        assert!(!ToolAvailability::is_allowed("pause", SessionState::Halted));
    }

    #[test]
    fn test_operations_for_state_returns_no_duplicates() {
        for state in &[
            SessionState::Disconnected,
            SessionState::Connected,
            SessionState::Initialized,
            SessionState::Running,
            SessionState::Halted,
        ] {
            let ops = ToolAvailability::operations_for_state(*state);
            let mut sorted = ops.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(ops.len(), sorted.len(), "duplicate operations for {state}");
        }
    }
}
