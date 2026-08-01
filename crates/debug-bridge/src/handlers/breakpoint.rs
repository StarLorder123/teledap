//! Breakpoint tool handlers: set_breakpoints, set_function_breakpoints,
//! list_breakpoints, set_data_breakpoints, data_breakpoint_info, and
//! set_exception_breakpoints.

use dap_types::enums::ExceptionBreakMode;
use dap_types::types::{
    DataBreakpoint, ExceptionFilterOptions, FunctionBreakpoint, Source, SourceBreakpoint,
};
use debug_session::DebugSession;
use mcp_protocol::CallToolResult;
use serde::Deserialize;

use crate::error::BridgeError;

fn text_result(value: &impl serde::Serialize) -> Result<CallToolResult, BridgeError> {
    Ok(CallToolResult::success_json(value)?)
}

// ── set_breakpoints ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BpItem {
    line: u64,
    #[serde(default)]
    column: Option<u64>,
    #[serde(default)]
    condition: Option<String>,
    #[serde(default)]
    hit_condition: Option<String>,
    #[serde(default)]
    log_message: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetBreakpointsParams {
    source_path: String,
    breakpoints: Vec<BpItem>,
}

pub async fn handle_set_breakpoints(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: SetBreakpointsParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "set_breakpoints".into(),
            message: e.to_string(),
        })?;

    // Resolve source path through the path mapper
    let resolved = session
        .resolve_path(&p.source_path)
        .await
        .unwrap_or_else(|| p.source_path.clone());

    let source = Source {
        name: Some(resolved.clone()),
        path: Some(resolved.clone()),
        ..Default::default()
    };

    let breakpoints: Vec<SourceBreakpoint> = p
        .breakpoints
        .iter()
        .map(|b| SourceBreakpoint {
            line: b.line,
            column: b.column,
            condition: b.condition.clone(),
            hit_condition: b.hit_condition.clone(),
            log_message: b.log_message.clone(),
            mode: b.mode.clone(),
        })
        .collect();

    let args = dap_types::requests::SetBreakpointsArguments {
        source,
        breakpoints: Some(breakpoints.clone()),
        lines: None,
        source_modified: None,
    };

    let resp = session.set_breakpoints(args).await?;
    session
        .update_source_breakpoints(&resolved, &breakpoints, &resp.breakpoints)
        .await;
    text_result(&resp)
}

// ── set_function_breakpoints ────────────────────────────────────────────

/// Per-function entry in the new format.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FnBpItem {
    name: String,
    #[serde(default)]
    condition: Option<String>,
    #[serde(default)]
    hit_condition: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetFunctionBreakpointsParams {
    /// New format: per-function entries with individual conditions.
    #[serde(default)]
    breakpoints: Option<Vec<FnBpItem>>,
    /// Old format: flat list of function names.
    #[serde(default)]
    names: Option<Vec<String>>,
    /// Old format: shared condition for all names.
    #[serde(default)]
    condition: Option<String>,
    /// Old format: shared hit condition for all names.
    #[serde(default)]
    hit_condition: Option<String>,
}

pub async fn handle_set_function_breakpoints(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: SetFunctionBreakpointsParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "set_function_breakpoints".into(),
            message: e.to_string(),
        })?;

    let breakpoints: Vec<FunctionBreakpoint> = if let Some(bp_items) = &p.breakpoints {
        // New format: per-function entries
        bp_items
            .iter()
            .map(|b| FunctionBreakpoint {
                name: b.name.clone(),
                condition: b.condition.clone(),
                hit_condition: b.hit_condition.clone(),
                mode: b.mode.clone(),
            })
            .collect()
    } else if let Some(names) = &p.names {
        // Old format: shared condition/hitCondition for all names
        names
            .iter()
            .map(|name| FunctionBreakpoint {
                name: name.clone(),
                condition: p.condition.clone(),
                hit_condition: p.hit_condition.clone(),
                mode: None,
            })
            .collect()
    } else {
        return Err(BridgeError::InvalidParams {
            tool: "set_function_breakpoints".into(),
            message:
                "Either 'breakpoints' (new per-function format) or 'names' (legacy format) is required"
                    .into(),
        });
    };

    let args = dap_types::requests::SetFunctionBreakpointsArguments {
        breakpoints: breakpoints.clone(),
    };

    let resp = session.set_function_breakpoints(args).await?;
    session
        .update_function_breakpoints(&breakpoints, &resp.breakpoints)
        .await;
    text_result(&resp)
}

