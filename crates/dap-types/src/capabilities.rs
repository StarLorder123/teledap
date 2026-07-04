//! The `Capabilities` type returned by the `initialize` response.
//!
//! This is the largest single type in DAP, with ~50 optional fields that
//! describe what features the debug adapter supports.

use serde::{Deserialize, Serialize};

use crate::enums::ChecksumAlgorithm;
use crate::types::{BreakpointMode, ColumnDescriptor, ExceptionBreakpointsFilter};

/// Information about the debug adapter's capabilities.
///
/// Almost every field is an `Option<bool>` because:
/// - The adapter may omit unsupported capabilities entirely.
/// - `false` is the default for all boolean capabilities per the spec.
/// - Some fields are non-boolean (lists of filters, columns, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// The debug adapter supports the `configurationDone` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_configuration_done_request: Option<bool>,

    /// The debug adapter supports function breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_function_breakpoints: Option<bool>,

    /// The debug adapter supports conditional breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_conditional_breakpoints: Option<bool>,

    /// The debug adapter supports breakpoints that break after a number of hits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_hit_conditional_breakpoints: Option<bool>,

    /// The debug adapter supports `evaluate` for hover contexts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_evaluate_for_hovers: Option<bool>,

    /// Available exception filter options for `setExceptionBreakpoints`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exception_breakpoint_filters: Option<Vec<ExceptionBreakpointsFilter>>,

    /// The debug adapter supports stepping backwards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_step_back: Option<bool>,

    /// The debug adapter supports setting a variable to a value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_set_variable: Option<bool>,

    /// The debug adapter supports restarting a frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_restart_frame: Option<bool>,

    /// The debug adapter supports the `gotoTargets` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_goto_targets_request: Option<bool>,

    /// The debug adapter supports the `stepInTargets` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_step_in_targets_request: Option<bool>,

    /// The debug adapter supports the `completions` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_completions_request: Option<bool>,

    /// Characters that trigger completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_trigger_characters: Option<Vec<String>>,

    /// The debug adapter supports the `modules` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_modules_request: Option<bool>,

    /// Additional columns supported by the modules view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_module_columns: Option<Vec<ColumnDescriptor>>,

    /// Checksum algorithms supported by the debug adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_checksum_algorithms: Option<Vec<ChecksumAlgorithm>>,

    /// The debug adapter supports the `restart` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_restart_request: Option<bool>,

    /// The debug adapter supports exception options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_exception_options: Option<bool>,

    /// The debug adapter supports value formatting options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_value_formatting_options: Option<bool>,

    /// The debug adapter supports the `exceptionInfo` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_exception_info_request: Option<bool>,

    /// The debug adapter supports the `terminateDebuggee` attribute on `disconnect`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_terminate_debuggee: Option<bool>,

    /// The debug adapter supports the `suspendDebuggee` attribute on `disconnect`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_suspend_debuggee: Option<bool>,

    /// The debug adapter supports delayed loading of parts of the stack.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_delayed_stack_trace_loading: Option<bool>,

    /// The debug adapter supports the `loadedSources` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_loaded_sources_request: Option<bool>,

    /// The debug adapter supports log points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_log_points: Option<bool>,

    /// The debug adapter supports the `terminateThreads` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_terminate_threads_request: Option<bool>,

    /// The debug adapter supports the `setExpression` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_set_expression: Option<bool>,

    /// The debug adapter supports the `terminate` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_terminate_request: Option<bool>,

    /// The debug adapter supports data breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_data_breakpoints: Option<bool>,

    /// The debug adapter supports the `readMemory` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_read_memory_request: Option<bool>,

    /// The debug adapter supports the `writeMemory` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_write_memory_request: Option<bool>,

    /// The debug adapter supports the `disassemble` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_disassemble_request: Option<bool>,

    /// The debug adapter supports the `cancel` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_cancel_request: Option<bool>,

    /// The debug adapter supports the `breakpointLocations` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_breakpoint_locations_request: Option<bool>,

    /// The debug adapter supports clipboard context for `evaluate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_clipboard_context: Option<bool>,

    /// The debug adapter supports stepping granularities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_stepping_granularity: Option<bool>,

    /// The debug adapter supports instruction breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_instruction_breakpoints: Option<bool>,

    /// The debug adapter supports exception filter options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_exception_filter_options: Option<bool>,

    /// The debug adapter supports single-thread execution requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_single_thread_execution_requests: Option<bool>,

    /// The debug adapter supports requesting byte counts for data breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_data_breakpoint_bytes: Option<bool>,

    /// Available breakpoint modes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakpoint_modes: Option<Vec<BreakpointMode>>,

    /// The debug adapter supports ANSI styling in output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_ansi_styling: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_capabilities() {
        let caps = Capabilities::default();
        let json = serde_json::to_string(&caps).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn test_partial_capabilities() {
        let json = r#"{"supportsConfigurationDoneRequest":true,"supportsStepBack":false}"#;
        let caps: Capabilities = serde_json::from_str(json).unwrap();
        assert_eq!(caps.supports_configuration_done_request, Some(true));
        assert_eq!(caps.supports_step_back, Some(false));
        assert!(caps.supports_function_breakpoints.is_none());
    }

    #[test]
    fn test_complex_capability() {
        let json = r#"{"exceptionBreakpointFilters":[{"filter":"cpp_throw","label":"C++ Throw","default":false}]}"#;
        let caps: Capabilities = serde_json::from_str(json).unwrap();
        let filters = caps.exception_breakpoint_filters.unwrap();
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].filter, "cpp_throw");
        assert_eq!(filters[0].label, "C++ Throw");
    }
}
