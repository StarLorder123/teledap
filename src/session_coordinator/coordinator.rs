//! Core session coordinator — `SessionCoordinator` implementation and helpers.

use crate::audit_tracker::{AuditLogger, LogDirection, LogSource};
use crate::drivers::lldb_dap::DapDriver;
use crate::drivers::openocd_tcl::OpenOcdDriver;
use crate::error::TeleDapError;
use super::state_machine::SessionState;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Central coordinator for the debug session.
///
/// Maintains the session state machine, owns the protocol drivers,
/// and exposes a tool-oriented API for the MCP interface layer.
pub struct SessionCoordinator {
    /// Current session state (RwLock for concurrent read access).
    state: RwLock<SessionState>,

    /// OpenOCD Tcl RPC driver (set after connection).
    openocd: RwLock<Option<OpenOcdDriver>>,

    /// CodeLLDB DAP driver (set after process start).
    dap: RwLock<Option<DapDriver>>,

    /// Audit logger for command tracing.
    audit: Arc<AuditLogger>,

    // ── Configuration ──
    /// Default OpenOCD host.
    openocd_host: String,
    /// Default OpenOCD Tcl RPC port.
    openocd_tcl_port: u16,
    /// Default OpenOCD GDB server port.
    openocd_gdb_port: u16,
    /// Default path to codelldb executable.
    codelldb_path: String,
    /// Maximum DAP frame size.
    max_dap_frame: usize,
}

impl SessionCoordinator {
    /// Creates a new `SessionCoordinator` in the `Disconnected` state.
    pub fn new(
        audit: Arc<AuditLogger>,
        openocd_host: String,
        openocd_tcl_port: u16,
        openocd_gdb_port: u16,
        codelldb_path: String,
        max_dap_frame: usize,
    ) -> Self {
        Self {
            state: RwLock::new(SessionState::Disconnected),
            openocd: RwLock::new(None),
            dap: RwLock::new(None),
            audit,
            openocd_host,
            openocd_tcl_port,
            openocd_gdb_port,
            codelldb_path,
            max_dap_frame,
        }
    }

    /// Returns the current session state.
    pub async fn current_state(&self) -> SessionState {
        *self.state.read().await
    }

    // ── State Transitions ─────────────────────────────────────────

    /// Attempt to transition to a new state.
    /// Validates the transition and logs it to the audit trail.
    async fn transition(
        &self,
        target: SessionState,
    ) -> Result<(), TeleDapError> {
        let mut state = self.state.write().await;
        state.validate_transition(target).map_err(|e| {
            TeleDapError::InvalidState {
                current: *state,
                expected: e,
            }
        })?;

        let old = *state;
        *state = target;

        self.audit.log(
            LogSource::Internal,
            LogDirection::Outbound,
            &format!("state_transition: {:?} -> {:?}", old, target),
            None,
            None,
            None,
        );

        tracing::info!("State transition: {:?} -> {:?}", old, target);
        Ok(())
    }

    // ── High-Level Operations ─────────────────────────────────────

