//! Data types defined in the DAP specification.
//!
//! All types use `#[serde(rename_all = "camelCase")]` to match the DAP JSON wire format.
//! Optional fields use `#[serde(skip_serializing_if = "Option::is_none")]`.

use crate::enums::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Message ────────────────────────────────────────────────────────

/// A structured message object. Used for error details and other user-visible messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique identifier for the message.
    pub id: u64,
    /// Format string. Variables can be used with `{name}`.
    pub format: String,
    /// Variables for format string substitution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables: Option<HashMap<String, String>>,
    /// If true, send this message to telemetry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_telemetry: Option<bool>,
    /// If true, show this message to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_user: Option<bool>,
    /// URL for additional information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Label for the URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_label: Option<String>,
}

// ── Module ─────────────────────────────────────────────────────────

/// A module or shared library loaded into the debuggee.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    /// Unique identifier for the module (number or string).
    pub id: ModuleId,
    /// A name for the module.
    pub name: String,
    /// Logical full path to the module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// True if the module is optimized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_optimized: Option<bool>,
    /// True if the module is considered user code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_user_code: Option<bool>,
    /// Version information for the module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Description of symbol status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_status: Option<String>,
    /// Logical full path to the symbol file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_file_path: Option<String>,
    /// Module creation or modification timestamp (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_time_stamp: Option<String>,
    /// Address range covered by this module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address_range: Option<String>,
}

/// A module ID can be either a number or a string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModuleId {
    Number(u64),
    String(String),
}

// ── ColumnDescriptor ───────────────────────────────────────────────

/// Describes an additional column for the Modules view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDescriptor {
    /// Name of the attribute rendered in this column.
    #[serde(rename = "attributeName")]
    pub attribute_name: String,
    /// Header UI label of the column.
    pub label: String,
    /// Format string for rendering values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Datatype of the column values.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub column_type: Option<ColumnDescriptorType>,
    /// Width of this column in characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
}

// ── Thread ─────────────────────────────────────────────────────────

/// A thread in the debuggee.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    /// Unique identifier for the thread.
    pub id: u64,
    /// Name of the thread.
    pub name: String,
}

// ── Source ─────────────────────────────────────────────────────────

/// A source file or location.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Source {
    /// Short name of the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Path of the source to show in the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// If > 0, the contents must be retrieved through the `source` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<u32>,
    /// Hint for how to present the source in the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<SourcePresentationHint>,
    /// Origin of this source (e.g. "internal module").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Related sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<Source>>,
    /// Additional data persisted across debug sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_data: Option<serde_json::Value>,
    /// Checksums associated with this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksums: Option<Vec<Checksum>>,
}

// ── StackFrame ─────────────────────────────────────────────────────

/// A stack frame in a debug session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    /// Unique identifier for the frame across all threads.
    pub id: u64,
    /// Name of the frame (typically the method name).
    pub name: String,
    /// Source location of the frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// Line within the source (0 if no source).
    pub line: u64,
    /// Start column position in UTF-16 code units.
    pub column: u64,
    /// End line of the range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    /// End column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
    /// Whether this frame can be restarted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_restart: Option<bool>,
    /// Memory reference for the instruction pointer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_pointer_reference: Option<String>,
    /// Module associated with this frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_id: Option<ModuleId>,
    /// Hint for UI presentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<StackFramePresentationHint>,
}

// ── Scope ──────────────────────────────────────────────────────────

/// A variable scope (e.g. arguments, locals, registers).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    /// Name of the scope.
    pub name: String,
    /// Hint for UI presentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<ScopePresentationHint>,
    /// Reference for retrieving child variables.
    pub variables_reference: u64,
    /// Number of named variables in this scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<u64>,
    /// Number of indexed variables in this scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<u64>,
    /// If true, retrieving children is expensive.
    pub expensive: bool,
    /// Source location for this scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// Start line of the range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Start column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    /// End line of the range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    /// End column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
}

// ── Variable ───────────────────────────────────────────────────────

