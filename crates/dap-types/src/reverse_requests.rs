//! Reverse requests: requests sent from the debug adapter *to* the client.
//!
//! These are part of the DAP specification but are handled by the client (IDE),
//! not by the debug adapter itself. They're included here for completeness.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::enums::TerminalKind;

// ── RunInTerminal ──────────────────────────────────────────────────

/// The `runInTerminal` reverse request asks the client to run a command in a terminal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInTerminalRequestArguments {
    /// What kind of terminal to launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<TerminalKind>,
    /// Title of the terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Working directory for the command.
    pub cwd: String,
    /// List of arguments. First argument is the command to run.
    pub args: Vec<String>,
    /// Environment variable key-value pairs. `null` values mean the variable
    /// should be removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, Option<String>>>,
    /// If true, arguments should not be escaped (shell interpretation requested).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_can_be_interpreted_by_shell: Option<bool>,
}

/// Response body for `runInTerminal`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInTerminalResponse {
    /// Process ID of the terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_id: Option<u64>,
    /// Process ID of the shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_process_id: Option<u64>,
}

// ── StartDebugging ─────────────────────────────────────────────────

/// The `startDebugging` reverse request asks the client to start a new debug
/// session of the same type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDebuggingRequestArguments {
    /// Arguments for the new debug session (passed to `launch` or `attach`).
    pub configuration: HashMap<String, serde_json::Value>,
    /// Whether to launch or attach.
    pub request: super::enums::StartDebuggingRequestKind,
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_in_terminal_args() {
        let args = RunInTerminalRequestArguments {
            kind: Some(TerminalKind::Integrated),
            title: Some("Debug Console".into()),
            cwd: "/project".into(),
            args: vec!["bash".into(), "-c".into(), "echo hello".into()],
            env: None,
            args_can_be_interpreted_by_shell: Some(true),
        };
        let json = serde_json::to_string(&args).unwrap();
        assert!(json.contains("Debug Console"));
        assert!(json.contains("bash"));
        let back: RunInTerminalRequestArguments = serde_json::from_str(&json).unwrap();
        assert_eq!(back.cwd, "/project");
        assert_eq!(back.args.len(), 3);
    }

    #[test]
    fn test_run_in_terminal_response() {
        let json = r#"{"processId":12345,"shellProcessId":12346}"#;
        let resp: RunInTerminalResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.process_id, Some(12345));
        assert_eq!(resp.shell_process_id, Some(12346));
    }

    #[test]
    fn test_start_debugging_args() {
        let json = r#"{"configuration":{"program":"/app"},"request":"launch"}"#;
        let args: StartDebuggingRequestArguments = serde_json::from_str(json).unwrap();
        assert_eq!(
            args.request,
            crate::enums::StartDebuggingRequestKind::Launch
        );
        assert_eq!(args.configuration["program"], "/app");
    }
}
