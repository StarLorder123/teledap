# TeleDAP

MCP (Model Context Protocol) server that bridges AI assistants to embedded hardware debuggers. Speaks JSON-RPC 2.0 over stdin/stdout and translates stateless MCP tool calls into stateful interactions with [CodeLLDB](https://github.com/vadimcn/codelldb) (DAP protocol) and [OpenOCD](https://openocd.org/) (Tcl RPC over TCP).

## Architecture

```
LLM → stdin JSON-RPC → TeleDapServer → SessionCoordinator → DapDriver (CodeLLDB)
                                        ↘                  ↘ OpenOcdDriver (Tcl RPC)
                                         AuditLogger (ring buffer + JSONL)
```

**State machine:** `Disconnected → Initialized → Attached → Halted ↔ Running`

Tool availability is gated by the current session state. Calling a tool in the wrong state returns `ToolUnavailable`.

## Quick Start

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) toolchain (1.70+)
- [CodeLLDB](https://github.com/vadimcn/codelldb/releases) native binary
- [OpenOCD](https://openocd.org/) (optional, for hardware debugging)

### Build

```bash
cargo build --release
```

### Usage

TeleDAP runs as a subprocess of an MCP client (e.g., Claude Desktop). Configure your MCP client to launch:

```json
{
  "mcpServers": {
    "teledap": {
      "command": "/path/to/teledap",
      "args": [
        "--codelldb-path", "/usr/bin/codelldb",
        "--openocd-host", "192.168.1.10",
        "--openocd-tcl-port", "6666",
        "--openocd-gdb-port", "3333",
        "--log-dir", "./logs",
        "-v"
      ]
    }
  }
}
```

### CLI Options

| Option | Default | Description |
|--------|---------|-------------|
| `--codelldb-path` | `codelldb` | Path to CodeLLDB binary |
| `--openocd-host` | `127.0.0.1` | OpenOCD Tcl RPC host |
| `--openocd-tcl-port` | `6666` | OpenOCD Tcl RPC port |
| `--openocd-gdb-port` | `3333` | OpenOCD GDB server port |
| `--log-dir` | `./logs` | Audit log output directory |
| `-v`, `--verbose` | off | Enable debug logging |

## MCP Tools (17)

### Connection
- `auto_launch` — connect OpenOCD, launch CodeLLDB, attach, halt
- `connect_openocd` — establish OpenOCD TCP connection

### Hardware (via OpenOCD)
- `flash_write` / `flash_read` / `flash_erase` — flash memory operations
- `read_register` / `write_register` — CPU register access
- `read_memory` / `write_memory` — memory read/write
- `reset_halt` — reset target and halt

### Debug Control (via CodeLLDB/DAP)
- `set_breakpoint` / `remove_breakpoint` / `list_breakpoints`
- `step_over` / `step_into` / `step_out`
- `get_variables` / `get_stack_trace`
- `evaluate` — evaluate expressions in current frame
- `continue_execution` / `halt`

## Running Tests

```bash
cargo test                          # all 32 unit tests
cargo test -p teledap -- frame_decoder  # specific test
cargo test -- --nocapture           # show output
```

## License

MIT — see [LICENSE](LICENSE) for details.
