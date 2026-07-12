# test_mcp_e2e.ps1 -- TeleDAP MCP Server end-to-end test (Windows)
#
# Pipe a sequence of JSON-RPC requests into teledap (which auto-detects
# MCP mode when stdin is not a terminal) and print the responses.
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File test_mcp_e2e.ps1
$ErrorActionPreference = "Continue"

Write-Host "=== TeleDAP MCP E2E Test ===" -ForegroundColor Cyan
Write-Host ""

$requests = @(
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e-test","version":"1.0"}}}'
    '{"jsonrpc":"2.0","method":"notifications/initialized"}'
    '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
    '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_state","arguments":{}}}'
)

$input = $requests -join "`n"
$output = $input | cargo run --quiet 2>$null

$pass = 0
$fail = 0
foreach ($line in $output) {
    if ($line -match '"jsonrpc":"2.0"') {
        $pass++
        $preview = if ($line.Length -gt 120) { $line.Substring(0, 120) } else { $line }
        Write-Host "  PASS: $preview" -ForegroundColor Green
    } else {
        $fail++
        $preview = if ($line.Length -gt 120) { $line.Substring(0, 120) } else { $line }
        Write-Host "  FAIL: unexpected output: $preview" -ForegroundColor Red
    }
}

Write-Host ""
Write-Host "=== E2E test completed: $pass passed, $fail failed ===" -ForegroundColor Cyan