/// A variable in the debuggee.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    /// The variable's name.
    pub name: String,
    /// The variable's value (can be multi-line).
    pub value: String,
    /// The type of the variable's value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub var_type: Option<String>,
    /// Presentation properties for the variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<VariablePresentationHint>,
    /// Evaluatable name usable in an `evaluate` request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluate_name: Option<String>,
    /// If > 0, children can be retrieved via `variables` request.
    pub variables_reference: u64,
    /// Number of named child variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<u64>,
    /// Number of indexed child variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<u64>,
    /// Memory reference associated with this variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_reference: Option<String>,
    /// Reference for variable declaration location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declaration_location_reference: Option<u64>,
    /// Reference for value's declaration location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_location_reference: Option<u64>,
}

// ── VariablePresentationHint ───────────────────────────────────────

/// Properties of a variable for UI presentation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VariablePresentationHint {
    /// The kind of the variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<VariableKind>,
    /// Set of attributes of the variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Vec<VariableAttribute>>,
    /// Visibility of the variable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<VariableVisibility>,
    /// If true, clients can present the variable with a lazy evaluation gesture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lazy: Option<bool>,
}

// ── BreakpointLocation ─────────────────────────────────────────────

/// A possible location for a breakpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointLocation {
    /// Start line of the breakpoint location.
    pub line: u64,
    /// Start column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    /// End line if the location covers a range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    /// End column if the location covers a range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
}

// ── SourceBreakpoint ───────────────────────────────────────────────

/// Properties of a breakpoint to be set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceBreakpoint {
    /// The source line.
    pub line: u64,
    /// Start column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    /// Expression for conditional breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Expression for hit count conditional breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Log message for logpoints. Expressions in `{}` are interpolated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
    /// Breakpoint mode from Capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

// ── FunctionBreakpoint ─────────────────────────────────────────────

/// Properties of a function breakpoint to be set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionBreakpoint {
    /// Name of the function.
    pub name: String,
    /// Expression for conditional breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Expression for hit count conditional breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
}

// ── DataBreakpoint ─────────────────────────────────────────────────

/// Properties of a data breakpoint to be set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataBreakpoint {
    /// ID representing the data (from `dataBreakpointInfo`).
    pub data_id: String,
    /// Access type of the data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_type: Option<DataBreakpointAccessType>,
    /// Expression for conditional breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Expression for hit count conditional breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
}

// ── InstructionBreakpoint ──────────────────────────────────────────

/// Properties of an instruction breakpoint to be set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionBreakpoint {
    /// Instruction reference (memory or instruction pointer).
    pub instruction_reference: String,
    /// Offset from the instruction reference in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Expression for conditional breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Expression for hit count conditional breakpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// Breakpoint mode from Capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

// ── Breakpoint ─────────────────────────────────────────────────────

/// Information about a breakpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    /// Identifier for the breakpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// If true, the breakpoint could be set (but not necessarily at the desired location).
    pub verified: bool,
    /// Message about the state of the breakpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Source where the breakpoint is located.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// Start line of the actual range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Start column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    /// End line of the actual range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    /// End column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
    /// Memory reference to where the breakpoint is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_reference: Option<String>,
    /// Offset from instruction reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Reason why the breakpoint may not be verified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<BreakpointReason>,
}

// ── StepInTarget ───────────────────────────────────────────────────

/// A target into which a `stepIn` request can go.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepInTarget {
    /// Unique identifier for the step-in target.
    pub id: u64,
    /// Name shown in the UI.
    pub label: String,
    /// Line of the step-in target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    /// End line of the range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    /// End column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
}

// ── GotoTarget ─────────────────────────────────────────────────────

/// A target for a `goto` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GotoTarget {
    /// Unique identifier for the goto target.
    pub id: u64,
    /// Name shown in the UI.
    pub label: String,
    /// Line of the goto target.
    pub line: u64,
    /// Column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    /// End line of the range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    /// End column in UTF-16 code units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
    /// Memory reference for the instruction pointer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_pointer_reference: Option<String>,
}

