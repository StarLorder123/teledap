# TeleDAP AI Agent 集成指南

本文档面向希望将 TeleDAP 接入 AI Agent 的开发者和用户，涵盖 stdio 与 HTTP/SSE 两种 MCP 接入模式、配置步骤、生命周期管理以及常见问题。

---

## 1. 快速选择接入模式

TeleDAP 同时支持两种 MCP 传输层：

| 模式 | 启动方式 | 适用场景 | 对 AI Agent 的要求 |
|---|---|---|---|
| **stdio** | `cargo run --`（stdin 为 pipe） | Claude Desktop、Claude Code 等 spawn 子进程的客户端 | 支持 stdio MCP server（最普遍） |
| **HTTP/SSE** | `cargo run -- --http --port 8080` | 自建 Agent、多客户端共享会话、Web UI、远程接入 | 能连 HTTP/SSE，或自行实现 MCP client |

> **注意**：Claude Desktop 目前主要支持 stdio MCP server。若你的目标平台是 Claude Desktop，建议优先使用 stdio 模式。

---

## 2. stdio 模式接入

### 2.1 基本配置

以 Claude Desktop 为例，在 `claude_desktop_config.json` 中添加：

```json
{
  "mcpServers": {
    "teledap": {
      "command": "E:\\Code\\cpp\\teledap\\target\\release\\teledap.exe",
      "args": []
    }
  }
}
```

由于 Claude Desktop 启动 server 时无法交互式询问路径，建议用包装脚本固定常用路径。

### 2.2 推荐：PowerShell 包装脚本

创建 `run_teledap.ps1`：

```powershell
$env:CODE_LLDB_PATH = "C:\tools\codelldb\extension\adapter\codelldb.exe"
$env:ELF_PATH = "C:\projects\myapp\build\myapp.elf"
$env:PROJECT_ROOT = "C:\projects\myapp"
E:\Code\cpp\teledap\target\release\teledap.exe
```

然后在 MCP 配置中指向该脚本：

```json
{
  "mcpServers": {
    "teledap": {
      "command": "powershell",
      "args": ["-ExecutionPolicy", "Bypass", "-File", "C:\\tools\\run_teledap.ps1"]
    }
  }
}
```

### 2.3 首次对话流程

AI 启动后应先完成路径注册和调试器启动：

```json
{"name": "register_base_dir", "arguments": {"dir": "C:\\projects\\myapp"}}
{"name": "start", "arguments": {"adapterPath": "C:\\tools\\codelldb\\extension\\adapter\\codelldb.exe"}}
{"name": "initialize", "arguments": {}}
{"name": "launch", "arguments": {"program": "C:\\projects\\myapp\\build\\myapp.elf"}}
{"name": "configuration_done", "arguments": {}}
```

---

## 3. HTTP/SSE 模式接入

### 3.1 启动服务器

```bash
cargo run --release -- --http --port 8080
```

日志输出到 stderr，stdout 保持干净。

### 3.2 协议流程

1. **建立 SSE 连接**：
   ```bash
   curl -N http://localhost:8080/sse
   ```
   服务器返回：
   ```
   event: endpoint
   data: /message?session_id=550e8400-e29b-41d4-a716-446655440000
   ```

2. **发送 JSON-RPC**：
   ```bash
   curl -X POST "http://localhost:8080/message?session_id=550e8400-e29b-41d4-a716-446655440000" \
     -H "Content-Type: application/json" \
     -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
   ```
   服务器返回 `202 Accepted`。

3. **接收响应**：响应通过 SSE 流异步推送：
   ```
   event: message
   data: {"jsonrpc":"2.0","id":1,"result":{...}}
   ```

### 3.3 自建 AI Agent 代码示例（Python）

```python
import asyncio
import aiohttp

async def main():
    async with aiohttp.ClientSession() as session:
        # 1. 建立 SSE 连接
        async with session.get("http://localhost:8080/sse") as resp:
            endpoint = None
            async for line in resp.content:
                text = line.decode().strip()
                if text.startswith("data:"):
                    endpoint = text[5:].strip()
                    break

        session_id = endpoint.split("session_id=")[1]

        # 2. 发送 initialize
        await session.post(
            f"http://localhost:8080{endpoint}",
            json={"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        )

        # 3. 在另一个任务中持续读取 SSE 响应...

asyncio.run(main())
```

### 3.4 多客户端共享

所有 SSE 连接共享同一个 `DebugSession`/`OpenOcdClient`。建议：

