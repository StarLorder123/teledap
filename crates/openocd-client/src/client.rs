//! OpenOcdClient — manages an OpenOCD child process, provides Tcl command
//! communication, and captures stdout/stderr output to optional log files.
//!
//! # Architecture
//!
//! ```text
//!                     ┌──────────────────────────┐
//!  OpenOcdClient      │   openocd process         │
//!                     │                           │
//!  send_command() ───►│ stdin  (Tcl commands)     │
//!  stdout_lines ◄─────│ stdout (responses + logs)  │──► log file (optional)
//!  (discard/log) ◄────│ stderr (diagnostics)       │──► log file (optional)
//!                     └──────────────────────────┘
//! ```
//!
//! A background task continuously reads stdout line by line, pushes lines
//! to a shared buffer, and optionally writes to a log file. `send_command()`
//! writes a Tcl command to stdin, then collects lines from the shared buffer
//! until the `\x1a` response terminator is found. stderr is drained by a
//! separate background task to prevent pipe buffer deadlock.

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, Notify, RwLock};
use tokio::time::{timeout, Duration};

use crate::error::OpenOcdClientError;

/// Default timeout for Tcl command responses (5 seconds).
pub const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 5000;

/// Maximum lines retained in the stdout line buffer.
const MAX_STDOUT_BUFFER_LINES: usize = 1000;

/// Manages an OpenOCD process with Tcl command communication and log capture.
pub struct OpenOcdClient {
    /// OpenOCD child process handle.
    child: Mutex<Option<Child>>,

    /// Stdin writer for sending Tcl commands.
    stdin: Mutex<Option<ChildStdin>>,

    /// Directory for stdout/stderr log files (None = discard).
    log_dir: RwLock<Option<PathBuf>>,

    /// Serializes send_command calls to prevent interleaved responses.
    command_lock: Mutex<()>,

    /// Process start time for uptime reporting.
    started_at: Mutex<Option<Instant>>,

    /// Shared buffer of recent stdout lines, consumed by send_command.
    stdout_lines: Arc<Mutex<VecDeque<String>>>,

    /// Signalled when new stdout lines are available.
    stdout_notify: Arc<Notify>,

    /// Whether the process is currently running.
    running: Arc<AtomicBool>,
}

impl OpenOcdClient {
    /// Creates a new `OpenOcdClient` with no running process.
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            log_dir: RwLock::new(None),
            command_lock: Mutex::new(()),
            started_at: Mutex::new(None),
            stdout_lines: Arc::new(Mutex::new(VecDeque::new())),
            stdout_notify: Arc::new(Notify::new()),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns true if the OpenOCD process is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    // ── Process Lifecycle ─────────────────────────────────────────────

