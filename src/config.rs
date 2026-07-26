//! Configuration file support for TeleDAP.
//!
//! Reads a TOML file that pre-configures the debug session, replacing the
//! sequence of MCP tool calls an AI would normally make at startup:
//!
//! ```text
//! register_base_dir → register_path_alias → openocd_start → start →
//! initialize → set_breakpoints → launch → configuration_done
//! ```
//!
//! # Example (minimal)
//!
//! ```toml
//! [adapter]
//! path = "codelldb"
//!
//! [launch]
//! program = "./build/firmware.elf"
//!
//! [path_mapping]
//! base_dirs = ["/home/user/project"]
//! ```
//!
//! # Example (embedded with OpenOCD)
//!
//! ```toml
//! [adapter]
//! path = "/opt/codelldb/codelldb"
//! kind = "codelldb"
//!
//! [openocd]
//! path = "/usr/bin/openocd"
//! config_files = ["board/stm32f4discovery.cfg"]
//!
//! [path_mapping]
//! base_dirs = ["/home/user/firmware"]
//! aliases = { "src" = "/home/user/firmware/Core/Src" }
//!
//! [launch]
//! program = "build/firmware.elf"
//! gdb_remote = "localhost:3333"
//!
//! [[breakpoints]]
//! source = "src/main.c"
//! lines = [42, 67]
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use dap_client::{AdapterConfig, AdapterKind};
use dap_types::requests::{
    InitializeRequestArguments, LaunchRequestArguments, SetBreakpointsArguments,
};
use dap_types::types::{Source, SourceBreakpoint};
use debug_session::DebugSession;
use openocd_client::OpenOcdClient;
use serde::Deserialize;
use tokio::sync::RwLock;
use tracing::{error, info};

// ── Top-level config ──────────────────────────────────────────────────────

/// Top-level configuration loaded from a TOML file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Adapter configuration.
    #[serde(default)]
    pub adapter: AdapterSection,

    /// Debuggee launch configuration.
    #[serde(default)]
    pub launch: LaunchSection,

    /// Path mapping (aliases and base directories).
    #[serde(default)]
    pub path_mapping: PathMappingSection,

    /// OpenOCD configuration — omitted or empty path skips OpenOCD.
    #[serde(default)]
    pub openocd: OpenOcdSection,

    /// Breakpoint entries.
    #[serde(default)]
    pub breakpoints: Vec<BreakpointEntry>,

    /// Optional settings (auto-start, etc.).
    #[serde(default)]
    pub options: OptionsSection,
}

/// Optional settings section.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionsSection {
    /// Whether to automatically advance through the state machine
    /// (start → initialize → set_breakpoints → launch → configuration_done).
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

fn default_true() -> bool {
    true
}

impl Default for OptionsSection {
    fn default() -> Self {
        OptionsSection { auto_start: true }
    }
}

// ── Adapter ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterSection {
    /// Path to the debug adapter binary (e.g. "codelldb" or absolute path).
    #[serde(default = "default_adapter_path")]
    pub path: String,

    /// Adapter kind: "codelldb" (default) or "gdb".
    #[serde(default = "default_adapter_kind")]
    pub kind: String,

    /// CLI arguments for the adapter binary.
    #[serde(default)]
    pub args: Vec<String>,

    /// Optional path to liblldb shared library.
    #[serde(default)]
    pub liblldb_path: Option<String>,
}

fn default_adapter_path() -> String {
    "codelldb".into()
}

fn default_adapter_kind() -> String {
    "codelldb".into()
}

impl Default for AdapterSection {
    fn default() -> Self {
        AdapterSection {
            path: "codelldb".into(),
            kind: "codelldb".into(),
            args: Vec::new(),
            liblldb_path: None,
        }
    }
}

impl AdapterSection {
    pub fn adapter_kind(&self) -> AdapterKind {
        match self.kind.as_str() {
            "gdb" => AdapterKind::Gdb,
            _ => AdapterKind::Codelldb,
        }
    }
}

