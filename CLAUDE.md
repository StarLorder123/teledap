# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Status

**Phases 1–3 complete** — DAP types/codec/client/trace, debug-session state machine with context-chain assembly, and MCP JSON-RPC server with 24 state-gated debug tools are fully implemented with 196 tests. Phase 4 (OpenOCD) and Phase 5 (advanced features) are planned but not started.

## Commands

```bash
# Build
cargo build --workspace

# Tests — all pass without codelldb (integration tests self-skip via env probe)
cargo test --workspace               # everything
cargo test --workspace --lib          # unit tests only (fast)
cargo test -p dap-codec               # single crate
cargo test -p debug-bridge            # bridge unit + MCP integration tests
cargo test -p debug-session           # session unit + integration tests
cargo test -p dap-client -- --nocapture  # integration test output

# Lint (must pass before commit)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Run CLI debug session (terminal mode)
cargo run -- --codelldb-path /path/to/codelldb --elf-path ./app.elf -v

# Run MCP server (pipe-in mode — auto-detected when stdin is not a terminal)
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run --quiet --

# Force CLI mode when stdin is a pipe (e.g. PowerShell)
cargo run -- --cli --codelldb-path /path/to/codelldb --elf-path ./app.elf

# E2E MCP protocol smoke test
./test_mcp_e2e.sh                   # Unix/Mac
powershell -File test_mcp_e2e.ps1   # Windows
```

## Architecture

```
teledap (root binary — auto-detect: pipe→MCP server, terminal→CLI)
  ├── debug-bridge        — MCP→DAP translation: ToolRegistry, 24 tool handlers, state gating
  │   ├── mcp-protocol    — JSON-RPC 2.0 types, line-delimited stdio transport
  │   ├── debug-session   — state machine, context-chain, path mapping, variable cache
  │   │   ├── dap-client  — codelldb process lifecycle, typed RPC (oneshot + mpsc)
  │   │   │   ├── dap-codec   — tokio Decoder/Encoder for Content-Length framing
  │   │   │   ├── dap-types   — all 103 DAP spec types (serde-tagged enums)
  │   │   │   └── dap-trace   — non-blocking session audit (ring buffer + JSONL)
  │   │   └── dap-trace
  │   └── dap-types
  ├── mcp-protocol
  └── (direct) dap-client, dap-trace, debug-session
```

**Crate dependency direction:** `teledap → {debug-bridge, mcp-protocol, debug-session, dap-client}`. `dap-types` is the leaf crate with zero internal dependencies; every other crate depends on it.

**Dual-mode binary:** `src/main.rs` detects mode automatically — pipe stdin → MCP server (`src/server.rs`), terminal stdin → CLI (`src/cli.rs`). Use `--cli` to force CLI when piped.

## Key Design Patterns

### Two wire protocols

| Layer | Transport | Framing |
|-------|-----------|---------|
| **MCP** (AI ↔ TeleDAP) | stdin/stdout | Line-delimited JSON (`\n` terminated) |
| **DAP** (TeleDAP ↔ codelldb) | child process stdin/stdout | `Content-Length: <N>\r\n\r\n<JSON body>` |

`McpServer` reads lines via `BufReader::read_line()`. `DapCodec` implements `tokio_util::codec::Decoder<Item=ProtocolMessage>` handling partial reads, sticky packets, oversized frames (max 4 MiB), and header size limits (4 KiB).

### ProtocolMessage — serde internal tagging

```rust
#[serde(tag = "type")]
pub enum ProtocolMessage {
    #[serde(rename = "request")]  Request(Request),
    #[serde(rename = "response")] Response(Response),
    #[serde(rename = "event")]    Event(Event),
}
```

### DAP Request/Response dispatch (oneshot channels)

