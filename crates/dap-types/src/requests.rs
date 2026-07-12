//! All 42 client-to-adapter DAP requests with their arguments and response types.
//!
//! Each request is represented by a unit struct implementing the `DapRequest` trait,
//! which associates a `const COMMAND: &str`, an `Arguments` type, and a `Response` type.
//!
//! Request argument structs use `#[serde(rename_all = "camelCase")]` to match the DAP
//! JSON wire format. Optional fields use `#[serde(skip_serializing_if = "Option::is_none")]`.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::capabilities::Capabilities;
use crate::enums::*;
use crate::types::*;

// ── DapRequest trait ───────────────────────────────────────────────

/// Trait that binds a DAP command to its argument and response types.
pub trait DapRequest {
    /// The command string sent over the wire (e.g. "launch", "setBreakpoints").
    const COMMAND: &'static str;

    /// The type of the `arguments` field for this request.
    type Arguments: Serialize + DeserializeOwned;

    /// The type extracted from the response `body` on success.
    type Response: Serialize + DeserializeOwned;
}

// ── Helper: empty types for no-argument / no-response-body requests ─

/// Placeholder for requests that take no arguments.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NoArguments {}

/// Placeholder for responses that have no body.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NoResponseBody {}

// ============================================================================
// 1. initialize
// ============================================================================

pub struct InitializeRequest;

