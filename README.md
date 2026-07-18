# TeleDAP

MCP (Model Context Protocol) server that bridges AI assistants to embedded hardware debuggers. Speaks JSON-RPC 2.0 over stdin/stdout and translates stateless MCP tool calls into stateful interactions with [CodeLLDB](https://github.com/vadimcn/codelldb) (DAP protocol) and [OpenOCD](https://openocd.org/) (Tcl RPC over TCP).

> **Status: Phase 1–3 complete.** DAP protocol stack, session state machine with context-chain assembly, and 24-tool MCP server are all implemented. The binary auto-detects execution mode: pipe → MCP server, terminal → verification CLI.

## Architecture

```
teledap (auto-detect: MCP server / verification CLI)
  ├── debug-bridge    — 24 MCP tools, state-aware dispatch, handler routing
  │   └── handlers    — lifecycle, execution, breakpoint, inspect (4 modules)
  ├── mcp-protocol    — JSON-RPC 2.0 types + line-delimited stdio transport
  ├── debug-session   — state machine, context-chain, variable expansion, path mapping
  │   ├── dap-client  — codelldb process lifecycle, typed request/response, event streaming
  │   │   ├── dap-codec  — Content-Length framed protocol (tokio Decoder/Encoder)
  │   │   └── dap-types  — 103 DAP spec types with serde support
  │   └── dap-trace   — non-blocking session audit (ring buffer + JSONL)
```

### Crate Map

| Crate | Description |
|-------|-------------|
| `dap-types` | All 103 DAP specification types: 42 requests, 17 events, 36 data types |
| `dap-codec` | Tokio codec for `Content-Length: N\r\n\r\n<JSON>` wire framing |
| `dap-client` | Async codelldb process manager with typed RPC and event streaming |
| `dap-trace` | Non-blocking debug session recorder with ring buffer and JSONL output |
| `debug-session` | State machine (5 states), context-chain assembly, C++ variable expansion, path mapping, variable handle cache |
| `mcp-protocol` | JSON-RPC 2.0 types and line-delimited stdin/stdout transport |
| `debug-bridge` | 24 MCP tools with state-aware dispatch via `ToolRegistry` |
| `teledap` (root) | Binary: MCP server (pipe) or verification CLI (terminal) — auto-detected |

## Quick Start

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) stable toolchain
- [CodeLLDB](https://github.com/vadimcn/codelldb/releases) native binary (for integration tests and runtime)

### Build

```bash
cargo build --release
```

### Usage — MCP Server

When spawned by an AI client (e.g. Claude Desktop) via pipe, TeledAP auto-detects MCP mode and speaks JSON-RPC 2.0 over stdin/stdout:

```json
// → initialize
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
// ← capabilities
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"teleDAP","version":"0.1.0"}}}

// → list available tools for current state
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}

// → start codelldb
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"start","arguments":{"codelldb_path":"/usr/bin/codelldb"}}}

// → full debug lifecycle: initialize → launch → set_breakpoints → configuration_done → …
```

**24 MCP tools** (20 state-gated + 4 utility, always available):

| Category | Tools |
|----------|-------|
| Lifecycle | `start`, `initialize`, `launch`, `attach`, `configuration_done`, `shutdown` |
| Execution | `continue`, `step_over`, `step_in`, `step_out`, `pause` |
| Breakpoints | `set_breakpoints`, `set_function_breakpoints` |
| Introspection | `get_threads`, `get_stack_trace`, `get_scopes`, `get_variables`, `evaluate`, `set_variable`, `assemble_context` |
| Utility | `get_state`, `register_path_alias`, `register_base_dir`, `search_variables` |

Tools are gated by session state — e.g. `continue` only appears in `tools/list` when `Halted`; `pause` only when `Running`.

### Usage — Verification CLI

When run from a terminal, TeledAP runs a full debug session with breakpoints, variable inspection, and stack backtraces:

```bash
# Basic handshake (no ELF needed)
cargo run -- --codelldb-path /usr/bin/codelldb

# Full debug session with breakpoints and variable inspection
cargo run -- --codelldb-path /usr/bin/codelldb --elf-path ./target/debug/my_app

# Custom source and breakpoints
cargo run -- --codelldb-path /usr/bin/codelldb --elf-path ./app.elf \
    --source-path ./src/main.c --breakpoints "10,15,22"

# Remote debugging via GDB server
cargo run -- --codelldb-path /usr/bin/codelldb --elf-path ./firmware.elf \
    --gdb-remote 192.168.1.10:3333

# With debug trace recording
cargo run -- --codelldb-path /usr/bin/codelldb --elf-path ./app.elf --log-dir ./traces -v

# Force CLI mode (even when piped)
cargo run -- --cli --codelldb-path /usr/bin/codelldb
```

### CLI Options

| Option | Default | Description |
|--------|---------|-------------|
| `--codelldb-path` | `codelldb` | Path to CodeLLDB binary |
| `--elf-path` | *(empty)* | Path to ELF binary to debug |
| `--source-path` | *(inferred)* | Path to source file for breakpoints |
| `--breakpoints` | `9,13,4` | Comma-separated line numbers for breakpoints |
| `--gdb-remote` | *(none)* | Remote GDB server address (`host:port`) |
| `--log-dir` | *(none)* | Directory for debug trace JSONL output |
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
`get_variables` supports recursive expansion of pointers, structs, and arrays with depth limiting and paging. A thread-safe variable handle cache maintains name → handle mappings with auto-invalidation on state transitions.

### Path Mapping
`register_path_alias` and `register_base_dir` let AI clients work with short relative paths (e.g. `src/main.cpp`) while the debugger resolves them to absolute system paths.

### Operation Gating
`ToolAvailability` maps 21 operations to the session states in which they are legal. The MCP server filters `tools/list` responses to only show tools available in the current state, preventing invalid operations before they reach the debugger.

### Dual-Mode Binary
The same binary serves both roles:
- **stdin is a pipe** → MCP server (JSON-RPC 2.0, tracing to stderr)
- **stdin is a terminal** → Phase 2 verification CLI (interactive debug session)
- `--cli` flag forces CLI mode regardless of stdin type

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
- 24 MCP tools: 6 lifecycle, 5 execution, 2 breakpoint, 7 introspection, 4 utility
- State-aware `tools/list` filtering and `tools/call` dispatch
- Auto-detection: pipe → MCP server, terminal → CLI
- E2E integration tests with live codelldb (7-phase dispatch verification)

### Phase 4 📋 — OpenOCD & Hardware Integration
- OpenOCD Tcl RPC client (TCP transport)
- Hardware-level tools: flash, reset, register peek/poke, memory dump
- Combined DAP + OpenOCD workflows (e.g. flash → debug → inspect)
- Multi-target support (simultaneous GDB server + local debug)

### Phase 5 📋 — Advanced Features
- Multi-thread aware debugging (per-thread breakpoints, thread-specific stepping)
- Disassembly view and instruction-level stepping
- Memory watchpoints and data breakpoints
- Expression evaluation with persistent watch expressions
- Debug session replay from trace JSONL

## Running Tests

```bash
# All tests (196 unit + integration, skips gracefully without codelldb)
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
