这是一份重构后的 **C++ CodeLLDB MCP Server 设计方案文档**。本次设计完全切换为 **Stdio 管道架构** 来驱动 CodeLLDB，消除了本地 TCP 端口分配的复杂性，使系统更轻量、进程控制更精准。

---

# 📝 C++ CodeLLDB MCP Server 设计方案说明书 (基于 Stdio 架构)

## 1. 项目概述 (Project Overview)

本项目旨在基于 **Rust** 开发一个符合 **MCP (Model Context Protocol)** 规范的插件服务（Server）。该服务作为“协议双向网桥”：

* **对上**：连接 AI 客户端（如 Claude Desktop），通过标准输入输出（Stdio）暴露高级 C++ 调试工具箱。
* **对下**：在内部以子进程形式拉起 **CodeLLDB**，并通过管道（Piped Stdio）直接发送 **DAP (Debug Adapter Protocol)** 指令。

通过该网桥，大模型能够获得自主下发断点、单步跟踪、捕获异常、以及深度查看 C++ 变量上下文的能力。

---

## 2. 架构设计与数据流 (Architecture & Data Flow)

由于主进程自身的 Stdio 已被 AI 客户端占用，系统内部通过 `tokio::process::Command` 建立第二层 Stdio 管道来驱动 CodeLLDB：

```text
       AI Client (Claude Desktop)
                   │
           (1) Stdio (MCP 协议)
                   │
                   ▼
┌──────────────────────────────────────────────┐
│                Your MCP Server               │
│                                              │
│  ┌─────────────┐            ┌─────────────┐  │
│  │ mcp-protocol│            │debug-bridge │  │
│  └─────────────┘            └─────────────┘  │
│         │                          │         │
│         └───── (内部路由交互) ───────┘         │
│                                              │
│                 ┌─────────────┐              │
│                 │ dap-client  │              │
│                 └─────────────┘              │
└────────────────────────┬─────────────────────┘
                         │
         (2) 重定向后的 Piped Stdio (DAP 协议)
                         │
                         ▼
             CodeLLDB (子进程守护)

```

---

## 3. 项目模块与文件结构 (Project Structure)

项目采用 **Cargo Workspace** 进行模块化开发，确保底层协议解析与上层 AI 业务语义完全解耦：

```text
mcp-codelldb/                      # 工作区根目录
├── Cargo.toml                    # 统一声明成员子项目与依赖版本
├── src/                          # 主程序入口 (CLI 包装层)
│   ├── main.rs                   # 读取配置，装配各子模块并启动主异步循环
│   └── config.rs                 # 本地环境配置 (如本地 codelldb 路径等)
└── crates/                       # 核心 Crate 目录
    ├── mcp-protocol/             # 【模块① MCP交互层】
    │   └── src/                  # 监听系统 Stdio，解析大模型的 Tool Call 请求并回传结果
    ├── dap-codec/                # 【模块② DAP帧编解码层】
    │   └── src/                  # 实现 Tokio Decoder，精准切分 Content-Length 文本帧，规避黏包
    ├── dap-client/               # 【模块③ DAP子进程控制层】
    │   ├── process.rs            # 执行 Command::spawn("codelldb").arg("--stdio") 并捕获管道
    │   └── transport.rs          # 读写 ChildStdin/ChildStdout，处理多路复用请求序号(seq)
    └── debug-bridge/             # 【模块④&⑤ 状态机与业务网桥 - 核心大脑】
        └── src/
            ├── state.rs          # 维护全局调试状态 (Paused/Running/Exited) 及变量句柄映射 Map
            ├── mapping.rs        # 解决 AI 相对路径与系统绝对路径的双向翻译
            └── handlers/         # 复合语义翻译器 (将高层工具拆解为链式 DAP 指令)
                ├── launch.rs     # 引导启动/附加目标 C++ 程序
                ├── breakpoint.rs # 动态断点下发、清除与条件断点控制
                └── inspect.rs    # 堆栈抓取、作用域解析与 C++ 复杂变量展开

```

---

## 4. 关键执行流程 (Execution Flow)

### 4.1 进程拉起与双流初始化 (Initialization)

1. **MCP 激活**：AI Client 启动本服务，通过系统标准 Stdio 完成 MCP 协议的握手认证。
2. **子进程拉起**：`dap-client::process` 模块在后台执行 `codelldb --stdio`，并将系统的 `.stdin()` 和 `.stdout()` 显式配置为 `Stdio::piped()`。
3. **管道移交**：Rust 主进程获取到子进程的 `ChildStdin` 和 `ChildStdout`所有权，移交给异步传输任务（`transport`）。
4. **DAP 握手**：通过管道向 CodeLLDB 发送 DAP `initialize` 请求，协商彼此支持的特性并建立连接。

### 4.2 运行时交互生命周期 (Debug Loop)

以大模型执行“**单步步入（Step Into）并查看当前上下文变量**”为例：

