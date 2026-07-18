//! DebugSession: managed debug session wrapping DapClient with state machine,
//! event handling, and operation gating.

use dap_client::{AdapterConfig, AdapterKind, DapClient};
use dap_trace::TraceHandle;
use dap_types::{
    base::Event,
    capabilities::Capabilities,
    events::{ContinuedEventBody, ExitedEventBody, StoppedEventBody},
    requests::{
        AttachRequest, AttachRequestArguments, ConfigurationDoneRequest, ContinueArguments,
        ContinueRequest, EvaluateArguments, EvaluateRequest, InitializeRequest,
        InitializeRequestArguments, LaunchRequest, LaunchRequestArguments, NextArguments,
        NextRequest, NoArguments, PauseArguments, PauseRequest, ScopesArguments, ScopesRequest,
        SetBreakpointsArguments, SetBreakpointsRequest, SetFunctionBreakpointsArguments,
        SetFunctionBreakpointsRequest, SetVariableArguments, SetVariableRequest,
        StackTraceArguments, StackTraceRequest, StepInArguments, StepInRequest, StepOutArguments,
        StepOutRequest, ThreadsRequest, VariablesArguments, VariablesRequest,
    },
    requests::{
        ContinueResponse, EvaluateResponse, SetBreakpointsResponse, SetFunctionBreakpointsResponse,
        SetVariableResponse,
    },
    types::{Scope, StackFrame, Thread, Variable},
};
use tokio::sync::{watch, RwLock};
use tracing::{debug, info, warn};

use crate::cache::{VariableHandleCache, VariableHandleEntry};
use crate::error::DebugSessionError;
use crate::gating::ToolAvailability;
use crate::mapping::PathMapper;
use crate::state::{HaltState, SessionState};

/// A managed debug session with state tracking, operation gating, and event handling.
///
/// Wraps a `DapClient` and maintains a formal state machine:
/// `Disconnected → Connected → Initialized → Running ↔ Halted`.
///
/// All operations validate the current session state before delegating to the underlying client.
///
/// # Event handling
///
/// Callers should use `client().recv_event()` to pull events from the adapter,
/// then pass each event to `handle_event()` to update the state machine.
/// Passthrough events (output, module, process, etc.) can be handled separately.
pub struct DebugSession {
    /// The underlying DAP client.
    client: DapClient,
    /// Current session state.
    state: RwLock<SessionState>,
    /// Detailed halt information (only meaningful when state is Halted).
    halt_state: RwLock<HaltState>,
    /// Capabilities returned by the initialize handshake.
    capabilities: RwLock<Option<Capabilities>>,
    /// Whether configurationDone has been sent.
    _configuration_done: RwLock<bool>,
    /// Optional trace handle for recording state transitions.
    trace: Option<TraceHandle>,
    /// Watch channel sender for state change notifications.
    state_tx: watch::Sender<SessionState>,
    /// Path mapper for AI relative ↔ system absolute path translation.
    path_mapper: RwLock<PathMapper>,
    /// Variable handle cache: variable name → variablesReference mapping.
    /// Auto-invalidated when execution resumes.
    variable_cache: VariableHandleCache,
    /// The kind of debug adapter in use (set by `start()`).
    /// Controls behavioral branching for launch/configuration_done.
    adapter_kind: RwLock<Option<AdapterKind>>,
}

impl DebugSession {
    /// Create a new DebugSession wrapping an existing DapClient.
    pub fn new(client: DapClient, trace: Option<TraceHandle>) -> Self {
        let (state_tx, _) = watch::channel(SessionState::Disconnected);
        DebugSession {
            client,
            state: RwLock::new(SessionState::Disconnected),
            halt_state: RwLock::new(HaltState::default()),
            capabilities: RwLock::new(None),
            _configuration_done: RwLock::new(false),
            trace,
            state_tx,
            path_mapper: RwLock::new(PathMapper::new()),
            variable_cache: VariableHandleCache::new(),
            adapter_kind: RwLock::new(None),
        }
    }

