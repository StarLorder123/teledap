//! Introspection tool handlers: get_threads, get_stack_trace, get_scopes,
//! get_variables, evaluate, set_variable, assemble_context, search_variables.

use dap_types::enums::VariableFilter;
use dap_types::requests::{EvaluateArguments, SetVariableArguments};
use debug_session::DebugSession;
use mcp_protocol::CallToolResult;
use serde::Deserialize;

use crate::error::BridgeError;

fn text_result(value: &impl serde::Serialize) -> Result<CallToolResult, BridgeError> {
    Ok(CallToolResult::success_json(value)?)
}

// ── get_threads ─────────────────────────────────────────────────────────

pub async fn handle_get_threads(
    session: &DebugSession,
    _params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let threads = session.get_threads().await?;
    text_result(&threads)
}

// ── get_stack_trace ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StackTraceParams {
    thread_id: u64,
    #[serde(default)]
    levels: Option<u64>,
    #[serde(default)]
    start_frame: Option<u64>,
}

pub async fn handle_get_stack_trace(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: StackTraceParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "get_stack_trace".into(),
            message: e.to_string(),
        })?;

    let frames = session
        .get_stack_trace(p.thread_id, p.start_frame, p.levels)
        .await?;
    text_result(&frames)
}

// ── get_scopes ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScopesParams {
    frame_id: u64,
}

pub async fn handle_get_scopes(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: ScopesParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "get_scopes".into(),
            message: e.to_string(),
        })?;

    let scopes = session.get_scopes(p.frame_id).await?;
    text_result(&scopes)
}

// ── get_variables ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VariablesParams {
    variables_reference: u64,
    #[serde(default)]
    filter: Option<String>,
    #[serde(default)]
    start: Option<u64>,
    #[serde(default)]
    count: Option<u64>,
}

pub async fn handle_get_variables(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: VariablesParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "get_variables".into(),
            message: e.to_string(),
        })?;

    let filter = match p.filter.as_deref() {
        Some("named") => Some(VariableFilter::Named),
        Some("indexed") => Some(VariableFilter::Indexed),
        _ => None,
    };

    let variables = session
        .get_variables(p.variables_reference, filter, p.start, p.count)
        .await?;

    // Populate the variable cache so these variables can be searched later
    session.cache_variables(&variables, None, None).await;

    text_result(&variables)
}

// ── evaluate ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvaluateParams {
    expression: String,
    #[serde(default)]
    frame_id: Option<u64>,
    #[serde(default)]
    context: Option<String>,
}

pub async fn handle_evaluate(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: EvaluateParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "evaluate".into(),
            message: e.to_string(),
        })?;

    let context = match p.context.as_deref() {
        Some("watch") => Some(dap_types::enums::EvaluateContext::Watch),
        Some("repl") => Some(dap_types::enums::EvaluateContext::Repl),
        Some("hover") => Some(dap_types::enums::EvaluateContext::Hover),
        Some("clipboard") => Some(dap_types::enums::EvaluateContext::Clipboard),
        _ => None,
    };

    let args = EvaluateArguments {
        expression: p.expression,
        frame_id: p.frame_id,
        line: None,
        column: None,
        source: None,
        context,
        format: None,
    };

    let result = session.evaluate(args).await?;
    text_result(&result)
}

// ── set_variable ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetVariableParams {
    variables_reference: u64,
    name: String,
    value: String,
}

pub async fn handle_set_variable(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: SetVariableParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "set_variable".into(),
            message: e.to_string(),
        })?;

    let args = SetVariableArguments {
        variables_reference: p.variables_reference,
        name: p.name,
        value: p.value,
        format: None,
    };

    let result = session.set_variable(args).await?;
    text_result(&result)
}

// ── assemble_context ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssembleContextParams {
    #[serde(default)]
    thread_id: Option<u64>,
    #[serde(default)]
    max_frames: Option<usize>,
    #[serde(default)]
    max_depth: Option<usize>,
}

