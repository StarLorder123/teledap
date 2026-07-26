# TeleDAP System Prompt Samples

This directory contains ready-to-use system prompt templates for AI Agents integrating with TeleDAP.

## Files

| File | Language | Scenario |
|---|---|---|
| [`system-prompt.md`](./system-prompt.md) | 中文 | General-purpose prompt for stdio or HTTP/SSE agents |
| [`claude-desktop-prompt.md`](./claude-desktop-prompt.md) | 中文 | Optimized for Claude Desktop / Claude Code (stdio child process) |
| [`system-prompt-en.md`](./system-prompt-en.md) | English | General-purpose prompt for stdio or HTTP/SSE agents |
| [`claude-desktop-prompt-en.md`](./claude-desktop-prompt-en.md) | English | Optimized for Claude Desktop / Claude Code (stdio child process) |

## How to Use

1. Pick the file matching your AI client language and transport mode.
2. Copy its entire content into the AI Agent's system prompt field.
3. Replace the **User Project Info** / **用户工程信息** section with your actual project paths:
   - Project root directory
   - codelldb / GDB adapter absolute path
   - ELF binary absolute path
   - (Optional) `liblldb.dll` path on Windows
   - (Optional) OpenOCD path and config files for embedded debugging

## Integration Guides

For full setup instructions, see:

- [`docs/AI_Agent_集成指南.md`](../AI_Agent_集成指南.md) — 中文指南
- [`docs/AI_Agent_Integration_Guide.md`](../AI_Agent_Integration_Guide.md) — English guide
