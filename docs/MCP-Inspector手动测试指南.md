# TeleDAP MCP Inspector 手动测试指南

本文档介绍如何使用官方 **MCP Inspector**（`@modelcontextprotocol/inspector`）在浏览器中交互式地体验和手动测试 TeleDAP MCP 服务器 —— 无需编写任何代码，即可完整走一遍「启动调试器 → 打断点 → 命中断点 → 查看变量」的调试流程。

## 1. 工具简介

MCP Inspector 是 Model Context Protocol 官方提供的可视化调试工具。它会：

- 将 MCP 服务器（本项目的 `teledap.exe`）作为子进程拉起，通过 stdio 通信
- 启动一个本地 Web UI，可以在浏览器里点击调用工具、填写参数、查看每一条原始 JSON-RPC 往返

由于 teledap 在 stdin 为管道时自动进入 MCP server 模式，Inspector 无需任何额外参数即可直接连接。

## 2. 前置条件

| 依赖 | 说明 |
|---|---|
| Node.js ≥ v18 | 提供 `npx` 命令 |
| teledap 二进制 | `cargo build --release` 编译产出 `target/release/teledap.exe` |
| codelldb | DAP 调试适配器，见下文获取方式 |
| 调试目标 | 仓库自带 `test_debuggee/test_debuggee.exe`（含 pdb） |

### 获取 codelldb

两种方式任选其一：

1. **已安装 VSCode + CodeLLDB 插件**：直接使用
   `%USERPROFILE%\.vscode\extensions\vadimcn.vscode-lldb-*\adapter\codelldb.exe`
