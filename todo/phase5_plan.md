# TeleDAP Phase 5 实施计划：高级调试能力

> 对应原项目 Roadmap 中的 **Phase 5 — Advanced Features**。  
> 目标：在现有 DAP/MCP 桥上增加 watchpoints、反汇编、多线程/RTOS、持久 watch expressions、调试会话回放。

---

## 1. 目标与范围

Phase 5 聚焦“复杂程序调试”场景，新增/增强以下能力：

1. **数据断点 / Watchpoints** —— 在变量/地址被读/写/读写时暂停。
2. **反汇编视图** —— 查看指定地址的汇编指令，支持符号解析。
3. **多线程 / RTOS 感知** —— 更好地列出线程、做线程级断点/单步。
4. **持久 Watch Expressions** —— 设置表达式后，每次 halted 自动重求值。
5. **Trace 回放** —— 从 `dap-trace` JSONL 重建调试会话时间线。

---

## 2. 关键设计决策

| 决策 | 方案 | 理由 |
|---|---|---|
| Watchpoints | 复用 DAP 标准 `setDataBreakpoints` 请求 | 无需发明新协议；codelldb/GDB 均支持 |
| 反汇编 | 复用 DAP 标准 `disassemble` 请求 | 标准请求，结构清晰 |
| 多线程/RTOS | 主要增强 `get_threads` 和线程级断点；不重新发明线程模型 | DAP `threads` 已能返回 RTOS 线程 |
| 持久 Watch Expressions | 在 `DebugSession` 内维护列表，`handle_event()` 在 `stopped` 时自动 evaluate | 与状态机天然集成 |
| Trace 回放 | 新增 `dap-trace/src/replay.rs`，提供 `TraceReplay` | 不污染运行时调试逻辑 |
| 状态门控 | watchpoints 允许在 Initialized/Running/Halted；disassemble 仅在 Halted；watch expressions 工具无门控 | 与 DAP 语义一致 |

---

## 3. 详细任务清单

### 3.1 扩展 DAP 类型（dap-types）

在 `crates/dap-types/src/requests.rs` 中新增：

```rust
pub struct SetDataBreakpointsRequest;
pub struct SetDataBreakpointsArguments {
    pub breakpoints: Vec<DataBreakpoint>,
}
pub struct SetDataBreakpointsResponse {
    pub breakpoints: Vec<Breakpoint>,
}

pub struct DisassembleRequest;
pub struct DisassembleArguments {
    pub memory_reference: String,
    pub offset: Option<i64>,
    pub instruction_offset: Option<i64>,
    pub instruction_count: i64,
    pub resolve_symbols: Option<bool>,
}
pub struct DisassembleResponse {
    pub instructions: Vec<DisassembledInstruction>,
}
```

在 `crates/dap-types/src/types.rs` 中新增：

```rust
pub struct DataBreakpoint {
    pub data_id: String,
    pub access_type: Option<DataBreakpointAccessType>,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
}

pub enum DataBreakpointAccessType { Read, Write, ReadWrite }

pub struct DisassembledInstruction {
    pub address: String,
    pub instruction: String,
    pub symbol: Option<String>,
    pub location: Option<Source>,
    pub line: Option<i64>,
    pub column: Option<i64>,
    pub end_line: Option<i64>,
}
```

在 `crates/dap-types/src/base.rs` 中注册 `DataBreakpointInfoRequest/Response`（如果需要 DAP 标准的 `dataBreakpointInfo` 来把表达式转成 `dataId`）。

---

### 3.2 DebugSession 新增方法

在 `crates/debug-session/src/session.rs` 中新增：

```rust
pub async fn set_data_breakpoints(
    &self,
    args: SetDataBreakpointsArguments,
) -> Result<SetDataBreakpointsResponse, DebugSessionError>

pub async fn disassemble(
    &self,
    args: DisassembleArguments,
) -> Result<DisassembleResponse, DebugSessionError>
```

状态门控：
- `set_data_breakpoints`：Initialized、Running、Halted。
- `disassemble`：Halted。

---

### 3.3 Watchpoint Tool & Handler

在 `crates/debug-bridge/src/tools.rs` 新增 tool `set_data_breakpoints`。

在 `crates/debug-bridge/src/handlers/breakpoint.rs` 新增 handler：

```rust
pub async fn handle_set_data_breakpoints(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError>
```

输入示例：

