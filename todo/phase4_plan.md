# TeleDAP Phase 4 实施计划：嵌入式硬件一级调试能力

> 对应原项目 Roadmap 中的 **Phase 4 — Advanced Hardware Integration**。  
> 目标：把 OpenOCD 从“进程管理 + raw Tcl 发送”升级为 AI 可直接使用的硬件调试层。

---

## 1. 目标与范围

在 Phase 3 已有能力（OpenOCD 进程生命周期 + `openocd_send` raw 命令）之上，提供：

1. **寄存器读写** —— 读取/写入 CPU 通用寄存器、特殊寄存器。
2. **内存读写** —— 按字节/半字/字/双字访问任意物理地址。
3. **Flash 烧录** —— 把 `.elf`/`.bin` 一键烧写到目标芯片。
4. **目标复位** —— `reset halt` / `reset run` 一级工具。
5. **组合工作流** —— `flash_and_debug`：启动 OpenOCD → 烧录 → 连接 GDB remote → 启动 DAP 调试。
6. **OpenOCD 集成测试** —— 至少覆盖 read_registers / read_memory / flash_image。

---

## 2. 关键设计决策

| 决策 | 方案 | 理由 |
|---|---|---|
| OpenOCD 与 DAP 状态机关系 | 保持独立；新工具仍不纳入 `SessionState`，但组合工具内部协调两者 | OpenOCD 是可选扩展，不应污染 DAP 状态机 |
| Tcl 命令封装位置 | 在 `openocd-client` 增加高层 helper，handler 只负责 MCP 参数解析 | 可测试、可复用、避免 handler 里拼字符串 |
| 地址/数值输入格式 | 统一使用**十六进制字符串**（如 `"0x08000000"`） | 对 AI 和人类最直观 |
| 输出格式 | JSON 表格 + 十六进制字符串，必要时保留原始 Tcl 输出片段 | 方便 AI 解析，也方便人类核对 |
| 错误处理 | 保持 `is_error: true` 的 tool result，不抛 JSON-RPC error | 与现有 30 个工具风格一致 |
| 组合工作流失败策略 | 顺序执行，任何一步失败即回滚已启动资源 | 避免留下僵尸 OpenOCD / codelldb 进程 |
| 目标平台优先支持 | ARM Cortex-M（STM32 系列） | 用户群最大；后续再泛化到 RISC-V/ESP32 |

---

## 3. 详细任务清单

### 3.1 openocd-client 高层 API（不含 MCP）

在 `crates/openocd-client/src/client.rs` 中新增方法：

```rust
pub async fn read_registers(&self, names: &[String]) -> Result<Vec<RegisterValue>, ...>
pub async fn write_register(&self, name: &str, value: u64) -> Result<(), ...>
pub async fn read_memory(&self, address: u64, width: MemoryWidth, count: usize) -> Result<Vec<MemoryRow>, ...>
pub async fn write_memory(&self, address: u64, width: MemoryWidth, value: u64) -> Result<(), ...>
pub async fn flash_image(&self, image_path: &str, verify: bool, reset_after: bool) -> Result<(), ...>
pub async fn reset_target(&self, mode: ResetMode) -> Result<(), ...>
```

新增类型定义在 `crates/openocd-client/src/lib.rs` 或新建 `crates/openocd-client/src/types.rs`：

```rust
pub enum MemoryWidth { Byte, HalfWord, Word, DoubleWord }
pub enum ResetMode { Halt, Run }
pub struct RegisterValue { pub name: String, pub value: u64, pub bits: u8 }
pub struct MemoryRow { pub address: u64, pub values: Vec<u64> }
```

**关键技术点：**
- 使用 `send_command` 发送 `reg`、`mdw/mdh/mdb`、`mww/mwl`、`flash write_image`、`reset halt` 等命令。
- 解析 OpenOCD 文本输出：寄存器行通常是 `r0 (/32): 0x2000FF00`；内存行是 `0x08000000: 00000000 00000000 ...`。
- 为解析错误增加结构化错误类型到 `crates/openocd-client/src/error.rs`。