    /// Returns a receiver that yields the current state plus every future transition.
    pub fn state_watcher(&self) -> watch::Receiver<SessionState> {
        self.state_tx.subscribe()
    }

    /// Returns the current session state.
    pub async fn current_state(&self) -> SessionState {
        *self.state.read().await
    }

    /// Returns a reference to the underlying DapClient (for edge cases like raw event access).
    pub fn client(&self) -> &DapClient {
        &self.client
    }

    /// Returns a clone of the capabilities (if initialized).
    pub async fn capabilities(&self) -> Option<Capabilities> {
        self.capabilities.read().await.clone()
    }

    // ── Path mapping ──────────────────────────────────────────────────────────

    /// Register a path alias (AI path → system absolute path).
    ///
    /// After registration, `resolve_path("src/main.cpp")` will return the
    /// absolute system path.
    pub async fn register_path_alias(&self, alias: &str, absolute_path: &str) {
        self.path_mapper
            .write()
            .await
            .register_alias(alias, absolute_path);
    }

    /// Register a base directory for relative path resolution.
    pub async fn register_base_dir(&self, dir: &str) {
        self.path_mapper.write().await.register_base_dir(dir);
    }

    /// Resolve a path (potentially relative or aliased) to an absolute system path.
    pub async fn resolve_path(&self, path: &str) -> Option<String> {
        self.path_mapper.read().await.resolve(path)
    }

    /// Reverse-resolve an absolute system path to the most specific registered alias.
    pub async fn reverse_path(&self, absolute_path: &str) -> Option<String> {
        self.path_mapper.read().await.reverse(absolute_path)
    }

    // ── Variable handle cache ─────────────────────────────────────────────────

    /// Returns a reference to the variable handle cache.
    pub fn variable_cache(&self) -> &VariableHandleCache {
        &self.variable_cache
    }

    /// Cache a variable entry (typically called during context assembly).
    pub async fn cache_variable(&self, entry: VariableHandleEntry) {
        self.variable_cache.insert(entry).await;
    }

    /// Look up a variable handle by name, with optional frame/scope scoping.
    pub async fn lookup_variable(
        &self,
        name: &str,
        frame_id: Option<u64>,
        scope_name: Option<&str>,
    ) -> Option<VariableHandleEntry> {
        self.variable_cache.lookup(name, frame_id, scope_name).await
    }

    /// Search the variable cache with a fuzzy query.
    pub async fn search_variables(&self, query: &str, limit: usize) -> Vec<VariableHandleEntry> {
        self.variable_cache.search(query, limit).await
    }

