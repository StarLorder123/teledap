# TeleDAP 后续 Roadmap & 行动清单

> 本文件汇总 TeleDAP 当前基线、后续阶段规划、优先级建议以及维护者/用户需要执行的具体任务。  
> 配套详细实施计划见：
> - `todo/phase4_plan.md` —— 嵌入式硬件一级调试能力
> - `todo/phase5_plan.md` —— 高级调试能力

---

## 1. 当前基线（As of 2026-07-26）

| 维度 | 现状 |
|---|---|
| 已实现阶段 | Phases 1–3 完成 |
| MCP tools | 30 个（lifecycle 6、execution 5、breakpoint 3、introspection 8、utility 3、OpenOCD 5） |
| 传输层 | MCP stdio ✅、MCP HTTP/SSE ✅ |
| 调试后端 | codelldb、GDB DAP、OpenOCD 进程管理 |
| 测试 | 196 个测试通过；PC 端 codelldb 集成测试已覆盖；OpenOCD/硬件/多客户端压测缺失 |
| 文档 | README（中英）、AI Agent 集成指南、系统提示模板、架构文档、推广计划、测试报告 |

---

## 2. 阶段规划

### Phase 4：嵌入式硬件一级调试能力

**核心目标**：让 AI 不用记 raw Tcl 命令，就能做嵌入式调试。

| # | 任务 | 产出 |
|---|---|---|
| 4.1 | OpenOCD 高层 API：`read_registers`/`write_register` | `crates/openocd-client/src/client.rs` 新增方法 |
| 4.2 | OpenOCD 高层 API：`read_memory`/`write_memory` | 内存按 byte/halfword/word/doubleword 访问 |
| 4.3 | OpenOCD 高层 API：`flash_image` / `reset_target` | 一键烧录和复位 |
| 4.4 | 新增 6 个 MCP tools + handlers | `crates/debug-bridge/src/tools.rs`、`handlers/openocd.rs` |
| 4.5 | 组合工作流 `flash_and_debug` | 启动 OpenOCD → 烧录 → DAP remote debug |
| 4.6 | OpenOCD 集成测试 | `crates/debug-bridge/tests/openocd_integration_test.rs` |
| 4.7 | 文档更新 | README、AI Agent 集成指南、prompts |

**详细计划**：`todo/phase4_plan.md`

---

### Phase 5：高级调试能力

**核心目标**：支持复杂程序调试（watchpoints、反汇编、多线程/RTOS、持久 watch expressions、trace replay）。

| # | 任务 | 产出 |
|---|---|---|
| 5.1 | 扩展 DAP types：`setDataBreakpoints`、`disassemble` | `crates/dap-types/src/requests.rs`、`types.rs` |
| 5.2 | Watchpoint 工具 | `set_data_breakpoints` / `set_watchpoint` |
| 5.3 | 反汇编工具 | `disassemble` |
| 5.4 | 持久 watch expressions | `add/remove/list_watch_expressions`，stopped 自动重求值 |
| 5.5 | 多线程 / RTOS 增强 | `get_threads(detail)`、线程级断点 |
| 5.6 | Trace 回放 | `load_trace`、`replay_next`、`replay_to_event` |
| 5.7 | 测试与文档 | 单元测试、集成测试、README/指南更新 |

**详细计划**：`todo/phase5_plan.md`

---

### Phase 6：产品化与推广

| # | 任务 | 说明 |
|---|---|---|
| 6.1 | Release 二进制 | GitHub Actions 构建 Windows/Linux/macOS |
| 6.2 | 包管理器 | `cargo install` 验证；Homebrew/Scoop/choco |
| 6.3 | HTTP/SSE 安全 | TLS + token（如需要远程接入） |
| 6.4 | VS Code 扩展 / Web UI | 可视化前端 |
| 6.5 | 更多 AI 客户端兼容 | Cursor、Windsurf、Cline 测试 |
| 6.6 | 推广内容 | 博客、演示视频、社区帖子 |

### Phase 7：长期工程化

- 多客户端 SSE 压测与并发修复
- GDB DAP adapter 集成测试
- 真实硬件 nightly 回归
- SVD/外设寄存器解码（可选）
- SWO/ITM trace 接入（可选）

---

## 3. 优先级建议

