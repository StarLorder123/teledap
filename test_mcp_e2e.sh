#!/bin/bash
# test_mcp_e2e.sh -- TeleDAP MCP Server end-to-end test
#
# Pipe a sequence of JSON-RPC requests into teledap (which auto-detects
# MCP mode when stdin is not a terminal) and print the responses.
#
# Usage:
#   chmod +x test_mcp_e2e.sh
#   ./test_mcp_e2e.sh
set -e

TELEDAP="cargo run --quiet --"
COUNTER=0
PASSED=0
FAILED=0

send() {
    COUNTER=$((COUNTER + 1))
    echo "{\"jsonrpc\":\"2.0\",\"id\":$COUNTER,\"method\":\"$1\",\"params\":$2}"
}

check_response() {
    # Read one line from stdin; check that it contains a valid JSON-RPC response.
    read -r line
    if echo "$line" | grep -q '"jsonrpc":"2.0"'; then
        PASSED=$((PASSED + 1))
        echo "  PASS: $(echo "$line" | head -c 120)"
    else
        FAILED=$((FAILED + 1))
        echo "  FAIL: unexpected output: $(echo "$line" | head -c 120)"
    fi
}

echo "=== TeleDAP MCP E2E Test ==="
echo ""

# Build the request sequence, pipe into teledap
{
    # 1. initialize
    send "initialize" '{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e-test","version":"1.0"}}'

    # 2. Send initialized notification
    echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'

    # 3. tools/list
    send "tools/list" '{}'

    # 4. tools/call start
    send "tools/call" '{"name":"start","arguments":{"codelldbPath":"codelldb"}}'

    # 5. tools/call initialize
    send "tools/call" '{"name":"initialize","arguments":{}}'

    # 6. tools/call get_state
    send "tools/call" '{"name":"get_state","arguments":{}}'

    # 7. tools/call shutdown
    send "tools/call" '{"name":"shutdown","arguments":{}}'

} | $TELEDAP 2>/dev/null | while read -r line; do
    check_response
done

echo ""
echo "=== E2E test completed: $PASSED passed, $FAILED failed out of $COUNTER requests ==="