    /// One-click auto-launch: connects OpenOCD (remote mode) or launches
    /// locally (local mode), starts CodeLLDB, loads the binary, and halts.
    ///
    /// This is the primary entry point for AI-driven debugging.
    ///
    /// # Modes
    /// - `"remote"` (default): hardware debugging via OpenOCD + gdb-remote
    /// - `"local"`: host binary debugging via CodeLLDB only, no hardware needed
    pub async fn auto_launch(
        &self,
        elf_path: &str,
        mode: &str,
    ) -> Result<String, TeleDapError> {
        // Validate starting state
        let current = self.current_state().await;
        if current != SessionState::Disconnected {
            return Err(TeleDapError::InvalidState {
                current,
                expected: "Must be in Disconnected state for auto_launch"
                    .into(),
            });
        }

        let is_local = mode == "local";

        if is_local {
            // ── Local Mode: CodeLLDB only, no OpenOCD ──────────────

            // 1. Start CodeLLDB
            {
                let driver = DapDriver::new(
                    self.audit.clone(),
                    self.max_dap_frame,
                );
                driver
                    .start(&self.codelldb_path)
                    .await
                    .map_err(TeleDapError::Driver)?;
                *self.dap.write().await = Some(driver);
            }

            // 2. Initialize the DAP session
            {
                let dap_guard = self.dap.read().await;
                let dap = dap_guard
                    .as_ref()
                    .ok_or(TeleDapError::NotConnected(
                        "DAP driver not available".into(),
                    ))?;
                dap.initialize().await.map_err(TeleDapError::Driver)?;
            }

            // 3. Launch locally: no gdb-remote, stop at main()
            {
                let dap_guard = self.dap.read().await;
                let dap = dap_guard
                    .as_ref()
                    .ok_or(TeleDapError::NotConnected(
                        "DAP driver not available".into(),
                    ))?;
                dap.launch(elf_path, None, true)
                    .await
                    .map_err(TeleDapError::Driver)?;
            }

            self.transition(SessionState::Attached).await?;
            self.transition(SessionState::Halted).await?;

            let msg = format!(
                "Auto-launch (local) complete. Binary '{}' loaded. Target halted at entry point.",
                elf_path
            );

            self.audit.log(
                LogSource::McpTrigger,
                LogDirection::Inbound,
                "auto_launch",
                Some(serde_json::json!({"elf_path": elf_path, "mode": "local"})),
                Some(msg.clone()),
                None,
            );

            Ok(msg)
        } else {
            // ── Remote Mode: OpenOCD + CodeLLDB + gdb-remote ──────

            self.transition(SessionState::Initialized).await?;

            let host = &self.openocd_host.clone();
            let tcl_port = self.openocd_tcl_port;
            let gdb_port = self.openocd_gdb_port;

            // 1. Connect to OpenOCD
            {
                let driver = OpenOcdDriver::new(self.audit.clone());
                driver.connect(host, tcl_port).await.map_err(|e| {
                    TeleDapError::Driver(e)
                })?;
                *self.openocd.write().await = Some(driver);
            }

            // 2. Start CodeLLDB
            {
                let driver = DapDriver::new(
                    self.audit.clone(),
                    self.max_dap_frame,
                );
                driver
                    .start(&self.codelldb_path)
                    .await
                    .map_err(TeleDapError::Driver)?;
                *self.dap.write().await = Some(driver);
            }

            // 3. Initialize the DAP session
            {
                let dap_guard = self.dap.read().await;
                let dap = dap_guard
                    .as_ref()
                    .ok_or(TeleDapError::NotConnected(
                        "DAP driver not available".into(),
                    ))?;
                dap.initialize().await.map_err(TeleDapError::Driver)?;
            }

            // 4. Launch with gdb-remote to OpenOCD
            {
                let dap_guard = self.dap.read().await;
                let dap = dap_guard
                    .as_ref()
                    .ok_or(TeleDapError::NotConnected(
                        "DAP driver not available".into(),
                    ))?;
                let gdb_target = format!("{}:{}", host, gdb_port);
                dap.launch(elf_path, Some(&gdb_target), false)
                    .await
                    .map_err(TeleDapError::Driver)?;
            }

            self.transition(SessionState::Attached).await?;

            // 5. Reset and halt the target
            {
                let ocd_guard = self.openocd.read().await;
                let ocd = ocd_guard
                    .as_ref()
                    .ok_or(TeleDapError::NotConnected(
                        "OpenOCD driver not available".into(),
                    ))?;
                ocd.reset_halt().await.map_err(TeleDapError::Driver)?;
            }

            self.transition(SessionState::Halted).await?;

            let msg = format!(
                "Auto-launch (remote) complete. ELF '{}' loaded. OpenOCD: {}:{}, GDB: {}:{}. Target halted at reset vector.",
                elf_path, host, tcl_port, host, gdb_port
            );

            self.audit.log(
                LogSource::McpTrigger,
                LogDirection::Inbound,
                "auto_launch",
                Some(serde_json::json!({"elf_path": elf_path, "mode": "remote"})),
                Some(msg.clone()),
                None,
            );

            Ok(msg)
        }
    }

    /// Connect to OpenOCD Tcl RPC server.
    pub async fn connect_openocd(
        &self,
        host: Option<&str>,
        port: Option<u16>,
    ) -> Result<(), TeleDapError> {
        let h = host.unwrap_or(&self.openocd_host);
        let p = port.unwrap_or(self.openocd_tcl_port);

        let driver = OpenOcdDriver::new(self.audit.clone());
        driver
            .connect(h, p)
            .await
            .map_err(TeleDapError::Driver)?;

        *self.openocd.write().await = Some(driver);
        self.transition(SessionState::Initialized).await?;

        Ok(())
    }