// ── set_data_breakpoints ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataBpItem {
    data_id: String,
    #[serde(default)]
    access_type: Option<String>,
    #[serde(default)]
    condition: Option<String>,
    #[serde(default)]
    hit_condition: Option<String>,
}

impl DataBpItem {
    fn parse_access_type(&self) -> Option<dap_types::enums::DataBreakpointAccessType> {
        self.access_type.as_deref().and_then(|s| match s {
            "read" => Some(dap_types::enums::DataBreakpointAccessType::Read),
            "write" => Some(dap_types::enums::DataBreakpointAccessType::Write),
            "readWrite" | "readwrite" | "read_write" => {
                Some(dap_types::enums::DataBreakpointAccessType::ReadWrite)
            }
            _ => None,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetDataBreakpointsParams {
    breakpoints: Vec<DataBpItem>,
}

pub async fn handle_set_data_breakpoints(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: SetDataBreakpointsParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "set_data_breakpoints".into(),
            message: e.to_string(),
        })?;

    let breakpoints: Vec<DataBreakpoint> = p
        .breakpoints
        .iter()
        .map(|b| DataBreakpoint {
            data_id: b.data_id.clone(),
            access_type: b.parse_access_type(),
            condition: b.condition.clone(),
            hit_condition: b.hit_condition.clone(),
        })
        .collect();

    let args = dap_types::requests::SetDataBreakpointsArguments {
        breakpoints: breakpoints.clone(),
    };

    let resp = session.set_data_breakpoints(args).await?;
    session
        .update_data_breakpoints(&breakpoints, &resp.breakpoints)
        .await;
    text_result(&resp)
}

// ── data_breakpoint_info ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataBreakpointInfoParams {
    /// Variable name (required).
    name: String,
    /// Optional variables reference for the parent scope.
    #[serde(default)]
    variables_reference: Option<u64>,
    /// Optional frame ID for context.
    #[serde(default)]
    frame_id: Option<u64>,
    /// Optional size in bytes.
    #[serde(default)]
    bytes: Option<u64>,
    /// Whether to treat the name as an address.
    #[serde(default)]
    as_address: Option<bool>,
    /// Breakpoint mode.
    #[serde(default)]
    mode: Option<String>,
}

pub async fn handle_data_breakpoint_info(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: DataBreakpointInfoParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "data_breakpoint_info".into(),
            message: e.to_string(),
        })?;

    let args = dap_types::requests::DataBreakpointInfoArguments {
        variables_reference: p.variables_reference,
        name: p.name,
        frame_id: p.frame_id,
        bytes: p.bytes,
        as_address: p.as_address,
        mode: p.mode,
    };

    let resp = session.data_breakpoint_info(args).await?;
    text_result(&resp)
}

// ── set_exception_breakpoints ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExceptionBpItem {
    filter: String,
    /// Whether this filter is enabled (default: true).
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    condition: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetExceptionBreakpointsParams {
    /// List of exception breakpoint filter options.
    /// Each entry can specify a filter ID and optional condition/mode.
    breakpoints: Vec<ExceptionBpItem>,
    /// Optional advanced exception configuration for break mode.
    #[serde(default)]
    break_mode: Option<String>,
    /// Optional exception path segments for tree-based selection.
    #[serde(default)]
    exception_path_names: Option<Vec<String>>,
    /// Whether to negate the exception path matching.
    #[serde(default)]
    exception_path_negate: Option<bool>,
}