    /// Spawns OpenOCD as a child process with the given configuration.
    ///
    /// # Arguments
    /// * `openocd_path` — Absolute path to the OpenOCD binary.
    /// * `config_files` — OpenOCD config files in order (e.g. `["board/stm32f4discovery.cfg"]`).
    /// * `extra_args` — Additional CLI arguments (e.g. `["-d", "2"]` for debug level 2).
    /// * `log_dir` — If set, stdout and stderr are written to files in this directory.
    ///   If `None`, output is discarded (but still drained to avoid pipe deadlock).
    pub async fn start(
        &self,
        openocd_path: &str,
        config_files: &[String],
        extra_args: &[String],
        log_dir: Option<&str>,
    ) -> Result<(), OpenOcdClientError> {
        if self.is_running() {
            return Err(OpenOcdClientError::AlreadyRunning);
        }

        // Build the command: openocd -f config1 -f config2 ... extra_args
        let mut cmd = Command::new(openocd_path);
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        for cfg in config_files {
            cmd.arg("-f").arg(cfg);
        }
        for arg in extra_args {
            cmd.arg(arg);
        }

        let mut child = cmd.spawn().map_err(|e| {
            OpenOcdClientError::SpawnFailed(format!("Failed to spawn '{}': {}", openocd_path, e))
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            OpenOcdClientError::SpawnFailed("OpenOCD stdout is not available".into())
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            OpenOcdClientError::SpawnFailed("OpenOCD stderr is not available".into())
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            OpenOcdClientError::SpawnFailed("OpenOCD stdin is not available".into())
        })?;

        // Store log_dir
        let log_path = log_dir.map(PathBuf::from);
        *self.log_dir.write().await = log_path.clone();

        // Spawn background stdout reader
        {
            let stdout_lines = self.stdout_lines.clone();
            let stdout_notify = self.stdout_notify.clone();
            let log_path_for_stdout = log_path.clone();
            let running = self.running.clone();
            running.store(true, Ordering::SeqCst);

            tokio::spawn(async move {
                read_stream_to_buffer_and_log(
                    stdout,
                    stdout_lines,
                    stdout_notify,
                    log_path_for_stdout,
                    "openocd_stdout.log",
                    running.clone(),
                )
                .await;
            });
        }

        // Spawn background stderr reader (drain to prevent pipe deadlock)
        {
            let log_path_for_stderr = log_path.clone();
            let running = self.running.clone();

            tokio::spawn(async move {
                read_stream_to_log_or_discard(
                    stderr,
                    log_path_for_stderr,
                    "openocd_stderr.log",
                    running.clone(),
                )
                .await;
            });
        }

        *self.stdin.lock().await = Some(stdin);
        *self.child.lock().await = Some(child);
        *self.started_at.lock().await = Some(Instant::now());

        tracing::info!(
            path = %openocd_path,
            configs = ?config_files,
            log_dir = ?log_path,
            "OpenOCD started"
        );

        Ok(())
    }

    // ── Tcl Command ───────────────────────────────────────────────────

    /// Send a Tcl command to OpenOCD and wait for the response.
    ///
    /// OpenOCD terminates each command response with the ASCII SUB character
    /// (`\x1a`, byte 0x1A). This method collects stdout lines until the
    /// terminator is found or the timeout expires.
    ///
    /// # Arguments
    /// * `command` — Tcl command to send (e.g. `"reset halt"`).
    /// * `timeout_ms` — Response timeout in milliseconds.
    pub async fn send_command(
        &self,
        command: &str,
        timeout_ms: u64,
    ) -> Result<String, OpenOcdClientError> {
        let _lock = self.command_lock.lock().await;

        // Verify we're connected
        {
            let stdin_guard = self.stdin.lock().await;
            if stdin_guard.is_none() {
                return Err(OpenOcdClientError::NotConnected(
                    "OpenOCD is not running".into(),
                ));
            }
        }

        // Write the command to stdin
        {
            let mut stdin_guard = self.stdin.lock().await;
            let stdin = stdin_guard.as_mut().unwrap();
            let cmd_line = format!("{}\n", command);
            stdin.write_all(cmd_line.as_bytes()).await?;
            stdin.flush().await?;
        }

        tracing::debug!(command = %command, "Tcl command sent");

        // Collect response lines until \x1a or timeout
        let mut response_lines: Vec<String> = Vec::new();
        let deadline = Duration::from_millis(timeout_ms);

        let collect_result = timeout(deadline, async {
            loop {
                // Wait for new data
                self.stdout_notify.notified().await;

                let mut lines = self.stdout_lines.lock().await;
                while let Some(line) = lines.pop_front() {
                    if line.contains('\x1a') {
                        // Found terminator — extract text before \x1a
                        let clean = line.replace('\x1a', "");
                        if !clean.is_empty() {
                            response_lines.push(clean);
                        }
                        return;
                    }
                    response_lines.push(line);
                }
            }
        })
        .await;

        match collect_result {
            Ok(()) => {
                // Normal termination — got \x1a
                let response = response_lines.join("\n");
                tracing::debug!(
                    command = %command,
                    response_len = response.len(),
                    "Tcl response received"
                );
                Ok(response)
            }
            Err(_elapsed) => {
                // Timeout — return what we have so far
                let response = response_lines.join("\n");
                tracing::warn!(
                    command = %command,
                    timeout_ms = timeout_ms,
                    partial_len = response.len(),
                    "Tcl command timed out, returning partial response"
                );
                if response.is_empty() {
                    Err(OpenOcdClientError::Timeout {
                        command: command.to_string(),
                        timeout_ms,
                    })
                } else {
                    Ok(response)
                }
            }
        }
    }