---

### 3.2 新增 MCP Tools

在 `crates/debug-bridge/src/tools.rs` 的 `all_tools()` 中加入：

| Tool | 类别 | 说明 |
|---|---|---|
| `read_registers` | OpenOCD | 读一个或多个寄存器；空列表表示全部 |
| `write_register` | OpenOCD | 写单个寄存器 |
| `read_memory` | OpenOCD | 从指定地址读取内存 |
| `write_memory` | OpenOCD | 向指定地址写入内存 |
| `flash_image` | OpenOCD | 烧录镜像文件 |
| `reset_target` | OpenOCD | 复位目标芯片 |
| `flash_and_debug` | Utility / 组合 | 一键烧录并进入 DAP 调试 |

同时更新 `crates/debug-bridge/src/tools.rs` 中的 `tool_operation()` 映射（如果新工具需要状态门控）。  
OpenOCD 独立工具无需门控；`flash_and_debug` 设计为 Utility（无门控）。

---

### 3.3 新增 Handlers

实现位置二选一：

- **方案 A**：扩展 `crates/debug-bridge/src/handlers/openocd.rs`（推荐，与现有 OpenOCD handler 集中）。
- **方案 B**：新建 `crates/debug-bridge/src/handlers/openocd_debug.rs`。

新增 handler 函数签名示例：

```rust
pub async fn handle_read_registers(
    _session: &DebugSession,
    params: serde_json::Value,
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
) -> Result<CallToolResult, BridgeError>
```

`flash_and_debug` handler 需要同时访问 `session` 和 `openocd`：

```rust
pub async fn handle_flash_and_debug(
    session: &DebugSession,
    params: serde_json::Value,
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
) -> Result<CallToolResult, BridgeError>
```

---

### 3.4 Registry 路由

更新 `crates/debug-bridge/src/registry.rs` 的 `dispatch()` match：

```rust
"read_registers" => handlers::openocd::handle_read_registers(session, params, openocd).await,
"write_register" => handlers::openocd::handle_write_register(session, params, openocd).await,
"read_memory"    => handlers::openocd::handle_read_memory(session, params, openocd).await,
"write_memory"   => handlers::openocd::handle_write_memory(session, params, openocd).await,
"flash_image"    => handlers::openocd::handle_flash_image(session, params, openocd).await,
"reset_target"   => handlers::openocd::handle_reset_target(session, params, openocd).await,
"flash_and_debug"=> handlers::openocd::handle_flash_and_debug(session, params, openocd).await,
```

---

### 3.5 `flash_and_debug` 组合工作流详细设计

输入参数示例：

```json
{
  "openocdPath": "C:\\tools\\openocd\\bin\\openocd.exe",
  "configFiles": ["interface/stlink.cfg", "target/stm32f4x.cfg"],
  "imagePath": "C:\\projects\\myapp\\build\\myapp.elf",
  "verify": true,
  "gdbPort": "localhost:3333",
  "adapterPath": "C:\\tools\\codelldb\\extension\\adapter\\codelldb.exe",
  "program": "C:\\projects\\myapp\\build\\myapp.elf",
  "stopOnEntry": false
}
```

执行步骤：

1. 启动 OpenOCD：`openocd_start` 逻辑复用。
2. 等待 OpenOCD GDB server 就绪（可轮询 `send_command " targets "` 或 sleep）。
3. 烧录镜像：`flash_image(..., verify=true, reset_after=false)`。
4. 复位暂停：`reset_target(Halt)`。
5. 启动 debug adapter：`session.start(...)`。
6. DAP initialize：`session.initialize(...)`。
7. DAP launch：带 `gdbRemote: "localhost:3333"`。
8. `configuration_done`。

回滚：任何一步失败，关闭已启动的 OpenOCD 和 debug adapter，返回聚合错误信息。

---

## 4. 接口草案

### `read_registers`

