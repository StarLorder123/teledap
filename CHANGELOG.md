# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- MCP protocol layer (`mcp-protocol` crate): JSON-RPC 2.0 types and line-delimited stdio transport for AI client communication
- MCP tool dispatch (`debug-bridge` crate): 24 MCP tool handlers (lifecycle, execution control, breakpoints, introspection, utilities) bridging AI tool calls to DebugSession operations with state-aware gating
- MCP server mode: auto-detection via `is_terminal()`, background DAP event handler, `tools/list` returns state-filtered tools, errors returned as `isError: true` per MCP spec
- Phase 3 test suite: 26 debug-bridge unit tests + 7 integration tests (lifecycle dispatch, state gating, utility tools)

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