    /// Start CodeLLDB and initialize the DAP session.
    pub async fn start_codelldb(
        &self,
        codelldb_path: Option<&str>,
    ) -> Result<(), TeleDapError> {
        let path = codelldb_path.unwrap_or(&self.codelldb_path);

        let driver = DapDriver::new(
            self.audit.clone(),
            self.max_dap_frame,
        );
        driver
            .start(path)
            .await
            .map_err(TeleDapError::Driver)?;

        driver
            .initialize()
            .await
            .map_err(TeleDapError::Driver)?;

        *self.dap.write().await = Some(driver);
        Ok(())
    }

    /// Launch the ELF with an optional gdb-remote target.
    pub async fn launch_dap(
        &self,
        elf_path: &str,
        gdb_remote_port: Option<u16>,
    ) -> Result<(), TeleDapError> {
        let dap_guard = self.dap.read().await;
        let dap = dap_guard
            .as_ref()
            .ok_or(TeleDapError::NotConnected(
                "DAP driver not available".into(),
            ))?;

        let gdb_target = gdb_remote_port.map(|p| {
            format!("{}:{}", self.openocd_host, p)
        });

        dap.launch(
            elf_path,
            gdb_target.as_deref(),
            false,
        )
        .await
        .map_err(TeleDapError::Driver)?;

        self.transition(SessionState::Attached).await?;
        Ok(())
    }

    // ── Tool Dispatch ─────────────────────────────────────────────

