use crate::audit_tracker::model::{AuditLogEntry, LogDirection, LogSource};
use chrono::Utc;
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

/// Non-blocking, async audit logger for debugging operations.
///
/// All `log()` calls complete in constant time by pushing to an unbounded
/// channel. A single background consumer task handles disk I/O and ring
/// buffer maintenance, ensuring that debug physical links are never
/// blocked by logging overhead.
pub struct AuditLogger {
    /// Sender side of the log channel. Cloning this (via Arc) shares the channel.
    tx: mpsc::UnboundedSender<AuditLogEntry>,

    /// Thread-safe ring buffer of recent entries (parking_lot for short lock holds).
    ring: Arc<RwLock<VecDeque<AuditLogEntry>>>,

    /// Maximum ring buffer capacity.
    max_ring_size: usize,

    /// Session correlation ID.
    session_id: String,

    /// Monotonic sequence counter.
    seq: AtomicU64,
}

impl AuditLogger {
    /// Creates a new `AuditLogger` and spawns the background consumer task.
    ///
    /// # Arguments
    /// * `log_dir` — Optional directory for `.jsonl` file output. If `None`,
    ///   only the in-memory ring buffer is used.
    /// * `max_ring_size` — Maximum entries in the in-memory ring buffer.
    ///
    /// # Returns
    /// `(Arc<AuditLogger>, JoinHandle<()>)` — The logger handle and the
    /// background task handle (for awaiting graceful shutdown).
    pub fn new(
        log_dir: Option<PathBuf>,
        max_ring_size: usize,
    ) -> (Arc<AuditLogger>, tokio::task::JoinHandle<()>) {
        let (tx, mut rx) = mpsc::unbounded_channel::<AuditLogEntry>();
        let ring = Arc::new(RwLock::new(VecDeque::with_capacity(max_ring_size)));
        let session_id = uuid::Uuid::new_v4().to_string();

        let ring_clone = ring.clone();
        let session_id_clone = session_id.clone();
        let handle = tokio::spawn(async move {
            // Open log file if directory is configured
            let mut file = match &log_dir {
                Some(dir) => {
                    if let Err(e) = tokio::fs::create_dir_all(dir).await {
                        tracing::error!("Failed to create log directory {:?}: {}", dir, e);
                        None
                    } else {
                        let path = dir.join(format!("teledap_{}.jsonl", &session_id_clone[..8]));
                        match OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&path)
                            .await
                        {
                            Ok(f) => {
                                tracing::info!("Audit log file: {:?}", path);
                                Some(f)
                            }
                            Err(e) => {
                                tracing::error!("Failed to open audit log {:?}: {}", path, e);
                                None
                            }
                        }
                    }
                }
                None => None,
            };

            // Consume log entries until channel closes
            while let Some(entry) = rx.recv().await {
                // 1. Push to ring buffer (lock held very briefly)
                {
                    let mut buf = ring_clone.write();
                    if buf.len() >= max_ring_size {
                        buf.pop_front();
                    }
                    buf.push_back(entry.clone());
                }

                // 2. Append to JSONL file (async I/O, never blocks the channel)
                let write_result = if let Some(ref mut f) = file {
                    let line = serde_json::to_string(&entry).unwrap_or_default();
                    let r1 = f.write_all(line.as_bytes()).await;
                    let r2 = f.write_all(b"\n").await;
                    r1.or(r2)
                } else {
                    Ok(())
                };

                if let Err(e) = write_result {
                    tracing::error!("Failed to write audit log: {}", e);
                    file = None; // Stop trying to write
                }
            }

            // Graceful shutdown: flush and close the file
            if let Some(ref mut f) = file {
                let _ = f.flush().await;
                tracing::info!("Audit log file flushed.");
            }
        });

        (
            Arc::new(AuditLogger {
                tx,
                ring,
                max_ring_size,
                session_id,
                seq: AtomicU64::new(0),
            }),
            handle,
        )
    }

    /// Non-blocking log entry push.
    ///
    /// This method always succeeds immediately — the unbounded channel
    /// guarantees that driver code is never stalled by disk I/O.
    pub fn log(
        &self,
        source: LogSource,
        direction: LogDirection,
        command: &str,
        payload: Option<serde_json::Value>,
        result: Option<String>,
        duration_us: Option<i64>,
    ) {
        let entry = AuditLogEntry {
            timestamp: Utc::now(),
            source,
            direction,
            command: command.to_string(),
            payload,
            result,
            duration_us,
            session_id: self.session_id.clone(),
            seq: self.seq.fetch_add(1, Ordering::Relaxed),
        };
        // Unbounded send — never fails
        let _ = self.tx.send(entry);
    }

    /// Query recent entries from the in-memory ring buffer.
    ///
    /// Returns entries in reverse chronological order (most recent first),
    /// up to `count` entries.
    pub fn get_logs(&self, count: usize) -> Vec<AuditLogEntry> {
        let ring = self.ring.read();
        ring.iter().rev().take(count).cloned().collect()
    }

    /// Returns the session correlation ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

