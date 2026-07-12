//! Context-chain assembly: structured snapshot of debug state at a halt point.
//!
//! Assembles the full hierarchy: Threads → StackFrames → Scopes → Variables.

use dap_types::types::{Scope, StackFrame, Thread};

use crate::variables::ExpandedVariable;

/// A complete snapshot of the debug context when execution halts.
#[derive(Debug, Clone)]
pub struct ContextChain {
    /// All threads in the debuggee.
    pub threads: Vec<ThreadContext>,
    /// Whether this context is stale (e.g., after an invalidated event).
    pub stale: bool,
}

/// Per-thread context: stack frames and their scopes/variables.
#[derive(Debug, Clone)]
pub struct ThreadContext {
    /// The thread metadata.
    pub thread: Thread,
    /// Stack frames for this thread (outermost first).
    pub frames: Vec<FrameContext>,
}

/// Per-frame context: scopes and their variables.
#[derive(Debug, Clone)]
pub struct FrameContext {
    /// The stack frame metadata.
    pub frame: StackFrame,
    /// Scopes for this frame (Locals, Arguments, Registers, etc.).
    pub scopes: Vec<ScopeContext>,
}

/// Per-scope context: variables within that scope.
#[derive(Debug, Clone)]
pub struct ScopeContext {
    /// The scope metadata.
    pub scope: Scope,
    /// Variables in this scope (may be partially expanded).
    pub variables: Vec<ExpandedVariable>,
}

impl ContextChain {
    /// Create an empty context chain.
    pub fn new() -> Self {
        ContextChain {
            threads: Vec::new(),
            stale: false,
        }
    }

    /// Returns the total number of stack frames across all threads.
    pub fn total_frames(&self) -> usize {
        self.threads.iter().map(|t| t.frames.len()).sum()
    }

    /// Returns the total number of variables across all scopes.
    pub fn total_variables(&self) -> usize {
        self.threads
            .iter()
            .flat_map(|t| t.frames.iter())
            .flat_map(|f| f.scopes.iter())
            .map(|s| s.variables.len())
            .sum()
    }
}

impl Default for ContextChain {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadContext {
    /// Create a thread context with no frames.
    pub fn new(thread: Thread) -> Self {
        ThreadContext {
            thread,
            frames: Vec::new(),
        }
    }
}

impl FrameContext {
    /// Create a frame context with no scopes.
    pub fn new(frame: StackFrame) -> Self {
        FrameContext {
            frame,
            scopes: Vec::new(),
        }
    }
}

impl ScopeContext {
    /// Create a scope context with no variables.
    pub fn new(scope: Scope) -> Self {
        ScopeContext {
            scope,
            variables: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dap_types::types::{Scope, StackFrame, Thread};

    fn make_thread(id: u64, name: &str) -> Thread {
        Thread {
            id,
            name: name.to_string(),
        }
    }

    fn make_frame(id: u64, name: &str, line: u64) -> StackFrame {
        StackFrame {
            id,
            name: name.to_string(),
            source: None,
            line,
            column: 0,
            end_line: None,
            end_column: None,
            can_restart: None,
            instruction_pointer_reference: None,
            module_id: None,
            presentation_hint: None,
        }
    }

    fn make_scope(name: &str, var_ref: u64) -> Scope {
        Scope {
            name: name.to_string(),
            presentation_hint: None,
            variables_reference: var_ref,
            named_variables: None,
            indexed_variables: None,
            expensive: false,
            source: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
        }
    }

    #[test]
    fn test_context_chain_empty() {
        let chain = ContextChain::new();
        assert!(!chain.stale);
        assert_eq!(chain.total_frames(), 0);
        assert_eq!(chain.total_variables(), 0);
    }

    #[test]
    fn test_thread_context() {
        let thread = make_thread(1, "main");
        let tc = ThreadContext::new(thread);
        assert_eq!(tc.thread.id, 1);
        assert_eq!(tc.thread.name, "main");
        assert!(tc.frames.is_empty());
    }

    #[test]
    fn test_frame_context() {
        let frame = make_frame(100, "main", 42);
        let fc = FrameContext::new(frame);
        assert_eq!(fc.frame.id, 100);
        assert_eq!(fc.frame.name, "main");
        assert!(fc.scopes.is_empty());
    }

    #[test]
    fn test_scope_context() {
        let scope = make_scope("Locals", 1);
        let sc = ScopeContext::new(scope);
        assert_eq!(sc.scope.name, "Locals");
        assert_eq!(sc.scope.variables_reference, 1);
        assert!(sc.variables.is_empty());
    }

    #[test]
    fn test_context_chain_counts() {
        let mut chain = ContextChain::new();

        let mut tc = ThreadContext::new(make_thread(1, "main"));
        let mut fc = FrameContext::new(make_frame(100, "main", 10));
        let mut sc = ScopeContext::new(make_scope("Locals", 1));

        // Add an unexpanded variable to the scope
        use crate::variables::ExpandedVariable;
        use dap_types::types::Variable;
        let var = Variable {
            name: "x".to_string(),
            value: "42".to_string(),
            var_type: Some("int".to_string()),
            presentation_hint: None,
            evaluate_name: None,
            variables_reference: 0,
            named_variables: None,
            indexed_variables: None,
            memory_reference: None,
            declaration_location_reference: None,
            value_location_reference: None,
        };
        sc.variables.push(ExpandedVariable::new(var, 0));

        fc.scopes.push(sc);
        tc.frames.push(fc);
        chain.threads.push(tc);

        assert_eq!(chain.total_frames(), 1);
        assert_eq!(chain.total_variables(), 1);
    }
}
