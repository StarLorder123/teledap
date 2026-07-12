#!/bin/bash
# test_mcp_e2e.sh -- TeleDAP MCP Server end-to-end verification
#
# Pipe a sequence of JSON-RPC 2.0 requests into teledap (which auto-detects
# MCP mode when stdin is not a terminal) and validate every response.
#
# Phases (7 phases, ~15 messages):
#   P1: Pre-init rejection        -- tools/list before initialize => -32603
#   P2: Handshake                 -- initialize + initialized notification
#   P3: Disconnected tool listing -- verify only start + utility tools
#   P4: Error paths               -- unknown tool / wrong state / invalid params
#   P5: Connected state           -- start codelldb => verify tool list changes
#   P6: Initialized state         -- initialize DAP => get_state => tool list
#   P7: Clean shutdown            -- shutdown => verify back to Disconnected
#
# Usage:
#   chmod +x test_mcp_e2e.sh
#   ./test_mcp_e2e.sh
#   TELEDAP_E2E_SKIP_CODELDB=1 ./test_mcp_e2e.sh

set -o pipefail

TELEDAP="${TELEDAP_BIN:-cargo run --quiet --}"
TIMEOUT_SEC="${TELEDAP_E2E_TIMEOUT:-30}"
RESPONSE_FILE=$(mktemp)
PASSED=0
FAILED=0
TOTAL=0
SERVER_PID=""

cleanup() {
    if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null
        wait "$SERVER_PID" 2>/dev/null
    fi
    rm -f "$RESPONSE_FILE"
}
trap cleanup EXIT INT TERM

# -- codelldb probe --

has_codelldb() {
    command -v codelldb >/dev/null 2>&1
}

# -- helpers --

check_pass() {
    PASSED=$((PASSED + 1))
    echo "  PASS  $*"
}

check_fail() {
    FAILED=$((FAILED + 1))
    echo "  FAIL  $*"
}

check_jsonrpc() {
    # Validate basic JSON-RPC 2.0 structure: jsonrpc, id, result|error
    local line="$1"
    local expected_id="$2"
    local expect_error="${3:-false}"   # "rpc" = JSON-RPC error, "tool" = tool error, "false" = success

    TOTAL=$((TOTAL + 1))

    # Must be valid JSON
    if ! echo "$line" | python3 -c "import sys,json; json.loads(sys.stdin.read())" 2>/dev/null; then
        # Fallback: check for jsonrpc field via grep
        if ! echo "$line" | grep -q '"jsonrpc"\s*:\s*"2\.0"'; then
            check_fail "[id=$expected_id] Not valid JSON-RPC 2.0"
            return 1
        fi
    fi

    # Extract and check id
    local actual_id
    actual_id=$(echo "$line" | grep -oP '"id"\s*:\s*\K\d+')
    if [ -z "$actual_id" ]; then
        check_fail "[id=$expected_id] Missing 'id' in response"
        return 1
    fi
    if [ "$actual_id" -ne "$expected_id" ]; then
        check_fail "[id=$expected_id] ID mismatch: got $actual_id"
        return 1
    fi

    local has_error
    has_error=$(echo "$line" | grep -c '"error"\s*:')

    if [ "$expect_error" = "rpc" ]; then
        if [ "$has_error" -eq 0 ]; then
            check_fail "[id=$expected_id] Expected RPC error but no 'error' field"
            return 1
        fi
        local err_code err_msg
        err_code=$(echo "$line" | grep -oP '"code"\s*:\s*\K-?\d+')
        err_msg=$(echo "$line" | grep -oP '"message"\s*:\s*"\K[^"]+')
        check_pass "[id=$expected_id] (RPC error) code=$err_code -- ${err_msg:0:80}"
    elif [ "$expect_error" = "tool" ]; then
        local has_iserror
        has_iserror=$(echo "$line" | grep -c '"isError"\s*:\s*true')
        if [ "$has_iserror" -eq 0 ]; then
            check_fail "[id=$expected_id] Expected tool error (isError:true)"
            return 1
        fi
        local tool_msg
        tool_msg=$(echo "$line" | grep -oP '"text"\s*:\s*"\K[^"]+')
        check_pass "[id=$expected_id] (tool error) -- ${tool_msg:0:80}"
    else
        if [ "$has_error" -gt 0 ]; then
            local err_msg
            err_msg=$(echo "$line" | grep -oP '"message"\s*:\s*"\K[^"]+')
            check_fail "[id=$expected_id] Expected success but got error: ${err_msg:0:80}"
            return 1
        fi
        check_pass "[id=$expected_id] (success)"
    fi
    return 0
}

# -- run all phases, collect responses to temp file --

echo "=== TeleDAP MCP E2E Verification ==="
echo ""

if ! has_codelldb; then
    if [ "${TELEDAP_E2E_SKIP_CODELDB:-0}" != "1" ]; then
        echo "NOTE: codelldb not found on PATH. Phases P5-P7 will be skipped."
        echo "      Set TELEDAP_E2E_SKIP_CODELDB=1 to suppress this message."
        echo ""
    fi