// On Drop: when all Arc<AuditLogger> references are released, the sender
// is dropped, the channel closes, and the background task exits after
// flushing the JSONL file.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit_tracker::model::LogDirection;
    use crate::audit_tracker::model::LogSource;

    #[tokio::test]
    async fn test_log_and_retrieve() {
        let (logger, _handle) = AuditLogger::new(None, 10);

        logger.log(
            LogSource::Internal,
            LogDirection::Outbound,
            "test_command",
            None,
            Some("success".into()),
            None,
        );

        // Give the background task a moment to process
        tokio::task::yield_now().await;

        let logs = logger.get_logs(10);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].command, "test_command");
        assert_eq!(logs[0].result.as_deref(), Some("success"));
        assert_eq!(logs[0].seq, 0);
    }

    #[tokio::test]
    async fn test_ring_buffer_eviction() {
        let max_size = 5;
        let (logger, _handle) = AuditLogger::new(None, max_size);

        // Push more entries than the ring can hold
        for i in 0..10 {
            logger.log(
                LogSource::Internal,
                LogDirection::Outbound,
                &format!("cmd_{}", i),
                None,
                None,
                None,
            );
        }

        // Allow background task to process
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let logs = logger.get_logs(20);
        // Should only have max_size entries (the most recent ones)
        assert_eq!(logs.len(), max_size);
        // Most recent entries should be cmd_9, cmd_8, ...
        assert_eq!(logs[0].command, "cmd_9");
        assert_eq!(logs[4].command, "cmd_5");
    }

    #[tokio::test]
    async fn test_ring_buffer_reverse_chronological() {
        let (logger, _handle) = AuditLogger::new(None, 100);

        logger.log(LogSource::Internal, LogDirection::Outbound, "first", None, None, None);
        logger.log(LogSource::Internal, LogDirection::Outbound, "second", None, None, None);
        logger.log(LogSource::Internal, LogDirection::Outbound, "third", None, None, None);

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let logs = logger.get_logs(10);
        // Most recent first
        assert_eq!(logs[0].command, "third");
        assert_eq!(logs[1].command, "second");
        assert_eq!(logs[2].command, "first");
    }

    #[tokio::test]
    async fn test_session_id_is_unique() {
        let (logger1, _h1) = AuditLogger::new(None, 10);
        let (logger2, _h2) = AuditLogger::new(None, 10);

        assert_ne!(logger1.session_id(), logger2.session_id());
    }

    #[tokio::test]
    async fn test_log_with_payload() {
        let (logger, _handle) = AuditLogger::new(None, 10);

        let payload = serde_json::json!({"key": "value", "num": 42});
        logger.log(
            LogSource::McpTrigger,
            LogDirection::Inbound,
            "auto_launch",
            Some(payload.clone()),
            Some("ok".into()),
            Some(1500),
        );

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let logs = logger.get_logs(1);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].source, LogSource::McpTrigger);
        assert_eq!(logs[0].direction, LogDirection::Inbound);
        assert_eq!(logs[0].payload.as_ref().unwrap()["key"], "value");
        assert_eq!(logs[0].duration_us, Some(1500));
    }

    #[tokio::test]
    async fn test_jsonl_file_output() {
        let tmpdir = std::env::temp_dir().join(format!("teledap_test_{}", uuid::Uuid::new_v4()));
        let (logger, handle) = AuditLogger::new(Some(tmpdir.clone()), 10);

        logger.log(LogSource::Internal, LogDirection::Outbound, "file_test", None, None, None);

        // Drop the logger to close the channel
        drop(logger);

        // Wait for background task to finish
        let _ = handle.await;

        // Find the JSONL file
        let mut entries = std::fs::read_dir(&tmpdir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("teledap_")
            })
            .collect::<Vec<_>>();

        assert_eq!(entries.len(), 1);

        let content = std::fs::read_to_string(entries.pop().unwrap().path()).unwrap();
        let parsed: AuditLogEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed.command, "file_test");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmpdir);
    }
}