```json
{
  "breakpoints": [
    { "dataId": "&global_counter", "accessType": "write" },
    { "dataId": "0x20000000", "accessType": "readWrite", "condition": "value == 0xFF" }
  ]
}
```

**注意**：`dataId` 的生成通常需要先调用 DAP `dataBreakpointInfo`。可以分两步：

1. 先实现 `data_breakpoint_info(expression)` 获取 `dataId`。
2. 再在 `set_data_breakpoints` 里接受 `dataId`。

或者提供更高层的 tool `set_watchpoint(expression, accessType)`，内部自动调用 `dataBreakpointInfo` + `setDataBreakpoints`。

---

### 3.4 反汇编 Tool & Handler

在 `crates/debug-bridge/src/tools.rs` 新增 tool `disassemble`。

在 `crates/debug-bridge/src/handlers/inspect.rs` 新增 handler：

```rust
pub async fn handle_disassemble(
    session: &DebugSession,
    params: serde_json::Value,
) -> Result<CallToolResult, BridgeError>
```

输入示例：

```json
{
  "memoryReference": "0x08000000",
  "offset": 0,
  "instructionCount": 20,
  "resolveSymbols": true
}
```

输出示例：

```json
{
  "instructions": [
    { "address": "0x08000000", "instruction": "movs r0, #0", "symbol": "Reset_Handler" },
    { "address": "0x08000002", "instruction": "ldr r1, [pc, #28]", "symbol": "Reset_Handler+2" }
  ]
}
```

---

### 3.5 持久 Watch Expressions

#### 数据结构设计

在 `DebugSession` 中新增：

```rust
struct WatchExpression {
    id: u64,
    expression: String,
    frame_id: Option<u64>,
    last_value: Option<String>,
}

watch_expressions: RwLock<Vec<WatchExpression>>,
next_watch_id: AtomicU64,
```

#### 新增 Tools

| Tool | 说明 |
|---|---|
| `add_watch_expression` | 添加一个表达式 |
| `remove_watch_expression` | 按 id 删除 |
| `list_watch_expressions` | 列出所有表达式及上次求值结果 |

实现位置：`crates/debug-bridge/src/handlers/inspect.rs`。

#### 自动重求值

修改 `crates/debug-session/src/session.rs` 中的 `handle_event()`：

- 当收到 `stopped` 事件并转入 `Halted` 后，遍历 `watch_expressions`，对每个表达式调用 `evaluate(expression, frame_id)`。
- 更新 `last_value`。
- 可通过 tracing 输出一次汇总，便于 AI 读取。

**关键技术点**：`frame_id` 在每次 stop 都会变化，持久 watch 应该优先在“当前 selected frame”或“同名函数帧”上重求值；可先简单使用 `threadId=stopped.threadId` 的顶层 frame。

---

### 3.6 多线程 / RTOS 增强

#### 增强 `get_threads`

现有 `get_threads` 已在 Running/Halted 可用。增加可选 `detail` 参数：

```json
{ "detail": "full" }
```

返回中增加：

```json
{
  "threads": [
    { "id": 1, "name": "main", "running": false },
    { "id": 2, "name": "FreeRTOS: IDLE", "running": true }
  ]
}
```

DAP `Thread` 已有 `name`，`running` 可由 `continued`/`stopped` 事件推断。

#### 线程级断点

新增 tool `set_thread_breakpoint`：

```json
{
  "sourcePath": "src/main.cpp",
  "line": 42,
  "threadId": 2
}
```

实现：在 `condition` 中注入 `$_thread == 2` 的 GDB 表达式（adapter-specific），或提示用户该功能依赖 GDB。

更安全的做法：先提供文档说明“使用 `set_breakpoints` 的 `condition` 字段加 `$_thread == 2`”，不新增 tool。

---

### 3.7 Trace 回放

#### 新增模块

`crates/dap-trace/src/replay.rs`：

```rust
pub struct TraceReplay {
    entries: Vec<TraceEntry>,
    position: usize,
}

impl TraceReplay {
    pub async fn load(path: &Path) -> Result<Self, ...>
    pub fn current_state(&self) -> Option<&TraceEntry>
    pub fn next(&mut self) -> Option<&TraceEntry>
    pub fn goto(&mut self, index: usize) -> Option<&TraceEntry>
    pub fn state_at(&self, index: usize) -> Option<SessionState>
}
```

#### 新增 Tools（无状态门控）