```json
// input
{ "names": ["r0", "r1", "sp", "lr", "pc"] }

// output
{
  "registers": [
    { "name": "r0", "value": "0x2000FF00", "bits": 32 },
    { "name": "r1", "value": "0x00000000", "bits": 32 }
  ]
}
```

### `write_register`

```json
{ "name": "r0", "value": "0x12345678" }
```

### `read_memory`

```json
// input
{ "address": "0x08000000", "width": "word", "count": 16 }

// output
{
  "rows": [
    { "address": "0x08000000", "values": ["0x00000000", "0x00000000", "0x00000000", "0x00000000"] }
  ]
}
```

`width` 枚举：`byte`、`halfWord`、`word`、`doubleWord`。

### `write_memory`

```json
{ "address": "0x20000000", "width": "word", "value": "0xDEADBEEF" }
```

### `flash_image`

```json
{
  "imagePath": "C:\\projects\\myapp\\build\\myapp.elf",
  "verify": true,
  "resetAfter": false
}
```

### `reset_target`

```json
{ "mode": "halt" }
```

`mode` 枚举：`halt`、`run`。

### `flash_and_debug`

见 3.5。

---

## 5. 测试计划

### 5.1 单元测试

- `crates/openocd-client/src/client.rs` 中新增 mock：模拟 OpenOCD 文本输出，验证解析函数。
- `crates/debug-bridge/src/handlers/openocd.rs` 中新增参数反序列化测试。
- `crates/debug-bridge/src/registry.rs` 中验证新 tools 在 `tools/list` 中出现。

### 5.2 集成测试

新增 `crates/debug-bridge/tests/openocd_integration_test.rs`：

- 探测 `openocd` 是否可用（类似现有 `codelldb_available()`），不可用时跳过。
- 使用 **OpenOCD 模拟器**（如 `stellaris` 模拟器或 QEMU `qemu-system-arm -machine lm3s6965evb`）或真实板子。
- 测试用例：
  1. `openocd_start` → `read_registers` 返回非空寄存器列表。
  2. `write_register` → `read_registers` 验证回读。
  3. `read_memory` 从 Flash 基地址读取到非零值。
  4. `flash_image` 烧录一个测试 `.bin` 后 `reset halt` 不报错。
  5. `flash_and_debug` 端到端：启动 → 烧录 → DAP initialize 成功。

---

## 6. 依赖与风险

| 风险 | 缓解措施 |
|---|---|
| OpenOCD 输出格式因 target 而异 | 先支持 ARM Cortex-M；解析失败时返回原始文本 + 错误提示 |
| 集成测试需要真实硬件 | 先用 OpenOCD 模拟器；真实板子测试作为 nightly/manual |
| `flash_and_debug` 流程长，容易遗留进程 | 任何步骤失败都执行 `shutdown` + `openocd_stop` 回滚；加 tracing 日志 |
| GDB remote 连接时机不确定 | 烧录后/复位后显式等待 `targets` 命令成功 |

---

## 7. 验收标准

- [ ] `tools/list` 中出现 7 个新工具。
- [ ] OpenOCD 未启动时调用新工具返回清晰错误（`is_error: true`）。
- [ ] 至少在一个 ARM Cortex-M 目标（真实板子或 OpenOCD 模拟器）上跑通 `read_memory` 和 `flash_image`。
- [ ] `flash_and_debug` 在端到端场景下能进入 Halted/Running 状态。
- [ ] 单元测试 + 集成测试通过；`cargo fmt --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 通过。
- [ ] `README.md` 和 `docs/AI_Agent_集成指南.md` 更新新工具表格与示例。

---

## 8. 推荐的第一步

从 **4.1 + 4.3 + 4.4（`read_registers` 和 `read_memory`）** 开始：

- 改动范围小，不依赖 DAP 状态机。
- 可立即让 AI 做 “帮我看看 R0/R1/SP 和 0x08000000 内存”。
- 为后续 `flash_and_debug` 组合工具验证 OpenOCD 命令封装正确性。
