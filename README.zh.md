# TeleDAP

[English](README.md) · [中文](README.zh.md)

MCP（Model Context Protocol）服务器，用于将 AI 助手桥接到嵌入式硬件调试器。支持通过 stdin/stdout（stdio MCP）或 HTTP/SSE 传输 JSON-RPC 2.0，并将无状态的 MCP 工具调用转换为与 [CodeLLDB](https://github.com/vadimcn/codelldb) / GDB（DAP 协议）和 [OpenOCD](https://openocd.org/)（TCP 上的 Tcl RPC）的有状态交互。

> **状态：第 1–3 阶段已完成。** DAP 协议栈、带上下文链组装的会话状态机、30 工具 MCP 服务器（stdio + HTTP/SSE）以及 OpenOCD 管理工具均已实现。二进制文件自动检测执行模式：`--http` → HTTP/SSE MCP 服务器，pipe 标准输入 → stdio MCP 服务器，终端标准输入 → 验证 CLI。

## 架构

```
teledap（自动检测：--http→HTTP/SSE，pipe→stdio MCP，terminal→CLI）
  ├── debug-bridge       — 30 个 MCP 工具、状态感知分发、处理器路由
  │   ├── handlers       — lifecycle、execution、breakpoint、inspect、openocd
  │   └── tools.rs       — 工具定义与输入模式
  ├── mcp-protocol       — JSON-RPC 2.0 类型 + 行分隔 stdio 传输
  ├── debug-session      — 状态机、上下文链、变量展开、路径映射
  │   ├── dap-client     — codelldb/GDB 进程生命周期、类型化 RPC、事件流
  │   │   ├── dap-codec  — Content-Length 帧协议（tokio Decoder/Encoder）
  │   │   └── dap-types  — 103 个 DAP 规范类型，支持 serde
  │   └── dap-trace      — 非阻塞会话审计（环形缓冲区 + JSONL）
  └── openocd-client     — OpenOCD Tcl RPC 客户端（TCP 传输）
```

### Crate 一览

| Crate | 说明 |
|-------|------|
| `dap-types` | 全部 103 个 DAP 规范类型：42 个请求、17 个事件、36 个数据类型 |
| `dap-codec` | Tokio codec，处理 `Content-Length: N\r\n\r\n<JSON>` 帧格式 |
| `dap-client` | 异步调试适配器进程管理器，支持类型化 RPC 和事件流 |
| `dap-trace` | 非阻塞调试会话记录器，使用环形缓冲区和 JSONL 输出 |
| `debug-session` | 状态机（5 个状态）、上下文链组装、C++ 变量展开、路径映射、变量句柄缓存 |
| `mcp-protocol` | JSON-RPC 2.0 类型和行分隔 stdin/stdout 传输 |
| `openocd-client` | OpenOCD Tcl RPC 客户端：启动、停止、发送命令、读取输出 |
| `debug-bridge` | 通过 `ToolRegistry` 实现 30 个 MCP 工具的状态感知分发 |
| `teledap`（根 crate）| 二进制文件：MCP 服务器（stdio/HTTP/SSE）或验证 CLI（终端）— 自动检测 |

## 快速开始

### 前置要求

- [Rust](https://www.rust-lang.org/tools/install) 稳定工具链
- [CodeLLDB](https://github.com/vadimcn/codelldb/releases) 原生二进制文件（用于集成测试和运行时）
-（可选）[OpenOCD](https://openocd.org/)，用于嵌入式硬件调试

### 构建

```bash
cargo build --release
```

## MCP Server 用法

TeleDAP 支持两种 MCP 传输层：

| 模式 | 触发方式 | 适用场景 |
|------|---------|---------|
| **stdio** | 通过 pipe 由 AI 客户端启动（如 Claude Desktop、Claude Code） | 最常用；单一 AI 客户端 |
| **HTTP/SSE** | `cargo run --release -- --http --port 8080` | 自建 Agent、多客户端共享会话、Web UI |

### stdio 模式

当通过 pipe 由 AI 客户端启动时，TeleDAP 自动检测 MCP 模式，并通过 stdin/stdout 使用 JSON-RPC 2.0：

```json
// → initialize
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
// ← capabilities
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"teleDAP","version":"0.1.0"}}}

// → 列出当前状态下可用工具
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}

// → 启动 codelldb
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"start","arguments":{"adapterPath":"/usr/bin/codelldb"}}}

// → 完整调试生命周期：initialize → launch → set_breakpoints → configuration_done → …
```

### HTTP/SSE 模式

```bash
cargo run --release -- --http --port 8080
```

1. 打开 SSE 流：
   ```bash
   curl -N http://localhost:8080/sse
   # event: endpoint
   # data: /message?session_id=550e8400-e29b-41d4-a716-446655440000
   ```
2. 通过 POST 发送 JSON-RPC：
   ```bash
   curl -X POST "http://localhost:8080/message?session_id=..." \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
   # → 202 Accepted
   ```
3. 通过 SSE 流异步接收响应。

### MCP 工具

**30 个 MCP 工具**（21 个状态门控 + 9 个始终可用的工具）：

| 分类 | 工具 |
|------|------|
| 生命周期 | `start`、`initialize`、`launch`、`attach`、`configuration_done`、`shutdown` |
| 执行控制 | `continue`、`step_over`、`step_in`、`step_out`、`pause` |
| 断点 | `set_breakpoints`、`set_function_breakpoints`、`list_breakpoints` |
| 内省 | `get_threads`、`get_stack_trace`、`get_scopes`、`get_variables`、`evaluate`、`set_variable`、`assemble_context`、`search_variables` |
| 实用工具 | `get_state`、`register_path_alias`、`register_base_dir` |
| OpenOCD | `openocd_start`、`openocd_stop`、`openocd_status`、`openocd_output`、`openocd_send` |

工具按会话状态门控——例如 `continue` 只在 `Halted` 时出现在 `tools/list` 中；`pause` 只在 `Running` 时出现。

## 验证 CLI

从终端运行时，TeleDAP 会执行完整的调试会话，包括断点、变量检查和堆栈回溯：

```bash
# 基础握手（不需要 ELF）
cargo run -- --adapter-path /usr/bin/codelldb

# 带断点和变量检查的完整调试会话
cargo run -- --adapter-path /usr/bin/codelldb --elf-path ./target/debug/my_app

# 自定义源文件和断点
cargo run -- --adapter-path /usr/bin/codelldb --elf-path ./app.elf \
    --source-path ./src/main.c --breakpoints "10,15,22"

# 通过 GDB 服务器远程调试
cargo run -- --adapter-path /usr/bin/codelldb --elf-path ./firmware.elf \
    --gdb-remote 192.168.1.10:3333

# 启用调试跟踪记录
cargo run -- --adapter-path /usr/bin/codelldb --elf-path ./app.elf --log-dir ./traces -v

# 强制 CLI 模式（即使通过 pipe 启动）
cargo run -- --cli --adapter-path /usr/bin/codelldb
```

### CLI 选项

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `--adapter-path` | `codelldb` | 调试适配器二进制路径（codelldb 或 gdb） |
| `--adapter-kind` | `codelldb` | 适配器类型：`codelldb` 或 `gdb` |
| `--adapter-args` | *(无)* | 适配器二进制命令行参数（可重复指定） |
| `--liblldb-path` | *(无)* | liblldb 共享库路径（Windows 为 `liblldb.dll`） |
| `--elf-path` | *(空)* | 待调试 ELF 二进制路径 |
| `--source-path` | *(推断)* | 设置断点所用的源文件路径 |
| `--breakpoints` | `9,13,4` | 逗号分隔的断点行号 |
| `--gdb-remote` | *(无)* | 远程 GDB 服务器地址（`host:port`） |
| `--log-dir` | *(无)* | 调试跟踪 JSONL 输出目录 |
| `--http` | `false` | 以 HTTP/SSE MCP 服务器运行 |
| `--port` | `8080` | HTTP/SSE MCP 服务器端口 |
| `--cli` | `false` | 强制 CLI 模式（跳过 stdin 终端检测） |
| `-v`, `--verbose` | 关 | 启用详细/调试日志 |

## 会话状态机

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

每个工具调用都会验证当前状态；在错误状态下调用工具会返回描述性错误，并说明所需状态。

## 核心特性

### 上下文链组装
`assemble_context` 构建完整快照：threads → frames → scopes → variables，递归展开到可配置深度。结果是一个适合 AI 消费的嵌套 JSON 结构。

### C++ 变量展开
`get_variables` 支持指针、结构体和数组的递归展开，具有深度限制和分页功能。线程安全的变量句柄缓存维护 name → handle 映射，并在状态转换时自动失效。`search_variables` 提供跨缓存模糊名称查找。

### 路径映射
`register_path_alias` 和 `register_base_dir` 允许 AI 客户端使用短相对路径（例如 `src/main.cpp`），而调试器将其解析为绝对系统路径。

### 操作门控
`ToolAvailability` 将 21 个操作映射到其合法的会话状态。MCP 服务器会过滤 `tools/list` 响应，仅显示当前状态下可用的工具，从而在无效操作到达调试器之前阻止它们。

### 双传输、三模式二进制
同一个二进制文件承担三种角色：
- **`--http`** → HTTP/SSE MCP 服务器（多客户端共享会话）
- **stdin 是 pipe** → stdio MCP 服务器（JSON-RPC 2.0，跟踪输出到 stderr）
- **stdin 是终端** → 第 2 阶段验证 CLI（交互式调试会话）
- `--cli` 标志可强制 CLI 模式，无论 stdin 类型如何

### OpenOCD 集成
`openocd_start` / `openocd_stop` / `openocd_send` / `openocd_output` 管理 OpenOCD GDB 服务器生命周期，并暴露原始 Tcl 命令用于 flash、reset、寄存器检查和内存转储。

## AI Agent 集成

有关配置和现成系统提示，请参阅集成指南：

- [`docs/AI_Agent_集成指南.md`](docs/AI_Agent_集成指南.md) — 中文指南
- [`docs/AI_Agent_Integration_Guide.md`](docs/AI_Agent_Integration_Guide.md) — English guide
- [`docs/prompts/`](docs/prompts/) — Claude Desktop、stdio 和 HTTP/SSE Agent 的系统提示示例

## 路线图

### 第 1 阶段 ✅ — DAP 协议基础
- 103 个 DAP 规范类型，支持 serde
- 线格式编解码器（Content-Length 帧、粘包处理）
- 子进程客户端（codelldb 生命周期、类型化 RPC、事件流）
- 调试会话跟踪记录器（环形缓冲区 + JSONL）
- 带基本调试生命周期的验证 CLI
- 单元 + 集成测试套件

### 第 2 阶段 ✅ — 状态机与调试桥
- 会话状态机：`Disconnected → Connected → Initialized → Running ↔ Halted`
- 上下文链组装（threads、frames、scopes、variables — 嵌套展开）
- C++ 变量展开，支持深度限制、分页和句柄缓存
- 状态门控工具可用性（21 个操作 × 5 个状态）
- 路径映射：AI 相对路径 ↔ 系统绝对路径双向转换
- 增强 CLI：真实断点、变量检查、堆栈回溯

### 第 3 阶段 ✅ — MCP 集成
- 通过 stdin/stdout 的 JSON-RPC 2.0 行分隔传输
- 30 个 MCP 工具：6 个生命周期、5 个执行、3 个断点、8 个内省、4 个实用、5 个 OpenOCD
- 状态感知 `tools/list` 过滤和 `tools/call` 分发
- HTTP/SSE MCP 服务器，支持多客户端共享会话
- OpenOCD 管理工具（`openocd_start`、`openocd_stop`、`openocd_send` 等）
- 自动检测：`--http` → HTTP/SSE，pipe → stdio MCP，terminal → CLI
- 使用真实 codelldb 的端到端集成测试（7 阶段分发验证）

### 第 4 阶段 📋 — 高级硬件集成
- 组合 DAP + OpenOCD 工作流（例如 flash → debug → inspect）
- 寄存器 peek/poke 和内存转储工具
- 多目标支持（同时 GDB 服务器 + 本地调试）

### 第 5 阶段 📋 — 高级特性
- 多线程感知调试（每线程断点、线程特定单步）
- 反汇编视图和指令级单步
- 内存观察点和数据断点
- 持久监视表达式的表达式求值
- 从跟踪 JSONL 回放调试会话

## 运行测试

```bash
# 全部测试（单元 + 集成，没有 codelldb 时自动跳过）
cargo test --workspace

# 仅单元测试（快速，无需 codelldb）
cargo test --workspace --lib

# 指定 crate
cargo test -p dap-codec                         # codec 单元测试
cargo test -p dap-client                        # 客户端单元 + 集成测试
cargo test -p debug-bridge                      # bridge 单元测试 + MCP 集成测试
cargo test -p debug-session                     # 会话单元 + 集成测试

# 显示输出（对集成测试诊断有用）
cargo test -- --nocapture

# 代码检查
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 许可证

MIT — 详见 [LICENSE](LICENSE)。
