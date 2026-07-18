# test_mcp_e2e.ps1 -- TeleDAP MCP Server end-to-end verification
#
# Pipes a sequence of JSON-RPC 2.0 requests into teledap (which auto-detects
# MCP mode when stdin is not a terminal) and validates every response against
# the MCP/JSON-RPC 2.0 spec.
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
#   powershell -ExecutionPolicy Bypass -File test_mcp_e2e.ps1
#   powershell -ExecutionPolicy Bypass -File test_mcp_e2e.ps1 -SkipCodelldb

param(
    [switch]$SkipCodelldb,
    [switch]$VerboseOutput,
    [int]$StartTimeoutSec = 15,
    [int]$DefaultTimeoutSec = 8
)

$ErrorActionPreference = "Stop"
$script:Passed = 0
$script:Failed = 0
$script:Phase = ""

# -- Color helpers ----------------------------------------------------------

function Write-PhaseHeader([string]$Title) {
    $script:Phase = $Title
    Write-Host ""
    Write-Host ("--- " + $Title + " ---") -ForegroundColor Cyan
}

function Write-Pass([string]$Detail) {
    $script:Passed++
    Write-Host ("  PASS  " + $Detail) -ForegroundColor Green
}

function Write-Fail([string]$Detail) {
    $script:Failed++
    Write-Host ("  FAIL  " + $Detail) -ForegroundColor Red
}

function Write-Skip([string]$Detail) {
    Write-Host ("  SKIP  " + $Detail) -ForegroundColor Yellow
}

function Truncate-String([string]$S, [int]$Len = 120) {
    if ($S.Length -le $Len) { return $S }
    return $S.Substring(0, $Len) + "..."
}

# -- JSON-RPC validation ----------------------------------------------------

function Assert-JsonRpcResponse {
    param(
        [Parameter(Mandatory=$true)]
        [string]$Response,
        [Parameter(Mandatory=$true)]
        [uint64]$ExpectedId,
        [switch]$ExpectRpcError,
        [switch]$ExpectToolError,
        [scriptblock[]]$Validations = @()
    )

    $label = ""
    if ($ExpectRpcError) { $label = "RPC error" }
    elseif ($ExpectToolError) { $label = "tool error" }
    else { $label = "success" }

    # Parse as JSON
    try {
        $obj = $Response | ConvertFrom-Json
    } catch {
        Write-Fail ("[id=" + $ExpectedId + "] Response is not valid JSON: " + (Truncate-String $Response))
        return
    }

    # 1. jsonrpc field
    if ($obj.jsonrpc -ne "2.0") {
        Write-Fail ("[id=" + $ExpectedId + "] Missing or incorrect jsonrpc")
        return
    }

    # 2. id field
    if ($obj.id -eq $null) {
        Write-Fail ("[id=" + $ExpectedId + "] Missing 'id' field in response")
        return
    }
    $responseId = [uint64]$obj.id
    if ($responseId -ne $ExpectedId) {
        Write-Fail ("[id=" + $ExpectedId + "] ID mismatch: got " + $responseId)
        return
    }

    # 3. RPC error vs success discrimination
    if ($ExpectRpcError) {
        if (Get-Member -InputObject $obj -Name "result" -MemberType Properties) {
            Write-Fail ("[id=" + $ExpectedId + "] Expected RPC error but got 'result' field")
            return
        }
        if (-not (Get-Member -InputObject $obj -Name "error" -MemberType Properties)) {
            Write-Fail ("[id=" + $ExpectedId + "] Expected RPC error but no 'error' field")
            return
        }
        if ($obj.error.code -eq $null -or $obj.error.message -eq $null) {
            Write-Fail ("[id=" + $ExpectedId + "] RPC error missing code or message")
            return
        }
        $codeStr = "code=" + $obj.error.code
        $msgPreview = Truncate-String $obj.error.message 80
        Write-Pass ("[id=" + $ExpectedId + "] (" + $label + ") " + $codeStr + " -- " + $msgPreview)
    }
    elseif ($ExpectToolError) {
        if (-not (Get-Member -InputObject $obj -Name "result" -MemberType Properties)) {
            Write-Fail ("[id=" + $ExpectedId + "] Expected tool error result but no 'result' field")
            return
        }
        $r = $obj.result
        if (-not $r.isError) {
            Write-Fail ("[id=" + $ExpectedId + "] Expected isError:true")
            return
        }
        if ($r.content.Length -eq 0 -or $r.content[0].type -ne "text") {
            Write-Fail ("[id=" + $ExpectedId + "] Tool error result missing content[0].type='text'")
            return
        }
        $msgPreview = Truncate-String $r.content[0].text 80
        Write-Pass ("[id=" + $ExpectedId + "] (" + $label + ") -- " + $msgPreview)
    }
    else {
        if (-not (Get-Member -InputObject $obj -Name "result" -MemberType Properties)) {
            Write-Fail ("[id=" + $ExpectedId + "] Expected success but no 'result' field")
            return
        }
        if (Get-Member -InputObject $obj -Name "error" -MemberType Properties) {
            Write-Fail ("[id=" + $ExpectedId + "] Expected success but got 'error': " + $obj.error.message)
            return
        }
        Write-Pass ("[id=" + $ExpectedId + "] (" + $label + ")")
    }

    # 4. Run additional custom validations
    foreach ($v in $Validations) {
        try {
            $null = & $v $obj
        } catch {
            Write-Fail ("[id=" + $ExpectedId + "] Assertion failed: " + $_)
        }
    }
}