| 优先级 | 阶段 | 理由 |
|---|---|---|
| **P0** | Phase 4.1–4.3（寄存器/内存/flash） | 嵌入式用户刚需，`openocd_send` 体验差 |
| **P1** | Phase 4.5（`flash_and_debug` 组合工作流） | “AI 一句话调试”的核心卖点 |
| **P2** | Phase 5.1–5.3（watchpoints / 反汇编 / 多线程） | 扩大复杂调试场景受众 |
| **P3** | Phase 6.1–6.4（release/包管理器/安全/前端） | 降低非 Rust 用户使用门槛 |
| **P4** | Phase 5.4–5.5 / Phase 7 | 锦上添花与长期维护 |

---

## 4. 维护者（你）需要做的事

### 4.1 作为 AI 用户先体验自己的工具（强烈推荐先做）

这是最有价值的一步。流程：

```powershell
# 1. 编译 release
cargo build --release

# 2. 写包装脚本 run_teledap.ps1
$env:CODE_LLDB_PATH = "C:\tools\codelldb\extension\adapter\codelldb.exe"
$env:PROJECT_ROOT   = "C:\projects\myapp"
E:\Code\cpp\teledap\target\release\teledap.exe
```

```json
// 3. 配置 Claude Desktop
{
  "mcpServers": {
    "teledap": {
      "command": "powershell",
      "args": ["-ExecutionPolicy", "Bypass", "-File", "C:\\tools\\run_teledap.ps1"]
    }
  }
}
```

```text
// 4. 对话里让 AI 按顺序执行
register_base_dir -> start -> initialize -> launch -> configuration_done
set_breakpoints -> continue -> get_stack_trace -> evaluate -> step_over
```

详细模板见 `docs/AI_Agent_集成指南.md` 和 `docs/prompts/claude-desktop-prompt.md`。

### 4.2 提供目标硬件信息（想做 Phase 4 时需要）

- 目标芯片型号（如 STM32F103、ESP32-S3、RP2040）
- 调试器（J-Link / ST-Link / DAP-Link / CMSIS-DAP）
- OpenOCD 配置脚本路径
- 一个可编译的最小 .elf 工程

### 4.3 拍板几个关键决策

| 决策 | 选项 | 影响 |
|---|---|---|
| Phase 4 先做哪个？ | A. 寄存器+内存；B. flash 烧录；C. 联合工作流 | 决定下一个 PR 范围 |
| HTTP/SSE 安全？ | 本地用可不做；远程用必须加 TLS + token | 影响 Phase 6 计划 |
| 是否做 VS Code 扩展？ | 是 / 否 / 远期 | 需要前端/TS 技能 |
| 测试策略？ | A. 买块板子 nightly；B. OpenOCD 模拟器；C. 仅手动 | 影响 CI 设计 |

### 4.4 推广与社区

- 写 1 篇中文博客：《用 Claude 远程调试 STM32》
- 录 1 个 2 分钟演示：AI 对话里完成 `set breakpoint → continue → inspect variable`
- 在 README 加 AI 使用示例截图

---

## 5. 本周最小可行动作

如果只能做一件：**把 release 二进制配进 Claude Desktop，用 AI 对话跑通一次完整调试会话。**

预计 30 分钟，收益：

- 发现当前工具命名/返回格式里不自然的地方；
- 确定 Phase 4 该先做哪个功能；
- 拿到真实截图/日志用于推广。

---

## 6. 我可以立即帮你做的下一步

告诉我选哪个，我可以马上开工：

1. **实现 Phase 4.1/4.2**：`read_registers`、`read_memory`、`write_memory` tool + OpenOCD handler + 测试。
2. **生成你的 Claude Desktop 配置**：根据你的 `codelldb` 路径和工程路径写 `run_teledap.ps1` 和配置片段。
3. **跑通一个真实调试会话**：用 `test_debuggee` 或你的 ELF 走一遍 lifecycle，输出日志和修复点。
4. **设计 Phase 4.5 `flash_and_debug`**：接口、状态机、错误回滚策略。

---

## 7. 决策点（需要你回答）

1. 你更想先推进 **Phase 4（嵌入式硬件）** 还是 **Phase 5（高级调试）**？
2. 你是否有真实目标板/芯片？如果有，型号和调试器是什么？
3. 你想先让我帮你 **生成 Claude Desktop 配置并跑通一次调试** 吗？