// ── CompletionItem ─────────────────────────────────────────────────

/// A completion suggestion for the `completions` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    /// Label of this completion item (also the default insert text).
    pub label: String,
    /// If non-empty, inserted instead of the label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// String used for sorting comparisons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_text: Option<String>,
    /// Additional info like type or symbol.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The item's type.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "type")]
    pub item_type: Option<CompletionItemType>,
    /// Start position for insertion (in UTF-16 code units).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u64>,
    /// Number of characters to overwrite (in UTF-16 code units, default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
    /// Start of new selection after insertion (in UTF-16 code units).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_start: Option<u64>,
    /// Length of new selection (in UTF-16 code units, default 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_length: Option<u64>,
}

// ── Checksum ───────────────────────────────────────────────────────

/// Checksum of a source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checksum {
    /// Algorithm used to calculate the checksum.
    pub algorithm: ChecksumAlgorithm,
    /// Value of the checksum, encoded as hex.
    pub checksum: String,
}

// ── ValueFormat ────────────────────────────────────────────────────

/// Formatting options for values.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValueFormat {
    /// Display the value in hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hex: Option<bool>,
}

// ── StackFrameFormat ───────────────────────────────────────────────

/// Formatting options for stack frames.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StackFrameFormat {
    /// Display the value in hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hex: Option<bool>,
    /// Display parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<bool>,
    /// Display types of parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_types: Option<bool>,
    /// Display names of parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_names: Option<bool>,
    /// Display values of parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_values: Option<bool>,
    /// Display the line number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<bool>,
    /// Display the module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<bool>,
    /// Include all stack frames including hidden ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_all: Option<bool>,
}

// ── ExceptionFilterOptions ─────────────────────────────────────────

/// An exception filter with options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionFilterOptions {
    /// ID of an exception filter from the `exceptionBreakpointFilters` capability.
    pub filter_id: String,
    /// Expression for conditional exceptions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// Breakpoint mode from Capabilities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

// ── ExceptionOptions ───────────────────────────────────────────────

/// Configuration options for selected exceptions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionOptions {
    /// Path that selects exceptions in a tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<ExceptionPathSegment>>,
    /// Condition for when a thrown exception should break.
    pub break_mode: ExceptionBreakMode,
}

// ── ExceptionPathSegment ───────────────────────────────────────────

/// A segment of an exception path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionPathSegment {
    /// If true, matches anything except the names provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negate: Option<bool>,
    /// Names that should match or not match.
    pub names: Vec<String>,
}

// ── ExceptionDetails ───────────────────────────────────────────────

/// Details of an exception.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionDetails {
    /// Message contained in the exception.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Short type name of the exception object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    /// Fully-qualified type name of the exception object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_type_name: Option<String>,
    /// Expression to obtain the exception object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluate_name: Option<String>,
    /// Stack trace at the time the exception was thrown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,
    /// Details of contained exceptions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_exception: Option<Vec<ExceptionDetails>>,
}

// ── DisassembledInstruction ────────────────────────────────────────

/// A disassembled instruction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisassembledInstruction {
    /// The address in hex (prefixed with `0x`).
    pub address: String,
    /// Raw bytes in an implementation-defined format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_bytes: Option<String>,
    /// Text representing the instruction.
    pub instruction: String,
    /// Name of the symbol at this instruction location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Source location corresponding to this instruction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Source>,
    /// Line within the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Column within the line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    /// End line of the range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    /// End column of the range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
    /// Hint for UI presentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<InstructionPresentationHint>,
}

// ── BreakpointMode ─────────────────────────────────────────────────

/// A breakpoint mode supported by the debug adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointMode {
    /// Internal ID of the mode.
    pub mode: String,
    /// Name shown in the UI.
    pub label: String,
    /// Help text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Types of breakpoint this mode applies to.
    pub applies_to: Vec<BreakpointModeApplicability>,
}

