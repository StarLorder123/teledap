# TeleDAP

MCP (Model Context Protocol) server that bridges AI assistants to embedded hardware debuggers. Speaks JSON-RPC 2.0 over stdin/stdout and translates stateless MCP tool calls into stateful interactions with [CodeLLDB](https://github.com/vadimcn/codelldb) (DAP protocol) and [OpenOCD](https://openocd.org/) (Tcl RPC over TCP).

> **Status: Phase 1 complete.** DAP protocol types, wire-format codec, child-process client, and debug session tracing are implemented. The Phase 1 verification CLI demonstrates a full debug lifecycle. MCP integration is planned for Phase 3.

## Architecture

```
teledap (Phase 1 CLI → Phase 3 MCP server)
  ├── dap-client    — codelldb process lifecycle, typed request/response, event streaming
  │   ├── dap-codec — Content-Length framed protocol (tokio Decoder/Encoder)
  │   ├── dap-types — 103 DAP spec types with serde support
  │   └── dap-trace — non-blocking session audit (ring buffer + JSONL)
  ├── [Phase 2]     — state machine, context-chain assembly, C++ variable expansion
  └── [Phase 3]     — MCP JSON-RPC protocol, tool routing
```

### Crate Map

| Crate | Description |
|-------|-------------|
| `dap-types` | All 103 DAP specification types: 42 requests, 17 events, 36 data types |
| `dap-codec` | Tokio codec for `Content-Length: N\r\n\r\n<JSON>` wire framing |
| `dap-client` | Async codelldb process manager with typed RPC and event streaming |
| `dap-trace` | Non-blocking debug session recorder with ring buffer and JSONL output |
| `teledap` (root) | Phase 1 verification CLI demonstrating the full debug lifecycle |

## Quick Start

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) stable toolchain
- [CodeLLDB](https://github.com/vadimcn/codelldb/releases) native binary (for integration tests and runtime)

### Build

```bash
cargo build --release
```

### Usage (Phase 1 CLI)

The Phase 1 CLI verifies the DAP protocol stack by running a complete debug session:

```bash
# Basic handshake (no ELF needed)
cargo run -- --codelldb-path /usr/bin/codelldb

# Full debug session with an ELF binary
cargo run -- --codelldb-path /usr/bin/codelldb --elf-path ./target/debug/my_app

# Remote debugging via GDB server
cargo run -- --codelldb-path /usr/bin/codelldb --elf-path ./firmware.elf --gdb-remote 192.168.1.10:3333

# With debug trace recording
cargo run -- --codelldb-path /usr/bin/codelldb --elf-path ./app.elf --log-dir ./traces -v
```

### CLI Options

| Option | Default | Description |
|--------|---------|-------------|
| `--codelldb-path` | `codelldb` | Path to CodeLLDB binary |
| `--elf-path` | *(empty)* | Path to ELF binary to debug |
| `--gdb-remote` | *(none)* | Remote GDB server address (`host:port`) |
| `--log-dir` | *(none)* | Directory for debug trace JSONL output |
| `-v`, `--verbose` | off | Enable verbose/debug logging |

## Roadmap

### Phase 1 ✅ — DAP Protocol Foundation
- 103 DAP specification types with serde support
- Wire-format codec (Content-Length framing, sticky-packet handling)
- Child-process client (codelldb lifecycle, typed RPC, event streaming)
- Debug session trace recorder
- Verification CLI with full debug lifecycle
- Unit + integration test suite with CI pipeline

### Phase 2 🚧 — State Machine & Debug Bridge
- Session state machine: `Disconnected → Initialized → Attached → Halted ↔ Running`
- Context-chain assembly (variable scopes, handles, C++ variable expansion)
- Tool availability gated by session state

### Phase 3 📋 — MCP Integration
- JSON-RPC 2.0 over stdin/stdout
- 17 MCP tools: connection, hardware (OpenOCD), debug control (CodeLLDB/DAP)
- Path mapping and multi-thread support

## Running Tests

```bash
# All tests (76 unit + integration, skips gracefully without codelldb)
cargo test --workspace

# Unit tests only (fast, no codelldb needed)
cargo test --workspace --lib

# Specific crate
cargo test -p dap-codec                     # codec unit tests (17)
cargo test -p dap-client                    # client unit + integration tests (5)

# Show output (useful for integration test diagnostics)
cargo test -- --nocapture

# Lint
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## License

MIT — see [LICENSE](LICENSE) for details.