```text
[AI Client]     [MCP Layer]     [Tool Handlers]     [State Machine]     [DAP Client]      [CodeLLDB]
    │                │                 │                   │                 │                │
    │─(1) call_tool─>│                 │                   │                 │                │
    │  ("step_into") │─(2) Dispatch───>│                   │                 │                │
    │                │                 │─(3) Check State──>│                 │                │
    │                │                 │  (Must be Paused) │                 │                │
    │                │                 │                                     │                │
    │                │                 │─(4) Write DAP "stepIn"到管道───────>│                │
    │                │                 │                                     │──(5) Piped────>│
    │                │                 │<─(6) Ack (Command Sent)─────────────│                │
    │<─(7) Tool Ret──│                 │                                     │                │
    │  ("Stepping")  │                 │                                     │                │
    │                │                 │                   ┌─────────────────┤                │
    │                │                 │                   │ (异步管道读到)    │<─(8) Stdout────│
    │                │                 │                   │ (DAP "stopped") │                │
    │                │                 │                   ▼                 │                │
    │                │                 │           [DAP Receiver Loop]       │                │
    │                │                 │                   │                 │                │
    │                │                 │                   │─(9) Update────> │                │
    │                │                 │                   │  (State=Paused) │                │
    │                │                 │                   │  (thread_id=1)  │                │

```

*(注：当状态机感知到 `Paused` 后，大模型会在下一轮交互中连续调用 `inspect_variables`，网桥在内部自动链式发起 `stackTrace` $\rightarrow$ `scopes` $\rightarrow$ `variables` 并合并数据回传。)*

---

## 5. 核心技术痛点与工程设计方案

### 5.1 异步流编解码与黏包处理 (`dap-codec`)

* **痛点**：DAP 协议在 Stdio 传输时格式为 `Content-Length: X\r\n\r\n{JSON_BODY}`。由于管道读取是流式的，可能会发生断包或黏包。
* **方案**：利用 `tokio_util::codec::Decoder` 实现自定义状态机编解码器。在内部维护两个解析状态：
1. `ReadHeader`：通过流式扫描寻找 `Content-Length:` 和双换行符，解析出对应的字节长度 `X`。
2. `ReadBody`：精准从流中切出 `X` 字节的缓冲区，直接投喂给 `serde_json` 进行强类型反序列化。



### 5.2 多路复用与请求同步 (`dap-client`)

* **痛点**：DAP 的读管道是一个单一的异步长连接，但多个工具可能会并发发起查询，且 CodeLLDB 的事件（如停止、打印日志）是随机插播的。
* **方案**：
* 在发送 DAP 请求时，全局累加 `seq`（请求序号）。
* 内部维护一个 `HashMap<i64, tokio::sync::oneshot::Sender<DapResponse>>`。
* 后台常驻一个异步轮询任务专门死循环读取 CodeLLDB 的标准输出。当读到 `Response` 时，根据报文里的 `request_seq` 查表，通过 `oneshot` 频道定向唤醒对应正在等待的业务协程；若读到 `Event`，则直接派发给状态机。



### 5.3 变量引用句柄管理与深度展开 (`state_machine`)

* **痛点**：C++ 结构体或容器（如 `std::vector`）在 DAP 中只会先返回一个全局唯一的 `variablesReference`（整型句柄），不会直接展开其内部数万个子数组成员。
* **方案**：状态机模块除了维护运行状态，还必须提供一个高并发的线程安全哈希表（Cache）。将大模型当前可视的变量名（如 `my_class_obj`）与其底层的 `variablesReference` 绑定。当大模型下一次追问 *“帮我看看 `my_class_obj` 内部的 `ptr` 指针指向的值”* 时，网桥能够通过变量名找到句柄，并发起二次 DAP 查询。

### 5.4 进程组隔离与崩溃防护

* **痛点**：C++ 目标程序调试时常伴随 `SIGSEGV`（段错误）或硬件中断。如果子进程与 Rust 主进程共享进程组，某些信号可能引发连带雪崩，导致整个 MCP 服务随之挂掉。
* **方案**：在 Unix 环境下，利用 Rust 的 `.process_group(0)` 将拉起的 CodeLLDB 及其加载的 C++ 目标程序划分到独立的隔离进程组中。一旦子进程由于崩溃退出，Rust 端的读管道会立即捕捉到 `EOF`，此时通过 `child.try_wait()` 优雅捕获退出状态码，转化为结构化的错误文本安全回传给大模型。

---

## 6. 开发迭代路线 (Milestones)

1. **Phase 1 (管道控制与打通)**：不接 MCP 逻辑。先编写 `dap-codec` 和 `dap-client`，验证 Rust 能够成功唤醒 `codelldb --stdio` 并在读写管道中完成 `initialize` 握手。
2. **Phase 2 (上下文链式组装)**：编写状态机与 `inspect.rs` 处理器。通过手动编写一段死循环测试代码，验证当读到 `stopped` 事件后，网桥能自动完成多级 DAP 查询并打印出完美的 C++ 变量拓扑结构。
3. **Phase 3 (MCP 标准接入)**：引入 `mcp-protocol` 模块。将系统标准 I/O 绑定为标准的 MCP 协议接口，把大模型的 Tool Call 路由至 Phase 2 的处理器。
4. **Phase 4 (真实场景调优)**：引入路径映射算法，处理多线程调试切换，完成实际 C++ 项目的闭环联调。