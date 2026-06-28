# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
cargo build              # debug build
cargo build --release    # release build
cargo test               # run all 32 unit tests
cargo test -p teledap -- frame_decoder  # run tests matching "frame_decoder"
cargo test -- --nocapture               # show stdout/stderr during tests
```

There is no lint/format config yet. Use `cargo clippy` and `cargo fmt` with Rust defaults.

## Architecture

TeleDAP is an MCP server (Model Context Protocol) that bridges AI assistants to embedded hardware debuggers. It speaks JSON-RPC 2.0 over stdin/stdout and translates stateless MCP tool calls into stateful interactions with CodeLLDB (DAP protocol) and OpenOCD (Tcl RPC over TCP).

**Layered architecture — strict top-down dependencies.**

Each directory's `mod.rs` only declares submodules and re-exports public types. Business logic lives in named files:

```
main.rs                              Wiring: CLI → AuditLogger → SessionCoordinator → TeleDapServer → rmcp stdio
mcp_interface/
  mod.rs                             Module declarations + re-exports TeleDapServer
  server.rs                          TeleDapServer: impl ServerHandler trait → list_tools / call_tool → SessionCoordinator
  protocol_types.rs                  JSON Schema constants for all 17 tool input schemas
session_coordinator/
  mod.rs                             Module declarations + re-exports SessionCoordinator
  coordinator.rs                     SessionCoordinator: state machine + tool dispatch → DapDriver + OpenOcdDriver
  state_machine.rs                   SessionState enum + transition validation
drivers/
  mod.rs                             Module declarations
  openocd_tcl.rs                     OpenOcdDriver: TCP connection to port 6666, \x1a-delimited Tcl commands
  lldb_dap/
    mod.rs                           Module declarations + re-exports DapDriver
    driver.rs                        DapDriver: spawns codelldb process, DAP protocol methods
    frame_decoder.rs                 DapCodec: Content-Length-framed decoder/encoder (tokio_util Codec)
audit_tracker/
  mod.rs                             Module declarations + re-exports AuditLogger, LogSource, LogDirection
  logger.rs                          AuditLogger: non-blocking mpsc channel → ring buffer + JSONL file
  model.rs                           AuditLogEntry, LogSource, LogDirection types
error.rs                             TeleDapError + DriverError (thiserror), separate error layers
```

**Data flow:** LLM → stdin JSON-RPC → `TeleDapServer.call_tool()` → `SessionCoordinator.execute_tool()` → validates state → delegates to `DapDriver` or `OpenOcdDriver` → returns result string wrapped in `CallToolResult::success()`.

**Cross-cutting:** Every operation is logged to `AuditLogger.log()` (non-blocking, unbounded mpsc send). A single background tokio task writes to ring buffer + optional `.jsonl` file. Disk I/O never blocks the debug link.

## State Machine

`SessionState` in `session_coordinator/state_machine.rs`:

```
Disconnected → Initialized → Attached → Halted ↔ Running
```

- **Disconnected:** only `auto_launch` and `connect_openocd` are available
- **Initialized:** OpenOCD connected; hardware tools available (flash, registers, memory, reset_halt)
- **Attached:** CodeLLDB launched + gdb-remote connected; DAP control available
- **Halted:** target stopped; breakpoints, stepping, variables, stack trace, evaluate available
- **Running:** target executing; only `halt`, `continue_execution`, hardware tools available

Tool availability in `list_tools()` is gated by current state. Calling a tool in the wrong state returns `ToolUnavailable`.

## Key Design Patterns

**rmcp non-exhaustive structs:** `Tool`, `CallToolResult`, `ServerInfo`, `Implementation` are all `#[non_exhaustive]`. Use constructors only — `Tool::new(name, desc, schema)`, `CallToolResult::success(content)`, `CallToolResult::error(content)`, `ServerInfo::new(capabilities)`, `Implementation::new(name, version)`. Struct literal syntax with `..Default::default()` will fail to compile.

**DAP protocol framing:** DAP uses LSP-style `Content-Length: <N>\r\n\r\n<JSON>` framing. The `DapCodec` in `frame_decoder.rs` implements `tokio_util::codec::Decoder` — returns `Ok(None)` for partial frames, handles multiple messages per buffer, and rejects frames exceeding `max_frame_size`.

**OpenOCD Tcl RPC:** Commands and responses are `\x1a` (Ctrl+Z) terminated text. The `send_command()` method appends `\x1a`, sends, reads until `\x1a`, and strips it from the response. `TCP_NODELAY` is set for minimum latency.

**Audit logger channel architecture:** `log()` pushes to `mpsc::unbounded_channel` (never blocks). A single background consumer task handles ring buffer push + JSONL file append. When all `Arc<AuditLogger>` references drop, the channel closes, the consumer flushes and exits.

**Error flow:** `DriverError` (low-level: I/O, protocol) —`#[from]`→ `TeleDapError` (session: state, params) → `CallToolResult::error()` (visible to LLM). Use `CallToolResult::error()` for tool-level failures the LLM should see; use `Err(McpError)` only for protocol-level errors.

## Working with rmcp 1.8

- `ServerHandler` trait lives at `rmcp::handler::server::ServerHandler` and is auto-implemented as `Service<RoleServer>` via blanket impl
- `RequestContext` is at `rmcp::service::RequestContext` (NOT `handler::server::RequestContext` — that path is private)
- `rmcp::ErrorData as McpError` — the `rmcp::Error` alias is deprecated
- stdio transport: `rmcp::transport::io::stdio()` returns `(Stdin, Stdout)` tuple — pass directly to `.serve((stdin, stdout))`
- `CallToolRequestParams.arguments` is `Option<JsonObject>` (`Map<String, Value>`), not `Value` — wrap with `Value::Object(args_map)` for internal APIs

## CLI

```
teledap --codelldb-path /usr/bin/codelldb --openocd-host 192.168.1.10 --openocd-tcl-port 6666 --openocd-gdb-port 3333 --log-dir ./logs -v
```

All args have sensible defaults. The server starts on stdio — it's designed to be launched by an MCP client, not run interactively. Diagnostic output goes to stderr via `tracing-subscriber`; stdout is reserved for JSON-RPC.
