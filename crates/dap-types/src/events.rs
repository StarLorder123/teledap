//! Body types for all 17 DAP events defined in the specification.
//!
//! Each struct corresponds to the `body` field of an `Event` message.
//! The event name string (e.g. `"stopped"`) is the discriminator on the wire;
//! these types are what you deserialize the `body` into.

use serde::{Deserialize, Serialize};

use crate::capabilities::Capabilities;
use crate::enums::*;
use crate::types::*;

// ── Initialized ────────────────────────────────────────────────────

/// Body of the `initialized` event. Has no fields — signals readiness to accept
/// configuration requests (setBreakpoints, setExceptionBreakpoints, etc.).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InitializedEventBody {}

// ── Stopped ────────────────────────────────────────────────────────

/// Body of the `stopped` event — execution stopped due to breakpoint, step, exception, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedEventBody {
    /// Reason for the stop.
    pub reason: StoppedReason,
    /// Full reason text shown in UI (can be translated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The thread which was stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<u64>,
    /// Hint to not change focus.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserve_focus_hint: Option<bool>,
    /// Additional information (e.g. exception name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// If true, all threads have stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_threads_stopped: Option<bool>,
    /// IDs of breakpoints that triggered the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_breakpoint_ids: Option<Vec<u64>>,
}

// ── Continued ──────────────────────────────────────────────────────

/// Body of the `continued` event — execution has been resumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuedEventBody {
    /// The thread which was continued.
    pub thread_id: u64,
    /// If true, all threads were resumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_threads_continued: Option<bool>,
}

// ── Exited ─────────────────────────────────────────────────────────

/// Body of the `exited` event — the debuggee has exited.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitedEventBody {
    /// Exit code returned from the debuggee.
    pub exit_code: u64,
}

// ── Terminated ─────────────────────────────────────────────────────

/// Body of the `terminated` event — the debug session has terminated.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminatedEventBody {
    /// Arbitrary object passed to `launch`/`attach` as `__restart`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<serde_json::Value>,
}

// ── Thread ─────────────────────────────────────────────────────────

/// Body of the `thread` event — a thread has started or exited.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEventBody {
    /// Reason for the event.
    pub reason: ThreadReason,
    /// The identifier of the thread.
    pub thread_id: u64,
}

// ── Output ─────────────────────────────────────────────────────────

/// Body of the `output` event — the target has produced output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputEventBody {
    /// Output category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<OutputCategory>,
    /// The output text (may contain ANSI escape sequences).
    pub output: String,
    /// Grouping for collapsing output regions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<OutputGroup>,
    /// Reference to structured output children.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables_reference: Option<u64>,
    /// Source location where output was produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// Line where output was produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Column where output was produced (UTF-16 code units).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    /// Additional data to report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Reference for value declaration location.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_reference: Option<u64>,
}

// ── Breakpoint ─────────────────────────────────────────────────────

/// Body of the `breakpoint` event — breakpoint state changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointEventBody {
    /// Reason for the event.
    pub reason: BreakpointReason,
    /// The breakpoint with updated values.
    #[serde(rename = "breakpoint")]
    pub breakpoint: Breakpoint,
}

// ── Module ─────────────────────────────────────────────────────────

/// Body of the `module` event — module information changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleEventBody {
    /// Reason for the event.
    pub reason: ModuleReason,
    /// The new, changed, or removed module.
    pub module: Module,
}

// ── LoadedSource ───────────────────────────────────────────────────

/// Body of the `loadedSource` event — source file state changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSourceEventBody {
    /// Reason for the event.
    pub reason: LoadedSourceReason,
    /// The new, changed, or removed source.
    pub source: Source,
}

// ── Process ────────────────────────────────────────────────────────

/// Body of the `process` event — a new process is being debugged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessEventBody {
    /// Logical name of the process (e.g. full path to executable).
    pub name: String,
    /// OS process ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_process_id: Option<u64>,
    /// If true, the process is running on the same computer as the debug adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_local_process: Option<bool>,
    /// How debugging was started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_method: Option<ProcessStartMethod>,
    /// Size of a pointer or address in bits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer_size: Option<u32>,
}