`DapClient::send_request()` allocates a monotonic `seq`, inserts a `oneshot::Sender` into `pending_requests: HashMap<u64, oneshot::Sender<Response>>` **before** writing to stdin (avoids race). A background `tokio::spawn` task reads stdout via `FramedRead<ChildStdout, DapCodec>`, decodes frames, and routes:
- **Response** → looks up `request_seq` in the pending map, sends through the oneshot
- **Event** → pushes to `mpsc::unbounded_channel`, consumed via `recv_event()`
- **Request** → logged as warning (codelldb doesn't send reverse requests)

There is also `send_request_nb()` (no-block) for fire-and-forget requests like `launch` and `configurationDone` — codelldb defers the `launch` response until after `configurationDone`, so awaiting it would deadlock.

### DapRequest trait (typed RPC)

```rust
pub trait DapRequest {
    const COMMAND: &'static str;
    type Arguments: Serialize + Default;
    type Response: DeserializeOwned;
}
```

Each DAP command has a unit struct implementing this trait. Callers write `client.send_request::<InitializeRequest>(args).await` and get back a statically-typed `Capabilities`.

### MCP Tool dispatch (ToolRegistry)

`ToolRegistry` is a stateless struct. `dispatch(name, session, params, trace)`:
1. Maps tool name → operation via `tools::tool_operation()`
2. Validates state via `ToolAvailability::is_allowed(op, state)` — wrong state returns `is_error: true`, **not** a JSON-RPC error
3. Records a trace entry with `TraceSource::McpTrigger`
4. Routes to handler in `handlers/{lifecycle,execution,breakpoint,inspect}.rs`
5. Handler deserializes MCP params, calls `DebugSession` methods (which internally call DAP), returns `CallToolResult`

`tools/list` returns only tools valid for the **current session state** — this is how the AI discovers what it can do.

### Session state machine

Five states: `Disconnected → Connected → Initialized → {Running ↔ Halted}`. DAP events drive transitions in a background tokio task spawned by `server.rs`: `initialized` → Initialized, `stopped` → Halted, `continued` → Running, `terminated/exited` → Disconnected.

`handle_event()` also triggers variable cache invalidation (Halted→Running/Disconnected) and `watch::channel` broadcasts.

### Fire-and-forget pattern for launch

`launch` and `configurationDone` use `send_request_nb()`. codelldb defers the `launch` response until `configurationDone` is received, so awaiting `launch` in the traditional `send_request()` + oneshot pattern would deadlock.

### PathMapper — bidirectional path translation

AI works with short relative paths (`src/main.cpp`); the debugger needs absolute paths. `PathMapper` supports:
- Alias registration: `register_alias("src", "/home/user/project/src")`
- Base directory fallback: `register_base_dir("/home/user/project")`
- Longest-prefix matching for alias disambiguation
- `resolve()` (AI→system) and `reverse()` (system→AI)

`launch` and `set_breakpoints` handlers automatically resolve paths through the mapper.

### VariableHandleCache

Maps variable **names** to DAP `variablesReference` integer handles. Supports 4-level lookup priority (exact name+frame+scope → name+frame → name-only → fuzzy). Auto-invalidated on Halted→Running/Disconnected transitions. Enables the `search_variables` tool for fuzzy name search.

### camelCase convention

All DAP JSON uses camelCase. Rust structs use snake_case fields with `#[serde(rename_all = "camelCase")]`. Optional fields use `#[serde(default, skip_serializing_if = "Option::is_none")]`.

### Workspace dependencies

All dependency versions are declared in the root `Cargo.toml` `[workspace.dependencies]` table. Internal crates are referenced via `dap-types.workspace = true` (version) and resolved through `[workspace.dependencies] dap-types = { path = "crates/dap-types" }`. New crates must be added to both `members` and `[workspace.dependencies]`.

## Testing

- **Unit tests** are inline `#[cfg(test)] mod tests` blocks in each source file — spread across 27 files.
- **Integration tests** live in `crates/dap-client/tests/`, `crates/debug-session/tests/`, and `crates/debug-bridge/tests/`. They spawn a real codelldb process and use a `codelldb_available()` env probe that gracefully skips when codelldb is absent. Every async test is wrapped in `tokio::time::timeout()` to prevent hangs.
- **MCP E2E scripts** (`test_mcp_e2e.sh`, `test_mcp_e2e.ps1`) pipe 7 phases of JSON-RPC messages into the binary and assert every response — no test framework needed.
- **test_debuggee** (`test_debuggee/main.c`) is a simple C program compiled for integration tests — tests locate it relative to workspace root or `CARGO_MANIFEST_DIR`.
- New dap-codec tests go in `crates/dap-codec/src/lib.rs` in the existing `mod tests` block; use `make_codec()` and `make_wire()` / `make_wire_value()` helpers.
- New state machine tests go in `crates/debug-session/src/state.rs` and `gating.rs`.
- New handler param tests go in the handler's own `#[cfg(test)]` block.
