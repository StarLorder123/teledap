//! String enums and type aliases defined in the DAP specification.
//!
//! Each enum uses `#[serde(rename_all = "camelCase")]` to match the DAP JSON wire format.

use serde::{Deserialize, Serialize};

/// The access type for a data breakpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DataBreakpointAccessType {
    Read,
    Write,
    ReadWrite,
}

/// Stepping granularities for `next`, `stepIn`, `stepOut`, `stepBack`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SteppingGranularity {
    #[default]
    Statement,
    Line,
    Instruction,
}

/// Completion item types returned by the `completions` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompletionItemType {
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Interface,
    Module,
    Property,
    Unit,
    Value,
    Enum,
    Keyword,
    Snippet,
    Text,
    Color,
    File,
    Reference,
    Customcolor,
}

/// Checksum algorithms supported by the debug adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ChecksumAlgorithm {
    #[serde(rename = "MD5")]
    Md5,
    #[serde(rename = "SHA1")]
    Sha1,
    #[serde(rename = "SHA256")]
    Sha256,
    Timestamp,
}

/// The condition under which a thrown exception should result in a break.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExceptionBreakMode {
    Never,
    Always,
    Unhandled,
    UserUnhandled,
}

/// Logical areas that can be invalidated by the `invalidated` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InvalidatedAreas {
    #[serde(alias = "all")]
    All,
    #[serde(alias = "stacks")]
    Stacks,
    #[serde(alias = "threads")]
    Threads,
    #[serde(alias = "variables")]
    Variables,
    /// Catch-all for unknown area values.
    #[serde(untagged)]
    Other(String),
}

/// Types of breakpoint that a BreakpointMode applies to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BreakpointModeApplicability {
    Source,
    Exception,
    Data,
    Instruction,
    /// Catch-all for unknown applicability values.
    #[serde(untagged)]
    Other(String),
}

/// The reason for a `stopped` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoppedReason {
    Step,
    Breakpoint,
    Exception,
    Pause,
    Entry,
    Goto,
    #[serde(rename = "function breakpoint")]
    FunctionBreakpoint,
    #[serde(rename = "data breakpoint")]
    DataBreakpoint,
    #[serde(rename = "instruction breakpoint")]
    InstructionBreakpoint,
    /// Catch-all for unknown reason values.
    #[serde(untagged)]
    Other(String),
}

/// The reason for a `thread` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThreadReason {
    Started,
    Exited,
    #[serde(untagged)]
    Other(String),
}

/// The reason for a `breakpoint` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BreakpointReason {
    Changed,
    New,
    Removed,
    #[serde(untagged)]
    Other(String),
}

/// The reason for a `module` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModuleReason {
    New,
    Changed,
    Removed,
}

/// The reason for a `loadedSource` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoadedSourceReason {
    New,
    Changed,
    Removed,
}

/// How a process was started in a `process` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessStartMethod {
    Launch,
    Attach,
    AttachForSuspendedLaunch,
}

/// The output category for an `output` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputCategory {
    Console,
    Important,
    Stdout,
    Stderr,
    Telemetry,
    #[serde(untagged)]
    Other(String),
}

/// The output grouping for collapsing regions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputGroup {
    Start,
    StartCollapsed,
    End,
}

/// Presentation hint for a `Source` in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourcePresentationHint {
    Normal,
    Emphasize,
    Deemphasize,
}

/// Presentation hint for a `StackFrame` in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StackFramePresentationHint {
    Normal,
    Label,
    Subtle,
}

/// Presentation hint for a `Scope` in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScopePresentationHint {
    Arguments,
    Locals,
    Registers,
    ReturnValue,
    #[serde(untagged)]
    Other(String),
}

/// The kind of a variable for presentation purposes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VariableKind {
    Property,
    Method,
    Class,
    Data,
    Event,
    BaseClass,
    InnerClass,
    Interface,
    MostDerivedClass,
    Virtual,
    DataBreakpoint,
    #[serde(untagged)]
    Other(String),
}

/// Variable attributes for presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VariableAttribute {
    Static,
    Constant,
    ReadOnly,
    RawString,
    HasObjectId,
    CanHaveObjectId,
    HasSideEffects,
    HasDataBreakpoint,
    #[serde(untagged)]
    Other(String),
}

/// Visibility of a variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VariableVisibility {
    Public,
    Private,
    Protected,
    Internal,
    Final,
    #[serde(untagged)]
    Other(String),
}

/// The context in which an evaluate request is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvaluateContext {
    Watch,
    Repl,
    Hover,
    Clipboard,
    Variables,
    #[serde(untagged)]
    Other(String),
}

/// The filter for a `variables` request limiting child variables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VariableFilter {
    Indexed,
    Named,
}

/// The kind of terminal to launch for `runInTerminal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TerminalKind {
    Integrated,
    External,
}

/// The type of debug session to start for `startDebugging`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartDebuggingRequestKind {
    Launch,
    Attach,
}

/// Presentation hint for a `DisassembledInstruction`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstructionPresentationHint {
    Normal,
    Invalid,
}

/// Data type for a column descriptor value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColumnDescriptorType {
    String,
    Number,
    Boolean,
    UnixTimestampUTC,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stepping_granularity_serde() {
        let json = r#""statement""#;
        let val: SteppingGranularity = serde_json::from_str(json).unwrap();
        assert_eq!(val, SteppingGranularity::Statement);
        assert_eq!(serde_json::to_string(&val).unwrap(), json);

        let json = r#""line""#;
        let val: SteppingGranularity = serde_json::from_str(json).unwrap();
        assert_eq!(val, SteppingGranularity::Line);

        let json = r#""instruction""#;
        let val: SteppingGranularity = serde_json::from_str(json).unwrap();
        assert_eq!(val, SteppingGranularity::Instruction);
    }

    #[test]
    fn test_stopped_reason() {
        let json = r#""breakpoint""#;
        let reason: StoppedReason = serde_json::from_str(json).unwrap();
        assert_eq!(reason, StoppedReason::Breakpoint);

        let json = r#""function breakpoint""#;
        let reason: StoppedReason = serde_json::from_str(json).unwrap();
        assert_eq!(reason, StoppedReason::FunctionBreakpoint);
    }

    #[test]
    fn test_checksum_algorithm() {
        let json = r#""MD5""#;
        let alg: ChecksumAlgorithm = serde_json::from_str(json).unwrap();
        assert_eq!(alg, ChecksumAlgorithm::Md5);
        assert_eq!(serde_json::to_string(&alg).unwrap(), json);
    }

    #[test]
    fn test_exception_break_mode() {
        let json = r#""unhandled""#;
        let mode: ExceptionBreakMode = serde_json::from_str(json).unwrap();
        assert_eq!(mode, ExceptionBreakMode::Unhandled);
    }

    #[test]
    fn test_unknown_variant_untagged() {
        let json = r#""customReason""#;
        let reason: StoppedReason = serde_json::from_str(json).unwrap();
        assert_eq!(reason, StoppedReason::Other("customReason".into()));
    }
}