# -- Process management -----------------------------------------------------

function Send-McpMessage {
    param(
        [System.Diagnostics.Process]$Process,
        [string]$Json,
        [int]$TimeoutSec = $DefaultTimeoutSec
    )

    # Write the message line
    $Process.StandardInput.WriteLine($Json)
    $Process.StandardInput.Flush()

    if ($script:VerboseOutput) {
        $preview = if ($Json.Length -gt 200) { $Json.Substring(0, 200) + "..." } else { $Json }
        Write-Host ("  >> " + $preview) -ForegroundColor DarkCyan
    }

    # Read response with timeout
    $readTask = $Process.StandardOutput.ReadLineAsync()
    if (-not $readTask.Wait($TimeoutSec * 1000)) {
        throw ("Timeout after " + $TimeoutSec + "s waiting for response")
    }
    $line = $readTask.Result
    if ($null -eq $line) {
        throw "Server closed stdout (EOF) while waiting for response"
    }

    if ($script:VerboseOutput) {
        $preview = if ($line.Length -gt 300) { $line.Substring(0, 300) + "..." } else { $line }
        Write-Host ("  << " + $preview) -ForegroundColor DarkMagenta
    }

    return $line
}

function Start-TeleDAP {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = "cargo"
    $psi.Arguments = "run --quiet"
    $psi.WorkingDirectory = $PSScriptRoot
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.StandardOutputEncoding = [System.Text.Encoding]::UTF8
    $psi.StandardErrorEncoding = [System.Text.Encoding]::UTF8

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi

    # Register stderr consumer (background) to prevent buffer deadlock
    $stderrQueue = New-Object System.Collections.Concurrent.ConcurrentQueue[string]
    $null = Register-ObjectEvent -InputObject $proc -EventName ErrorDataReceived -Action {
        if ($EventArgs.Data -ne $null) {
            $Event.MessageData.TryAdd($EventArgs.Data)
        }
    } -MessageData $stderrQueue | Out-Null

    if (-not $proc.Start()) {
        throw "Failed to start cargo run"
    }
    $proc.BeginErrorReadLine()

    Write-Host ("Started teledap (PID " + $proc.Id + ")") -ForegroundColor DarkGray
    return @{
        Process = $proc
        Stderr  = $stderrQueue
    }
}

function Stop-TeleDAP {
    param($Server)

    $proc = $Server.Process
    if ($proc -and -not $proc.HasExited) {
        try {
            Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
        } catch { }
        if (-not $proc.HasExited) {
            $proc.Kill()
        }
        $proc.WaitForExit(3000) | Out-Null
        Write-Host "Stopped teledap." -ForegroundColor DarkGray
    }
    $proc.Dispose()
}