    /// Populate the variable cache from a flat list of variables for a given frame/scope.
    pub async fn cache_variables(
        &self,
        variables: &[Variable],
        frame_id: Option<u64>,
        scope_name: Option<&str>,
    ) {
        let entries: Vec<VariableHandleEntry> = variables
            .iter()
            .map(|v| VariableHandleEntry {
                variables_reference: v.variables_reference,
                name: v.name.clone(),
                frame_id,
                scope_name: scope_name.map(|s| s.to_string()),
                var_type: v.var_type.clone(),
                named_variables: v.named_variables,
                indexed_variables: v.indexed_variables,
                captured_at: std::time::Instant::now(),
            })
            .collect();
        self.variable_cache.insert_batch(entries).await;
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Validate that the current state allows `operation`, returning a formatted error if not.
    async fn gate(&self, operation: &str) -> Result<SessionState, DebugSessionError> {
        let state = *self.state.read().await;
        if !ToolAvailability::is_allowed(operation, state) {
            return Err(DebugSessionError::InvalidState {
                operation: operation.to_string(),
                current: state,
                required: ToolAvailability::allowed_states(operation).to_vec(),
            });
        }
        Ok(state)
    }

    /// Attempt to transition the session to a new state.
    async fn transition_to(
        &self,
        target: SessionState,
        reason: &str,
    ) -> Result<(), DebugSessionError> {
        let mut state = self.state.write().await;
        if !state.can_transition_to(target) {
            return Err(DebugSessionError::InvalidState {
                operation: reason.to_string(),
                current: *state,
                required: target.valid_predecessors().to_vec(),
            });
        }

        let from = *state;
        *state = target;

        // Invalidate variable cache when leaving Halted state
        // (variable handles are only valid while stopped)
        if from == SessionState::Halted
            && (target == SessionState::Running || target == SessionState::Disconnected)
        {
            self.variable_cache.invalidate().await;
        }

        // Trace the transition if tracing is enabled
        if let Some(ref t) = self.trace {
            t.trace_internal(
                "state_transition",
                Some(serde_json::json!({
                    "from": format!("{from}"),
                    "to": format!("{target}"),
                    "reason": reason,
                })),
            );
        }

        // Broadcast via watch channel
        let _ = self.state_tx.send(target);

        info!(from = %from, to = %target, reason = %reason, "Session state transition");

        Ok(())
    }

    // ── Lifecycle operations ──────────────────────────────────────────────────

    /// Start the debug adapter process. Valid only from Disconnected.
    pub async fn start(&self, config: &AdapterConfig) -> Result<(), DebugSessionError> {
        self.gate("start").await?;
        self.client.start(config).await?;
        *self.adapter_kind.write().await = Some(config.kind);
        self.transition_to(SessionState::Connected, "start").await?;
        Ok(())
    }

    /// Returns the kind of adapter currently in use (if started).
    pub async fn adapter_kind(&self) -> Option<AdapterKind> {
        *self.adapter_kind.read().await
    }

    /// Perform the initialize handshake. Valid only from Connected.
    ///
    /// Returns the adapter capabilities.
    pub async fn initialize(
        &self,
        args: InitializeRequestArguments,
    ) -> Result<Capabilities, DebugSessionError> {
        self.gate("initialize").await?;
        let caps = self.client.send_request::<InitializeRequest>(args).await?;
        *self.capabilities.write().await = Some(caps.clone());
        self.transition_to(SessionState::Initialized, "initialize")
            .await?;
        Ok(caps)
    }

    /// Launch the debuggee. Valid only from Initialized.
    ///
    /// For **codelldb**, uses `send_request_nb` (fire-and-forget) because
    /// codelldb defers the `launch` response until after `configurationDone`.
    /// The adapter sends the `initialized` event during `launch` processing,
    /// so callers must wait for that event separately.
    ///
    /// For **GDB** (and other spec-compliant adapters), uses the standard
    /// blocking `send_request` which waits for the `launch` response.
    pub async fn launch(&self, args: LaunchRequestArguments) -> Result<(), DebugSessionError> {
        self.gate("launch").await?;
        match *self.adapter_kind.read().await {
            Some(AdapterKind::Codelldb) | None => {
                self.client.send_request_nb::<LaunchRequest>(args).await?;
            }
            Some(AdapterKind::Gdb) => {
                self.client.send_request::<LaunchRequest>(args).await?;
            }
        }
        Ok(())
    }

    /// Attach to a running process. Valid only from Initialized.
    pub async fn attach(&self, args: AttachRequestArguments) -> Result<(), DebugSessionError> {
        self.gate("attach").await?;
        self.client.send_request::<AttachRequest>(args).await?;
        Ok(())
    }

    /// Signal that configuration is complete. Valid only from Initialized.
    /// Transitions to Running on success.
    ///
    /// For **codelldb**, uses `send_request_nb` (fire-and-forget) because
    /// the response body is meaningless and codelldb may not respond promptly.
    ///
    /// For **GDB**, uses the standard blocking `send_request`.
    pub async fn configuration_done(&self) -> Result<(), DebugSessionError> {
        self.gate("configuration_done").await?;
        match *self.adapter_kind.read().await {
            Some(AdapterKind::Codelldb) | None => {
                self.client
                    .send_request_nb::<ConfigurationDoneRequest>(NoArguments {})
                    .await?;
            }
            Some(AdapterKind::Gdb) => {
                self.client
                    .send_request::<ConfigurationDoneRequest>(NoArguments {})
                    .await?;
            }
        }
        *self._configuration_done.write().await = true;
        self.transition_to(SessionState::Running, "configuration_done")
            .await?;
        Ok(())
    }

    /// Disconnect and shut down the debug session. Valid from any non-Disconnected state.
    pub async fn shutdown(&self) -> Result<(), DebugSessionError> {
        self.gate("shutdown").await?;
        self.client.shutdown().await?;
        // If already Disconnected (e.g., after a terminated event), don't double-transition
        let current = *self.state.read().await;
        if current != SessionState::Disconnected {
            self.transition_to(SessionState::Disconnected, "shutdown")
                .await?;
        }
        Ok(())
    }

    // ── Execution control (Halted → Running) ──────────────────────────────────

    /// Continue execution. Valid only from Halted.
    pub async fn continue_execution(
        &self,
        thread_id: u64,
        single_thread: Option<bool>,
    ) -> Result<ContinueResponse, DebugSessionError> {
        self.gate("continue").await?;
        let args = ContinueArguments {
            thread_id,
            single_thread,
        };
        let result = self.client.send_request::<ContinueRequest>(args).await?;
        self.halt_state.write().await.clear();
        self.transition_to(SessionState::Running, "continue")
            .await?;
        Ok(result)
    }

    /// Step over. Valid only from Halted.
    pub async fn step_over(
        &self,
        thread_id: u64,
        single_thread: Option<bool>,
    ) -> Result<(), DebugSessionError> {
        self.gate("step_over").await?;
        let args = NextArguments {
            thread_id,
            single_thread,
            granularity: None,
        };
        self.client.send_request::<NextRequest>(args).await?;
        self.halt_state.write().await.clear();
        self.transition_to(SessionState::Running, "step_over")
            .await?;
        Ok(())
    }

    /// Step in. Valid only from Halted.
    pub async fn step_in(
        &self,
        thread_id: u64,
        single_thread: Option<bool>,
        target_id: Option<u64>,
    ) -> Result<(), DebugSessionError> {
        self.gate("step_in").await?;
        let args = StepInArguments {
            thread_id,
            single_thread,
            target_id,
            granularity: None,
        };
        self.client.send_request::<StepInRequest>(args).await?;
        self.halt_state.write().await.clear();
        self.transition_to(SessionState::Running, "step_in").await?;
        Ok(())
    }

    /// Step out. Valid only from Halted.
    pub async fn step_out(
        &self,
        thread_id: u64,
        single_thread: Option<bool>,
    ) -> Result<(), DebugSessionError> {
        self.gate("step_out").await?;
        let args = StepOutArguments {
            thread_id,
            single_thread,
            granularity: None,
        };
        self.client.send_request::<StepOutRequest>(args).await?;
        self.halt_state.write().await.clear();
        self.transition_to(SessionState::Running, "step_out")
            .await?;
        Ok(())
    }

    /// Pause execution. Valid only from Running.
    ///
    /// Note: state transitions to Halted when the `stopped` event arrives
    /// (handled via `handle_event()`), not when this method returns.
    pub async fn pause(&self, thread_id: u64) -> Result<(), DebugSessionError> {
        self.gate("pause").await?;
        let args = PauseArguments { thread_id };
        self.client.send_request::<PauseRequest>(args).await?;
        Ok(())
    }

    // ── Breakpoints ───────────────────────────────────────────────────────────

    /// Set source breakpoints. Valid from Initialized, Running, or Halted.
    pub async fn set_breakpoints(
        &self,
        args: SetBreakpointsArguments,
    ) -> Result<SetBreakpointsResponse, DebugSessionError> {
        self.gate("set_breakpoints").await?;
        let resp = self
            .client
            .send_request::<SetBreakpointsRequest>(args)
            .await?;
        Ok(resp)
    }

    /// Set function breakpoints. Valid from Initialized, Running, or Halted.
    pub async fn set_function_breakpoints(
        &self,
        args: SetFunctionBreakpointsArguments,
    ) -> Result<SetFunctionBreakpointsResponse, DebugSessionError> {
        self.gate("set_function_breakpoints").await?;
        let resp = self
            .client
            .send_request::<SetFunctionBreakpointsRequest>(args)
            .await?;
        Ok(resp)
    }

    // ── Introspection (Halted only) ───────────────────────────────────────────

    /// Get all threads. Valid only from Halted.
    pub async fn get_threads(&self) -> Result<Vec<Thread>, DebugSessionError> {
        self.gate("get_threads").await?;
        let resp = self
            .client
            .send_request::<ThreadsRequest>(NoArguments {})
            .await?;
        Ok(resp.threads)
    }

    /// Get stack trace for a thread. Valid only from Halted.
    pub async fn get_stack_trace(
        &self,
        thread_id: u64,
        start_frame: Option<u64>,
        levels: Option<u64>,
    ) -> Result<Vec<StackFrame>, DebugSessionError> {
        self.gate("get_stack_trace").await?;
        let args = StackTraceArguments {
            thread_id,
            start_frame,
            levels,
            format: None,
        };
        let resp = self.client.send_request::<StackTraceRequest>(args).await?;
        Ok(resp.stack_frames)
    }

    /// Get scopes for a stack frame. Valid only from Halted.
    pub async fn get_scopes(&self, frame_id: u64) -> Result<Vec<Scope>, DebugSessionError> {
        self.gate("get_scopes").await?;
        let args = ScopesArguments { frame_id };
        let resp = self.client.send_request::<ScopesRequest>(args).await?;
        Ok(resp.scopes)
    }

    /// Get variables for a variables reference. Valid only from Halted.
    pub async fn get_variables(
        &self,
        variables_reference: u64,
        filter: Option<dap_types::enums::VariableFilter>,
        start: Option<u64>,
        count: Option<u64>,
    ) -> Result<Vec<Variable>, DebugSessionError> {
        self.gate("get_variables").await?;
        let args = VariablesArguments {
            variables_reference,
            filter,
            start,
            count,
            format: None,
        };
        let resp = self.client.send_request::<VariablesRequest>(args).await?;
        Ok(resp.variables)
    }

    /// Evaluate an expression. Valid only from Halted.
    pub async fn evaluate(
        &self,
        args: EvaluateArguments,
    ) -> Result<EvaluateResponse, DebugSessionError> {
        self.gate("evaluate").await?;
        let resp = self.client.send_request::<EvaluateRequest>(args).await?;
        Ok(resp)
    }

    /// Set a variable's value. Valid only from Halted.
    pub async fn set_variable(
        &self,
        args: SetVariableArguments,
    ) -> Result<SetVariableResponse, DebugSessionError> {
        self.gate("set_variable").await?;
        let resp = self.client.send_request::<SetVariableRequest>(args).await?;
        Ok(resp)
    }

    // ── Event handling ────────────────────────────────────────────────────────

    /// Process a DAP event and update the state machine.
    ///
    /// Returns `true` if the event was a state-affecting event that was handled,
    /// `false` if it was a passthrough event (output, module, process, etc.) that
    /// callers should handle themselves.
    ///
    /// # Example event loop
    ///
    /// ```ignore
    /// while let Some(event) = session.client().recv_event().await {
    ///     if !session.handle_event(&event).await? {
    ///         // Handle passthrough events (output, etc.)
    ///     }
    ///     if session.current_state().await == SessionState::Disconnected {
    ///         break;
    ///     }
    /// }
    /// ```
    pub async fn handle_event(&self, event: &Event) -> Result<bool, DebugSessionError> {
        match event.event.as_str() {
            "initialized" => {
                // The `initialized` event may arrive after the `initialize`
                // DAP handshake has already transitioned us to Initialized.
                // In that case this is a no-op (idempotent).
                if self.current_state().await != SessionState::Initialized {
                    self.transition_to(SessionState::Initialized, "event:initialized")
                        .await?;
                }
                Ok(true)
            }
            "stopped" => {
                let body: StoppedEventBody = serde_json::from_value(
                    event.body.clone().unwrap_or_default(),
                )
                .unwrap_or_else(|e| {
                    warn!("Failed to deserialize stopped event body: {e}");
                    StoppedEventBody {
                        reason: dap_types::enums::StoppedReason::Other("unknown".to_string()),
                        description: None,
                        thread_id: None,
                        preserve_focus_hint: None,
                        text: None,
                        all_threads_stopped: None,
                        hit_breakpoint_ids: None,
                    }
                });

                let mut halt = self.halt_state.write().await;
                halt.clear();
                if let Some(tid) = body.thread_id {
                    halt.stopped_threads.insert(tid);
                }
                halt.all_threads_stopped = body.all_threads_stopped.unwrap_or(false);
                halt.last_stop_reason = Some(format!("{:?}", body.reason));
                halt.hit_breakpoint_ids = body.hit_breakpoint_ids.clone().unwrap_or_default();
                drop(halt);

                info!(
                    reason = ?body.reason,
                    thread_id = ?body.thread_id,
                    "Debuggee stopped"
                );

                self.transition_to(SessionState::Halted, "event:stopped")
                    .await?;
                Ok(true)
            }
            "continued" => {
                let body: ContinuedEventBody = serde_json::from_value(
                    event.body.clone().unwrap_or_default(),
                )
                .unwrap_or_else(|e| {
                    warn!("Failed to deserialize continued event body: {e}");
                    ContinuedEventBody {
                        thread_id: 0,
                        all_threads_continued: None,
                    }
                });

                self.halt_state.write().await.clear();
                info!(thread_id = body.thread_id, "Debuggee continued");
                // If already Running (e.g. after configurationDone → Running,
                // then the debuggee continues automatically), skip transition.
                if *self.state.read().await != SessionState::Running {
                    self.transition_to(SessionState::Running, "event:continued")
                        .await?;
                }
                Ok(true)
            }
            "terminated" => {
                info!("Debug session terminated");
                // Best-effort: try to close the client gracefully, but don't fail
                // if it's already closed (e.g., the background reader detected EOF).
                let _ = self.client.shutdown().await;
                let current = *self.state.read().await;
                if current != SessionState::Disconnected {
                    self.transition_to(SessionState::Disconnected, "event:terminated")
                        .await?;
                }
                Ok(true)
            }
            "exited" => {
                let body: ExitedEventBody = serde_json::from_value(
                    event.body.clone().unwrap_or_default(),
                )
                .unwrap_or_else(|e| {
                    warn!("Failed to deserialize exited event body: {e}");
                    ExitedEventBody { exit_code: 0 }
                });
                info!(exit_code = body.exit_code, "Debuggee exited");
                // Best-effort cleanup
                let _ = self.client.shutdown().await;
                let current = *self.state.read().await;
                if current != SessionState::Disconnected {
                    self.transition_to(SessionState::Disconnected, "event:exited")
                        .await?;
                }
                Ok(true)
            }
            "thread" => {
                // Track thread starts/exits but don't change top-level state
                let body: serde_json::Value = event.body.clone().unwrap_or_default();
                if let Some(reason) = body.get("reason").and_then(|v| v.as_str()) {
                    if reason == "exited" {
                        if let Some(tid) = body.get("thread_id").and_then(|v| v.as_u64()) {
                            self.halt_state.write().await.stopped_threads.remove(&tid);
                            debug!(thread_id = tid, "Thread exited");
                        }
                    }
                }
                Ok(false) // Not a state-affecting event; caller handles
            }
            _ => {
                // Passthrough events: output, module, process, breakpoint,
                // capabilities, progress*, invalidated, memory, loadedSource
                Ok(false)
            }
        }
    }
}