| Tool | 说明 |
|---|---|
| `load_trace` | 加载 JSONL trace 文件 |
| `replay_next` | 前进一条 trace entry |
| `replay_to_event` | 跳到指定事件（如 `stopped`、`state_transition`） |
| `replay_state` | 当前回放位置与对应会话状态 |

实现位置：新建 `crates/debug-bridge/src/handlers/replay.rs`，并在 `registry.rs` 路由。

---

### 3.8 Registry / Gating 更新

- `crates/debug-bridge/src/tools.rs`：新增 tool 定义和 `tool_operation()` 映射。
- `crates/debug-bridge/src/registry.rs`：新增 dispatch 分支。
- `crates/debug-session/src/gating.rs`：新增 operation 允许状态：
  - `set_data_breakpoints` → Initialized, Running, Halted
  - `disassemble` → Halted
  - `add_watch_expression` / `remove_watch_expression` / `list_watch_expressions` → 无门控
  - `load_trace` / `replay_next` / `replay_to_event` / `replay_state` → 无门控

---

## 4. 接口草案汇总

### `set_data_breakpoints` / `set_watchpoint`

```json
// set_data_breakpoints
{ "breakpoints": [{ "dataId": "&counter", "accessType": "write" }] }

// 或更友好的 set_watchpoint（推荐给用户）
{ "expression": "counter", "accessType": "write" }
```

### `disassemble`

```json
{ "memoryReference": "0x08000000", "instructionCount": 20, "resolveSymbols": true }
```

### `add_watch_expression`

```json
{ "expression": "counter * 2", "frameId": 123 }
```

### `list_watch_expressions`

```json
{
  "expressions": [
    { "id": 1, "expression": "counter", "lastValue": "42" }
  ]
}
```

### `load_trace`

```json
{ "path": "C:\\logs\\teledap_20260726.jsonl" }
```

### `replay_next` / `replay_to_event`

```json
{ "eventType": "stopped" }
```

---

## 5. 测试计划

### 5.1 单元测试

- `dap-types`：serde roundtrip for `DataBreakpoint`, `DisassembledInstruction`, `DisassembleArguments`。
- `debug-session`：`set_data_breakpoints` / `disassemble` 的状态门控测试（mock DAP client）。
- `debug-bridge`：参数反序列化测试。
- `dap-trace`：`TraceReplay` 解析和遍历测试。

### 5.2 集成测试

在 `crates/debug-bridge/tests/mcp_integration_test.rs` 中新增：

- `set_data_breakpoints`：在 test_debuggee 的 `counter` 变量上设置 write watchpoint，验证返回 verified。
- `disassemble`：在 `main` 函数地址反汇编若干条指令，验证返回非空。
- `add_watch_expression` + continue + halted：验证 `list_watch_expressions` 的 `lastValue` 已更新。

**注意**：watchpoint 和 disassemble 依赖 codelldb 支持程度，若某些平台不支持应优雅跳过。

---

## 6. 依赖与风险

| 风险 | 缓解措施 |
|---|---|
| codelldb 对 `setDataBreakpoints` / `disassemble` 支持有限 | 先探测 capabilities；不支持时返回明确错误 |
| 持久 watch expressions 的 frameId 失效 | 在 stopped 时用当前线程顶层 frame 重求值；记录失败原因 |
| Trace 回放需要稳定的事件格式 | 在 dap-trace 中增加 schema version 字段 |
| 多线程/RTOS 依赖 adapter/target 配置 | 提供文档说明所需 launch 配置（如 FreeRTOS 插件） |

---

## 7. 验收标准

- [ ] `tools/list` 中出现 watchpoint、disassemble、watch expressions、trace replay 相关工具。
- [ ] `set_data_breakpoints` 在支持的环境下能设置并命中 watchpoint。
- [ ] `disassemble` 在 Halted 状态下返回指令列表。
- [ ] 持久 watch expressions 在 continue → stopped 后自动更新 `lastValue`。
- [ ] `load_trace` + `replay_next` 能正确遍历 JSONL 文件。
- [ ] 单元测试 + 集成测试通过；lint 通过。
- [ ] README 和 AI Agent 集成指南更新。

---

## 8. 推荐的第一步

从 **3.4 反汇编（`disassemble`）** 开始：

- 只涉及标准 DAP 请求，改动范围可控。
- 不需要状态机复杂改动。
- 可立即给 AI 提供“帮我看看当前 PC 附近的汇编”能力。
- 完成后用户能直观感受到 Phase 5 的价值。