# -- codelldb probe ---------------------------------------------------------

function Test-CodelldbAvailable {
    $codelldb = Get-Command codelldb -ErrorAction SilentlyContinue
    if ($codelldb) {
        Write-Host ("codelldb found: " + $codelldb.Source) -ForegroundColor DarkGray
        return $true
    }
    return $false
}

# -- JSON message builders --------------------------------------------------

$msg_initialize = '{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e-test","version":"1.0"}}}'
$msg_initialized = '{"jsonrpc":"2.0","method":"notifications/initialized"}'
$msg_tools_list_1 = '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}'
$msg_tools_list_3 = '{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}'
$msg_tools_list_8 = '{"jsonrpc":"2.0","id":8,"method":"tools/list","params":{}}'
$msg_tools_list_11 = '{"jsonrpc":"2.0","id":11,"method":"tools/list","params":{}}'
$msg_tools_list_14 = '{"jsonrpc":"2.0","id":14,"method":"tools/list","params":{}}'
$msg_tool_call_4 = '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nonexistent_tool","arguments":{}}}'
$msg_tool_call_5 = '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"continue","arguments":{"threadId":1}}}'
$msg_tool_call_6 = '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"start","arguments":{}}}'
$msg_tool_call_7 = '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"start","arguments":{"adapterPath":"codelldb"}}}'
$msg_tool_call_9 = '{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"initialize","arguments":{}}}'
$msg_tool_call_10 = '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"get_state","arguments":{}}}'
$msg_tool_call_12 = '{"jsonrpc":"2.0","id":12,"method":"tools/call","params":{"name":"shutdown","arguments":{}}}'
$msg_tool_call_13 = '{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"get_state","arguments":{}}}'
$msg_tool_call_15 = '{"jsonrpc":"2.0","id":15,"method":"tools/call","params":{"name":"continue","arguments":{"threadId":1}}}'

# -- Main -------------------------------------------------------------------

Write-Host "TeleDAP MCP E2E Verification" -ForegroundColor Cyan
Write-Host ""

$hasCodelldb = Test-CodelldbAvailable
if (-not $hasCodelldb -and -not $SkipCodelldb) {
    Write-Host "NOTE: codelldb not found on PATH. Phases P5-P7 will be skipped." -ForegroundColor Yellow
    Write-Host ""
}
if ($SkipCodelldb) {
    Write-Host "NOTE: -SkipCodelldb set. Only protocol-level phases (P1-P4) will run." -ForegroundColor Yellow
    Write-Host ""
}

$server = $null

