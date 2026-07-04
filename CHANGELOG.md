# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- Complete DAP type system (`dap-types` crate): all 103 spec types including 42 requests, 17 events, and 36 data types with serde support
- DAP wire-format codec (`dap-codec` crate): tokio Decoder/Encoder for Content-Length framed protocol with sticky-packet handling
- DAP child-process client (`dap-client` crate): codelldb process management with typed request/response via oneshot channels and event streaming via mpsc
- Phase 1 verification CLI: initialize→launch→breakpoint→stopped→inspect→continue→disconnect debug flow
- Debug session trace recorder (`dap-trace` crate): non-blocking audit logging with in-memory ring buffer and JSONL file output, integrated into DapClient for automatic request/response/event tracing
- Phase 1 test suite: 3 new dap-codec edge-case unit tests (invalid JSON, 3-segment split feed, overdeclared Content-Length), 4 dap-client integration tests with real codelldb process (handshake, cleanup, full lifecycle), and GitHub Actions CI pipeline (unit tests, integration tests, clippy + rustfmt)