pub async fn handle_set_exception_breakpoints(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: SetExceptionBreakpointsParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "set_exception_breakpoints".into(),
            message: e.to_string(),
        })?;

    // Build filters from the enabled entries
    let filters: Vec<String> = p
        .breakpoints
        .iter()
        .filter(|b| b.enabled)
        .map(|b| b.filter.clone())
        .collect();

    // Build filter_options for entries with condition or mode
    let filter_options: Vec<ExceptionFilterOptions> = p
        .breakpoints
        .iter()
        .filter(|b| b.condition.is_some() || b.mode.is_some())
        .map(|b| ExceptionFilterOptions {
            filter_id: b.filter.clone(),
            condition: b.condition.clone(),
            mode: b.mode.clone(),
        })
        .collect();

    let filter_options = if filter_options.is_empty() {
        None
    } else {
        Some(filter_options)
    };

    // Build exception_options if break_mode or exception path was specified
    let exception_options = if let Some(break_mode_str) = &p.break_mode {
        let break_mode = match break_mode_str.as_str() {
            "never" => ExceptionBreakMode::Never,
            "always" => ExceptionBreakMode::Always,
            "unhandled" => ExceptionBreakMode::Unhandled,
            "userUnhandled" | "user_unhandled" => ExceptionBreakMode::UserUnhandled,
            _ => ExceptionBreakMode::Unhandled,
        };
        let path = p.exception_path_names.as_ref().map(|names| {
            vec![dap_types::types::ExceptionPathSegment {
                negate: p.exception_path_negate,
                names: names.clone(),
            }]
        });
        Some(vec![dap_types::types::ExceptionOptions {
            path,
            break_mode: break_mode.clone(),
        }])
    } else {
        None
    };

    let args = dap_types::requests::SetExceptionBreakpointsArguments {
        filters,
        filter_options,
        exception_options,
    };

    let resp = session.set_exception_breakpoints(args).await?;
    text_result(&resp)
}

// ── list_breakpoints ────────────────────────────────────────────────────