pub async fn handle_assemble_context(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: AssembleContextParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "assemble_context".into(),
            message: e.to_string(),
        })?;

    let max_frames = p.max_frames.unwrap_or(10);
    let max_depth = p.max_depth.unwrap_or(2);

    let threads = session.get_threads().await?;

    let target_threads: Vec<_> = if let Some(tid) = p.thread_id {
        threads.into_iter().filter(|t| t.id == tid).collect()
    } else {
        threads
    };

    let mut context = serde_json::Map::new();
    let mut thread_contexts = Vec::new();

    for thread in &target_threads {
        let frames = match session
            .get_stack_trace(thread.id, None, Some(max_frames as u64))
            .await
        {
            Ok(f) => f,
            Err(_) => continue,
        };

        let mut frame_contexts = Vec::new();
        for frame in &frames {
            let scopes = match session.get_scopes(frame.id).await {
                Ok(s) => s,
                Err(_) => continue,
            };

            let mut scope_contexts = Vec::new();
            for scope in &scopes {
                let variables = if scope.variables_reference > 0 {
                    match session
                        .get_variables(scope.variables_reference, None, None, None)
                        .await
                    {
                        Ok(v) => {
                            // Cache variables for future search
                            session
                                .cache_variables(&v, Some(frame.id), Some(&scope.name))
                                .await;

                            // Limited expansion: expand top-level children only
                            let mut expanded = Vec::new();
                            for var in &v {
                                let children = if var.variables_reference > 0 && max_depth > 1 {
                                    session
                                        .get_variables(
                                            var.variables_reference,
                                            None,
                                            None,
                                            Some(50),
                                        )
                                        .await
                                        .ok()
                                } else {
                                    None
                                };
                                expanded.push(serde_json::json!({
                                    "name": var.name,
                                    "value": var.value,
                                    "type": var.var_type,
                                    "variablesReference": var.variables_reference,
                                    "children": children,
                                }));
                            }
                            expanded
                        }
                        Err(_) => Vec::new(),
                    }
                } else {
                    Vec::new()
                };

                scope_contexts.push(serde_json::json!({
                    "name": scope.name,
                    "variablesReference": scope.variables_reference,
                    "namedVariables": scope.named_variables,
                    "indexedVariables": scope.indexed_variables,
                    "variables": variables,
                }));
            }

            frame_contexts.push(serde_json::json!({
                "id": frame.id,
                "name": frame.name,
                "source": frame.source.as_ref().map(|s| serde_json::json!({
                    "name": s.name,
                    "path": s.path,
                })),
                "line": frame.line,
                "column": frame.column,
                "scopes": scope_contexts,
            }));
        }

        thread_contexts.push(serde_json::json!({
            "id": thread.id,
            "name": thread.name,
            "frames": frame_contexts,
        }));
    }

    context.insert("threads".into(), serde_json::json!(thread_contexts));
    context.insert(
        "total_threads".into(),
        serde_json::json!(target_threads.len()),
    );

    Ok(CallToolResult::success_json(&serde_json::Value::Object(
        context,
    ))?)
}

// ── search_variables ────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchVariablesParams {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

pub async fn handle_search_variables(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError> {
    let p: SearchVariablesParams =
        serde_json::from_value(params).map_err(|e| BridgeError::InvalidParams {
            tool: "search_variables".into(),
            message: e.to_string(),
        })?;

    let limit = p.limit.unwrap_or(20);
    let entries = session.search_variables(&p.query, limit).await;

    let results: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "variablesReference": e.variables_reference,
                "type": e.var_type,
                "namedVariables": e.named_variables,
                "indexedVariables": e.indexed_variables,
                "frameId": e.frame_id,
                "scopeName": e.scope_name,
            })
        })
        .collect();

    let output = serde_json::json!({
        "query": p.query,
        "count": results.len(),
        "results": results,
    });

    text_result(&output)
}

// ── Unit tests (param deserialization) ──────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_trace_params() {
        let params = serde_json::json!({"threadId": 1, "levels": 10});
        let p: StackTraceParams = serde_json::from_value(params).unwrap();
        assert_eq!(p.thread_id, 1);
        assert_eq!(p.levels, Some(10));
        assert_eq!(p.start_frame, None);
    }

    #[test]
    fn test_stack_trace_params_minimal() {
        let params = serde_json::json!({"threadId": 42});
        let p: StackTraceParams = serde_json::from_value(params).unwrap();
        assert_eq!(p.thread_id, 42);
        assert!(p.levels.is_none());
    }

    #[test]
    fn test_evaluate_params() {
        let params = serde_json::json!({
            "expression": "x + y",
            "frameId": 0,
            "context": "repl"
        });
        let p: EvaluateParams = serde_json::from_value(params).unwrap();
        assert_eq!(p.expression, "x + y");
        assert_eq!(p.frame_id, Some(0));
        assert_eq!(p.context.as_deref(), Some("repl"));
    }

    #[test]
    fn test_evaluate_params_minimal() {
        let params = serde_json::json!({"expression": "this"});
        let p: EvaluateParams = serde_json::from_value(params).unwrap();
        assert_eq!(p.expression, "this");
        assert!(p.frame_id.is_none());
        assert!(p.context.is_none());
    }

    #[test]
    fn test_variables_params() {
        let params = serde_json::json!({
            "variablesReference": 1000,
            "filter": "named",
            "count": 50
        });
        let p: VariablesParams = serde_json::from_value(params).unwrap();
        assert_eq!(p.variables_reference, 1000);
        assert_eq!(p.filter.as_deref(), Some("named"));
        assert_eq!(p.count, Some(50));
    }

    #[test]
    fn test_assemble_context_params_defaults() {
        let params = serde_json::json!({});
        let p: AssembleContextParams = serde_json::from_value(params).unwrap();
        assert_eq!(p.thread_id, None);
        assert_eq!(p.max_frames, None);
        assert_eq!(p.max_depth, None);
    }

    #[test]
    fn test_assemble_context_params_full() {
        let params = serde_json::json!({
            "threadId": 1,
            "maxFrames": 5,
            "maxDepth": 3
        });
        let p: AssembleContextParams = serde_json::from_value(params).unwrap();
        assert_eq!(p.thread_id, Some(1));
        assert_eq!(p.max_frames, Some(5));
        assert_eq!(p.max_depth, Some(3));
    }

    #[test]
    fn test_search_variables_params() {
        let params = serde_json::json!({"query": "my_var", "limit": 10});
        let p: SearchVariablesParams = serde_json::from_value(params).unwrap();
        assert_eq!(p.query, "my_var");
        assert_eq!(p.limit, Some(10));
    }
}
