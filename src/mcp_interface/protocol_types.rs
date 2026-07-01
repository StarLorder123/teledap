//! JSON Schema constants for MCP tool input schemas.
//!
//! Each tool exposed by TeleDAP has an input schema defined here
//! as a `serde_json::Map<String, Value>`, wrapped in `Arc` for
//! efficient sharing via the rmcp `Tool` struct.

use serde_json::{json, Map, Value};
use std::sync::Arc;

/// Helper: build a JsonObject (serde_json::Map) from a `serde_json::Value`.
fn to_schema(v: Value) -> Arc<Map<String, Value>> {
    Arc::new(v.as_object().unwrap().clone())
}

// ── Always Available ─────────────────────────────────────────────

pub fn get_status_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {},
        "description": "Get the current TeleDAP session status"
    }))
}

pub fn get_debug_logs_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "count": {
                "type": "integer",
                "description": "Number of recent log entries to return (max 500)",
                "default": 20,
                "minimum": 1,
                "maximum": 500
            }
        }
    }))
}

pub fn shutdown_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {},
        "description": "Gracefully shutdown all connections and exit"
    }))
}

// ── Disconnected State ───────────────────────────────────────────

pub fn auto_launch_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "elf_path": {
                "type": "string",
                "description": "Absolute path to the ELF/EXE binary to debug"
            },
            "mode": {
                "type": "string",
                "enum": ["remote", "local"],
                "description": "Debug mode: 'remote' for hardware via OpenOCD, 'local' for host binary without hardware",
                "default": "remote"
            }
        },
        "required": ["elf_path"]
    }))
}

pub fn connect_openocd_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "host": {
                "type": "string",
                "description": "OpenOCD hostname or IP"
            },
            "tcl_port": {
                "type": "integer",
                "description": "OpenOCD Tcl RPC port",
                "default": 6666
            }
        }
    }))
}

// ── Initialized+ State ───────────────────────────────────────────

pub fn reset_halt_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {}
    }))
}

pub fn flash_erase_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "address": {
                "type": "integer",
                "description": "Flash address to erase (hex or decimal)"
            },
            "length": {
                "type": "integer",
                "description": "Number of bytes to erase"
            }
        },
        "required": ["address", "length"]
    }))
}

pub fn flash_write_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "address": {
                "type": "integer",
                "description": "Flash address to write to"
            },
            "data_hex": {
                "type": "string",
                "description": "Hex-encoded binary data to write"
            }
        },
        "required": ["address", "data_hex"]
    }))
}

pub fn read_register_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "register": {
                "type": "string",
                "description": "Register name or address (e.g., 'GPIOA_ODR', '0x40020014')"
            }
        },
        "required": ["register"]
    }))
}

pub fn write_register_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "register": {
                "type": "string",
                "description": "Register name or address"
            },
            "value": {
                "type": "integer",
                "description": "32-bit value to write (hex or decimal)"
            }
        },
        "required": ["register", "value"]
    }))
}

pub fn read_memory_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "address": {
                "type": "integer",
                "description": "Starting memory address"
            },
            "length": {
                "type": "integer",
                "description": "Number of bytes to read",
                "minimum": 1,
                "maximum": 4096
            }
        },
        "required": ["address", "length"]
    }))
}

pub fn write_memory_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "address": {
                "type": "integer",
                "description": "Starting memory address"
            },
            "data_hex": {
                "type": "string",
                "description": "Hex-encoded bytes to write"
            }
        },
        "required": ["address", "data_hex"]
    }))
}

// ── Halted State Only ────────────────────────────────────────────

pub fn set_breakpoint_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "file": {
                "type": "string",
                "description": "Source file path"
            },
            "line": {
                "type": "integer",
                "description": "Line number (1-based)",
                "minimum": 1
            }
        },
        "required": ["file", "line"]
    }))
}

pub fn continue_execution_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "thread_id": {
                "type": "integer",
                "description": "Thread ID (0 for all threads)",
                "default": 0
            }
        }
    }))
}

pub fn halt_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {}
    }))
}

pub fn step_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "thread_id": {
                "type": "integer",
                "description": "Thread ID to step",
                "default": 0
            }
        }
    }))
}

pub fn stack_trace_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "thread_id": {
                "type": "integer",
                "description": "Thread ID for stack trace",
                "default": 0
            }
        }
    }))
}

pub fn variables_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "frame_id": {
                "type": "integer",
                "description": "Stack frame ID from stack trace response"
            }
        },
        "required": ["frame_id"]
    }))
}

pub fn evaluate_schema() -> Arc<Map<String, Value>> {
    to_schema(json!({
        "type": "object",
        "properties": {
            "expression": {
                "type": "string",
                "description": "C/C++ expression to evaluate"
            },
            "frame_id": {
                "type": "integer",
                "description": "Stack frame context for variable resolution"
            }
        },
        "required": ["expression"]
    }))
}
