//! JSON-RPC 2.0 and MCP protocol types.
//!
//! MCP uses JSON-RPC 2.0 over line-delimited stdio. All MCP-specific fields
//! use camelCase (e.g. `inputSchema`, `protocolVersion`). Base JSON-RPC fields
//! (`jsonrpc`, `id`, `method`, `params`, `result`, `error`) use their standard
//! names per the JSON-RPC 2.0 spec.
//!
//! Only the subset of MCP types needed for a tool-server are defined here —
//! consistent with how `dap-types` hand-rolls only the DAP subset we need.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── JSON-RPC 2.0 incoming message ───────────────────────────────────────

/// An incoming message from the client, discriminated by the presence of `id`.
#[derive(Debug, Clone)]
pub enum IncomingMessage {
    /// A JSON-RPC request — expects a response.
    Request {
        id: u64,
        method: String,
        params: Option<serde_json::Value>,
    },
    /// A JSON-RPC notification — no response expected.
    Notification {
        method: String,
        params: Option<serde_json::Value>,
    },
}

// ── Standard JSON-RPC 2.0 error codes ───────────────────────────────────

pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

// ── MCP initialize types ────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: serde_json::Value,
    pub client_info: ImplementationInfo,
    /// Optional path to liblldb shared library (e.g. `C:\LLVM\bin\liblldb.dll`).
    /// When set during the MCP initialize handshake, the parent directory
    /// is prepended to `PATH` when spawning the debug adapter process,
    /// allowing codelldb to find its required shared library at runtime.
    #[serde(default)]
    pub liblldb_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImplementationInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ImplementationInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    pub tools: ToolsCapability,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    pub list_changed: bool,
}

// ── Tool types ──────────────────────────────────────────────────────────

/// A tool definition returned by `tools/list`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub input_schema: JsonSchema,
}

/// A simplified JSON Schema for tool input parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, PropertySchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

/// A single property within a tool's input schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertySchema {
    #[serde(rename = "type")]
    pub prop_type: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<PropertySchema>>,
}

impl JsonSchema {
    /// Create a new object-type schema with no properties.
    pub fn new_object() -> Self {
        JsonSchema {
            schema_type: "object".to_string(),
            properties: Some(HashMap::new()),
            required: Some(Vec::new()),
        }
    }

    /// Add a required property to the schema.
    pub fn with_required(mut self, name: &str, prop: PropertySchema) -> Self {
        self.properties
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), prop);
        self.required
            .get_or_insert_with(Vec::new)
            .push(name.to_string());
        self
    }

    /// Add an optional property to the schema.
    pub fn with_optional(mut self, name: &str, prop: PropertySchema) -> Self {
        self.properties
            .get_or_insert_with(HashMap::new)
            .insert(name.to_string(), prop);
        self
    }
}

// ── Helper constructors for property schemas ────────────────────────────

impl PropertySchema {
    pub fn string(description: &str) -> Self {
        PropertySchema {
            prop_type: "string".to_string(),
            description: description.to_string(),
            items: None,
        }
    }

    pub fn integer(description: &str) -> Self {
        PropertySchema {
            prop_type: "integer".to_string(),
            description: description.to_string(),
            items: None,
        }
    }

    pub fn boolean(description: &str) -> Self {
        PropertySchema {
            prop_type: "boolean".to_string(),
            description: description.to_string(),
            items: None,
        }
    }

    pub fn array_of(description: &str, item_type: &str) -> Self {
        PropertySchema {
            prop_type: "array".to_string(),
            description: description.to_string(),
            items: Some(Box::new(PropertySchema {
                prop_type: item_type.to_string(),
                description: String::new(),
                items: None,
            })),
        }
    }

    pub fn object_of(description: &str, item_type: &str) -> Self {
        PropertySchema {
            prop_type: "object".to_string(),
            description: description.to_string(),
            items: Some(Box::new(PropertySchema {
                prop_type: item_type.to_string(),
                description: String::new(),
                items: None,
            })),
        }
    }
}

// ── tools/list ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
}

