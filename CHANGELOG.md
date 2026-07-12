# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- MCP protocol layer (`mcp-protocol` crate): JSON-RPC 2.0 types and line-delimited stdio transport for AI client communication
- MCP tool dispatch (`debug-bridge` crate): 24 MCP tool handlers (lifecycle, execution control, breakpoints, introspection, utilities) bridging AI tool calls to DebugSession operations with state-aware gating
- MCP server mode: auto-detection via `is_terminal()`, background DAP event handler, `tools/list` returns state-filtered tools, errors returned as `isError: true` per MCP spec
- Phase 3 test suite: 26 debug-bridge unit tests + 7 integration tests (lifecycle dispatch, state gating, utility tools)

### Fixed

- Race condition in `DapClient`: `start()` created a separate pending-requests map for the background reader task, so responses could never be matched to their waiters. Fixed by sharing `pending_requests` via `Arc<Mutex<HashMap>>` between `start()` and `send_request()`.
- codelldb rejecting initialize request with "Malformed message": `adapterID`/`clientID` fields were serialized as `adapterId`/`clientId` (serde camelCase default) instead of DAP spec's uppercase `ID` suffix. Fixed with explicit `#[serde(rename)]` annotations. `adapterID` is a required field per DAP spec.
- GDB remote debugging completely broken: `customLaunchSetupCommands` field was silently ignored by codelldb (field does not exist). Fixed by using codelldb's actual `processCreateCommands` field with correct `Vec<String>` format instead of `[{"text": "..."}]`.
- CLI event flow deadlock: waited for `initialized` event before sending `launch`, but codelldb sends `initialized` only during `launch` processing. Fixed by sending launch first (fire-and-forget via `send_request_nb`), then awaiting the event.
- `continued` event error when already in Running state: `configurationDone` transitions to Running, then the subsequent `continued` event tried to transition Running→Running (illegal). Fixed by skipping the transition if already Running.
- CLI shutdown error when already Disconnected: `exited` event transitions to Disconnected before shutdown runs. Fixed by checking state before calling shutdown.

### Added

- Integration tests for dap-client: duplicate start rejection (CL-I06), send_request_nb fire-and-forget (CL-I07)
- Integration tests for debug-session: step operations (IT-04), program exit handling (IT-08)
- Unit test for dap-codec: non-numeric Content-Length rejection (DC-17)
- Unit test for dap-types: all 42 DAP request COMMAND constants must be non-empty
- Unit tests for CLI Args parsing (defaults, path, remote, flags)
- MCP E2E test scripts (`test_mcp_e2e.ps1`, `test_mcp_e2e.sh`) for CI/local smoke testing
- `--cli` flag to force CLI mode when stdin is not a terminal (e.g. PowerShell). Mode detection now checks `--cli` before falling back to `is_terminal()`.

### Changed

- Root binary restructured: existing Phase 2 CLI moved to `src/cli.rs`, new `src/server.rs` for MCP loop, `src/main.rs` performs mode auto-detection
- Tracing now writes exclusively to stderr (stdout is the MCP protocol channel)

- Complete DAP type system (`dap-types` crate): all 103 spec types including 42 requests, 17 events, and 36 data types with serde support
- DAP wire-format codec (`dap-codec` crate): tokio Decoder/Encoder for Content-Length framed protocol with sticky-packet handling
- DAP child-process client (`dap-client` crate): codelldb process management with typed request/response via oneshot channels and event streaming via mpsc
- Phase 1 verification CLI: initialize→launch→breakpoint→stopped→inspect→continue→disconnect debug flow
- Debug session trace recorder (`dap-trace` crate): non-blocking audit logging with in-memory ring buffer and JSONL file output, integrated into DapClient for automatic request/response/event tracing
- Phase 1 test suite: 3 new dap-codec edge-case unit tests (invalid JSON, 3-segment split feed, overdeclared Content-Length), 4 dap-client integration tests with real codelldb process (handshake, cleanup, full lifecycle), and GitHub Actions CI pipeline (unit tests, integration tests, clippy + rustfmt)
- Managed debug session (`debug-session` crate): formal 5-state machine (Disconnected→Connected→Initialized→Running↔Halted), operation gating by session state, structured context-chain assembly (Thread→Frame→Scope→Variable tree), recursive variable expansion with depth limiting and paging support, state-change watch channel for future MCP integration
- Phase 2 test suite: 28 unit tests (state transitions, gating rules, variable expansion, context types) and 6 integration tests (full lifecycle, state watcher, operation gating rejection, context chain assembly)
- Refactored CLI binary to use DebugSession API with automatic state tracking
- Path mapping (`mapping.rs`): bidirectional AI relative ↔ system absolute path translation with alias registration, base directory fallback, and longest-match prefix resolution
- Variable handle cache (`cache.rs`): thread-safe name → variablesReference mapping with scoped lookups (frame+scope priority), fuzzy search, and automatic invalidation on state transitions

### Fixed

- Duplicate tracing subscriber initialization causing panic in CLI mode: `main.rs` already sets up the global subscriber before dispatching to `cli::run()`, so the second `.init()` in `cli.rs` was removed

### Documentation

- CLAUDE.md: project architecture guide, key design patterns (wire protocol, oneshot dispatch, DapRequest trait), build/test/lint commands, and testing conventions
- PC 端调试通信测试报告 (`docs/测试报告.md`): 完整记录 4 个 Bug 的根因分析与修复、端到端测试步骤与结果