- 指定一个客户端负责 `start`/`launch`/`configuration_done` 等生命周期操作。
- 其他客户端只做观察（`get_state`、`get_stack_trace`、`evaluate` 等）。
- 高阶操作前调用 `get_state` 确认当前状态和可用工具。

---

## 4. 给 AI 的系统提示模板

将以下内容加入 AI Agent 的系统提示：

```markdown
你是一个 C/C++ 调试助手，通过 TeleDAP MCP server 控制调试器。

每次执行操作前，先调用 `get_state` 确认当前状态。

调试生命周期必须按顺序执行：
1. start（启动调试适配器）
2. initialize（DAP 初始化握手）
3. launch（加载被调试程序）
4. configuration_done（开始运行）

设置断点前，先调用 `register_base_dir` 注册项目根目录。

用户工程信息：
- 项目根目录：C:\projects\myapp
- codelldb 路径：C:\tools\codelldb\extension\adapter\codelldb.exe
- ELF 路径：C:\projects\myapp\build\myapp.elf

注意：
- 工具错误以 `isError: true` 返回，不是 JSON-RPC error。
- `tools/list` 会根据当前状态过滤，只能看到当前可用的工具。
```

---

## 5. 生命周期与状态机

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

常用检查点：

| 状态 | 可执行操作 |
|---|---|
| Disconnected | `start`、`register_base_dir`、`register_path_alias`、`get_state` |
| Connected | `initialize`、`shutdown` |
| Initialized | `launch`、`attach`、`set_breakpoints`、`shutdown` |
| Running | `pause`、`get_threads` |
| Halted | `continue`、`step_over`、`step_in`、`step_out`、`get_stack_trace`、`get_scopes`、`get_variables`、`evaluate` |

---

## 6. 路径映射

AI 通常使用短相对路径（如 `src/main.cpp`），而调试器需要绝对路径。

### 6.1 注册基目录

```json
{"name": "register_base_dir", "arguments": {"dir": "C:\\projects\\myapp"}}
```

之后 `set_breakpoints` 可直接传 `"sourcePath": "src/main.cpp"`。

### 6.2 注册别名

适用于非标准目录结构：

```json
{"name": "register_path_alias", "arguments": {
  "alias": "drivers/uart.c",
  "absolutePath": "C:\\projects\\myapp\\hal\\stm32\\uart.c"
}}
```

---

## 7. 嵌入式调试（OpenOCD）

针对 ARM/STM32 等硬件：

1. 启动 OpenOCD：
   ```json
   {"name": "openocd_start", "arguments": {
     "openocdPath": "/usr/bin/openocd",
     "configFiles": ["board/stm32f4discovery.cfg"]
   }}
   ```

2. 启动 codelldb 并 launch：
   ```json
   {"name": "start", "arguments": {"adapterPath": "/usr/bin/codelldb"}}
   {"name": "launch", "arguments": {
     "program": "firmware.elf",
     "gdbRemote": "localhost:3333"
   }}
   ```

3. 常用 OpenOCD 命令：
   ```json
   {"name": "openocd_send", "arguments": {"command": "reset halt"}}
   {"name": "openocd_send", "arguments": {"command": "flash write_image firmware.bin 0x08000000"}}
   ```

---

## 8. 常见问题

### Q1: `start` 报找不到 liblldb.dll

Windows 上 codelldb 需要找到 `liblldb.dll`。在 MCP `initialize` 参数中传入：

```json
{"liblldbPath": "C:\\LLVM\\bin\\liblldb.dll"}
```

或在 stdio 模式下通过 CLI `--liblldb-path` 预配置。

### Q2: `launch` 后没有响应

codelldb 会延迟 `launch` 响应直到收到 `configurationDone`。必须先调用 `configuration_done`。

### Q3: HTTP/SSE 下多个客户端如何协调？

目前所有客户端共享同一会话。建议由单一“主控”客户端执行生命周期操作，其他客户端观察。

### Q4: 可以暴露 HTTP/SSE 到公网吗？

**不建议**。当前 HTTP/SSE 模式无认证、无 TLS，仅适用于本地或可信内网。若需远程，建议前置反向代理 + mTLS/API key。

### Q5: Claude Desktop 能用 HTTP/SSE 吗？

目前 Claude Desktop 主要支持 stdio MCP server。HTTP/SSE 更适合自建 Agent 或未来支持 HTTP/SSE 的客户端。

---

## 9. 参考

- [CLAUDE.md](../CLAUDE.md) — 项目架构与开发约定
- [MCP-Inspector手动测试指南](./MCP-Inspector手动测试指南.md) — 交互式测试方法
- [MCP-DAP协议桥接架构.md](./MCP-DAP协议桥接架构.md) — 协议桥接设计