// ── Launch ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchSection {
    /// Path to the ELF binary to debug.
    #[serde(default)]
    pub program: String,

    /// CLI arguments for the debuggee.
    #[serde(default)]
    pub args: Vec<String>,

    /// Stop at program entry point.
    #[serde(default)]
    pub stop_on_entry: bool,

    /// Remote GDB server address (e.g. "localhost:3333").
    #[serde(default)]
    pub gdb_remote: Option<String>,

    /// Environment variables for the debuggee.
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
}

impl LaunchSection {
    /// Returns true if a program path is configured (launch should proceed).
    pub fn is_configured(&self) -> bool {
        !self.program.is_empty()
    }
}

// ── Path mapping ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathMappingSection {
    /// Base directories for resolving relative source paths.
    #[serde(default)]
    pub base_dirs: Vec<String>,

    /// Alias → absolute path mappings.
    #[serde(default)]
    pub aliases: HashMap<String, String>,
}

// ── OpenOCD ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenOcdSection {
    /// Absolute path to the OpenOCD binary.
    #[serde(default)]
    pub path: String,

    /// OpenOCD config files in order.
    #[serde(default)]
    pub config_files: Vec<String>,

    /// Additional CLI arguments for OpenOCD.
    #[serde(default)]
    pub extra_args: Vec<String>,

    /// Directory for stdout/stderr log files.
    #[serde(default)]
    pub log_dir: Option<String>,
}

impl OpenOcdSection {
    /// Returns true if OpenOCD should be started.
    pub fn is_configured(&self) -> bool {
        !self.path.is_empty() && !self.config_files.is_empty()
    }
}

// ── Breakpoints ───────────────────────────────────────────────────────────

/// A breakpoint entry for a single source file.
///
/// Supports two forms:
///
/// 1. Simple — just line numbers:
/// ```toml
/// [[breakpoints]]
/// source = "src/main.cpp"
/// lines = [42, 67]
/// ```
///
/// 2. Detailed — per-breakpoint conditions and log messages:
/// ```toml
/// [[breakpoints]]
/// source = "src/main.cpp"
/// breakpoints = [
///   { line = 42, condition = "x > 5" },
///   { line = 100, log_message = "hit line 100" },
/// ]
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakpointEntry {
    /// Path to the source file (resolved via path mapper).
    pub source: String,

    /// Simple form: just line numbers. Mutually exclusive with `breakpoints`.
    #[serde(default)]
    pub lines: Vec<u64>,

    /// Detailed form: per-breakpoint specs. Mutually exclusive with `lines`.
    #[serde(default)]
    pub breakpoints: Vec<BreakpointSpec>,
}

/// A single breakpoint specification with optional condition and log message.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakpointSpec {
    /// Line number (1-based).
    pub line: u64,

    /// Optional conditional expression.
    #[serde(default)]
    pub condition: Option<String>,

    /// Optional log message (DAP logpoint).
    #[serde(default)]
    pub log_message: Option<String>,
}

impl BreakpointEntry {
    /// Convert this entry into a flat list of SourceBreakpoint items.
    pub fn to_source_breakpoints(&self) -> Vec<SourceBreakpoint> {
        if !self.breakpoints.is_empty() {
            self.breakpoints
                .iter()
                .map(|bp| SourceBreakpoint {
                    line: bp.line,
                    column: None,
                    condition: bp.condition.clone(),
                    hit_condition: None,
                    log_message: bp.log_message.clone(),
                    mode: None,
                })
                .collect()
        } else {
            self.lines
                .iter()
                .map(|&line| SourceBreakpoint {
                    line,
                    column: None,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                    mode: None,
                })
                .collect()
        }
    }
}

// ── Config methods ────────────────────────────────────────────────────────

impl Config {
    /// Load and parse a TOML configuration file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::read(path, e))?;
        let config: Self = toml::from_str(&content).map_err(|e| ConfigError::parse(path, e))?;
        Ok(config)
    }
}