    // ── Output Reading ────────────────────────────────────────────────

    /// Read the last `count` lines from the OpenOCD stdout log file.
    ///
    /// Returns an error if no log directory was configured.
    pub async fn read_output(&self, count: usize) -> Result<Vec<String>, OpenOcdClientError> {
        let log_dir = self.log_dir.read().await;
        let log_path = match log_dir.as_ref() {
            Some(dir) => dir.join("openocd_stdout.log"),
            None => {
                return Err(OpenOcdClientError::NotConnected(
                    "No log directory configured. Call openocd_start with log_dir to enable output capture."
                        .into(),
                ));
            }
        };
        drop(log_dir);

        read_tail_lines(&log_path, count).await
    }

    /// Read the last `count` lines from the OpenOCD stderr log file.
    ///
    /// Returns an error if no log directory was configured.
    pub async fn read_errors(&self, count: usize) -> Result<Vec<String>, OpenOcdClientError> {
        let log_dir = self.log_dir.read().await;
        let log_path = match log_dir.as_ref() {
            Some(dir) => dir.join("openocd_stderr.log"),
            None => {
                return Err(OpenOcdClientError::NotConnected(
                    "No log directory configured. Call openocd_start with log_dir to enable output capture."
                        .into(),
                ));
            }
        };
        drop(log_dir);

        read_tail_lines(&log_path, count).await
    }

    // ── Status ────────────────────────────────────────────────────────

    /// Return status information about the OpenOCD process.
    pub async fn status(&self) -> serde_json::Value {
        let running = self.is_running();
        let log_dir = self.log_dir.read().await.clone();
        let uptime = self.started_at.lock().await.map(|t| t.elapsed().as_secs());

        serde_json::json!({
            "running": running,
            "pid": None::<u32>,  // PID not easily accessible from tokio::process::Child
            "uptime_secs": uptime,
            "log_dir": log_dir.map(|p| p.to_string_lossy().to_string()),
        })
    }

    // ── Shutdown ──────────────────────────────────────────────────────

    /// Send the shutdown command to OpenOCD and terminate the process.
    ///
    /// Tries to send "shutdown" gracefully, then kills the process.
    pub async fn shutdown(&self) -> Result<(), OpenOcdClientError> {
        // Best-effort graceful shutdown via Tcl command
        if self.is_running() {
            let _ = self.send_command("shutdown", 2000).await;
        }

        let mut child_guard = self.child.lock().await;
        if let Some(ref mut child) = *child_guard {
            let _ = child.kill().await;
        }
        *child_guard = None;
        *self.stdin.lock().await = None;
        self.running.store(false, Ordering::SeqCst);

        tracing::info!("OpenOCD shut down.");
        Ok(())
    }
}

impl Default for OpenOcdClient {
    fn default() -> Self {
        Self::new()
    }
}

// ── Background Stream Readers ─────────────────────────────────────────

/// Reads lines from stdout, pushes to a shared buffer for `send_command`,
/// and optionally writes to a log file.
async fn read_stream_to_buffer_and_log(
    stdout: ChildStdout,
    buffer: Arc<Mutex<VecDeque<String>>>,
    notify: Arc<Notify>,
    log_dir: Option<PathBuf>,
    log_name: &str,
    running: Arc<AtomicBool>,
) {
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // Open log file if log_dir is set
    let mut log_file: Option<File> = None;
    if let Some(ref dir) = log_dir {
        match open_log_file(dir, log_name).await {
            Ok(f) => log_file = Some(f),
            Err(e) => {
                tracing::error!(
                    "Failed to open OpenOCD log file {:?}: {}",
                    dir.join(log_name),
                    e
                );
            }
        }
    }

    while let Ok(Some(line)) = lines.next_line().await {
        // Write to log file
        if let Some(ref mut f) = log_file {
            let log_line = format!("{}\n", line);
            if let Err(e) = f.write_all(log_line.as_bytes()).await {
                tracing::error!("Failed to write to OpenOCD stdout log: {}", e);
            }
            let _ = f.flush().await;
        }

        // Push to shared buffer (keep the \x1a terminator in the line for detection)
        {
            let mut buf = buffer.lock().await;
            buf.push_back(line.clone());
            // Trim buffer if too large
            while buf.len() > MAX_STDOUT_BUFFER_LINES {
                buf.pop_front();
            }
        }

        // Notify waiters
        notify.notify_waiters();
    }

    running.store(false, Ordering::SeqCst);
    tracing::info!("OpenOCD stdout reader exited.");
}