impl DapRequest for InitializeRequest {
    const COMMAND: &'static str = "initialize";
    type Arguments = InitializeRequestArguments;
    type Response = Capabilities;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequestArguments {
    /// ID of the client (e.g. "vscode").
    /// DAP spec uses "clientID" (uppercase ID), not "clientId" (camelCase default).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "clientID")]
    pub client_id: Option<String>,
    /// Human-readable name of the client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// ID of the debug adapter.
    /// DAP spec uses "adapterID" (uppercase ID), required field.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "adapterID")]
    pub adapter_id: Option<String>,
    /// ISO-639 locale (e.g. "en-US").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// If true, all line numbers are 1-based (default true).
    #[serde(default = "default_true", skip_serializing_if = "Option::is_none")]
    pub lines_start_at1: Option<bool>,
    /// If true, all column numbers are 1-based (default true).
    #[serde(default = "default_true", skip_serializing_if = "Option::is_none")]
    pub columns_start_at1: Option<bool>,
    /// Format for paths ("path" or "uri").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_format: Option<String>,
    /// Client supports the `type` attribute for variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_variable_type: Option<bool>,
    /// Client supports paging of variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_variable_paging: Option<bool>,
    /// Client supports the `runInTerminal` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_run_in_terminal_request: Option<bool>,
    /// Client supports memory references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_memory_references: Option<bool>,
    /// Client supports progress reporting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_progress_reporting: Option<bool>,
    /// Client supports the `invalidated` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_invalidated_event: Option<bool>,
    /// Client supports the `memory` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_memory_event: Option<bool>,
    /// Client supports shell interpretation in `runInTerminal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_args_can_be_interpreted_by_shell: Option<bool>,
    /// Client supports the `startDebugging` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_start_debugging_request: Option<bool>,
    /// Client supports ANSI styling in output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_ansi_styling: Option<bool>,
}

fn default_true() -> Option<bool> {
    Some(true)
}

// ============================================================================
// 2. launch
// ============================================================================

pub struct LaunchRequest;

impl DapRequest for LaunchRequest {
    const COMMAND: &'static str = "launch";
    type Arguments = LaunchRequestArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRequestArguments {
    /// If true, launch without enabling debugging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_debug: Option<bool>,
    /// Arbitrary data from a previous restarted session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub __restart: Option<serde_json::Value>,
    /// Additional implementation-specific attributes are flattened.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

impl LaunchRequestArguments {
    /// Create launch arguments with a program path.
    pub fn with_program(program: &str) -> Self {
        LaunchRequestArguments {
            no_debug: None,
            __restart: None,
            extra: serde_json::json!({"program": program}),
        }
    }
}

// ============================================================================
// 3. attach
// ============================================================================

pub struct AttachRequest;

impl DapRequest for AttachRequest {
    const COMMAND: &'static str = "attach";
    type Arguments = AttachRequestArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachRequestArguments {
    /// Arbitrary data from a previous restarted session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub __restart: Option<serde_json::Value>,
    /// Additional implementation-specific attributes.
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

// ============================================================================
// 4. restart
// ============================================================================

pub struct RestartRequest;

impl DapRequest for RestartRequest {
    const COMMAND: &'static str = "restart";
    type Arguments = RestartArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RestartArguments {
    /// Latest launch/attach configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<serde_json::Value>,
}

// ============================================================================
// 5. disconnect
// ============================================================================

pub struct DisconnectRequest;

impl DapRequest for DisconnectRequest {
    const COMMAND: &'static str = "disconnect";
    type Arguments = DisconnectArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectArguments {
    /// Whether this disconnect is part of a restart sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<bool>,
    /// Whether to terminate the debuggee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminate_debuggee: Option<bool>,
    /// Whether the debuggee should stay suspended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspend_debuggee: Option<bool>,
}

// ============================================================================
// 6. terminate
// ============================================================================

pub struct TerminateRequest;

impl DapRequest for TerminateRequest {
    const COMMAND: &'static str = "terminate";
    type Arguments = TerminateArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TerminateArguments {
    /// Whether this is part of a restart sequence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<bool>,
}

// ============================================================================
// 7. configurationDone
// ============================================================================

pub struct ConfigurationDoneRequest;

impl DapRequest for ConfigurationDoneRequest {
    const COMMAND: &'static str = "configurationDone";
    type Arguments = NoArguments;
    type Response = NoResponseBody;
}

// ============================================================================
// 8. setBreakpoints
// ============================================================================

pub struct SetBreakpointsRequest;

impl DapRequest for SetBreakpointsRequest {
    const COMMAND: &'static str = "setBreakpoints";
    type Arguments = SetBreakpointsArguments;
    type Response = SetBreakpointsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetBreakpointsArguments {
    /// Source location.
    pub source: Source,
    /// Code locations of breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakpoints: Option<Vec<SourceBreakpoint>>,
    /// Deprecated: code locations as line numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<Vec<u64>>,
    /// Whether the underlying source was modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_modified: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetBreakpointsResponse {
    /// The actual breakpoints after setting.
    pub breakpoints: Vec<Breakpoint>,
}

// ============================================================================
// 9. setFunctionBreakpoints
// ============================================================================

pub struct SetFunctionBreakpointsRequest;

impl DapRequest for SetFunctionBreakpointsRequest {
    const COMMAND: &'static str = "setFunctionBreakpoints";
    type Arguments = SetFunctionBreakpointsArguments;
    type Response = SetFunctionBreakpointsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetFunctionBreakpointsArguments {
    pub breakpoints: Vec<FunctionBreakpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetFunctionBreakpointsResponse {
    pub breakpoints: Vec<Breakpoint>,
}

// ============================================================================
// 10. setExceptionBreakpoints
// ============================================================================

pub struct SetExceptionBreakpointsRequest;

impl DapRequest for SetExceptionBreakpointsRequest {
    const COMMAND: &'static str = "setExceptionBreakpoints";
    type Arguments = SetExceptionBreakpointsArguments;
    type Response = SetExceptionBreakpointsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExceptionBreakpointsArguments {
    pub filters: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_options: Option<Vec<ExceptionFilterOptions>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exception_options: Option<Vec<ExceptionOptions>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetExceptionBreakpointsResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breakpoints: Option<Vec<Breakpoint>>,
}

// ============================================================================
// 11. setDataBreakpoints
// ============================================================================

pub struct SetDataBreakpointsRequest;

impl DapRequest for SetDataBreakpointsRequest {
    const COMMAND: &'static str = "setDataBreakpoints";
    type Arguments = SetDataBreakpointsArguments;
    type Response = SetDataBreakpointsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetDataBreakpointsArguments {
    pub breakpoints: Vec<DataBreakpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetDataBreakpointsResponse {
    pub breakpoints: Vec<Breakpoint>,
}

// ============================================================================
// 12. setInstructionBreakpoints
// ============================================================================

pub struct SetInstructionBreakpointsRequest;

impl DapRequest for SetInstructionBreakpointsRequest {
    const COMMAND: &'static str = "setInstructionBreakpoints";
    type Arguments = SetInstructionBreakpointsArguments;
    type Response = SetInstructionBreakpointsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetInstructionBreakpointsArguments {
    pub breakpoints: Vec<InstructionBreakpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetInstructionBreakpointsResponse {
    pub breakpoints: Vec<Breakpoint>,
}

// ============================================================================
// 13. breakpointLocations
// ============================================================================

pub struct BreakpointLocationsRequest;

impl DapRequest for BreakpointLocationsRequest {
    const COMMAND: &'static str = "breakpointLocations";
    type Arguments = BreakpointLocationsArguments;
    type Response = BreakpointLocationsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointLocationsArguments {
    pub source: Source,
    pub line: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointLocationsResponse {
    pub breakpoints: Vec<BreakpointLocation>,
}

// ============================================================================
// 14. dataBreakpointInfo
// ============================================================================

pub struct DataBreakpointInfoRequest;

impl DapRequest for DataBreakpointInfoRequest {
    const COMMAND: &'static str = "dataBreakpointInfo";
    type Arguments = DataBreakpointInfoArguments;
    type Response = DataBreakpointInfoResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataBreakpointInfoArguments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables_reference: Option<u64>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_address: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataBreakpointInfoResponse {
    pub data_id: Option<String>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_types: Option<Vec<DataBreakpointAccessType>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_persist: Option<bool>,
}

// ============================================================================
// 15. continue
// ============================================================================

pub struct ContinueRequest;

impl DapRequest for ContinueRequest {
    const COMMAND: &'static str = "continue";
    type Arguments = ContinueArguments;
    type Response = ContinueResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueArguments {
    pub thread_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_thread: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_threads_continued: Option<bool>,
}

// ============================================================================
// 16. next (step over)
// ============================================================================

pub struct NextRequest;

impl DapRequest for NextRequest {
    const COMMAND: &'static str = "next";
    type Arguments = NextArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextArguments {
    pub thread_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_thread: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<SteppingGranularity>,
}

// ============================================================================
// 17. stepIn
// ============================================================================

pub struct StepInRequest;

impl DapRequest for StepInRequest {
    const COMMAND: &'static str = "stepIn";
    type Arguments = StepInArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInArguments {
    pub thread_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_thread: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<SteppingGranularity>,
}

// ============================================================================
// 18. stepOut
// ============================================================================

pub struct StepOutRequest;

impl DapRequest for StepOutRequest {
    const COMMAND: &'static str = "stepOut";
    type Arguments = StepOutArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepOutArguments {
    pub thread_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_thread: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<SteppingGranularity>,
}

// ============================================================================
// 19. stepBack
// ============================================================================

pub struct StepBackRequest;

impl DapRequest for StepBackRequest {
    const COMMAND: &'static str = "stepBack";
    type Arguments = StepBackArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepBackArguments {
    pub thread_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_thread: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity: Option<SteppingGranularity>,
}

// ============================================================================
// 20. reverseContinue
// ============================================================================

pub struct ReverseContinueRequest;

impl DapRequest for ReverseContinueRequest {
    const COMMAND: &'static str = "reverseContinue";
    type Arguments = ReverseContinueArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReverseContinueArguments {
    pub thread_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_thread: Option<bool>,
}

// ============================================================================
// 21. restartFrame
// ============================================================================

pub struct RestartFrameRequest;

impl DapRequest for RestartFrameRequest {
    const COMMAND: &'static str = "restartFrame";
    type Arguments = RestartFrameArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestartFrameArguments {
    pub frame_id: u64,
}

// ============================================================================
// 22. goto
// ============================================================================

pub struct GotoRequest;

impl DapRequest for GotoRequest {
    const COMMAND: &'static str = "goto";
    type Arguments = GotoArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GotoArguments {
    pub thread_id: u64,
    pub target_id: u64,
}

// ============================================================================
// 23. pause
// ============================================================================

pub struct PauseRequest;

impl DapRequest for PauseRequest {
    const COMMAND: &'static str = "pause";
    type Arguments = PauseArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseArguments {
    pub thread_id: u64,
}

// ============================================================================
// 24. stackTrace
// ============================================================================

pub struct StackTraceRequest;

impl DapRequest for StackTraceRequest {
    const COMMAND: &'static str = "stackTrace";
    type Arguments = StackTraceArguments;
    type Response = StackTraceResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackTraceArguments {
    pub thread_id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_frame: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<StackFrameFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackTraceResponse {
    pub stack_frames: Vec<StackFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_frames: Option<u64>,
}

// ============================================================================
// 25. scopes
// ============================================================================

pub struct ScopesRequest;

impl DapRequest for ScopesRequest {
    const COMMAND: &'static str = "scopes";
    type Arguments = ScopesArguments;
    type Response = ScopesResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopesArguments {
    pub frame_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopesResponse {
    pub scopes: Vec<Scope>,
}

// ============================================================================
// 26. variables
// ============================================================================

pub struct VariablesRequest;

impl DapRequest for VariablesRequest {
    const COMMAND: &'static str = "variables";
    type Arguments = VariablesArguments;
    type Response = VariablesResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariablesArguments {
    pub variables_reference: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<VariableFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ValueFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariablesResponse {
    pub variables: Vec<Variable>,
}

// ============================================================================
// 27. setVariable
// ============================================================================

pub struct SetVariableRequest;

impl DapRequest for SetVariableRequest {
    const COMMAND: &'static str = "setVariable";
    type Arguments = SetVariableArguments;
    type Response = SetVariableResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetVariableArguments {
    pub variables_reference: u64,
    pub name: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ValueFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetVariableResponse {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub var_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables_reference: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_location_reference: Option<u64>,
}

// ============================================================================
// 28. source
// ============================================================================

pub struct SourceRequest;

impl DapRequest for SourceRequest {
    const COMMAND: &'static str = "source";
    type Arguments = SourceArguments;
    type Response = SourceResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceArguments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    pub source_reference: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceResponse {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

// ============================================================================
// 29. threads
// ============================================================================

pub struct ThreadsRequest;

impl DapRequest for ThreadsRequest {
    const COMMAND: &'static str = "threads";
    type Arguments = NoArguments;
    type Response = ThreadsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadsResponse {
    pub threads: Vec<Thread>,
}

// ============================================================================
// 30. terminateThreads
// ============================================================================

pub struct TerminateThreadsRequest;

impl DapRequest for TerminateThreadsRequest {
    const COMMAND: &'static str = "terminateThreads";
    type Arguments = TerminateThreadsArguments;
    type Response = NoResponseBody;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminateThreadsArguments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_ids: Option<Vec<u64>>,
}

// ============================================================================
// 31. modules
// ============================================================================

pub struct ModulesRequest;

impl DapRequest for ModulesRequest {
    const COMMAND: &'static str = "modules";
    type Arguments = ModulesArguments;
    type Response = ModulesResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModulesArguments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_module: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModulesResponse {
    pub modules: Vec<Module>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_modules: Option<u64>,
}

// ============================================================================
// 32. loadedSources
// ============================================================================

pub struct LoadedSourcesRequest;

impl DapRequest for LoadedSourcesRequest {
    const COMMAND: &'static str = "loadedSources";
    type Arguments = NoArguments;
    type Response = LoadedSourcesResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedSourcesResponse {
    pub sources: Vec<Source>,
}

// ============================================================================
// 33. evaluate
// ============================================================================

pub struct EvaluateRequest;

impl DapRequest for EvaluateRequest {
    const COMMAND: &'static str = "evaluate";
    type Arguments = EvaluateArguments;
    type Response = EvaluateResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateArguments {
    pub expression: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<EvaluateContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ValueFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateResponse {
    pub result: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub var_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<VariablePresentationHint>,
    pub variables_reference: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_location_reference: Option<u64>,
}

// ============================================================================
// 34. setExpression
// ============================================================================

pub struct SetExpressionRequest;

impl DapRequest for SetExpressionRequest {
    const COMMAND: &'static str = "setExpression";
    type Arguments = SetExpressionArguments;
    type Response = SetExpressionResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExpressionArguments {
    pub expression: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<ValueFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetExpressionResponse {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub var_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<VariablePresentationHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables_reference: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_location_reference: Option<u64>,
}

// ============================================================================
// 35. stepInTargets
// ============================================================================

pub struct StepInTargetsRequest;

impl DapRequest for StepInTargetsRequest {
    const COMMAND: &'static str = "stepInTargets";
    type Arguments = StepInTargetsArguments;
    type Response = StepInTargetsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepInTargetsArguments {
    pub frame_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepInTargetsResponse {
    pub targets: Vec<StepInTarget>,
}

// ============================================================================
// 36. gotoTargets
// ============================================================================

pub struct GotoTargetsRequest;

impl DapRequest for GotoTargetsRequest {
    const COMMAND: &'static str = "gotoTargets";
    type Arguments = GotoTargetsArguments;
    type Response = GotoTargetsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GotoTargetsArguments {
    pub source: Source,
    pub line: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GotoTargetsResponse {
    pub targets: Vec<GotoTarget>,
}

// ============================================================================
// 37. completions
// ============================================================================

pub struct CompletionsRequest;

impl DapRequest for CompletionsRequest {
    const COMMAND: &'static str = "completions";
    type Arguments = CompletionsArguments;
    type Response = CompletionsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionsArguments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<u64>,
    pub text: String,
    pub column: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionsResponse {
    pub targets: Vec<CompletionItem>,
}

// ============================================================================
// 38. exceptionInfo
// ============================================================================

pub struct ExceptionInfoRequest;

impl DapRequest for ExceptionInfoRequest {
    const COMMAND: &'static str = "exceptionInfo";
    type Arguments = ExceptionInfoArguments;
    type Response = ExceptionInfoResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionInfoArguments {
    pub thread_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionInfoResponse {
    pub exception_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub break_mode: ExceptionBreakMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<ExceptionDetails>,
}

// ============================================================================
// 39. readMemory
// ============================================================================

pub struct ReadMemoryRequest;

impl DapRequest for ReadMemoryRequest {
    const COMMAND: &'static str = "readMemory";
    type Arguments = ReadMemoryArguments;
    type Response = ReadMemoryResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadMemoryArguments {
    pub memory_reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    pub count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadMemoryResponse {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable_bytes: Option<u64>,
    /// Base64 encoded bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

// ============================================================================
// 40. writeMemory
// ============================================================================

pub struct WriteMemoryRequest;

impl DapRequest for WriteMemoryRequest {
    const COMMAND: &'static str = "writeMemory";
    type Arguments = WriteMemoryArguments;
    type Response = WriteMemoryResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteMemoryArguments {
    pub memory_reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_partial: Option<bool>,
    /// Base64 encoded bytes to write.
    pub data: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteMemoryResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_written: Option<u64>,
}

// ============================================================================
// 41. disassemble
// ============================================================================

pub struct DisassembleRequest;

impl DapRequest for DisassembleRequest {
    const COMMAND: &'static str = "disassemble";
    type Arguments = DisassembleArguments;
    type Response = DisassembleResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisassembleArguments {
    pub memory_reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_offset: Option<i64>,
    pub instruction_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolve_symbols: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DisassembleResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Vec<DisassembledInstruction>>,
}

// ============================================================================
// 42. locations
// ============================================================================

pub struct LocationsRequest;

impl DapRequest for LocationsRequest {
    const COMMAND: &'static str = "locations";
    type Arguments = LocationsArguments;
    type Response = LocationsResponse;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationsArguments {
    pub location_reference: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationsResponse {
    pub source: Source,
    pub line: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_args_serde() {
        let args = InitializeRequestArguments {
            adapter_id: Some("codelldb".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&args).unwrap();
        assert!(json.contains("codelldb"));
        let back: InitializeRequestArguments = serde_json::from_str(&json).unwrap();
        assert_eq!(back.adapter_id.as_deref(), Some("codelldb"));
    }

    #[test]
    fn test_set_breakpoints_args() {
        let args = SetBreakpointsArguments {
            source: Source {
                name: Some("main.cpp".into()),
                path: Some("/src/main.cpp".into()),
                ..Default::default()
            },
            breakpoints: Some(vec![SourceBreakpoint {
                line: 42,
                column: None,
                condition: None,
                hit_condition: None,
                log_message: None,
                mode: None,
            }]),
            lines: None,
            source_modified: None,
        };
        let json = serde_json::to_string(&args).unwrap();
        assert!(json.contains("main.cpp"));
        assert!(json.contains("42"));
    }

    #[test]
    fn test_launch_with_program() {
        let args = LaunchRequestArguments::with_program("/path/to/elf");
        let json = serde_json::to_string(&args).unwrap();
        assert!(json.contains("/path/to/elf"));
    }

    #[test]
    fn test_evaluate_response() {
        let json = r#"{"result":"42","type":"int","variablesReference":0}"#;
        let resp: EvaluateResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.result, "42");
        assert_eq!(resp.var_type.as_deref(), Some("int"));
        assert_eq!(resp.variables_reference, 0);
    }

    /// Verify that all 42 DAP request types have a non-empty COMMAND constant.
    ///
    /// The COMMAND string is sent as the `command` field in the DAP request
    /// message; an empty string would produce an invalid protocol message.
    /// This test also serves as a catalogue of every request type.
    #[test]
    fn test_all_42_commands_non_empty() {
        let commands: &[(&str, &str)] = &[
            ("initialize", InitializeRequest::COMMAND),
            ("launch", LaunchRequest::COMMAND),
            ("attach", AttachRequest::COMMAND),
            ("restart", RestartRequest::COMMAND),
            ("disconnect", DisconnectRequest::COMMAND),
            ("terminate", TerminateRequest::COMMAND),
            ("configurationDone", ConfigurationDoneRequest::COMMAND),
            ("setBreakpoints", SetBreakpointsRequest::COMMAND),
            (
                "setFunctionBreakpoints",
                SetFunctionBreakpointsRequest::COMMAND,
            ),
            (
                "setExceptionBreakpoints",
                SetExceptionBreakpointsRequest::COMMAND,
            ),
            ("setDataBreakpoints", SetDataBreakpointsRequest::COMMAND),
            (
                "setInstructionBreakpoints",
                SetInstructionBreakpointsRequest::COMMAND,
            ),
            ("breakpointLocations", BreakpointLocationsRequest::COMMAND),
            ("dataBreakpointInfo", DataBreakpointInfoRequest::COMMAND),
            ("continue", ContinueRequest::COMMAND),
            ("next", NextRequest::COMMAND),
            ("stepIn", StepInRequest::COMMAND),
            ("stepOut", StepOutRequest::COMMAND),
            ("stepBack", StepBackRequest::COMMAND),
            ("reverseContinue", ReverseContinueRequest::COMMAND),
            ("restartFrame", RestartFrameRequest::COMMAND),
            ("goto", GotoRequest::COMMAND),
            ("pause", PauseRequest::COMMAND),
            ("stackTrace", StackTraceRequest::COMMAND),
            ("scopes", ScopesRequest::COMMAND),
            ("variables", VariablesRequest::COMMAND),
            ("setVariable", SetVariableRequest::COMMAND),
            ("source", SourceRequest::COMMAND),
            ("threads", ThreadsRequest::COMMAND),
            ("terminateThreads", TerminateThreadsRequest::COMMAND),
            ("modules", ModulesRequest::COMMAND),
            ("loadedSources", LoadedSourcesRequest::COMMAND),
            ("evaluate", EvaluateRequest::COMMAND),
            ("setExpression", SetExpressionRequest::COMMAND),
            ("stepInTargets", StepInTargetsRequest::COMMAND),
            ("gotoTargets", GotoTargetsRequest::COMMAND),
            ("completions", CompletionsRequest::COMMAND),
            ("exceptionInfo", ExceptionInfoRequest::COMMAND),
            ("readMemory", ReadMemoryRequest::COMMAND),
            ("writeMemory", WriteMemoryRequest::COMMAND),
            ("disassemble", DisassembleRequest::COMMAND),
            ("locations", LocationsRequest::COMMAND),
        ];

        assert_eq!(
            commands.len(),
            42,
            "Should have exactly 42 request types, found {}",
            commands.len()
        );

        for (name, cmd) in commands {
            assert!(!cmd.is_empty(), "{name}::COMMAND should not be empty");
        }
    }
}