// ── Config error ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: std::path::PathBuf,
        source: Box<toml::de::Error>,
    },
}

impl ConfigError {
    fn read(path: &Path, source: std::io::Error) -> Self {
        ConfigError::Read {
            path: path.to_path_buf(),
            source,
        }
    }

    fn parse(path: &Path, source: toml::de::Error) -> Self {
        ConfigError::Parse {
            path: path.to_path_buf(),
            source: Box::new(source),
        }
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Read { path, source } => {
                write!(
                    f,
                    "failed to read config file '{}': {}",
                    path.display(),
                    source
                )
            }
            ConfigError::Parse { path, source } => {
                write!(
                    f,
                    "failed to parse config file '{}': {}",
                    path.display(),
                    source
                )
            }
        }
    }
}

// ── Auto-setup ────────────────────────────────────────────────────────────

/// Execute the auto-setup sequence from a loaded config.
///
/// This function takes exclusive control of the DAP event stream during
/// setup. Callers **must** spawn the background DAP event loop **after**
/// this function returns.
///
/// Sequence:
/// 1. Register base directories and path aliases
/// 2. Start OpenOCD (if configured)
/// 3. Set liblldb path (must be before adapter start)
/// 4. Start adapter → Connected
/// 5. Initialize handshake → Initialized
/// 6. Launch + poll for initialized event (if launch configured)
/// 7. Set breakpoints (if configured)
/// 8. Configuration done (if launch was performed) → Running
///
/// # Errors
///
/// Returns a descriptive `String` on failure. The caller is responsible for
/// cleanup (session shutdown, OpenOCD shutdown) before exiting.
pub async fn auto_configure(
    session: &Arc<DebugSession>,
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
    config: &Config,
) -> Result<(), String> {
    // ── Step 1: Register path mappings ──────────────────────────────────
    register_path_mappings(session, config).await;

    // ── Step 2: Start OpenOCD (if configured) ──────────────────────────
    start_openocd_if_configured(openocd, config).await?;

    // ── Step 3: Set liblldb path (before adapter start) ────────────────
    if let Some(ref lldb_path) = config.adapter.liblldb_path {
        if !lldb_path.is_empty() {
            session.set_lib_lldb_path(Some(lldb_path.clone())).await;
            info!("liblldb path configured: {lldb_path}");
        }
    }

    // ── Step 4: Start debug adapter ────────────────────────────────────
    start_adapter(session, config).await?;

    // ── Step 5: Initialize handshake ───────────────────────────────────
    let adapter_kind = config.adapter.adapter_kind();
    initialize_session(session, adapter_kind).await?;

    // ── Step 6: Launch if configured ───────────────────────────────────
    let launched = if config.launch.is_configured() {
        launch_debuggee(session, config, adapter_kind).await?;
        // Poll for the initialized event (consumes from adapter event stream)
        wait_for_initialized_event(session).await?;
        true
    } else {
        false
    };

    // ── Step 7: Set breakpoints ────────────────────────────────────────
    set_breakpoints_from_config(session, config).await;

    // ── Step 8: Configuration done ─────────────────────────────────────
    if launched {
        session
            .configuration_done()
            .await
            .map_err(|e| format!("configuration_done failed: {e}"))?;
        info!("Configuration done — session is now Running");
    }

    info!("Auto-setup complete");
    Ok(())
}

// ── Auto-setup helpers ────────────────────────────────────────────────────

async fn register_path_mappings(session: &Arc<DebugSession>, config: &Config) {
    let pm = &config.path_mapping;

    for dir in &pm.base_dirs {
        session.register_base_dir(dir).await;
    }
    if !pm.base_dirs.is_empty() {
        info!("Registered {} base directories", pm.base_dirs.len());
    }

    for (alias, abs_path) in &pm.aliases {
        session.register_path_alias(alias, abs_path).await;
    }
    if !pm.aliases.is_empty() {
        info!("Registered {} path aliases", pm.aliases.len());
    }
}