pub async fn handle_list_breakpoints(
    session: &DebugSession,
    _params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let breakpoints = session.list_breakpoints().await;
    text_result(&breakpoints)
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── set_breakpoints param deserialization ──────────────────────────

    #[test]
    fn test_bp_item_minimal() {
        let json = serde_json::json!({"line": 42});
        let item: BpItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.line, 42);
        assert_eq!(item.column, None);
        assert_eq!(item.condition, None);
        assert_eq!(item.hit_condition, None);
        assert_eq!(item.log_message, None);
        assert_eq!(item.mode, None);
    }

    #[test]
    fn test_bp_item_full() {
        let json = serde_json::json!({
            "line": 10,
            "column": 5,
            "condition": "x > 0",
            "hitCondition": ">3",
            "logMessage": "x={x}",
            "mode": "hardware"
        });
        let item: BpItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.line, 10);
        assert_eq!(item.column, Some(5));
        assert_eq!(item.condition.as_deref(), Some("x > 0"));
        assert_eq!(item.hit_condition.as_deref(), Some(">3"));
        assert_eq!(item.log_message.as_deref(), Some("x={x}"));
        assert_eq!(item.mode.as_deref(), Some("hardware"));
    }

    #[test]
    fn test_set_breakpoints_params() {
        let json = serde_json::json!({
            "sourcePath": "/src/main.cpp",
            "breakpoints": [
                {"line": 10, "condition": "x > 0"},
                {"line": 20, "logMessage": "here"}
            ]
        });
        let p: SetBreakpointsParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.source_path, "/src/main.cpp");
        assert_eq!(p.breakpoints.len(), 2);
        assert_eq!(p.breakpoints[0].line, 10);
        assert_eq!(p.breakpoints[0].condition.as_deref(), Some("x > 0"));
        assert_eq!(p.breakpoints[0].log_message, None);
        assert_eq!(p.breakpoints[1].line, 20);
        assert_eq!(p.breakpoints[1].log_message.as_deref(), Some("here"));
    }

    // ── set_function_breakpoints param deserialization ─────────────────

    #[test]
    fn test_fn_bp_item_minimal() {
        let json = serde_json::json!({"name": "main"});
        let item: FnBpItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.name, "main");
        assert_eq!(item.condition, None);
        assert_eq!(item.hit_condition, None);
        assert_eq!(item.mode, None);
    }

    #[test]
    fn test_fn_bp_item_full() {
        let json = serde_json::json!({
            "name": "malloc",
            "condition": "size > 1024",
            "hitCondition": ">10",
            "mode": "hardware"
        });
        let item: FnBpItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.name, "malloc");
        assert_eq!(item.condition.as_deref(), Some("size > 1024"));
        assert_eq!(item.hit_condition.as_deref(), Some(">10"));
        assert_eq!(item.mode.as_deref(), Some("hardware"));
    }

    #[test]
    fn test_fn_bp_params_new_format() {
        let json = serde_json::json!({
            "breakpoints": [
                {"name": "foo", "condition": "x > 0"},
                {"name": "bar", "hitCondition": ">5"}
            ]
        });
        let p: SetFunctionBreakpointsParams = serde_json::from_value(json).unwrap();
        assert!(p.breakpoints.is_some());
        assert!(p.names.is_none());
        let bps = p.breakpoints.unwrap();
        assert_eq!(bps.len(), 2);
        assert_eq!(bps[0].name, "foo");
        assert_eq!(bps[0].condition.as_deref(), Some("x > 0"));
        assert_eq!(bps[1].name, "bar");
        assert_eq!(bps[1].hit_condition.as_deref(), Some(">5"));
    }

    #[test]
    fn test_fn_bp_params_legacy_format() {
        let json = serde_json::json!({
            "names": ["main", "foo"],
            "condition": "x > 0"
        });
        let p: SetFunctionBreakpointsParams = serde_json::from_value(json).unwrap();
        assert!(p.breakpoints.is_none());
        assert_eq!(p.names.as_deref(), Some(&["main".to_string(), "foo".to_string()][..]));
        assert_eq!(p.condition.as_deref(), Some("x > 0"));
        assert_eq!(p.hit_condition, None);
    }

    // ── set_data_breakpoints param deserialization ─────────────────────

    #[test]
    fn test_data_bp_item_minimal() {
        let json = serde_json::json!({"dataId": "var_x"});
        let item: DataBpItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.data_id, "var_x");
        assert_eq!(item.access_type, None);
        assert_eq!(item.condition, None);
        assert_eq!(item.hit_condition, None);
    }

    #[test]
    fn test_data_bp_item_full() {
        let json = serde_json::json!({
            "dataId": "var_x",
            "accessType": "readWrite",
            "condition": "count > 0",
            "hitCondition": ">10"
        });
        let item: DataBpItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.data_id, "var_x");
        assert_eq!(item.access_type.as_deref(), Some("readWrite"));
        assert_eq!(item.condition.as_deref(), Some("count > 0"));
        assert_eq!(item.hit_condition.as_deref(), Some(">10"));
    }

    #[test]
    fn test_data_bp_access_type_parsing() {
        let item: DataBpItem = serde_json::from_value(serde_json::json!({"dataId": "x", "accessType": "read"})).unwrap();
        assert!(matches!(item.parse_access_type(), Some(dap_types::enums::DataBreakpointAccessType::Read)));

        let item: DataBpItem = serde_json::from_value(serde_json::json!({"dataId": "x", "accessType": "write"})).unwrap();
        assert!(matches!(item.parse_access_type(), Some(dap_types::enums::DataBreakpointAccessType::Write)));

        let item: DataBpItem = serde_json::from_value(serde_json::json!({"dataId": "x", "accessType": "readWrite"})).unwrap();
        assert!(matches!(item.parse_access_type(), Some(dap_types::enums::DataBreakpointAccessType::ReadWrite)));

        let item: DataBpItem = serde_json::from_value(serde_json::json!({"dataId": "x"})).unwrap();
        assert_eq!(item.parse_access_type(), None);

        let item: DataBpItem = serde_json::from_value(serde_json::json!({"dataId": "x", "accessType": "invalid"})).unwrap();
        assert_eq!(item.parse_access_type(), None);
    }

    #[test]
    fn test_set_data_breakpoints_params() {
        let json = serde_json::json!({
            "breakpoints": [
                {"dataId": "x", "accessType": "readWrite"},
                {"dataId": "y", "condition": "y > 0"}
            ]
        });
        let p: SetDataBreakpointsParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.breakpoints.len(), 2);
        assert_eq!(p.breakpoints[0].data_id, "x");
        assert_eq!(p.breakpoints[0].access_type.as_deref(), Some("readWrite"));
        assert_eq!(p.breakpoints[1].data_id, "y");
        assert_eq!(p.breakpoints[1].condition.as_deref(), Some("y > 0"));
    }

    // ── data_breakpoint_info param deserialization ─────────────────────

    #[test]
    fn test_data_breakpoint_info_params_minimal() {
        let json = serde_json::json!({"name": "myVar"});
        let p: DataBreakpointInfoParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.name, "myVar");
        assert_eq!(p.variables_reference, None);
        assert_eq!(p.frame_id, None);
        assert_eq!(p.bytes, None);
        assert_eq!(p.as_address, None);
        assert_eq!(p.mode, None);
    }

    #[test]
    fn test_data_breakpoint_info_params_full() {
        let json = serde_json::json!({
            "name": "myVar",
            "variablesReference": 1000,
            "frameId": 5,
            "bytes": 4,
            "asAddress": true,
            "mode": "hardware"
        });
        let p: DataBreakpointInfoParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.name, "myVar");
        assert_eq!(p.variables_reference, Some(1000));
        assert_eq!(p.frame_id, Some(5));
        assert_eq!(p.bytes, Some(4));
        assert_eq!(p.as_address, Some(true));
        assert_eq!(p.mode.as_deref(), Some("hardware"));
    }

    // ── set_exception_breakpoints param deserialization ────────────────

    #[test]
    fn test_exception_bp_item_minimal() {
        let json = serde_json::json!({"filter": "cpp_throw"});
        let item: ExceptionBpItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.filter, "cpp_throw");
        assert!(item.enabled); // default
        assert_eq!(item.condition, None);
        assert_eq!(item.mode, None);
    }

    #[test]
    fn test_exception_bp_item_disabled() {
        let json = serde_json::json!({"filter": "unhandled", "enabled": false});
        let item: ExceptionBpItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.filter, "unhandled");
        assert!(!item.enabled);
    }

    #[test]
    fn test_exception_bp_item_with_condition() {
        let json = serde_json::json!({
            "filter": "cpp_throw",
            "condition": "type == std::runtime_error",
            "mode": "hardware"
        });
        let item: ExceptionBpItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.filter, "cpp_throw");
        assert_eq!(item.condition.as_deref(), Some("type == std::runtime_error"));
        assert_eq!(item.mode.as_deref(), Some("hardware"));
    }

    #[test]
    fn test_set_exception_breakpoints_params_minimal() {
        let json = serde_json::json!({
            "breakpoints": [{"filter": "cpp_throw"}]
        });
        let p: SetExceptionBreakpointsParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.breakpoints.len(), 1);
        assert_eq!(p.breakpoints[0].filter, "cpp_throw");
        assert_eq!(p.break_mode, None);
        assert_eq!(p.exception_path_names, None);
    }

    #[test]
    fn test_set_exception_breakpoints_params_full() {
        let json = serde_json::json!({
            "breakpoints": [
                {"filter": "cpp_throw", "condition": "msg contains 'fatal'"},
                {"filter": "unhandled", "enabled": false}
            ],
            "breakMode": "unhandled",
            "exceptionPathNames": ["std", "boost"],
            "exceptionPathNegate": false
        });
        let p: SetExceptionBreakpointsParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.breakpoints.len(), 2);
        assert!(p.breakpoints[0].enabled);
        assert!(!p.breakpoints[1].enabled);
        assert_eq!(p.break_mode.as_deref(), Some("unhandled"));
        assert_eq!(
            p.exception_path_names.as_deref(),
            Some(&["std".to_string(), "boost".to_string()][..])
        );
    }
}
