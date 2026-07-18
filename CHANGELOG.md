# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

- E2E test scripts enhanced: 7-phase MCP dispatch verification (22 assertions) covering pre-init rejection, state-aware tool listing, error paths, and codelldb lifecycle; bash subshell bug fixed

### Changed

- Generalize debug adapter layer for multi-adapter support: `dap-client` introduces `AdapterKind` (Codelldb/Gdb) and `AdapterConfig`, replacing the hard-coded `start(path)` with `start(&config)`; `debug-session` gains adapter-aware `launch()` and `configuration_done()` dispatch (fire-and-forget for codelldb, blocking for GDB); `debug-bridge` `start` tool accepts `adapterPath`/`adapterKind`/`adapterArgs` with backward-compatible `codelldbPath` alias; CLI adds `--adapter-path`/`--adapter-kind`/`--adapter-args` flags; all error messages and documentation rewritten to be adapter-agnostic

### Added

- GDB DAP mode support (`--adapter-kind gdb --adapter-args=-i --adapter-args=dap`): adapter-aware `launch`/`configuration_done` behavior, GDB remote debugging via `target` field (codelldb keeps `processCreateCommands`), and runtime-derived default `adapterId` ("gdb" vs "lldb")

### Fixed

- Stderr pipe buffer deadlock prevention: `DapClient::start()` now spawns a background task to drain adapter stderr line-by-line, preventing hang with verbose adapters

### Removed

- `docs/fangan.md` — obsolete Chinese-language design document superseded by the implemented architecture

### Documentation

- MCP Inspector 手动测试指南：使用官方 `@modelcontextprotocol/inspector` 在浏览器中交互式测试 TeleDAP，覆盖 codelldb 获取、状态门控体验、14 步完整调试流程、错误路径/路径映射/模糊搜索场景、排障表及两种替代测试方式
- MCP-DAP 协议桥接架构文档：覆盖项目整体架构、crate 依赖关系、MCP/DAP 协议层、桥接转换机制、24 个工具映射、事件流、会话状态机、变量缓存与路径映射、完整数据流示例

### Added

- `openocd-client` crate：OpenOCD 进程管理客户端，支持进程生命周期管理、Tcl 命令通信（`\x1a` 终结符）、stdout/stderr 管道日志文件记录或丢弃、后台管道防死锁读取、串行化命令锁
- 5 个 OpenOCD MCP 工具（全部 utility，无 SessionState 门控）：`openocd_start`（启动服务器，可选日志目录）、`openocd_stop`（关闭进程）、`openocd_status`（查询运行状态和运行时间）、`openocd_output`（从日志文件读取尾部行，支持增量读取）、`openocd_send`（发送 Tcl 命令并等待响应，可配置超时）
- OpenOCD 与 DebugSession 采用组合关系：`server.rs` 通过 `Arc<RwLock<Option<OpenOcdClient>>>` 独立持有，启动时默认为 `None`，仅当 AI 调用 `openocd_start` 时创建，纯 codelldb 会话零影响
- `dap-trace` 中 `TraceSource::OpenOcdTx`/`OpenOcdRx` 变体已预留，OpenOCD 命令/响应可接入追踪系统

### Added

- MCP tool dispatch E2E integration tests (8 new tests in `debug-bridge`): state gating for 11 Halted tools + pause, full lifecycle with debuggee through MCP dispatch, breakpoint + inspect chain (get_threads, get_stack_trace, get_scopes, get_variables, evaluate, assemble_context), step operations, function breakpoints, launch/config_done dispatch, pause dispatch
- Test helpers: `test_debuggee_path()` (multi-candidate path resolution), `wait_for_stopped()`, `wait_for_initialized()` (DAP event loop utilities), `extract_first_thread_id()`

### Fixed

- 工具 schema 字段名与 handler 反序列化不一致：`tools/list` 公布的 21 个参数名为 snake_case（如 `codelldb_path`、`thread_id`、`source_path`），但所有 handler 参数结构体标注 `#[serde(rename_all = "camelCase")]` 实际要求 camelCase（如 `codelldbPath`）——按公布的 schema 调用必然报 `Invalid parameters: missing field`。已将 `tools.rs` 全部 schema 字段名统一为 camelCase，与 handler、集成测试及 CLAUDE.md 约定一致
- `session::launch()` deadlock: codelldb defers the launch response until after `configurationDone`. Changed from blocking `send_request` to fire-and-forget `send_request_nb` (matching the CLI pattern and `configuration_done`).
- `initialized` event idempotency: the DAP `initialized` event may arrive after the `initialize` handshake has already transitioned state to Initialized. Added a guard to skip the transition when already in Initialized (same pattern as Bug #4 `continued` event fix).
- `NoResponseBody` null deserialization: codelldb sends `null` (not `{}`) for `next`, `stepIn`, `stepOut`, and `pause` response bodies. Replaced derived `Deserialize` with a custom impl that accepts any JSON value.

### Added

- CLI `--source-path` and `--breakpoints` arguments: set breakpoints with real source file paths and line numbers instead of `line: 0`/elf_path placeholder
- Variable inspection in CLI `stopped` event handler: fetches and logs local variables with name, value, and type after scopes
- `continue_execution()` after breakpoint inspection in CLI mode: program resumes automatically after each stop, allowing multiple breakpoint hits and clean exit
- Stack backtrace display: 5-6 frame call chain (user function → system CRT) logged per breakpoint hit
- Source path auto-inference: if `--source-path` not provided, looks for `main.c` in the ELF binary's directory

### Fixed

- `ScopesArguments`, `ScopesResponse`, `VariablesResponse` in `dap-types` were missing `#[serde(rename_all = "camelCase")]`, causing `frame_id` to serialize as `frame_id` instead of DAP spec's `frameId`. codelldb rejected scopes requests with "Malformed message".
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