async fn start_openocd_if_configured(
    openocd: &Arc<RwLock<Option<OpenOcdClient>>>,
    config: &Config,
) -> Result<(), String> {
    let ocd = &config.openocd;
    if !ocd.is_configured() {
        return Ok(());
    }

    let client = OpenOcdClient::new();
    client
        .start(
            &ocd.path,
            &ocd.config_files,
            &ocd.extra_args,
            ocd.log_dir.as_deref(),
        )
        .await
        .map_err(|e| format!("OpenOCD start failed: {e}"))?;

    *openocd.write().await = Some(client);
    info!("OpenOCD started with config files: {:?}", ocd.config_files);
    Ok(())
}

async fn start_adapter(session: &Arc<DebugSession>, config: &Config) -> Result<(), String> {
    let adapter = &config.adapter;
    let adapter_config = AdapterConfig {
        path: adapter.path.clone(),
        kind: adapter.adapter_kind(),
        args: adapter.args.clone(),
    };

    session
        .start(&adapter_config)
        .await
        .map_err(|e| format!("adapter start failed: {e}"))?;

    info!(
        "Adapter started: {} (kind: {:?})",
        adapter.path,
        adapter.adapter_kind()
    );
    Ok(())
}

async fn initialize_session(
    session: &Arc<DebugSession>,
    adapter_kind: AdapterKind,
) -> Result<(), String> {
    let default_adapter_id = match adapter_kind {
        AdapterKind::Gdb => "gdb",
        AdapterKind::Codelldb => "lldb",
    };

    session
        .initialize(InitializeRequestArguments {
            client_id: Some("teledap".into()),
            client_name: Some("TeleDAP".into()),
            adapter_id: Some(default_adapter_id.into()),
            locale: Some("en-US".into()),
            lines_start_at1: Some(true),
            columns_start_at1: Some(true),
            path_format: Some("path".into()),
            supports_variable_type: Some(true),
            supports_variable_paging: Some(false),
            supports_run_in_terminal_request: Some(false),
            supports_memory_references: Some(true),
            supports_progress_reporting: Some(true),
            supports_invalidated_event: Some(true),
            supports_memory_event: Some(true),
            ..Default::default()
        })
        .await
        .map_err(|e| format!("initialize failed: {e}"))?;

    info!("DAP initialize completed");
    Ok(())
}

async fn launch_debuggee(
    session: &Arc<DebugSession>,
    config: &Config,
    adapter_kind: AdapterKind,
) -> Result<(), String> {
    let launch = &config.launch;
    let program = &launch.program;

    // Resolve program path through the path mapper
    let resolved_program = session
        .resolve_path(program)
        .await
        .unwrap_or_else(|| program.clone());

    let mut launch_extra = serde_json::json!({
        "program": resolved_program,
        "stopOnEntry": launch.stop_on_entry,
    });

    // Add debuggee arguments if provided
    if !launch.args.is_empty() {
        launch_extra["args"] = serde_json::json!(launch.args);
    }

    // Add environment variables if provided
    if let Some(ref env) = launch.env {
        launch_extra["env"] = serde_json::json!(env);
    }

    // Configure remote debugging if gdb_remote is set
    if let Some(ref remote) = launch.gdb_remote {
        match adapter_kind {
            AdapterKind::Gdb => {
                launch_extra["target"] = serde_json::json!(format!("remote {remote}"));
            }
            AdapterKind::Codelldb => {
                launch_extra["processCreateCommands"] =
                    serde_json::json!([format!("gdb-remote {remote}")]);
            }
        }
    }

    let launch_args = LaunchRequestArguments {
        no_debug: None,
        __restart: None,
        extra: launch_extra,
    };

    session
        .launch(launch_args)
        .await
        .map_err(|e| format!("launch failed: {e}"))?;

    info!("Launch request sent for: {resolved_program}");
    Ok(())
}