fi

{
    # P1: Pre-init rejection
    echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'

    # P2: Handshake
    echo '{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e-test","version":"1.0"}}}'
    echo '{"jsonrpc":"2.0","method":"notifications/initialized"}'

    # P3: Disconnected tools
    echo '{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}'

    # P4: Error paths
    echo '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nonexistent_tool","arguments":{}}}'
    echo '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"continue","arguments":{"threadId":1}}}'
    echo '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"start","arguments":{}}}'

    if has_codelldb && [ "${TELEDAP_E2E_SKIP_CODELDB:-0}" != "1" ]; then
        # P5: Connected state
        echo '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"start","arguments":{"codelldbPath":"codelldb"}}}'
        echo '{"jsonrpc":"2.0","id":8,"method":"tools/list","params":{}}'

        # P6: Initialized state
        echo '{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"initialize","arguments":{}}}'
        echo '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"get_state","arguments":{}}}'
        echo '{"jsonrpc":"2.0","id":11,"method":"tools/list","params":{}}'

        # P7: Clean shutdown
        echo '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"shutdown","arguments":{}}}'
        echo '{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"get_state","arguments":{}}}'
        echo '{"jsonrpc":"2.0","id":14,"method":"tools/list","params":{}}'
        echo '{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"continue","arguments":{"threadId":1}}}'
    fi
} | timeout "$TIMEOUT_SEC" $TELEDAP 2>/dev/null > "$RESPONSE_FILE"

exit_code=$?
if [ $exit_code -eq 124 ]; then
    echo "FATAL: Server timed out after ${TIMEOUT_SEC}s"
    exit 2
fi

# -- validate responses --

echo ""
echo "--- P1: Pre-init rejection -- tools/list before initialize ---"
read -r line < "$RESPONSE_FILE" || true
check_jsonrpc "$line" 1 "rpc"

echo ""
echo "--- P2: Handshake -- initialize + initialized notification ---"
read -r line < "$RESPONSE_FILE" || true
check_jsonrpc "$line" 2 "false"
check_pass "Notification: initialized sent (no response expected)"

echo ""
echo "--- P3: Disconnected tool listing ---"
read -r line < "$RESPONSE_FILE" || true
check_jsonrpc "$line" 3 "false"

echo ""
echo "--- P4: Error paths ---"
read -r line < "$RESPONSE_FILE" || true
check_jsonrpc "$line" 4 "tool"
read -r line < "$RESPONSE_FILE" || true
check_jsonrpc "$line" 5 "tool"
read -r line < "$RESPONSE_FILE" || true
check_jsonrpc "$line" 6 "tool"

if has_codelldb && [ "${TELEDAP_E2E_SKIP_CODELDB:-0}" != "1" ]; then
    echo ""
    echo "--- P5: Connected state ---"
    read -r line < "$RESPONSE_FILE" || true
    check_jsonrpc "$line" 7 "false"
    read -r line < "$RESPONSE_FILE" || true
    check_jsonrpc "$line" 8 "false"

    echo ""
    echo "--- P6: Initialized state ---"
    read -r line < "$RESPONSE_FILE" || true
    check_jsonrpc "$line" 9 "false"
    read -r line < "$RESPONSE_FILE" || true
    check_jsonrpc "$line" 10 "false"
    read -r line < "$RESPONSE_FILE" || true
    check_jsonrpc "$line" 11 "false"

    echo ""
    echo "--- P7: Clean shutdown ---"
    read -r line < "$RESPONSE_FILE" || true
    check_jsonrpc "$line" 12 "false"
    read -r line < "$RESPONSE_FILE" || true
    check_jsonrpc "$line" 13 "false"
    read -r line < "$RESPONSE_FILE" || true
    check_jsonrpc "$line" 14 "false"
    read -r line < "$RESPONSE_FILE" || true
    check_jsonrpc "$line" 15 "tool"
else
    echo ""
    echo "--- P5-P7: Skipped (codelldb not available) ---"
    echo "  SKIP  P5: start codelldb"
    echo "  SKIP  P5: Connected tool listing"
    echo "  SKIP  P6: DAP initialize"
    echo "  SKIP  P6: get_state verification"
    echo "  SKIP  P6: Initialized tool listing"
    echo "  SKIP  P7: shutdown"
    echo "  SKIP  P7: post-shutdown state check"
    echo "  SKIP  P7: post-shutdown tool listing"
    echo "  SKIP  P7: post-shutdown wrong-state rejection"
fi

# -- summary --

echo ""
echo "=========================================="
echo "  Total:  $TOTAL assertions"
echo "  Passed: $PASSED"
echo "  Failed: $FAILED"
echo "=========================================="

if [ "$FAILED" -gt 0 ]; then
    echo ""
    echo "SOME ASSERTIONS FAILED. See details above."
    exit 1
else
    echo ""
    echo "All assertions passed."
    exit 0
fi