2. **单独下载**：从 [vadimcn/codelldb Releases](https://github.com/vadimcn/codelldb/releases) 下载 `codelldb-win64.vsix`，改后缀为 `.zip` 解压，取 `extension/adapter/codelldb.exe`

> 建议：LLDB 对 DWARF 调试信息支持最好。如本机有 clang，可重新编译调试目标以获得最佳体验：
> ```powershell
> clang -g -O0 test_debuggee/main.c -o test_debuggee/test_debuggee.exe
> ```
> MSVC 编译的 PDB 也能用，但变量/源码映射体验略差。

## 3. 启动 Inspector

```powershell
# 先编译
cargo build --release

# 启动 Inspector 并挂载 teledap
npx @modelcontextprotocol/inspector E:\Code\cpp\teledap\target\release\teledap.exe
```

启动后终端会打印一个带鉴权 token 的地址，形如：

```
http://localhost:6274/?MCP_PROXY_AUTH_TOKEN=xxxxxxxx
```

用浏览器打开该地址。

## 4. 连接服务器

在左侧面板确认：

- **Transport Type**: `STDIO`
- **Command**: teledap.exe 的路径（已自动填入）

点击 **Connect**。左下角变绿表示 MCP 握手（`initialize` + `initialized` 通知）已完成。

## 5. 体验状态门控（本项目核心设计）

点击顶部 **Tools** 标签 → **List Tools**。

**此时只会看到少量工具**：`start`、`get_state`、`register_path_alias`、`register_base_dir` 以及 `openocd_*` 系列。

这不是 bug —— TeleDAP 的 `tools/list` **只返回当前会话状态下合法的工具**。会话状态机为：

```
Disconnected → Connected → Initialized → { Running ⇄ Halted }
```

每次状态变化后重新点 **List Tools**，可以观察到可用工具集随状态扩展/收缩。这就是 AI 客户端"按状态发现能做什么"的机制。

## 6. 完整调试流程

每一步操作方式：在工具列表点击工具名 → 右侧填写参数 → **Run Tool**。

| 步骤 | 工具 | 参数示例 | 预期结果 |
|---|---|---|---|
| 1 | `start` | `codelldbPath`: codelldb.exe 的绝对路径 | 状态 → Connected |
| 2 | *(重新 List Tools)* | — | 工具列表明显变多 |
| 3 | `initialize` | `{}` | 状态 → Initialized，返回调试器能力 |
| 4 | `set_breakpoints` | `path`: `E:/Code/cpp/teledap/test_debuggee/main.c`<br>`lines`: `[10]` | 返回断点确认（verified） |
| 5 | `launch` | `program`: `E:/Code/cpp/teledap/test_debuggee/test_debuggee.exe` | fire-and-forget，暂无响应体 |
| 6 | `configuration_done` | `{}` | 程序开始执行，命中断点 → Halted |
| 7 | `get_state` | `{}` | 显示 `Halted` |
| 8 | `get_stack_trace` | `{}` | 调用栈，栈顶停在 main.c 断点行 |
| 9 | `get_scopes` | `frameId`: 上一步返回的 frame id | Locals 等作用域 |
| 10 | `get_variables` | `variablesReference`: 上一步返回的引用 | 局部变量列表 |
| 11 | `evaluate` | `expression`: 某个变量名 | 表达式求值结果 |
| 12 | `assemble_context` | `{}` | **一次性返回组装好的完整调试上下文**（状态+栈+变量），这是 TeleDAP 为 AI 设计的核心特性 |
| 13 | `continue` / `step_over` | `{}` | 状态 → Running，再次命中断点则回到 Halted |
| 14 | `shutdown` | `{}` | 干净收尾，状态 → Disconnected |

> 断点行号注意：`lines` 必须指向**有实际可执行语句的行**（不能是空行、`{`、声明行），否则程序可能直接跑完不停。先打开 `test_debuggee/main.c` 确认行号。

## 7. 值得刻意尝试的场景

### 7.1 乱序调用（错误路径）

在 `Disconnected` 状态直接调用 `launch` 或 `continue`：

- 返回的是 **`is_error: true` 的工具级错误**，附带当前状态与允许的操作说明
- **不是** JSON-RPC 协议错误（`-32xxx`）

这是刻意的设计：工具级错误会作为文本返回给 AI，让 AI 理解"现在还不能这么做"并自行纠正。

### 7.2 路径映射

1. 先调用 `register_base_dir`，参数 `baseDir`: `E:/Code/cpp/teledap`
2. 之后 `set_breakpoints` 的 `path` 只需写相对路径 `test_debuggee/main.c`

也可以用 `register_path_alias` 注册别名做更精细的映射。响应中的路径会被反向翻译回短路径。

### 7.3 模糊变量搜索

断点停下后调用 `search_variables`，输入变量名前缀或片段 —— 该工具基于变量句柄缓存做 4 级优先匹配（精确 → 帧内 → 全局同名 → 模糊）。

## 8. 排障

| 现象 | 可能原因 / 解决 |
|---|---|
| Connect 直接失败 | 二进制未编译或路径错误。先在终端验证：`echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \| .\target\release\teledap.exe` 看是否有响应 |
| `start` 返回错误 | `codelldbPath` 不对，改用绝对路径；确认 codelldb.exe 可独立运行 |
| `launch` 后一直 Running 不停 | 断点行不可执行，换到有实际语句的行；或调试信息与源码不匹配（重新编译） |
| 工具列表里找不到某工具 | 当前状态不允许该操作，先 `get_state` 确认状态，参考第 5 节 |
| 想看原始报文 | Inspector 底部 **History** 面板记录每条 JSON-RPC 往返；teledap 侧另有 dap-trace 的 JSONL 审计日志 |

## 9. 补充：不用 Inspector 的两种替代方式

### 9.1 E2E 脚本（快速验证环境）

```powershell
powershell -ExecutionPolicy Bypass -File test_mcp_e2e.ps1
```

codelldb 在 PATH 上时跑满 7 个阶段；否则自动跳过 P5–P7。

### 9.2 接入 Claude Code（最终使用形态）

```powershell
claude mcp add teledap -- E:/Code/cpp/teledap/target/release/teledap.exe
```

然后在新会话中用自然语言驱动：

> 用 teledap 启动调试，codelldb 在 `<路径>`，调试 `test_debuggee/test_debuggee.exe`，在 main 打断点，停下后告诉我各变量的值。

让 AI 自动完成 start → initialize → set_breakpoints → launch → 变量检查的全流程 —— 这正是本项目的设计目标。