/// Poll the adapter event stream until the `initialized` event arrives.
///
/// This function *consumes* the event from the stream — after it returns,
/// the background event loop will not see the `initialized` event.
async fn wait_for_initialized_event(session: &Arc<DebugSession>) -> Result<(), String> {
    loop {
        match session.client().recv_event().await {
            Some(event) => {
                let event_name = event.event.clone();

                // Feed all events through the state machine
                let _ = session.handle_event(&event).await;

                if event_name == "initialized" {
                    info!("Received initialized event — adapter is ready");
                    return Ok(());
                }

                // Log any output events
                if event_name == "output" {
                    if let Some(ref body) = event.body {
                        if let Ok(output) = serde_json::from_value::<
                            dap_types::events::OutputEventBody,
                        >(body.clone())
                        {
                            info!("[debuggee] {}", output.output.trim_end());
                        }
                    }
                }
            }
            None => {
                return Err("Event stream closed before initialized event".into());
            }
        }
    }
}

async fn set_breakpoints_from_config(session: &Arc<DebugSession>, config: &Config) {
    for entry in &config.breakpoints {
        let source_bps = entry.to_source_breakpoints();
        if source_bps.is_empty() {
            continue;
        }

        // Resolve the source path through the path mapper
        let resolved_path = session
            .resolve_path(&entry.source)
            .await
            .unwrap_or_else(|| entry.source.clone());

        let file_name = std::path::Path::new(&resolved_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| resolved_path.clone());

        let args = SetBreakpointsArguments {
            source: Source {
                name: Some(file_name),
                path: Some(resolved_path.clone()),
                ..Default::default()
            },
            breakpoints: Some(source_bps),
            lines: None,
            source_modified: None,
        };

        match session.set_breakpoints(args).await {
            Ok(resp) => {
                info!(
                    "Breakpoints set in {}: {} total",
                    resolved_path,
                    resp.breakpoints.len()
                );
                for bp in &resp.breakpoints {
                    if bp.verified {
                        info!("  ✓ line {:?} (id={:?})", bp.line, bp.id);
                    } else {
                        info!(
                            "  ✗ line {:?} unverified (id={:?}): {}",
                            bp.line,
                            bp.id,
                            bp.message.as_deref().unwrap_or("unknown reason")
                        );
                    }
                }
            }
            Err(e) => {
                error!("Failed to set breakpoints in {resolved_path}: {e}");
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Deserialization ──────────────────────────────────────────────────

    #[test]
    fn test_deserialize_minimal() {
        let toml_str = r#"
[adapter]
path = "/usr/bin/codelldb"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.adapter.path, "/usr/bin/codelldb");
        assert_eq!(config.adapter.kind, "codelldb"); // default
        assert!(config.adapter.args.is_empty());
        assert!(!config.launch.is_configured());
        assert!(!config.openocd.is_configured());
        assert!(config.path_mapping.base_dirs.is_empty());
        assert!(config.path_mapping.aliases.is_empty());
        assert!(config.breakpoints.is_empty());
        assert!(config.options.auto_start);
    }

    #[test]
    fn test_deserialize_full() {
        let toml_str = r#"
[adapter]
path = "/opt/codelldb/codelldb"
kind = "gdb"
args = ["--some-flag"]
liblldb_path = "/opt/llvm/lib/liblldb.so"

[launch]
program = "./build/firmware.elf"
args = ["--verbose"]
stop_on_entry = true
gdb_remote = "localhost:3333"

[launch.env]
FOO = "bar"
BAZ = "qux"

[path_mapping]
base_dirs = ["/home/user/project"]
aliases = { "src/main.cpp" = "/home/user/project/src/main.cpp", "lib" = "/home/user/project/lib" }

[openocd]
path = "/usr/bin/openocd"
config_files = ["board/stm32f4discovery.cfg"]
extra_args = ["-d", "2"]
log_dir = "/tmp/ocd_logs"

[[breakpoints]]
source = "src/main.cpp"
lines = [42, 67]

[[breakpoints]]
source = "src/helper.cpp"
breakpoints = [
  { line = 10, condition = "x > 5" },
  { line = 20, log_message = "hit helper" },
]

[options]
auto_start = true
"#;
        let config: Config = toml::from_str(toml_str).unwrap();

        // Adapter
        assert_eq!(config.adapter.path, "/opt/codelldb/codelldb");
        assert_eq!(config.adapter.kind, "gdb");
        assert_eq!(config.adapter.args, vec!["--some-flag"]);
        assert_eq!(
            config.adapter.liblldb_path.as_deref(),
            Some("/opt/llvm/lib/liblldb.so")
        );

        // Launch
        assert_eq!(config.launch.program, "./build/firmware.elf");
        assert_eq!(config.launch.args, vec!["--verbose"]);
        assert!(config.launch.stop_on_entry);
        assert_eq!(config.launch.gdb_remote.as_deref(), Some("localhost:3333"));
        let env = config.launch.env.unwrap();
        assert_eq!(env.get("FOO").map(|s| s.as_str()), Some("bar"));
        assert_eq!(env.get("BAZ").map(|s| s.as_str()), Some("qux"));

        // Path mapping
        assert_eq!(config.path_mapping.base_dirs, vec!["/home/user/project"]);
        assert_eq!(config.path_mapping.aliases.len(), 2);
        assert_eq!(
            config
                .path_mapping
                .aliases
                .get("src/main.cpp")
                .map(|s| s.as_str()),
            Some("/home/user/project/src/main.cpp")
        );

        // OpenOCD
        assert!(config.openocd.is_configured());
        assert_eq!(config.openocd.path, "/usr/bin/openocd");
        assert_eq!(
            config.openocd.config_files,
            vec!["board/stm32f4discovery.cfg"]
        );

        // Breakpoints
        assert_eq!(config.breakpoints.len(), 2);
        assert_eq!(config.breakpoints[0].source, "src/main.cpp");
        assert_eq!(config.breakpoints[0].lines, vec![42, 67]);
        assert_eq!(config.breakpoints[1].source, "src/helper.cpp");
        assert_eq!(config.breakpoints[1].breakpoints.len(), 2);
        assert_eq!(config.breakpoints[1].breakpoints[0].line, 10);
        assert_eq!(
            config.breakpoints[1].breakpoints[0].condition.as_deref(),
            Some("x > 5")
        );
        assert_eq!(config.breakpoints[1].breakpoints[1].line, 20);
        assert_eq!(
            config.breakpoints[1].breakpoints[1].log_message.as_deref(),
            Some("hit helper")
        );

        assert!(config.options.auto_start);
    }

    #[test]
    fn test_deserialize_openocd_omitted() {
        let toml_str = r#"
[adapter]
path = "codelldb"

[launch]
program = "app.elf"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        // OpenOCD should have default (empty) values
        assert!(!config.openocd.is_configured());
        assert!(config.openocd.path.is_empty());
    }

    #[test]
    fn test_deserialize_empty_openocd_section() {
        let toml_str = r#"
[adapter]
path = "codelldb"

[launch]
program = "app.elf"

[openocd]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        // Empty openocd section — should not be configured
        assert!(!config.openocd.is_configured());
        assert!(config.openocd.path.is_empty());
    }

    #[test]
    fn test_deserialize_unknown_field_rejected() {
        let toml_str = r#"
[adapter]
path = "codelldb"
typo_field = "oops"
"#;
        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_unknown_top_level_field_rejected() {
        let toml_str = r#"
[adapter]
path = "codelldb"

[unknown_section]
foo = "bar"
"#;
        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_deserialize_no_breakpoints() {
        let toml_str = r#"
[adapter]
path = "codelldb"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.breakpoints.is_empty());
    }

    #[test]
    fn test_deserialize_auto_start_defaults_true() {
        let toml_str = r#"
[adapter]
path = "codelldb"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.options.auto_start);
    }

    #[test]
    fn test_deserialize_auto_start_false() {
        let toml_str = r#"
[adapter]
path = "codelldb"

[options]
auto_start = false
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.options.auto_start);
    }

    #[test]
    fn test_adapter_kind_gdb() {
        let section = AdapterSection {
            kind: "gdb".into(),
            ..Default::default()
        };
        assert!(matches!(section.adapter_kind(), AdapterKind::Gdb));
    }

    #[test]
    fn test_adapter_kind_default_codelldb() {
        let section = AdapterSection::default();
        assert!(matches!(section.adapter_kind(), AdapterKind::Codelldb));
    }

    // ── BreakpointEntry ──────────────────────────────────────────────────

    #[test]
    fn test_breakpoint_entry_simple_lines() {
        let entry = BreakpointEntry {
            source: "src/main.cpp".into(),
            lines: vec![10, 20],
            breakpoints: vec![],
        };
        let bps = entry.to_source_breakpoints();
        assert_eq!(bps.len(), 2);
        assert_eq!(bps[0].line, 10);
        assert!(bps[0].condition.is_none());
        assert_eq!(bps[1].line, 20);
        assert!(bps[1].log_message.is_none());
    }

    #[test]
    fn test_breakpoint_entry_detailed() {
        let entry = BreakpointEntry {
            source: "src/main.cpp".into(),
            lines: vec![],
            breakpoints: vec![
                BreakpointSpec {
                    line: 42,
                    condition: Some("x > 5".into()),
                    log_message: None,
                },
                BreakpointSpec {
                    line: 100,
                    condition: None,
                    log_message: Some("hit".into()),
                },
            ],
        };
        let bps = entry.to_source_breakpoints();
        assert_eq!(bps.len(), 2);
        assert_eq!(bps[0].line, 42);
        assert_eq!(bps[0].condition.as_deref(), Some("x > 5"));
        assert_eq!(bps[1].line, 100);
        assert_eq!(bps[1].log_message.as_deref(), Some("hit"));
    }

    #[test]
    fn test_launch_section_not_configured_by_default() {
        let section = LaunchSection::default();
        assert!(!section.is_configured());
    }

    #[test]
    fn test_launch_section_configured_when_program_set() {
        let section = LaunchSection {
            program: "app.elf".into(),
            ..Default::default()
        };
        assert!(section.is_configured());
    }

    #[test]
    fn test_openocd_section_not_configured_by_default() {
        let section = OpenOcdSection::default();
        assert!(!section.is_configured());
    }

    #[test]
    fn test_openocd_section_configured_when_path_and_config_files_set() {
        let section = OpenOcdSection {
            path: "/usr/bin/openocd".into(),
            config_files: vec!["board/stm32f4discovery.cfg".into()],
            ..Default::default()
        };
        assert!(section.is_configured());
    }

    #[test]
    fn test_openocd_section_not_configured_with_only_path() {
        let section = OpenOcdSection {
            path: "/usr/bin/openocd".into(),
            config_files: vec![],
            ..Default::default()
        };
        assert!(!section.is_configured());
    }

    // ── ConfigError Display ──────────────────────────────────────────────

    #[test]
    fn test_config_error_read_display() {
        let err = ConfigError::Read {
            path: std::path::PathBuf::from("test.toml"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        };
        let msg = err.to_string();
        assert!(msg.contains("test.toml"));
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn test_config_error_parse_display() {
        // Create a real toml parse error
        let toml_str = "invalid {{{ toml";
        let result: Result<toml::Value, _> = toml::from_str(toml_str);
        if let Err(e) = result {
            let err = ConfigError::Parse {
                path: std::path::PathBuf::from("bad.toml"),
                source: Box::new(e),
            };
            let msg = err.to_string();
            assert!(msg.contains("bad.toml"));
        }
    }
}
