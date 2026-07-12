# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Status

**Phase 1 complete** — DAP types, codec, client, trace, and verification CLI are implemented. Phase 2 (state machine, variable expansion) and Phase 3 (MCP JSON-RPC, OpenOCD) are planned but not started.

## Commands

```bash
# Build
cargo build --workspace

# Run Phase 1 CLI (basic handshake, no ELF needed)
cargo run -- --codelldb-path /path/to/codelldb

# Run with ELF binary and trace recording
cargo run -- --codelldb-path /path/to/codelldb --elf-path ./app.elf --log-dir ./traces -v

# Tests — all pass without codelldb (integration tests self-skip via env probe)
cargo test --workspace               # everything
cargo test --workspace --lib          # unit tests only (fast)
cargo test -p dap-codec               # single crate
cargo test -p dap-client -- --nocapture  # integration test output

# Lint (must pass before commit)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Architecture

```
teledap (root binary — Phase 1 CLI, eventually MCP server)
  └── dap-client        — spawns codelldb, typed RPC, event streaming
        ├── dap-codec   — tokio Decoder/Encoder for Content-Length framing
        ├── dap-types   — all 103 DAP spec types (serde-tagged enums)
        └── dap-trace   — non-blocking session audit (ring buffer + JSONL)
```

**Crate dependency direction:** `teledap → dap-client → {dap-codec, dap-types, dap-trace}`. `dap-types` is the leaf crate with zero internal dependencies; every other crate depends on it.

## Key Design Patterns

### Wire protocol

DAP uses LSP-style framing over stdio: `Content-Length: <N>\r\n\r\n<JSON body>`. `DapCodec` implements `tokio_util::codec::Decoder<Item=ProtocolMessage>` and handles partial reads, sticky packets, oversized frames (max 4 MiB default), and header size limits (4 KiB).

### ProtocolMessage — serde internal tagging

`ProtocolMessage` is a serde-tagged enum discriminated by the JSON `"type"` field:

```rust
#[serde(tag = "type")]
pub enum ProtocolMessage {
    #[serde(rename = "request")]  Request(Request),
    #[serde(rename = "response")] Response(Response),
    #[serde(rename = "event")]    Event(Event),
}
```

### Request/Response dispatch (oneshot channels)

`DapClient::send_request()` allocates a monotonic `seq`, inserts a `oneshot::Sender` into `pending_requests: HashMap<u64, oneshot::Sender<Response>>` **before** writing to stdin (avoids race). A background `tokio::spawn` task reads stdout via `FramedRead<ChildStdout, DapCodec>`, decodes frames, and routes:
- **Response** → looks up `request_seq` in the pending map, sends through the oneshot
- **Event** → pushes to `mpsc::unbounded_channel`, consumed via `recv_event()`
- **Request** → logged as warning (codelldb doesn't send reverse requests)

### DapRequest trait (typed RPC)

```rust
pub trait DapRequest {
    const COMMAND: &'static str;
    type Arguments: Serialize + Default;
    type Response: DeserializeOwned;
}
```

Each DAP command (initialize, launch, setBreakpoints, etc.) has a unit struct implementing this trait. Callers write `client.send_request::<InitializeRequest>(args).await` and get back a statically-typed `Capabilities`.

### camelCase convention

All DAP JSON uses camelCase. Rust structs use snake_case fields with `#[serde(rename_all = "camelCase")]`. Optional fields use `#[serde(default, skip_serializing_if = "Option::is_none")]`.

### Workspace dependencies

All dependency versions are declared in the root `Cargo.toml` `[workspace.dependencies]` table. Internal crates are referenced via `dap-types.workspace = true` (version) and resolved through `[workspace.dependencies] dap-types = { path = "crates/dap-types" }`. New crates must be added to both `members` and `[workspace.dependencies]`.

## Testing

- **Unit tests** are inline `#[cfg(test)] mod tests` blocks in each source file.
- **Integration tests** live in `crates/dap-client/tests/integration_test.rs`. They spawn a real codelldb process and use a `codelldb_available()` env probe that gracefully skips when codelldb is absent. Every async test is wrapped in `tokio::time::timeout(2s)` to prevent hangs.
- New dap-codec tests go in `crates/dap-codec/src/lib.rs` in the existing `mod tests` block; use the `make_codec()` and `make_wire()` / `make_wire_value()` helpers.