/// Reads lines from stderr and either writes to a log file or discards them.
/// This is essential to prevent the stderr pipe buffer from filling up and
/// deadlocking the OpenOCD process.
async fn read_stream_to_log_or_discard(
    stderr: ChildStderr,
    log_dir: Option<PathBuf>,
    log_name: &str,
    _running: Arc<AtomicBool>,
) {
    let reader = BufReader::new(stderr);
    let mut lines = reader.lines();

    // Open log file if log_dir is set
    let mut log_file: Option<File> = None;
    if let Some(ref dir) = log_dir {
        match open_log_file(dir, log_name).await {
            Ok(f) => log_file = Some(f),
            Err(e) => {
                tracing::error!(
                    "Failed to open OpenOCD log file {:?}: {}",
                    dir.join(log_name),
                    e
                );
            }
        }
    }

    while let Ok(Some(line)) = lines.next_line().await {
        if let Some(ref mut f) = log_file {
            let log_line = format!("{}\n", line);
            if let Err(e) = f.write_all(log_line.as_bytes()).await {
                tracing::error!("Failed to write to OpenOCD stderr log: {}", e);
            }
            let _ = f.flush().await;
        }
        // If no log file, line is simply discarded (but the pipe is drained)
    }

    tracing::info!("OpenOCD stderr reader exited.");
}

/// Opens (or truncates) a log file in the given directory.
async fn open_log_file(dir: &PathBuf, name: &str) -> io::Result<File> {
    // Create directory if needed
    tokio::fs::create_dir_all(dir).await?;
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dir.join(name))
        .await
}

/// Reads the last `count` lines from a file efficiently.
async fn read_tail_lines(path: &PathBuf, count: usize) -> Result<Vec<String>, OpenOcdClientError> {
    if count == 0 {
        return Ok(Vec::new());
    }

    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(OpenOcdClientError::Io)?;

    let all_lines: Vec<&str> = content.lines().collect();
    let start = if all_lines.len() > count {
        all_lines.len() - count
    } else {
        0
    };

    Ok(all_lines[start..].iter().map(|s| s.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_new() {
        let client = OpenOcdClient::new();
        assert!(!client.is_running());
    }

    #[test]
    fn test_default() {
        let client = OpenOcdClient::default();
        assert!(!client.is_running());
    }

    #[test]
    fn test_send_command_not_connected() {
        // send_command needs a tokio runtime
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let client = OpenOcdClient::new();
            let result = client.send_command("reset halt", 1000).await;
            assert!(result.is_err());
            match result {
                Err(OpenOcdClientError::NotConnected(_)) => {} // expected
                other => panic!("Expected NotConnected, got {:?}", other),
            }
        });
    }

    #[test]
    fn test_status_when_not_running() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let client = OpenOcdClient::new();
            let status = client.status().await;
            assert_eq!(status["running"], false);
            assert_eq!(status["log_dir"], serde_json::Value::Null);
        });
    }

    #[test]
    fn test_read_output_no_log_dir() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let client = OpenOcdClient::new();
            let result = client.read_output(10).await;
            assert!(result.is_err());
            match result {
                Err(OpenOcdClientError::NotConnected(ref msg)) => {
                    assert!(msg.contains("log directory"));
                }
                other => panic!("Expected NotConnected about log dir, got {:?}", other),
            }
        });
    }
}
