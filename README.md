# TeleDAP

[English](README.md) · [中文](README.zh.md)

MCP (Model Context Protocol) server that bridges AI assistants to embedded hardware debuggers. Speaks JSON-RPC 2.0 over stdin/stdout (stdio MCP) or HTTP/SSE, and translates stateless MCP tool calls into stateful interactions with [CodeLLDB](https://github.com/vadimcn/codelldb) / GDB (DAP protocol) and [OpenOCD](https://openocd.org/) (Tcl RPC over TCP).

> **Status: Phases 1–3 complete.** DAP protocol stack, session state machine with context-chain assembly, 30-tool MCP server (stdio + HTTP/SSE), and OpenOCD management tools are all implemented. The binary auto-detects execution mode: `--http` → HTTP/SSE MCP server, pipe stdin → stdio MCP server, terminal stdin → verification CLI.

## Architecture

```
teledap (auto-detect: --http→HTTP/SSE, pipe→stdio MCP, terminal→CLI)
  ├── debug-bridge       — 30 MCP tools, state-aware dispatch, handler routing
  │   ├── handlers       — lifecycle, execution, breakpoint, inspect, openocd
  │   └── tools.rs       — tool definitions and input schemas
  ├── mcp-protocol       — JSON-RPC 2.0 types + line-delimited stdio transport
  ├── debug-session      — state machine, context-chain, variable expansion, path mapping
  │   ├── dap-client     — codelldb/GDB process lifecycle, typed RPC, event streaming
  │   │   ├── dap-codec  — Content-Length framed protocol (tokio Decoder/Encoder)
  │   │   └── dap-types  — 103 DAP spec types with serde support
  │   └── dap-trace      — non-blocking session audit (ring buffer + JSONL)
  └── openocd-client     — OpenOCD Tcl RPC client (TCP transport)
```

### Crate Map

| Crate | Description |
|-------|-------------|
| `dap-types` | All 103 DAP specification types: 42 requests, 17 events, 36 data types |
| `dap-codec` | Tokio codec for `Content-Length: N\r\n\r\n<JSON>` wire framing |
| `dap-client` | Async debug adapter process manager with typed RPC and event streaming |
| `dap-trace` | Non-blocking debug session recorder with ring buffer and JSONL output |
| `debug-session` | State machine (5 states), context-chain assembly, C++ variable expansion, path mapping, variable handle cache |
| `mcp-protocol` | JSON-RPC 2.0 types and line-delimited stdin/stdout transport |
| `openocd-client` | OpenOCD Tcl RPC client: start, stop, send commands, read output |
| `debug-bridge` | 30 MCP tools with state-aware dispatch via `ToolRegistry` |
| `teledap` (root) | Binary: MCP server (stdio/HTTP/SSE) or verification CLI (terminal) — auto-detected |

## Quick Start

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) stable toolchain
- [CodeLLDB](https://github.com/vadimcn/codelldb/releases) native binary (for integration tests and runtime)
- (Optional) [OpenOCD](https://openocd.org/) for embedded hardware debugging

### Build

```bash
cargo build --release
```

## MCP Server Usage

TeleDAP supports two MCP transport layers:

| Mode | Trigger | Use Case |
|------|---------|----------|
| **stdio** | Spawned by AI client via pipe (e.g. Claude Desktop, Claude Code) | Most common; single AI client |
| **HTTP/SSE** | `cargo run --release -- --http --port 8080` | Custom agents, multi-client shared session, Web UI |

### stdio Mode

When spawned by an AI client via pipe, TeleDAP auto-detects MCP mode and speaks JSON-RPC 2.0 over stdin/stdout:

```json
// → initialize
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
// ← capabilities
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"teleDAP","version":"0.1.0"}}}

// → list available tools for current state
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}

// → start codelldb
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"start","arguments":{"adapterPath":"/usr/bin/codelldb"}}}

// → full debug lifecycle: initialize → launch → set_breakpoints → configuration_done → …
```

### HTTP/SSE Mode

```bash
cargo run --release -- --http --port 8080
```

1. Open SSE stream:
   ```bash
   curl -N http://localhost:8080/sse
   # event: endpoint
   # data: /message?session_id=550e8400-e29b-41d4-a716-446655440000
   ```
2. Send JSON-RPC via POST:
   ```bash
   curl -X POST "http://localhost:8080/message?session_id=..." \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
   # → 202 Accepted
   ```
3. Receive responses asynchronously through the SSE stream.

### MCP Tools

**30 MCP tools** (21 state-gated + 9 utility, always available):

| Category | Tools |
|----------|-------|
| Lifecycle | `start`, `initialize`, `launch`, `attach`, `configuration_done`, `shutdown` |
| Execution | `continue`, `step_over`, `step_in`, `step_out`, `pause` |
| Breakpoints | `set_breakpoints`, `set_function_breakpoints`, `list_breakpoints` |
| Introspection | `get_threads`, `get_stack_trace`, `get_scopes`, `get_variables`, `evaluate`, `set_variable`, `assemble_context`, `search_variables` |
| Utility | `get_state`, `register_path_alias`, `register_base_dir` |
| OpenOCD | `openocd_start`, `openocd_stop`, `openocd_status`, `openocd_output`, `openocd_send` |

Tools are gated by session state — e.g. `continue` only appears in `tools/list` when `Halted`; `pause` only when `Running`.

## Verification CLI

When run from a terminal, TeleDAP runs a full debug session with breakpoints, variable inspection, and stack backtraces:

```bash
# Basic handshake (no ELF needed)
cargo run -- --adapter-path /usr/bin/codelldb

# Full debug session with breakpoints and variable inspection
cargo run -- --adapter-path /usr/bin/codelldb --elf-path ./target/debug/my_app

# Custom source and breakpoints
cargo run -- --adapter-path /usr/bin/codelldb --elf-path ./app.elf \
    --source-path ./src/main.c --breakpoints "10,15,22"

# Remote debugging via GDB server
cargo run -- --adapter-path /usr/bin/codelldb --elf-path ./firmware.elf \
    --gdb-remote 192.168.1.10:3333

# With debug trace recording
cargo run -- --adapter-path /usr/bin/codelldb --elf-path ./app.elf --log-dir ./traces -v

# Force CLI mode (even when piped)
cargo run -- --cli --adapter-path /usr/bin/codelldb
```

### CLI Options

| Option | Default | Description |
|--------|---------|-------------|
| `--adapter-path` | `codelldb` | Path to debug adapter binary (codelldb or gdb) |
| `--adapter-kind` | `codelldb` | Adapter kind: `codelldb` or `gdb` |
| `--adapter-args` | *(none)* | Command-line arguments for the adapter binary (repeatable) |
| `--liblldb-path` | *(none)* | Path to liblldb shared library (Windows: `liblldb.dll`) |
| `--elf-path` | *(empty)* | Path to ELF binary to debug |
| `--source-path` | *(inferred)* | Path to source file for breakpoints |
| `--breakpoints` | `9,13,4` | Comma-separated line numbers for breakpoints |
| `--gdb-remote` | *(none)* | Remote GDB server address (`host:port`) |
| `--log-dir` | *(none)* | Directory for debug trace JSONL output |
| `--http` | `false` | Run as HTTP/SSE MCP server |
| `--port` | `8080` | Port for HTTP/SSE MCP server |
| `--cli` | `false` | Force CLI mode (skip stdin terminal detection) |
| `-v`, `--verbose` | off | Enable verbose/debug logging |

## Session State Machine

```
Disconnected ──start()──▶ Connected ──initialize()──▶ Initialized
     ▲                         │                          │
     │                         │ shutdown()               │ launch() + configurationDone()
     │                         ▼                          ▼
     │                    Disconnected              Running ──pause()──▶ Halted
     │                         ▲                          │               │
     │                         │ shutdown()               │ continue()    │
     │                         │                          ▼               │
     └───────── shutdown() ────┴────────────────── Disconnected ◀─────────┘
```

Every tool call validates the current state; calling a tool in the wrong state returns a descriptive error with the required states.

## Key Features

### Context-Chain Assembly
`assemble_context` builds a full snapshot: threads → frames → scopes → variables, expanded recursively up to a configurable depth. The result is a nested JSON structure suitable for AI consumption.

### C++ Variable Expansion
`get_variables` supports recursive expansion of pointers, structs, and arrays with depth limiting and paging. A thread-safe variable handle cache maintains name → handle mappings with auto-invalidation on state transitions. `search_variables` provides fuzzy name lookup across the cache.

### Path Mapping
`register_path_alias` and `register_base_dir` let AI clients work with short relative paths (e.g. `src/main.cpp`) while the debugger resolves them to absolute system paths.

### Operation Gating
`ToolAvailability` maps 21 operations to the session states in which they are legal. The MCP server filters `tools/list` responses to only show tools available in the current state, preventing invalid operations before they reach the debugger.

### Dual-Transport, Tri-Mode Binary
The same binary serves three roles:
- **`--http`** → HTTP/SSE MCP server (multi-client shared session)
- **stdin is a pipe** → stdio MCP server (JSON-RPC 2.0, tracing to stderr)
- **stdin is a terminal** → Phase 2 verification CLI (interactive debug session)
- `--cli` flag forces CLI mode regardless of stdin type

### OpenOCD Integration
`openocd_start` / `openocd_stop` / `openocd_send` / `openocd_output` manage the OpenOCD GDB server lifecycle and expose raw Tcl commands for flash, reset, register inspection, and memory dumps.

## AI Agent Integration

See the integration guides for configuration and ready-to-use system prompts:

- [`docs/AI_Agent_集成指南.md`](docs/AI_Agent_集成指南.md) — 中文指南
- [`docs/AI_Agent_Integration_Guide.md`](docs/AI_Agent_Integration_Guide.md) — English guide
- [`docs/prompts/`](docs/prompts/) — sample system prompts for Claude Desktop, stdio, and HTTP/SSE agents

## Roadmap

### Phase 1 ✅ — DAP Protocol Foundation
- 103 DAP specification types with serde support
- Wire-format codec (Content-Length framing, sticky-packet handling)
- Child-process client (codelldb lifecycle, typed RPC, event streaming)
- Debug session trace recorder (ring buffer + JSONL)
- Verification CLI with basic debug lifecycle
- Unit + integration test suite

### Phase 2 ✅ — State Machine & Debug Bridge
- Session state machine: `Disconnected → Connected → Initialized → Running ↔ Halted`
- Context-chain assembly (threads, frames, scopes, variables — nested expansion)
- C++ variable expansion with depth limiting, paging, and handle caching
- State-gated tool availability (21 operations × 5 states)
- Path mapping: AI-relative ↔ system-absolute bidirectional translation
- Enhanced CLI: real breakpoints, variable inspection, stack backtraces

### Phase 3 ✅ — MCP Integration
- JSON-RPC 2.0 line-delimited transport over stdin/stdout
- 30 MCP tools: 6 lifecycle, 5 execution, 3 breakpoint, 8 introspection, 4 utility, 5 OpenOCD
- State-aware `tools/list` filtering and `tools/call` dispatch
- HTTP/SSE MCP server with multi-client shared session
- OpenOCD management tools (`openocd_start`, `openocd_stop`, `openocd_send`, etc.)
- Auto-detection: `--http` → HTTP/SSE, pipe → stdio MCP, terminal → CLI
- E2E integration tests with live codelldb (7-phase dispatch verification)

### Phase 4 📋 — Advanced Hardware Integration
- Combined DAP + OpenOCD workflows (e.g. flash → debug → inspect)
- Register peek/poke and memory dump tools
- Multi-target support (simultaneous GDB server + local debug)

### Phase 5 📋 — Advanced Features
- Multi-thread aware debugging (per-thread breakpoints, thread-specific stepping)
- Disassembly view and instruction-level stepping
- Memory watchpoints and data breakpoints
- Expression evaluation with persistent watch expressions
- Debug session replay from trace JSONL

## Running Tests

```bash
# All tests (unit + integration, skips gracefully without codelldb)
cargo test --workspace

# Unit tests only (fast, no codelldb needed)
cargo test --workspace --lib

# Specific crate
cargo test -p dap-codec                         # codec unit tests
cargo test -p dap-client                        # client unit + integration tests
cargo test -p debug-bridge                      # bridge unit tests + MCP integration tests
cargo test -p debug-session                     # session unit + integration tests

# Show output (useful for integration test diagnostics)
cargo test -- --nocapture

# Lint
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## License

MIT — see [LICENSE](LICENSE) for details.