// ── ExceptionBreakpointsFilter ─────────────────────────────────────

/// An exception filter from the `exceptionBreakpointFilters` capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExceptionBreakpointsFilter {
    /// Internal ID of the filter option.
    pub filter: String,
    /// Name shown in the UI.
    pub label: String,
    /// Help text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Initial value (default false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
    /// Whether a condition can be specified for this filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_condition: Option<bool>,
    /// Help text for the condition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition_description: Option<String>,
}

// ── MemoryRange ────────────────────────────────────────────────────

/// A memory range for event context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRange {
    /// Memory reference.
    pub memory_reference: String,
    /// Starting offset in bytes.
    pub offset: i64,
    /// Number of bytes.
    pub count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_serde() {
        let json = r#"{"id":1,"name":"main"}"#;
        let thread: Thread = serde_json::from_str(json).unwrap();
        assert_eq!(thread.id, 1);
        assert_eq!(thread.name, "main");
        let out = serde_json::to_string(&thread).unwrap();
        let back: Thread = serde_json::from_str(&out).unwrap();
        assert_eq!(back.id, 1);
        assert_eq!(back.name, "main");
    }

    #[test]
    fn test_source_serde() {
        let json = r#"{"name":"main.cpp","path":"/src/main.cpp"}"#;
        let source: Source = serde_json::from_str(json).unwrap();
        assert_eq!(source.name.as_deref(), Some("main.cpp"));
        assert_eq!(source.path.as_deref(), Some("/src/main.cpp"));
        assert!(source.source_reference.is_none());
    }

    #[test]
    fn test_stack_frame_serde() {
        let json = r#"{"id":0,"name":"main","source":{"name":"main.cpp","path":"/src/main.cpp"},"line":42,"column":5}"#;
        let frame: StackFrame = serde_json::from_str(json).unwrap();
        assert_eq!(frame.id, 0);
        assert_eq!(frame.name, "main");
        assert_eq!(frame.line, 42);
        assert_eq!(frame.column, 5);
    }

    #[test]
    fn test_scope_serde() {
        let json = r#"{"name":"Locals","variablesReference":1000,"expensive":false}"#;
        let scope: Scope = serde_json::from_str(json).unwrap();
        assert_eq!(scope.name, "Locals");
        assert_eq!(scope.variables_reference, 1000);
        assert!(!scope.expensive);
    }

    #[test]
    fn test_variable_serde() {
        let json = r#"{"name":"x","value":"42","type":"int","variablesReference":0}"#;
        let var: Variable = serde_json::from_str(json).unwrap();
        assert_eq!(var.name, "x");
        assert_eq!(var.value, "42");
        assert_eq!(var.var_type.as_deref(), Some("int"));
        assert_eq!(var.variables_reference, 0);
    }

    #[test]
    fn test_breakpoint_serde() {
        let json = r#"{"verified":true,"line":10}"#;
        let bp: Breakpoint = serde_json::from_str(json).unwrap();
        assert!(bp.verified);
        assert_eq!(bp.line, Some(10));
    }

    #[test]
    fn test_module_id() {
        let json = "42";
        let id: ModuleId = serde_json::from_str(json).unwrap();
        assert_eq!(id, ModuleId::Number(42));

        let json = r#""mod_name""#;
        let id: ModuleId = serde_json::from_str(json).unwrap();
        assert_eq!(id, ModuleId::String("mod_name".into()));
    }

    #[test]
    fn test_optional_fields_omitted() {
        let bp = Breakpoint {
            id: Some(1),
            verified: true,
            message: None,
            source: None,
            line: Some(10),
            column: None,
            end_line: None,
            end_column: None,
            instruction_reference: None,
            offset: None,
            reason: None,
        };
        let json = serde_json::to_string(&bp).unwrap();
        // Optional fields should be omitted
        assert!(!json.contains("message"));
        assert!(!json.contains("source"));
        let back: Breakpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, Some(1));
        assert!(back.verified);
    }
}