    /// Execute a named tool with the given arguments.
    ///
    /// Returns a human-readable result string suitable for MCP response.
    pub async fn execute_tool(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<String, TeleDapError> {
        let start = std::time::Instant::now();
        let state = self.current_state().await;

        let result: Result<String, TeleDapError> = match name {
            // ── Always Available ──
            "get_status" => Ok(format!(
                "TeleDAP session status:\n\
                 - State: {:?}\n\
                 - OpenOCD: {}\n\
                 - CodeLLDB: {}\n\
                 - Session ID: {}",
                state,
                if self.openocd.read().await.is_some() {
                    "connected"
                } else {
                    "not connected"
                },
                if self.dap.read().await.is_some() {
                    "running"
                } else {
                    "not started"
                },
                self.audit.session_id(),
            )),

            "get_debug_logs" => {
                let count = args
                    .get("count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20)
                    .min(500) as usize;
                let logs = self.audit.get_logs(count);
                serde_json::to_string_pretty(&logs)
                    .map_err(|e| TeleDapError::Communication(e.to_string()))
            }

            // ── Disconnected State ──
            "auto_launch" if state == SessionState::Disconnected => {
                let elf = get_str_param(args, "elf_path")?;
                let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("remote");
                self.auto_launch(elf, mode).await
            }

            "connect_openocd" if state == SessionState::Disconnected => {
                let host = args.get("host").and_then(|v| v.as_str());
                let port = args.get("tcl_port").and_then(|v| v.as_u64()).map(|v| v as u16);
                self.connect_openocd(host, port).await?;
                Ok("OpenOCD connected.".into())
            }

            // ── Initialized+ State ──
            "reset_halt"
                if matches!(
                    state,
                    SessionState::Initialized
                        | SessionState::Attached
                        | SessionState::Halted
                        | SessionState::Running
                ) =>
            {
                {
                    let guard = self.openocd.read().await;
                    let ocd = guard.as_ref().ok_or_else(|| {
                        TeleDapError::NotConnected(
                            "OpenOCD driver not initialized".into(),
                        )
                    })?;
                    ocd.reset_halt()
                        .await
                        .map_err(TeleDapError::Driver)?;
                }
                if state != SessionState::Initialized {
                    self.transition(SessionState::Halted).await?;
                }
                Ok("Target reset and halted.".into())
            }

            "flash_erase"
                if matches!(
                    state,
                    SessionState::Initialized
                        | SessionState::Attached
                        | SessionState::Halted
                ) =>
            {
                let address = get_u32_param(args, "address")?;
                let length = get_u32_param(args, "length")?;
                let guard = self.openocd.read().await;
                let ocd = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "OpenOCD driver not initialized".into(),
                    )
                })?;
                ocd.flash_erase(address, length)
                    .await
                    .map_err(TeleDapError::Driver)?;
                Ok(format!(
                    "Flash erased: 0x{:x} + {} bytes",
                    address, length
                ))
            }

            "flash_write"
                if matches!(
                    state,
                    SessionState::Initialized
                        | SessionState::Attached
                        | SessionState::Halted
                ) =>
            {
                let address = get_u32_param(args, "address")?;
                let data_hex = get_str_param(args, "data_hex")?;
                let data = hex_to_bytes(data_hex)?;
                let guard = self.openocd.read().await;
                let ocd = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "OpenOCD driver not initialized".into(),
                    )
                })?;
                ocd.flash_write(address, &data)
                    .await
                    .map_err(TeleDapError::Driver)?;
                Ok(format!(
                    "Flash written: 0x{:x} ({} bytes)",
                    address,
                    data.len()
                ))
            }

            "read_register"
                if matches!(
                    state,
                    SessionState::Initialized
                        | SessionState::Attached
                        | SessionState::Halted
                        | SessionState::Running
                ) =>
            {
                let reg = get_str_param(args, "register")?;
                let guard = self.openocd.read().await;
                let ocd = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "OpenOCD driver not initialized".into(),
                    )
                })?;
                let value = ocd
                    .read_register(reg)
                    .await
                    .map_err(TeleDapError::Driver)?;
                Ok(format!(
                    "Register {} = 0x{:08X} ({})",
                    reg, value, value
                ))
            }

            "write_register"
                if matches!(
                    state,
                    SessionState::Initialized
                        | SessionState::Attached
                        | SessionState::Halted
                        | SessionState::Running
                ) =>
            {
                let reg = get_str_param(args, "register")?;
                let value = get_u32_param(args, "value")?;
                let guard = self.openocd.read().await;
                let ocd = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "OpenOCD driver not initialized".into(),
                    )
                })?;
                ocd.write_register(reg, value)
                    .await
                    .map_err(TeleDapError::Driver)?;
                Ok(format!(
                    "Register {} := 0x{:08X}",
                    reg, value
                ))
            }

            "read_memory"
                if matches!(
                    state,
                    SessionState::Initialized
                        | SessionState::Attached
                        | SessionState::Halted
                        | SessionState::Running
                ) =>
            {
                let address = get_u32_param(args, "address")?;
                let length = args
                    .get("length")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(256)
                    .min(4096) as usize;
                let guard = self.openocd.read().await;
                let ocd = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "OpenOCD driver not initialized".into(),
                    )
                })?;
                let data = ocd
                    .read_memory(address, length)
                    .await
                    .map_err(TeleDapError::Driver)?;
                let hex: String =
                    data.iter().map(|b| format!("{:02x}", b)).collect();
                Ok(format!(
                    "Memory @ 0x{:08X} ({} bytes): {}",
                    address, length, hex
                ))
            }

            "write_memory"
                if matches!(
                    state,
                    SessionState::Initialized
                        | SessionState::Attached
                        | SessionState::Halted
                        | SessionState::Running
                ) =>
            {
                let address = get_u32_param(args, "address")?;
                let data_hex = get_str_param(args, "data_hex")?;
                let data = hex_to_bytes(data_hex)?;
                let guard = self.openocd.read().await;
                let ocd = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "OpenOCD driver not initialized".into(),
                    )
                })?;
                ocd.write_memory(address, &data)
                    .await
                    .map_err(TeleDapError::Driver)?;
                Ok(format!(
                    "Memory written @ 0x{:08X} ({} bytes)",
                    address,
                    data.len()
                ))
            }

            // ── Halted State Only ──
            "set_breakpoint" if state == SessionState::Halted => {
                let file = get_str_param(args, "file")?;
                let line = get_u32_param(args, "line")?;
                let guard = self.dap.read().await;
                let dap = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "DAP driver not initialized".into(),
                    )
                })?;
                let resp = dap
                    .set_breakpoint(file, line)
                    .await
                    .map_err(TeleDapError::Driver)?;
                Ok(format!(
                    "Breakpoint set at {}:{}. Response: {}",
                    file, line, resp
                ))
            }

            "continue_execution"
                if matches!(state, SessionState::Halted | SessionState::Running) =>
            {
                let thread_id = args
                    .get("thread_id")
                    .and_then(|v| v.as_u64());
                let guard = self.dap.read().await;
                let dap = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "DAP driver not initialized".into(),
                    )
                })?;
                dap.continue_execution(thread_id)
                    .await
                    .map_err(TeleDapError::Driver)?;
                self.transition(SessionState::Running).await?;
                Ok("Target continuing.".into())
            }

            "halt" if state == SessionState::Running => {
                let guard = self.dap.read().await;
                let dap = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "DAP driver not initialized".into(),
                    )
                })?;
                dap.pause(0) // thread_id=0 means all threads
                    .await
                    .map_err(TeleDapError::Driver)?;
                self.transition(SessionState::Halted).await?;
                Ok("Target halted.".into())
            }

            "step_in" if state == SessionState::Halted => {
                let thread_id = args
                    .get("thread_id")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let guard = self.dap.read().await;
                let dap = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "DAP driver not initialized".into(),
                    )
                })?;
                dap.step("in", thread_id)
                    .await
                    .map_err(TeleDapError::Driver)?;
                Ok("Stepped in.".into())
            }

            "step_over" if state == SessionState::Halted => {
                let thread_id = args
                    .get("thread_id")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let guard = self.dap.read().await;
                let dap = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "DAP driver not initialized".into(),
                    )
                })?;
                dap.step("over", thread_id)
                    .await
                    .map_err(TeleDapError::Driver)?;
                Ok("Stepped over.".into())
            }

            "step_out" if state == SessionState::Halted => {
                let thread_id = args
                    .get("thread_id")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let guard = self.dap.read().await;
                let dap = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "DAP driver not initialized".into(),
                    )
                })?;
                dap.step("out", thread_id)
                    .await
                    .map_err(TeleDapError::Driver)?;
                Ok("Stepped out.".into())
            }

            "get_stack_trace" if state == SessionState::Halted => {
                let thread_id = args
                    .get("thread_id")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let guard = self.dap.read().await;
                let dap = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "DAP driver not initialized".into(),
                    )
                })?;
                let trace = dap
                    .stack_trace(thread_id)
                    .await
                    .map_err(TeleDapError::Driver)?;
                serde_json::to_string_pretty(&trace)
                    .map_err(|e| TeleDapError::Communication(e.to_string()))
            }

            "get_variables" if state == SessionState::Halted => {
                let frame_id = get_u64_param(args, "frame_id")?;
                let guard = self.dap.read().await;
                let dap = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "DAP driver not initialized".into(),
                    )
                })?;
                let vars = dap
                    .variables(frame_id)
                    .await
                    .map_err(TeleDapError::Driver)?;
                serde_json::to_string_pretty(&vars)
                    .map_err(|e| TeleDapError::Communication(e.to_string()))
            }

            "evaluate" if state == SessionState::Halted => {
                let expr = get_str_param(args, "expression")?;
                let frame_id = args
                    .get("frame_id")
                    .and_then(|v| v.as_u64());
                let guard = self.dap.read().await;
                let dap = guard.as_ref().ok_or_else(|| {
                    TeleDapError::NotConnected(
                        "DAP driver not initialized".into(),
                    )
                })?;
                let result = dap
                    .evaluate(expr, frame_id)
                    .await
                    .map_err(TeleDapError::Driver)?;
                serde_json::to_string_pretty(&result)
                    .map_err(|e| TeleDapError::Communication(e.to_string()))
            }

            // ── Always Available ──
            "shutdown" => {
                self.shutdown().await?;
                Ok("TeleDAP shutdown complete. All connections closed."
                    .into())
            }

            // Unknown or unavailable tool
            _ => {
                // Check if it's a known tool in the wrong state
                let known_tools = [
                    "auto_launch",
                    "connect_openocd",
                    "reset_halt",
                    "flash_erase",
                    "flash_write",
                    "read_register",
                    "write_register",
                    "read_memory",
                    "write_memory",
                    "set_breakpoint",
                    "continue_execution",
                    "halt",
                    "step_in",
                    "step_over",
                    "step_out",
                    "get_stack_trace",
                    "get_variables",
                    "evaluate",
                    "get_status",
                    "get_debug_logs",
                    "shutdown",
                ];

                if known_tools.contains(&name) {
                    Err(TeleDapError::ToolUnavailable(
                        name.to_string(),
                        state,
                    ))
                } else {
                    Err(TeleDapError::UnknownTool(name.to_string()))
                }
            }
        };

        // Log the tool execution
        let duration = start.elapsed().as_micros() as i64;
        match &result {
            Ok(msg) => {
                self.audit.log(
                    LogSource::McpTrigger,
                    LogDirection::Inbound,
                    name,
                    Some(args.clone()),
                    Some(format!("success: {}", &msg[..msg.len().min(200)])),
                    Some(duration),
                );
            }
            Err(e) => {
                self.audit.log(
                    LogSource::McpTrigger,
                    LogDirection::Inbound,
                    name,
                    Some(args.clone()),
                    Some(format!("error: {}", e)),
                    Some(duration),
                );
            }
        }

        result
    }

    /// Graceful shutdown: disconnect DAP, disconnect OpenOCD, log state.
    pub async fn shutdown(&self) -> Result<(), TeleDapError> {
        tracing::info!("Shutting down TeleDAP session...");

        // Disconnect DAP (kills codelldb process)
        if let Some(ref dap) = *self.dap.read().await {
            let _ = dap.shutdown().await;
        }

        // Disconnect OpenOCD
        if let Some(ref ocd) = *self.openocd.read().await {
            let _ = ocd.disconnect().await;
        }

        *self.state.write().await = SessionState::Disconnected;

        self.audit.log(
            LogSource::Internal,
            LogDirection::Outbound,
            "session_shutdown",
            None,
            None,
            None,
        );

        Ok(())
    }
}