try {
    $server = Start-TeleDAP

    # Give the server a moment to start
    Start-Sleep -Milliseconds 500

    # =======================================================================
    # P1: Pre-initialization rejection
    # =======================================================================
    Write-PhaseHeader "P1: Pre-init rejection -- tools/list before initialize must fail"

    $r = Send-McpMessage -Process $server.Process -Json $msg_tools_list_1
    Assert-JsonRpcResponse -Response $r -ExpectedId 1 -ExpectRpcError -Validations @(
        { param($o) if ($o.error.code -ne -32603) { throw ("Expected code -32603, got " + $o.error.code) } }
        { param($o) if ($o.error.message -notmatch "not initialized") { throw ("Expected 'not initialized' in message, got: " + $o.error.message) } }
    )

    # =======================================================================
    # P2: Handshake
    # =======================================================================
    Write-PhaseHeader "P2: Handshake -- initialize + initialized notification"

    $r = Send-McpMessage -Process $server.Process -Json $msg_initialize
    Assert-JsonRpcResponse -Response $r -ExpectedId 2 -Validations @(
        { param($o) if (-not $o.result.protocolVersion) { throw "Missing protocolVersion" } }
        { param($o) if (-not $o.result.capabilities) { throw "Missing capabilities" } }
        { param($o) if (-not $o.result.capabilities.tools) { throw "Missing capabilities.tools" } }
        { param($o) if ($o.result.serverInfo.name -ne "teleDAP") { throw ("Expected server name 'teleDAP', got '" + $o.result.serverInfo.name + "'") } }
    )

    # Send initialized notification (no response expected)
    $server.Process.StandardInput.WriteLine($msg_initialized)
    $server.Process.StandardInput.Flush()
    Write-Pass "Notification: initialized sent (no response expected)"

    # =======================================================================
    # P3: Disconnected-state tool listing
    # =======================================================================
    Write-PhaseHeader "P3: Disconnected tool listing -- only start + utility tools"

    $r = Send-McpMessage -Process $server.Process -Json $msg_tools_list_3
    Assert-JsonRpcResponse -Response $r -ExpectedId 3 -Validations @(
        { param($o)
            $names = $o.result.tools | ForEach-Object { $_.name }
            if ("start" -notin $names) { throw "'start' missing from tools" }
            if ("get_state" -notin $names) { throw "'get_state' missing" }
            if ("register_path_alias" -notin $names) { throw "'register_path_alias' missing" }
            if ("register_base_dir" -notin $names) { throw "'register_base_dir' missing" }
            if ("search_variables" -notin $names) { throw "'search_variables' missing" }
            if ("initialize" -in $names) { throw "'initialize' should NOT be in Disconnected tools" }
            if ("continue" -in $names) { throw "'continue' should NOT be in Disconnected tools" }
            if ("get_threads" -in $names) { throw "'get_threads' should NOT be in Disconnected tools" }
            if ("launch" -in $names) { throw "'launch' should NOT be in Disconnected tools" }
            if ("pause" -in $names) { throw "'pause' should NOT be in Disconnected tools" }
            Write-Pass ("  [P3] Disconnected tools: " + $names.Count + " tools, correct set")
        }
    )

    # =======================================================================
    # P4: Error paths
    # =======================================================================
    Write-PhaseHeader "P4: Error paths -- unknown tool / wrong state / invalid params"

    # 4a: Unknown tool
    $r = Send-McpMessage -Process $server.Process -Json $msg_tool_call_4
    Assert-JsonRpcResponse -Response $r -ExpectedId 4 -ExpectToolError -Validations @(
        { param($o) if ($o.result.content[0].text -notmatch "Unknown tool") { throw ("Expected 'Unknown tool', got: " + $o.result.content[0].text) } }
    )

    # 4b: Wrong state -- "continue" requires Halted, session is Disconnected
    $r = Send-McpMessage -Process $server.Process -Json $msg_tool_call_5
    Assert-JsonRpcResponse -Response $r -ExpectedId 5 -ExpectToolError -Validations @(
        { param($o)
            $msg = $o.result.content[0].text
            if ($msg -notmatch "continue") { throw ("Error should mention 'continue': " + $msg) }
        }
    )

    # 4c: Invalid params -- "start" requires adapterPath
    $r = Send-McpMessage -Process $server.Process -Json $msg_tool_call_6
    Assert-JsonRpcResponse -Response $r -ExpectedId 6 -ExpectToolError -Validations @(
        { param($o)
            $msg = $o.result.content[0].text
            if ($msg -notmatch "start" -and $msg -notmatch "adapterPath" -and $msg -notmatch "Invalid parameters") {
                throw ("Error should mention parameter issue: " + $msg)
            }
        }
    )

    # =======================================================================
    # P5-P7 require codelldb
    # =======================================================================

    if ($hasCodelldb -and -not $SkipCodelldb) {

        # ===================================================================
        # P5: Connected state -- start codelldb
        # ===================================================================
        Write-PhaseHeader "P5: Connected state -- start codelldb"

        $r = Send-McpMessage -Process $server.Process -TimeoutSec $StartTimeoutSec -Json $msg_tool_call_7
        Assert-JsonRpcResponse -Response $r -ExpectedId 7 -Validations @(
            { param($o) if ($o.result.isError) { throw ("start should succeed, got error: " + $o.result.content[0].text) } }
        )

        # Verify tool list changed: "initialize" now available, "start" gone
        $r = Send-McpMessage -Process $server.Process -Json $msg_tools_list_8
        Assert-JsonRpcResponse -Response $r -ExpectedId 8 -Validations @(
            { param($o)
                $names = $o.result.tools | ForEach-Object { $_.name }
                if ("initialize" -notin $names) { throw "'initialize' should be present in Connected state" }
                if ("shutdown" -notin $names) { throw "'shutdown' should be present in Connected state" }
                if ("start" -in $names) { throw "'start' should NOT be present after start()" }
                if ("continue" -in $names) { throw "'continue' should NOT be in Connected state" }
                Write-Pass ("  [P5] Connected tools: " + $names.Count + " tools, 'initialize' present, 'start' absent")
            }
        )

        # ===================================================================
        # P6: Initialized state -- DAP handshake
        # ===================================================================
        Write-PhaseHeader "P6: Initialized state -- DAP initialize"

        $r = Send-McpMessage -Process $server.Process -TimeoutSec $StartTimeoutSec -Json $msg_tool_call_9
        Assert-JsonRpcResponse -Response $r -ExpectedId 9 -Validations @(
            { param($o) if ($o.result.isError) { throw ("initialize should succeed, got error: " + $o.result.content[0].text) } }
            { param($o) if ($o.result.content[0].text -notmatch "[{,]") { throw "Response should contain DAP capabilities JSON object" } }
        )

        # get_state -- verify session is Initialized
        $r = Send-McpMessage -Process $server.Process -Json $msg_tool_call_10
        Assert-JsonRpcResponse -Response $r -ExpectedId 10 -Validations @(
            { param($o)
                if ($o.result.isError) { throw ("get_state should succeed, got: " + $o.result.content[0].text) }
                $innerText = $o.result.content[0].text
                try {
                    $stateInfo = $innerText | ConvertFrom-Json
                    if ($stateInfo.state -ne "Initialized") {
                        throw ("Expected state 'Initialized', got '" + $stateInfo.state + "'")
                    }
                    Write-Pass ("  [P6] State confirmed: " + $stateInfo.state)
                } catch {
                    throw ("Failed to parse get_state JSON: " + $_)
                }
            }
        )

        # Verify tool list for Initialized state
        $r = Send-McpMessage -Process $server.Process -Json $msg_tools_list_11
        Assert-JsonRpcResponse -Response $r -ExpectedId 11 -Validations @(
            { param($o)
                $names = $o.result.tools | ForEach-Object { $_.name }
                if ("launch" -notin $names) { throw "'launch' missing from Initialized tools" }
                if ("attach" -notin $names) { throw "'attach' missing from Initialized tools" }
                if ("configuration_done" -notin $names) { throw "'configuration_done' missing from Initialized tools" }
                if ("set_breakpoints" -notin $names) { throw "'set_breakpoints' missing from Initialized tools" }
                if ("set_function_breakpoints" -notin $names) { throw "'set_function_breakpoints' missing from Initialized tools" }
                if ("shutdown" -notin $names) { throw "'shutdown' missing from Initialized tools" }
                if ("start" -in $names) { throw "'start' should NOT be in Initialized tools" }
                if ("initialize" -in $names) { throw "'initialize' should NOT be in Initialized tools" }
                if ("continue" -in $names) { throw "'continue' should NOT be in Initialized tools" }
                if ("get_threads" -in $names) { throw "'get_threads' should NOT be in Initialized tools" }
                Write-Pass ("  [P6] Initialized tools: " + $names.Count + " tools, correct set")
            }
        )

        # ===================================================================
        # P7: Clean shutdown
        # ===================================================================
        Write-PhaseHeader "P7: Clean shutdown -- return to Disconnected"

        $r = Send-McpMessage -Process $server.Process -Json $msg_tool_call_12
        Assert-JsonRpcResponse -Response $r -ExpectedId 12 -Validations @(
            { param($o) if ($o.result.isError) { throw ("shutdown should succeed, got: " + $o.result.content[0].text) } }
        )

        # Verify back to Disconnected
        $r = Send-McpMessage -Process $server.Process -Json $msg_tool_call_13
        Assert-JsonRpcResponse -Response $r -ExpectedId 13 -Validations @(
            { param($o)
                $stateInfo = $o.result.content[0].text | ConvertFrom-Json
                if ($stateInfo.state -ne "Disconnected") {
                    throw ("Expected state 'Disconnected' after shutdown, got '" + $stateInfo.state + "'")
                }
                Write-Pass ("  [P7] State confirmed: " + $stateInfo.state)
            }
        )

        # Tool list should be back to Disconnected set
        $r = Send-McpMessage -Process $server.Process -Json $msg_tools_list_14
        Assert-JsonRpcResponse -Response $r -ExpectedId 14 -Validations @(
            { param($o)
                $names = $o.result.tools | ForEach-Object { $_.name }
                if ("start" -notin $names) { throw "'start' should be back after shutdown" }
                if ("initialize" -in $names) { throw "'initialize' should NOT be present after shutdown" }
                Write-Pass "  [P7] Post-shutdown tools: back to Disconnected set"
            }
        )

        # Wrong state after shutdown -- "continue" should still be rejected
        $r = Send-McpMessage -Process $server.Process -Json $msg_tool_call_15
        Assert-JsonRpcResponse -Response $r -ExpectedId 15 -ExpectToolError -Validations @(
            { param($o)
                $msg = $o.result.content[0].text
                if ($msg -notmatch "continue") { throw ("Error should mention 'continue': " + $msg) }
            }
        )

    } else {
        # -- Skip codelldb phases -------------------------------------------
        Write-Host ""
        Write-Host "--- P5-P7: Skipped (codelldb not available) ---" -ForegroundColor Yellow
        Write-Skip "P5: start codelldb"
        Write-Skip "P5: Connected tool listing"
        Write-Skip "P6: DAP initialize"
        Write-Skip "P6: get_state verification"
        Write-Skip "P6: Initialized tool listing"
        Write-Skip "P7: shutdown"
        Write-Skip "P7: post-shutdown state check"
        Write-Skip "P7: post-shutdown tool listing"
        Write-Skip "P7: post-shutdown wrong-state rejection"
    }

    # -- Server stderr log (verbose) ----------------------------------------
    if ($VerboseOutput -and $server -and $server.Stderr.Count -gt 0) {
        Write-Host ""
        Write-Host "--- Server stderr (teledap tracing log) ---" -ForegroundColor DarkGray
        $server.Stderr.ToArray() | ForEach-Object {
            Write-Host ("  [teledap] " + $_) -ForegroundColor DarkGray
        }
    }

    # -- Summary ------------------------------------------------------------
    Write-Host ""
    Write-Host "==========================================" -ForegroundColor Cyan
    $total = $script:Passed + $script:Failed
    $statusColor = if ($script:Failed -eq 0) { "Green" } else { "Red" }
    Write-Host ("  Total:  " + $total + " assertions") -ForegroundColor $statusColor
    Write-Host ("  Passed: " + $script:Passed) -ForegroundColor Green
    if ($script:Failed -gt 0) {
        Write-Host ("  Failed: " + $script:Failed) -ForegroundColor Red
    } else {
        Write-Host "  Failed: 0" -ForegroundColor Green
    }
    Write-Host "==========================================" -ForegroundColor Cyan

    if ($script:Failed -gt 0) {
        Write-Host ""
        Write-Host "SOME ASSERTIONS FAILED. See details above." -ForegroundColor Red
        exit 1
    } else {
        Write-Host ""
        Write-Host "All assertions passed." -ForegroundColor Green
        exit 0
    }

} catch {
    Write-Host ""
    Write-Host ("FATAL ERROR: " + $_) -ForegroundColor Red
    Write-Host $_.ScriptStackTrace -ForegroundColor DarkGray
    # Dump stderr from server for debugging
    if ($server -and $server.Stderr.Count -gt 0) {
        Write-Host ""
        Write-Host "--- Server stderr (last 20 lines) ---" -ForegroundColor DarkGray
        $server.Stderr.ToArray() | Select-Object -Last 20 | ForEach-Object {
            Write-Host ("  [stderr] " + $_) -ForegroundColor DarkGray
        }
    }
    exit 2
} finally {
    if ($server) {
        Stop-TeleDAP -Server $server
    }
}