// ── Capabilities ───────────────────────────────────────────────────

/// Body of the `capabilities` event — capabilities have changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesEventBody {
    /// The set of updated capabilities. Only changed values need to be included.
    pub capabilities: Capabilities,
}

// ── ProgressStart ──────────────────────────────────────────────────

/// Body of the `progressStart` event — a long-running operation is starting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressStartEventBody {
    /// Unique ID for this progress.
    pub progress_id: String,
    /// Short title of the progress.
    pub title: String,
    /// Related request ID, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<u64>,
    /// If true, the operation can be cancelled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellable: Option<bool>,
    /// More detailed progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Progress percentage (0-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percentage: Option<f64>,
}

// ── ProgressUpdate ─────────────────────────────────────────────────

/// Body of the `progressUpdate` event — progress needs to be updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressUpdateEventBody {
    /// ID from the initial ProgressStartEvent.
    pub progress_id: String,
    /// Updated progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Updated progress percentage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percentage: Option<f64>,
}

// ── ProgressEnd ────────────────────────────────────────────────────

/// Body of the `progressEnd` event — progress has ended.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEndEventBody {
    /// ID from the initial ProgressStartEvent.
    pub progress_id: String,
    /// Final progress message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// ── Invalidated ────────────────────────────────────────────────────

/// Body of the `invalidated` event — state needs to be re-fetched.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvalidatedEventBody {
    /// Set of logical areas that got invalidated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub areas: Option<Vec<InvalidatedAreas>>,
    /// If specified, only refetch data related to this thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<u64>,
    /// If specified, only refetch data related to this stack frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack_frame_id: Option<u64>,
}

// ── Memory ─────────────────────────────────────────────────────────

/// Body of the `memory` event — memory range has been updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEventBody {
    /// Memory reference of the updated range.
    pub memory_reference: String,
    /// Starting offset in bytes.
    pub offset: i64,
    /// Number of bytes updated.
    pub count: u64,
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stopped_event_body() {
        let json = r#"{"reason":"breakpoint","threadId":1,"allThreadsStopped":true}"#;
        let body: StoppedEventBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.reason, StoppedReason::Breakpoint);
        assert_eq!(body.thread_id, Some(1));
        assert_eq!(body.all_threads_stopped, Some(true));
    }

    #[test]
    fn test_output_event_body() {
        let json = r#"{"category":"stdout","output":"Hello, World!\n"}"#;
        let body: OutputEventBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.category, Some(OutputCategory::Stdout));
        assert_eq!(body.output, "Hello, World!\n");
    }

    #[test]
    fn test_exited_event_body() {
        let json = r#"{"exitCode":0}"#;
        let body: ExitedEventBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.exit_code, 0);
    }

    #[test]
    fn test_thread_event_body() {
        let json = r#"{"reason":"started","threadId":42}"#;
        let body: ThreadEventBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.reason, ThreadReason::Started);
        assert_eq!(body.thread_id, 42);
    }

    #[test]
    fn test_breakpoint_event_body() {
        let json = r#"{"reason":"new","breakpoint":{"id":1,"verified":true,"line":10}}"#;
        let body: BreakpointEventBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.reason, BreakpointReason::New);
        assert_eq!(body.breakpoint.id, Some(1));
        assert!(body.breakpoint.verified);
    }

    #[test]
    fn test_invalidated_event_body() {
        let json = r#"{"areas":["stacks","variables"],"threadId":1}"#;
        let body: InvalidatedEventBody = serde_json::from_str(json).unwrap();
        let areas = body.areas.unwrap();
        assert_eq!(areas.len(), 2);
        assert_eq!(body.thread_id, Some(1));
    }

    #[test]
    fn test_memory_event_body() {
        let json = r#"{"memoryReference":"0x1000","offset":0,"count":16}"#;
        let body: MemoryEventBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.memory_reference, "0x1000");
        assert_eq!(body.offset, 0);
        assert_eq!(body.count, 16);
    }
}
