# MCP-DAP 协议桥接架构文档

> TeleDAP 是一个 MCP (Model Context Protocol) 服务器，将 AI 助手的调试请求桥接到 CodeLLDB 调试器（使用 DAP — Debug Adapter Protocol）。

---

## 目录

1. [项目整体架构](#1-项目整体架构)
2. [Crate 依赖关系](#2-crate-依赖关系)
3. [MCP 协议层](#3-mcp-协议层)
4. [DAP 协议层](#4-dap-协议层)
5. [桥接层 — MCP 与 DAP 的转换](#5-桥接层--mcp-与-dap-的转换)
6. [工具定义与 DAP 命令映射](#6-工具定义与-dap-命令映射)
7. [事件流 — DAP 到 MCP](#7-事件流--dap-到-mcp)
8. [会话状态机](#8-会话状态机)
9. [辅助机制](#9-辅助机制)
10. [完整数据流示例](#10-完整数据流示例)

---

## 1. 项目整体架构

```
┌─────────────────────────────────────────────────────────┐
│                    AI 客户端 (Claude Desktop)              │
│              MCP JSON-RPC over stdin/stdout               │
│                   (行分隔 JSON)                            │
└──────────────────────┬──────────────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────────────┐
│               teledap (根 binary crate)                   │
│  ┌──────────────────────────────────────────────────┐   │
│  │              server.rs — MCP 事件循环              │   │
│  │  • 接收 MCP 请求 (initialize/tools/list/tools/call)│   │
│  │  • 分发到 ToolRegistry                            │   │
│  │  • 返回 JSON-RPC 响应                             │   │
│  └──────────────────┬───────────────────────────────┘   │
│                     │                                    │
│  ┌──────────────────▼───────────────────────────────┐   │
│  │         debug-bridge — MCP→DAP 桥接层             │   │
│  │  • ToolRegistry: 工具注册、状态门控、分派          │   │
│  │  • tools.rs: 24 个 MCP 工具定义                    │   │
│  │  • handlers/: 生命周期/执行/断点/检查 处理器       │   │
│  └──────────────────┬───────────────────────────────┘   │
│                     │                                    │
│  ┌──────────────────▼───────────────────────────────┐   │
│  │         debug-session — 会话管理                   │   │
│  │  • SessionState 状态机                             │   │
│  │  • VariableHandleCache 变量缓存                    │   │
│  │  • PathMapper 路径映射                             │   │
│  │  • ContextChain 上下文链                           │   │
│  └──────────────────┬───────────────────────────────┘   │
│                     │                                    │
│  ┌──────────────────▼───────────────────────────────┐   │
│  │           dap-client — DAP 客户端                  │   │
│  │  • codelldb 进程生命周期管理                        │   │
│  │  • 类型化 RPC (DapRequest trait)                   │   │
│  │  • oneshot 通道请求/响应分发                        │   │
│  │  • mpsc 通道事件流                                 │   │
│  └──────────────────┬───────────────────────────────┘   │
│                     │                                    │
│  ┌──────────────────▼───────────────────────────────┐   │
│  │    dap-codec — Content-Length 帧编解码             │   │
│  │    dap-types — 103 个 DAP 协议类型                  │   │
│  │    dap-trace — 环形缓冲区 + JSONL 会话审计         │   │
│  └──────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
                       │
                       │ Content-Length 帧 + DAP JSON
                       ▼
┌─────────────────────────────────────────────────────────┐
│                codelldb (子进程)                          │
│              实际调试器后端                               │
└─────────────────────────────────────────────────────────┘
```

---

## 2. Crate 依赖关系

```
teledap (根 binary)
  ├── debug-bridge         ← MCP→DAP 转换核心
  │   ├── mcp-protocol     ← JSON-RPC 2.0 类型 + stdio 传输
  │   ├── debug-session    ← 状态机 + 变量缓存 + 路径映射
  │   │   ├── dap-client   ← codelldb 进程 + 类型化 RPC
  │   │   │   ├── dap-codec  ← Content-Length 帧编解码
  │   │   │   ├── dap-types  ← 103 个 DAP 规范类型
  │   │   │   └── dap-trace  ← 环形缓冲区追踪
  │   │   └── dap-trace
  │   └── dap-types
  ├── mcp-protocol
  └── (直接依赖) dap-client, dap-trace, debug-session
```

| Crate | 职责 |
|-------|------|
| `dap-types` | 零依赖叶子 crate，定义全部 103 个 DAP 协议类型（serde 标记枚举） |
| `dap-codec` | 实现 `tokio_util::codec::Decoder/Encoder`，处理 `Content-Length: <N>\r\n\r\n<JSON>` 帧 |
| `dap-client` | codelldb 子进程管理，类型化 RPC（`DapRequest` trait），oneshot 请求/响应，mpsc 事件流 |
| `dap-trace` | 非阻塞会话审计：环形缓冲区（内存）+ JSONL 文件持久化 |
| `debug-session` | 会话状态机、变量句柄缓存、路径映射器、上下文链 |
| `debug-bridge` | **核心桥接层**：工具注册、状态门控、MCP 工具→DAP 命令分派 |
| `mcp-protocol` | JSON-RPC 2.0 消息类型、`McpServer` stdio 传输、错误类型 |
| `teledap` | 根 binary：CLI 入口、服务器事件循环、模式自动检测 |

---

## 3. MCP 协议层

### 3.1 传输机制

MCP 层使用 **行分隔 JSON** over stdin/stdout（与 DAP 的 Content-Length 帧不同）。

```rust
// mcp-protocol/src/transport.rs
pub struct McpServer {
    reader: BufReader<Stdin>,
    writer: Stdout,
}
```

每条消息 = 一行有效 JSON，以 `\n` 结尾。`McpServer::parse_incoming()` 将 JSON 行解析为 `IncomingMessage` 枚举。

### 3.2 消息类型

```rust
// mcp-protocol/src/types.rs
pub enum IncomingMessage {
    Request {
        id: u64,         // 数字 ID → Request
        method: String,  // "initialize" | "tools/list" | "tools/call" | ...
        params: Option<Value>,
    },
    Notification {
        method: String,  // "initialized" | ...
        params: Option<Value>,
    },
}
```

判别规则：
- 存在数字 `id` 字段 → `Request`
- 无 `id` 或 `id: null` → `Notification`

### 3.3 服务器事件循环

```rust
// src/server.rs — run() 函数
```

1. **启动阶段**：创建 `TraceHandle`、`DapClient`，包装为 `DebugSession`（Arc 共享）
2. **后台事件消费**：spawn tokio 任务持续读取 DAP 事件，喂给 `session.handle_event()` 更新状态机
3. **MCP 主循环**：`while let Some(msg) = server.next_message().await`，按方法名分发：
   - `"initialize"` → 返回协议版本 `"2025-11-25"`、服务器能力、服务器信息
   - `"tools/list"` → `ToolRegistry::list_tools_for_state(state)` 仅返回当前状态可用的工具
   - `"tools/call"` → 提取 `name` + `arguments`，调用 `ToolRegistry::dispatch()`
   - 未知方法 → JSON-RPC 错误 `METHOD_NOT_FOUND`
4. **关闭**：断开 codelldb 连接

---

## 4. DAP 协议层

### 4.1 进程管理

```rust
// dap-client/src/client.rs
impl DapClient {
    pub async fn start(codelldb_path: &Path) -> Result<Self, DapClientError> {
        // 1. 启动 codelldb 子进程，stdin/stdout/stderr 管道化
        // 2. 创建 FramedRead<Stdout, DapCodec> 帧读取器
        // 3. spawn 后台 tokio 任务读取 stdout
    }
}
```

### 4.2 请求/响应分发（oneshot 通道）

```rust
pub async fn send_request<R: DapRequest>(
    &self,
    arguments: R::Arguments,
) -> Result<R::Response, DapClientError> {
    let seq = self.next_seq();  // 单调递增 AtomicU64
    let (tx, rx) = oneshot::channel();
    // 先插入 pending_requests（避免竞态），再写入 stdin
    self.pending_requests.lock().insert(seq, tx);
    // 编码为 Content-Length 帧，写入 stdin
    // 等待 oneshot channel
    let response = rx.await?;
    // 反序列化 response.body 为类型化返回值
}
```

后台读取器任务路由规则：
- **Response**（`type: "response"`）→ 按 `request_seq` 查找 `pending_requests`，通过 oneshot 发送
- **Event**（`type: "event"`）→ 推入 `mpsc::unbounded_channel`，通过 `recv_event()` 消费
- **Request**（`type: "request"`）→ 记录警告日志（codelldb 不发送反向请求）

### 4.3 类型化 RPC

```rust
// dap-types/src/requests.rs
pub trait DapRequest {
    const COMMAND: &'static str;
    type Arguments: Serialize + Default;
    type Response: DeserializeOwned;
}

// 示例：Continue 命令
pub struct ContinueRequest;
impl DapRequest for ContinueRequest {
    const COMMAND: &'static str = "continue";
    type Arguments = ContinueArguments;
    type Response = ContinueResponse;
}
```

调用方式：`client.send_request::<ContinueRequest>(args).await` → 返回静态类型 `ContinueResponse`。

### 4.4 线格式（DapCodec）

实现 `tokio_util::codec::Decoder<Item=ProtocolMessage>`：
- 解析 `Content-Length: <N>\r\n\r\n` 头
- 读取恰好 N 字节的 JSON 体
- 处理：部分读取（返回 `None`）、粘包、超大帧拒绝（默认 4 MiB）、头大小限制（4 KiB）

---

## 5. 桥接层 — MCP 与 DAP 的转换

### 5.1 架构概览

```
MCP tools/call {"name":"set_breakpoints","arguments":{...}}
    │
    ▼
ToolRegistry::dispatch(name, session, params, trace)
    │
    ├── 1. 状态门控：检查当前状态是否允许该操作
    ├── 2. 追踪记录：TraceEntry { source: McpTrigger }
    ├── 3. 路由到处理器：
    │       handlers::breakpoint::handle_set_breakpoints(params, session)
    │           ├── 反序列化 MCP 参数
    │           ├── 路径映射（AI 相对路径 → 系统绝对路径）
    │           ├── 构造 DAP SetBreakpointsArguments
    │           └── session.set_breakpoints(args)
    │                   └── client.send_request::<SetBreakpointsRequest>(args)
    │                           └── Content-Length 帧 → codelldb stdin
    │
    └── 4. 结果转换：DAP Response → CallToolResult
            ├── 成功：CallToolResult::success_json(value)
            └── 失败：BridgeError → CallToolResult::error(msg) (is_error: true)
```

### 5.2 核心组件

#### ToolRegistry（`debug-bridge/src/registry.rs`）

```rust
impl ToolRegistry {
    /// 列出当前状态可用的工具
    pub fn list_tools_for_state(state: SessionState) -> Vec<Tool>;

    /// 分派工具调用
    pub async fn dispatch(
        name: &str,
        session: &DebugSession,
        params: Option<Value>,
        trace: Option<&TraceHandle>,
    ) -> Result<CallToolResult, BridgeError>;
}
```

#### 工具定义（`debug-bridge/src/tools.rs`）

`all_tools()` 函数返回全部 24 个 `Tool`，每个包含：
- `name`：工具名称
- `title`：显示标题
- `description`：功能描述
- `input_schema`：JSON Schema 格式的输入参数定义

`tool_operation()` 函数将工具名映射到 `ToolAvailability` 操作字符串，用于状态门控。

#### 处理器模块（`debug-bridge/src/handlers/`）

```
handlers/
├── mod.rs          — 模块声明
├── lifecycle.rs    — 6 个生命周期处理器 + 3 个辅助处理器
├── execution.rs    — 5 个执行控制处理器
├── breakpoint.rs   — 2 个断点处理器
└── inspect.rs      — 8 个检查处理器
```

### 5.3 处理器模式

每个处理器函数遵循统一模式：

```rust
pub async fn handle_xxx(
    params: Option<Value>,
    session: &DebugSession,
) -> Result<CallToolResult, BridgeError> {
    // 1. 参数反序列化
    let params: XxxParams = serde_json::from_value(
        params.ok_or(BridgeError::MissingParams)?
    )?;

    // 2. 调用 DebugSession 方法（内部包含状态检查 + DAP 调用）
    let result = session.xxx(params).await?;

    // 3. 转换为 MCP 结果
    Ok(CallToolResult::success_json(&result))
}
```

---

## 6. 工具定义与 DAP 命令映射

### 6.1 完整映射表

共 **24 个工具**（20 个有状态门控 + 4 个无门控辅助工具）：

| MCP 工具 | DAP 命令 | 状态要求 | 处理器 |
|----------|----------|----------|--------|
| **生命周期** | | | |
| `start` | (启动 codelldb 进程) | Disconnected | `lifecycle::handle_start` |
| `initialize` | `initialize` | Connected | `lifecycle::handle_initialize` |
| `launch` | `launch` + 后台等待 stopped | Initialized | `lifecycle::handle_launch` |
| `attach` | `attach` | Initialized | `lifecycle::handle_attach` |
| `configuration_done` | `configurationDone` | Initialized | `lifecycle::handle_configuration_done` |
| `shutdown` | `disconnect` + 杀进程 | 非 Disconnected | `lifecycle::handle_shutdown` |
| **执行控制** | | | |
| `continue` | `continue` | Halted | `execution::handle_continue` |
| `step_over` | `next` | Halted | `execution::handle_step_over` |
| `step_in` | `stepIn` | Halted | `execution::handle_step_in` |
| `step_out` | `stepOut` | Halted | `execution::handle_step_out` |
| `pause` | `pause` | Running | `execution::handle_pause` |
| **断点** | | | |
| `set_breakpoints` | `setBreakpoints` | Initialized/Running/Halted | `breakpoint::handle_set_breakpoints` |
| `set_function_breakpoints` | `setFunctionBreakpoints` | Initialized/Running/Halted | `breakpoint::handle_set_function_breakpoints` |
| **检查（仅 Halted 状态）** | | | |
| `get_threads` | `threads` | Halted | `inspect::handle_get_threads` |
| `get_stack_trace` | `stackTrace` | Halted | `inspect::handle_get_stack_trace` |
| `get_scopes` | `scopes` | Halted | `inspect::handle_get_scopes` |
| `get_variables` | `variables` | Halted | `inspect::handle_get_variables` |
| `evaluate` | `evaluate` | Halted | `inspect::handle_evaluate` |
| `set_variable` | `setVariable` | Halted | `inspect::handle_set_variable` |
| `assemble_context` | `threads`→`stackTrace`→`scopes`→`variables` (链式) | Halted | `inspect::handle_assemble_context` |
| **辅助（无状态门控）** | | | |
| `get_state` | (无 DAP — 读取会话状态) | 无限制 | `lifecycle::handle_get_state` |
| `register_path_alias` | (无 DAP — 更新路径映射) | 无限制 | `lifecycle::handle_register_path_alias` |
| `register_base_dir` | (无 DAP — 更新路径映射) | 无限制 | `lifecycle::handle_register_base_dir` |
| `search_variables` | (查询本地缓存) | 无限制 | `inspect::handle_search_variables` |

### 6.2 工具输入示例

**`set_breakpoints` 输入 Schema**：
```json
{
  "type": "object",
  "properties": {
    "source_path": {
      "type": "string",
      "description": "Path to the source file"
    },
    "breakpoints": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "line": { "type": "integer" },
          "condition": { "type": "string" },
          "log_message": { "type": "string" }
        }
      }
    }
  },
  "required": ["source_path", "breakpoints"]
}
```

### 6.3 `assemble_context` 的特殊性

`assemble_context` 是唯一一个**链式调用多个 DAP 命令**的工具：

```
assemble_context
  ├── threads → 获取所有线程
  ├── 对每个线程: stackTrace → 获取调用栈
  ├── 对每个栈帧: scopes → 获取作用域
  └── 对每个作用域: variables (可选递归展开, max_depth 控制)
```

输出为嵌套 JSON 结构，包含了程序在停止时的完整调试上下文快照。

---

## 7. 事件流 — DAP 到 MCP

### 7.1 当前架构：轮询模式

**重要：当前架构采用 AI 客户端驱动的轮询模式，而非 DAP 事件的实时 MCP 推送。**

```
codelldb stdout
  │  DAP Events (stopped, continued, output, terminated, ...)
  ▼
后台 stdout 读取任务
  │  DapCodec 解码 Content-Length 帧
  │  ProtocolMessage::Event → mpsc 通道
  ▼
后台事件消费任务 (spawn 于 server.rs::run())
  │  session.client().recv_event().await
  ▼
  debug_session.handle_event(&event)
  │
  ├── "initialized" → 状态 → Initialized
  ├── "stopped" → 状态 → Halted, 记录停止原因/线程ID/断点ID
  ├── "continued" → 状态 → Running, 清除停止信息
  ├── "terminated" / "exited" → 状态 → Disconnected
  ├── "thread" → 跟踪线程启停 (无状态变化)
  ├── "output" → 日志记录
  └── 其他事件 → 透传
  │
  ▼
状态变化触发:
  ├── 变量缓存失效 (Halted→Running/Disconnected 时清空)
  ├── 追踪记录 (trace 启用时)
  └── watch::channel 广播状态变化
```

### 7.2 AI 如何感知状态变化

```
AI 客户端轮询:
  1. 调用 tools/list → 返回当前状态可用的工具列表
     └── 工具列表变化隐式告知当前状态
         (如 halted 时出现 continue/step_over/get_variables)

  2. 调用 get_state → 显式查询当前状态
     └── 返回: { state: "Halted", thread_id: 1, ... }

  3. 执行操作后 → 状态变化 → 工具列表变化 → AI 再次感知
```

### 7.3 为什么不用 MCP 推送？

1. **MCP 协议设计的交互模型**：AI 客户端主动发起请求
2. **减少竞态条件**：轮询模式消除了事件到达和工具调用之间的时序竞态
3. **简化实现**：无需处理 MCP 通知的路由和序列化
4. **`tools/list` 是天然的状态通道**：工具可用性本身就是状态信号

---

## 8. 会话状态机

### 8.1 状态定义

```rust
// debug-session/src/state.rs
pub enum SessionState {
    Disconnected,  // 未连接调试器
    Connected,     // 已连接，等待 initialize
    Initialized,   // initialize 完成，等待 launch
    Running,       // 程序运行中
    Halted,        // 程序暂停 (断点/单步/异常)
}
```

### 8.2 状态转换图

```
                  start()
Disconnected ───────────────► Connected
                                  │
                           initialize()
                                  │
                                  ▼
                launch() +  Initialized
                configDone    │         │
                     │        │    attach()
                     ▼        ▼         │
              ┌──────── Running ◄───────┘
              │            │
              │  pause()   │ stopped event
              │            │
              ▼            ▼
              │        Halted
              │            │
              │ continue() │ step_over/step_in/step_out
              │            │
              └────────────┘

   任意非 Disconnected 状态 ── shutdown() ──► Disconnected
   Halted/Running ── terminated/exited event ──► Disconnected
```

### 8.3 状态门控矩阵

```rust
// debug-session/src/gating.rs
// 静态矩阵：操作 × 状态 = 允许/禁止
```

| 操作 | Disconnected | Connected | Initialized | Running | Halted |
|------|:---:|:---:|:---:|:---:|:---:|
| start | ✓ | ✗ | ✗ | ✗ | ✗ |
| initialize | ✗ | ✓ | ✗ | ✗ | ✗ |
| launch | ✗ | ✗ | ✓ | ✗ | ✗ |
| attach | ✗ | ✗ | ✓ | ✗ | ✗ |
| configuration_done | ✗ | ✗ | ✓ | ✗ | ✗ |
| continue/step_* | ✗ | ✗ | ✗ | ✗ | ✓ |
| pause | ✗ | ✗ | ✗ | ✓ | ✗ |
| set_*_breakpoints | ✗ | ✗ | ✓ | ✓ | ✓ |
| get_* (inspect) | ✗ | ✗ | ✗ | ✗ | ✓ |
| shutdown | ✗ | ✓ | ✓ | ✓ | ✓ |
| get_state | ✓ | ✓ | ✓ | ✓ | ✓ |
| register_* | ✓ | ✓ | ✓ | ✓ | ✓ |
| search_variables | ✓ | ✓ | ✓ | ✓ | ✓ |

### 8.4 停止详情

```rust
pub struct HaltState {
    pub thread_ids: Vec<u64>,          // 停止的线程 ID
    pub stop_reason: Option<String>,   // "breakpoint" | "step" | "exception" | "pause"
    pub hit_breakpoint_ids: Vec<u64>,  // 命中的断点 ID
}
```

---

## 9. 辅助机制

### 9.1 变量句柄缓存（VariableHandleCache）

**问题**：DAP 的 `variablesReference` 是整数句柄，AI 无法通过变量名直接获取变量值。

**解决方案**：

```rust
// debug-session/src/cache.rs
pub struct VariableHandleCache {
    // 三层嵌套映射：frame_id → scope_name → variable_name → handle
    entries: HashMap<u64, HashMap<String, HashMap<String, u64>>>,
}
```

特性：
- **作用域查找**：优先匹配当前 `frame_id` 和 `scope_name` 的条目
- **模糊搜索**：`search_variables` 工具支持大小写不敏感的部分匹配
- **自动失效**：从 Halted→Running 或 Halted→Disconnected 时清空（变量句柄仅在暂停时有效）
- **自动填充**：每次 `get_scopes` 和 `get_variables` 调用后更新缓存

### 9.2 路径映射器（PathMapper）

**问题**：AI 客户端使用简短相对路径（如 `src/main.cpp`），调试器需要系统绝对路径。

**解决方案**：

```rust
// debug-session/src/mapping.rs
pub struct PathMapper {
    aliases: Vec<(String, String)>,  // (别名, 实际路径前缀)
    base_dirs: Vec<String>,          // 基础目录
}
```

API：
- `register_alias("src", "/home/user/project/src")` — 注册路径别名
- `register_base_dir("/home/user/project")` — 注册基础目录
- `resolve("src/main.cpp")` → `"/home/user/project/src/main.cpp"` — 正向解析
- `reverse("/home/user/project/src/main.cpp")` → `"src/main.cpp"` — 反向解析
- 最长前缀匹配：多个别名时选择最具体的匹配

**自动应用**：`launch` 和 `set_breakpoints` 处理器会自动解析 `source_path`。

### 9.3 追踪系统（dap-trace）

```rust
// dap-trace/src/lib.rs
// 环形缓冲区（内存）+ JSONL 文件持久化
// TraceSource: McpTrigger | DapEvent | DapRequest | DapResponse
```

每个 `TraceEntry` 记录：
- 时间戳
- 来源（MCP 触发 / DAP 事件 / DAP 请求 / DAP 响应）
- 操作名称
- 序列化数据

---

## 10. 完整数据流示例

以下以 AI 客户端设置断点并继续运行为例，展示完整的协议转换流程：

### 10.1 启动并初始化

```
AI                           TeleDAP                        codelldb
│                             │                               │
│  tools/call start           │                               │
│ ─────────────────────────►  │  spawn codelldb process       │
│                             │ ────────────────────────────►  │
│                             │  State → Connected             │
│  ◄───────────────────────── │  success                      │
│                             │                               │
│  tools/call initialize      │                               │
│ ─────────────────────────►  │  initialize request            │
│                             │ ────────────────────────────►  │
│                             │  ◄──── Capabilities ─────────  │
│                             │  State → Initialized           │
│  ◄───────────────────────── │  {capabilities}               │
│                             │                               │
│  tools/call launch          │                               │
│  {program, args, cwd}       │                               │
│ ─────────────────────────►  │  launch request (fire-forget)  │
│                             │ ────────────────────────────►  │
│                             │  configurationDone             │
│                             │ ────────────────────────────►  │
│                             │           initialized event    │
│                             │  ◄───────────────────────────  │
│                             │  State → Running               │
│                             │           stopped event        │
│                             │  ◄───────────────────────────  │
│                             │  State → Halted                │
│  ◄───────────────────────── │  success (含 stop_reason)     │
```

### 10.2 设置断点并继续

```
AI                           TeleDAP                        codelldb
│                             │                               │
│  tools/call                 │                               │
│  set_breakpoints            │                               │
│  {source_path:"src/main.cpp"│                               │
│   breakpoints:[{line:42}]}  │                               │
│ ─────────────────────────►  │                               │
│                             │  1. 检查状态: Halted ✓         │
│                             │  2. 路径映射:                   │
│                             │     "src/main.cpp" →           │
│                             │     "/home/.../src/main.cpp"   │
│                             │  3. 构造 SetBreakpointsArgs    │
│                             │  4. send_request::<            │
│                             │     SetBreakpointsRequest>()   │
│                             │ ────────────────────────────►  │
│                             │      SetBreakpointsResponse    │
│                             │  ◄───────────────────────────  │
│                             │  5. 转换结果→CallToolResult    │
│  ◄───────────────────────── │  [{id:1,line:42,verified:✓}] │
│                             │                               │
│  tools/call continue        │                               │
│ ─────────────────────────►  │                               │
│                             │  1. 检查状态: Halted ✓         │
│                             │  2. send_request::<            │
│                             │     ContinueRequest>()         │
│                             │ ────────────────────────────►  │
│                             │  ◄─── 继续执行 ───────────── │
│                             │           stopped event        │
│                             │  ◄───────────────────────────  │
│                             │  State → Running → Halted      │
│  ◄───────────────────────── │  {allThreadsContinued: true}  │
│                             │                               │
│  tools/call get_variables   │                               │
│  {variables_reference: 1001}│                               │
│ ─────────────────────────►  │                               │
│                             │  1. 检查状态: Halted ✓         │
│                             │  2. send_request::<            │
│                             │     VariablesRequest>()        │
│                             │ ────────────────────────────►  │
│                             │      VariablesResponse         │
│                             │  ◄───────────────────────────  │
│                             │  3. 更新 VariableHandleCache   │
│  ◄───────────────────────── │  [{name:"x",value:"42",...}] │
```

### 10.3 错误处理示例

```
AI                           TeleDAP
│                             │
│  tools/call continue        │
│  (当前状态: Running)         │
│ ─────────────────────────►  │
│                             │  ToolRegistry::dispatch()
│                             │  → 状态门控检查失败
│                             │  → BridgeError::Internal(
│                             │      "continue requires state(s): Halted"
│                             │    )
│                             │  → CallToolResult::error(...)
│  ◄───────────────────────── │  {is_error: true,
│                             │   content: [{type:"text",
│                             │     text:"continue requires
│                             │            state(s): Halted"}]}
```

### 10.4 协议线格式对比

**MCP 传输（行分隔 JSON）**：
```
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"continue","arguments":{}}}
```

**DAP 传输（Content-Length 帧）**：
```
Content-Length: 81\r\n
\r\n
{"seq":1,"type":"request","command":"continue","arguments":{"threadId":1}}
```

---

## 附录：关键文件索引

| 文件 | 职责 |
|------|------|
| `src/main.rs` | CLI 入口，模式自动检测 |
| `src/server.rs` | MCP 事件循环，后台事件消费 |
| `crates/mcp-protocol/src/types.rs` | JSON-RPC 消息类型、Tool、CallToolResult |
| `crates/mcp-protocol/src/transport.rs` | McpServer stdio 传输 |
| `crates/dap-client/src/client.rs` | DapClient 实现 |
| `crates/dap-codec/src/lib.rs` | Content-Length 帧编解码器 |
| `crates/dap-types/src/base.rs` | ProtocolMessage 枚举 |
| `crates/dap-types/src/requests.rs` | 42 个 DAP 请求类型 + DapRequest trait |
| `crates/dap-types/src/events.rs` | 17 个事件体类型 |
| `crates/debug-bridge/src/registry.rs` | ToolRegistry 门控与分派 |
| `crates/debug-bridge/src/tools.rs` | 24 个工具定义 + tool_operation 映射 |
| `crates/debug-bridge/src/handlers/lifecycle.rs` | 生命周期处理器 |
| `crates/debug-bridge/src/handlers/execution.rs` | 执行控制处理器 |
| `crates/debug-bridge/src/handlers/breakpoint.rs` | 断点处理器 |
| `crates/debug-bridge/src/handlers/inspect.rs` | 检查处理器 |
| `crates/debug-session/src/session.rs` | DebugSession 核心 |
| `crates/debug-session/src/state.rs` | SessionState 枚举 + HaltState |
| `crates/debug-session/src/gating.rs` | 操作×状态 门控矩阵 |
| `crates/debug-session/src/mapping.rs` | PathMapper 双向路径映射 |
| `crates/debug-session/src/cache.rs` | VariableHandleCache |
| `crates/debug-session/src/context.rs` | ContextChain 上下文链 |
| `crates/debug-session/src/variables.rs` | VariableExpander 递归展开 |