// ── Helper Functions ─────────────────────────────────────────────

/// Extract a required string parameter from tool arguments.
fn get_str_param<'a>(
    args: &'a serde_json::Value,
    name: &str,
) -> Result<&'a str, TeleDapError> {
    args.get(name)
        .and_then(|v| v.as_str())
        .ok_or_else(|| TeleDapError::MissingParameter(name.to_string()))
}

/// Extract a required u32 parameter from tool arguments.
fn get_u32_param(
    args: &serde_json::Value,
    name: &str,
) -> Result<u32, TeleDapError> {
    let val = args
        .get(name)
        .ok_or_else(|| TeleDapError::MissingParameter(name.to_string()))?;

    // Support both integer and hex string formats
    if let Some(n) = val.as_u64() {
        if n > u32::MAX as u64 {
            return Err(TeleDapError::InvalidParameter {
                name: name.to_string(),
                reason: "Value exceeds u32 range".into(),
            });
        }
        Ok(n as u32)
    } else if let Some(s) = val.as_str() {
        let hex = s.trim().trim_start_matches("0x").trim_start_matches("0X");
        u32::from_str_radix(hex, 16).map_err(|_| {
            TeleDapError::InvalidParameter {
                name: name.to_string(),
                reason: "Not a valid hex number".into(),
            }
        })
    } else {
        Err(TeleDapError::InvalidParameter {
            name: name.to_string(),
            reason: "Expected integer or hex string".into(),
        })
    }
}

/// Extract a required u64 parameter from tool arguments.
fn get_u64_param(
    args: &serde_json::Value,
    name: &str,
) -> Result<u64, TeleDapError> {
    args.get(name)
        .and_then(|v| v.as_u64())
        .ok_or_else(|| TeleDapError::MissingParameter(name.to_string()))
}

/// Convert a hex string to bytes.
fn hex_to_bytes(s: &str) -> Result<Vec<u8>, TeleDapError> {
    let hex: String = s
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();

    if hex.len() % 2 != 0 {
        return Err(TeleDapError::InvalidParameter {
            name: "data_hex".into(),
            reason: "Hex string must have even length".into(),
        });
    }

    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| {
                TeleDapError::InvalidParameter {
                    name: "data_hex".into(),
                    reason: "Invalid hex characters".into(),
                }
            })
        })
        .collect()
}