// ── tools/call ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl CallToolResult {
    /// Create a successful result with a single text block.
    pub fn success(text: impl Into<String>) -> Self {
        CallToolResult {
            content: vec![ContentBlock {
                content_type: "text".to_string(),
                text: text.into(),
            }],
            is_error: false,
        }
    }

    /// Create a successful result from a JSON-serializable value.
    pub fn success_json(value: &impl Serialize) -> Result<Self, serde_json::Error> {
        Ok(CallToolResult {
            content: vec![ContentBlock {
                content_type: "text".to_string(),
                text: serde_json::to_string_pretty(value)?,
            }],
            is_error: false,
        })
    }

    /// Create an error result (is_error: true, NOT a JSON-RPC error).
    pub fn error(message: impl Into<String>) -> Self {
        CallToolResult {
            content: vec![ContentBlock {
                content_type: "text".to_string(),
                text: message.into(),
            }],
            is_error: true,
        }
    }
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_serialization() {
        let tool = Tool {
            name: "step_in".into(),
            title: "Step Into".into(),
            description: "Steps into the current source line.".into(),
            input_schema: JsonSchema::new_object().with_optional(
                "thread_id",
                PropertySchema::integer("The thread ID to step in."),
            ),
        };

        let json = serde_json::to_string(&tool).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["name"], "step_in");
        assert_eq!(parsed["inputSchema"]["type"], "object");
        assert!(parsed["inputSchema"]["properties"]["thread_id"]["type"] == "integer");
        // Verify camelCase field names
        assert!(parsed.get("input_schema").is_none());
        assert!(parsed.get("inputSchema").is_some());
    }

    #[test]
    fn test_initialize_result_camelcase() {
        let result = InitializeResult {
            protocol_version: "2025-11-25".into(),
            capabilities: ServerCapabilities {
                tools: ToolsCapability { list_changed: true },
            },
            server_info: ImplementationInfo {
                name: "teleDAP".into(),
                version: "0.1.0".into(),
            },
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["protocolVersion"], "2025-11-25");
        assert_eq!(parsed["serverInfo"]["name"], "teleDAP");
        assert_eq!(parsed["capabilities"]["tools"]["listChanged"], true);
        // Ensure no snake_case leaks
        assert!(parsed.get("protocol_version").is_none());
        assert!(parsed.get("server_info").is_none());
    }

    #[test]
    fn test_call_tool_result_success() {
        let result = CallToolResult::success("Stepped into function foo()");
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["content"][0]["type"], "text");
        assert_eq!(parsed["content"][0]["text"], "Stepped into function foo()");
        // is_error should be omitted when false
        assert!(parsed.get("isError").is_none() || parsed["isError"] == false);
        assert!(parsed.get("is_error").is_none());
    }

    #[test]
    fn test_call_tool_result_error() {
        let result = CallToolResult::error("Debuggee not halted");
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["isError"], true);
        assert_eq!(parsed["content"][0]["text"], "Debuggee not halted");
    }

    #[test]
    fn test_call_tool_result_success_json() {
        #[derive(Serialize, Deserialize)]
        struct Data {
            threads: Vec<String>,
            count: u32,
        }
        let data = Data {
            threads: vec!["main".into(), "worker".into()],
            count: 2,
        };
        let result = CallToolResult::success_json(&data).unwrap();
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        let inner: Data =
            serde_json::from_str(parsed["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(inner.threads.len(), 2);
        assert_eq!(inner.count, 2);
    }

    #[test]
    fn test_json_schema_builder() {
        let schema = JsonSchema::new_object()
            .with_required("program", PropertySchema::string("Path to the ELF binary"))
            .with_optional(
                "stop_on_entry",
                PropertySchema::boolean("Stop at entry point"),
            );

        assert_eq!(schema.schema_type, "object");
        let props = schema.properties.unwrap();
        assert_eq!(props["program"].prop_type, "string");
        assert_eq!(props["stop_on_entry"].prop_type, "boolean");
        let required = schema.required.unwrap();
        assert!(required.contains(&"program".to_string()));
        assert!(!required.contains(&"stop_on_entry".to_string()));
    }

    #[test]
    fn test_property_schema_helpers() {
        let str_prop = PropertySchema::string("a string field");
        assert_eq!(str_prop.prop_type, "string");

        let int_prop = PropertySchema::integer("an integer field");
        assert_eq!(int_prop.prop_type, "integer");

        let bool_prop = PropertySchema::boolean("a boolean field");
        assert_eq!(bool_prop.prop_type, "boolean");

        let arr_prop = PropertySchema::array_of("an array of strings", "string");
        assert_eq!(arr_prop.prop_type, "array");
        assert_eq!(arr_prop.items.unwrap().prop_type, "string");

        let obj_prop = PropertySchema::object_of("a string-keyed object", "string");
        assert_eq!(obj_prop.prop_type, "object");
        assert_eq!(obj_prop.items.unwrap().prop_type, "string");
    }

    #[test]
    fn test_initialize_params_deserialization() {
        let json = serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {
                "name": "Claude Desktop",
                "version": "1.0"
            }
        });
        let params: InitializeParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.protocol_version, "2025-11-25");
        assert_eq!(params.client_info.name, "Claude Desktop");
        assert_eq!(params.client_info.version, "1.0");
    }

    #[test]
    fn test_initialize_params_with_liblldb_path() {
        let json = serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {
                "name": "Claude Desktop",
                "version": "1.0"
            },
            "liblldbPath": "C:\\LLVM\\bin\\liblldb.dll"
        });
        let params: InitializeParams = serde_json::from_value(json).unwrap();
        assert_eq!(
            params.liblldb_path.as_deref(),
            Some("C:\\LLVM\\bin\\liblldb.dll")
        );
    }

    #[test]
    fn test_initialize_params_without_liblldb_path() {
        let json = serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {
                "name": "Claude Desktop",
                "version": "1.0"
            }
        });
        let params: InitializeParams = serde_json::from_value(json).unwrap();
        assert_eq!(params.liblldb_path, None);
    }
}
