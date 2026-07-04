//! Internal logger — background task with ring buffer and JSONL file writer.
//!
//! All I/O is handled asynchronously on a dedicated tokio task so that
//! `TraceHandle::trace()` calls never block.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::entry::TraceEntry;

/// Creates a background consumer task for trace entries.
///
/// # Arguments
/// * `log_dir` — Optional directory for `.jsonl` file output. If `None`,
///   only the in-memory ring buffer is used.
/// * `max_ring_size` — Maximum entries in the in-memory ring buffer.
///
/// # Returns
/// `(tx, ring, session_id, JoinHandle<()>)` — The tx and ring are used to
/// construct a `TraceHandle`; the JoinHandle can be awaited for graceful
/// shutdown.
#[allow(clippy::type_complexity)]
pub(crate) fn spawn_logger(
    log_dir: Option<PathBuf>,
    max_ring_size: usize,
) -> (
    mpsc::UnboundedSender<TraceEntry>,
    Arc<RwLock<VecDeque<TraceEntry>>>,
    String,
    JoinHandle<()>,
) {
    let (tx, mut rx) = mpsc::unbounded_channel::<TraceEntry>();
    let ring = Arc::new(RwLock::new(VecDeque::with_capacity(max_ring_size)));
    let session_id = uuid::Uuid::new_v4().to_string();

    let ring_clone = ring.clone();
    let sid = session_id.clone();
    let handle = tokio::spawn(async move {
        // Open JSONL file if directory is configured
        let mut file = match &log_dir {
            Some(dir) => {
                if let Err(e) = tokio::fs::create_dir_all(dir).await {
                    tracing::error!("Failed to create log directory {:?}: {}", dir, e);
                    None
                } else {
                    let path = dir.join(format!("teledap_{}.jsonl", &sid[..8]));
                    match OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .await
                    {
                        Ok(f) => {
                            tracing::info!("Trace file: {:?}", path);
                            Some(f)
                        }
                        Err(e) => {
                            tracing::error!("Failed to open trace file {:?}: {}", path, e);
                            None
                        }
                    }
                }
            }
            None => None,
        };

        // Consume trace entries until channel closes
        while let Some(entry) = rx.recv().await {
            // 1. Push to ring buffer (lock held very briefly)
            {
                let mut buf = ring_clone.write();
                if buf.len() >= max_ring_size {
                    buf.pop_front();
                }
                buf.push_back(entry.clone());
            }

            // 2. Append to JSONL file
            if let Some(ref mut f) = file {
                match serde_json::to_string(&entry) {
                    Ok(mut line) => {
                        line.push('\n');
                        if let Err(e) = f.write_all(line.as_bytes()).await {
                            tracing::error!("Failed to write trace entry: {}", e);
                            file = None;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to serialize trace entry: {}", e);
                    }
                }
            }
        }

        // Flush on shutdown
        if let Some(ref mut f) = file {
            let _ = f.flush().await;
            tracing::info!("Trace file flushed.");
        }
    });

    (tx, ring, session_id, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{TraceDirection, TraceSource};
    use chrono::Utc;

    #[tokio::test]
    async fn test_log_and_retrieve() {
        let (tx, ring, _sid, _handle) = spawn_logger(None, 10);

        let entry = make_entry("test_cmd", 0);
        tx.send(entry).unwrap();

        // Give the background task a moment
        tokio::task::yield_now().await;

        let buf = ring.read();
        assert_eq!(buf.len(), 1);
        assert_eq!(buf[0].command, "test_cmd");
        assert_eq!(buf[0].seq, 0);
    }

    #[tokio::test]
    async fn test_ring_buffer_eviction() {
        let max_size = 5;
        let (tx, ring, _sid, _handle) = spawn_logger(None, max_size);

        for i in 0..10 {
            tx.send(make_entry(&format!("cmd_{}", i), i)).unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let buf = ring.read();
        assert_eq!(buf.len(), max_size);
        assert_eq!(buf[0].command, "cmd_5");
        assert_eq!(buf[4].command, "cmd_9");
    }

    #[tokio::test]
    async fn test_session_id_unique() {
        let (_tx1, _ring1, sid1, _h1) = spawn_logger(None, 10);
        let (_tx2, _ring2, sid2, _h2) = spawn_logger(None, 10);
        assert_ne!(sid1, sid2);
    }

    #[tokio::test]
    async fn test_jsonl_file_output() {
        let tmpdir = std::env::temp_dir().join(format!("teledap_test_{}", uuid::Uuid::new_v4()));
        let (tx, _ring, _sid, handle) = spawn_logger(Some(tmpdir.clone()), 10);

        tx.send(make_entry("file_test", 0)).unwrap();
        drop(tx); // Close channel
        let _ = handle.await;

        // Find the JSONL file
        let entries: Vec<_> = std::fs::read_dir(&tmpdir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("teledap_"))
            .collect();

        assert_eq!(entries.len(), 1);
        let content = std::fs::read_to_string(entries[0].path()).unwrap();
        let parsed: TraceEntry = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed.command, "file_test");

        let _ = std::fs::remove_dir_all(&tmpdir);
    }

    fn make_entry(cmd: &str, seq: u64) -> TraceEntry {
        TraceEntry {
            timestamp: Utc::now(),
            source: TraceSource::Internal,
            direction: TraceDirection::Outbound,
            command: cmd.into(),
            payload: None,
            result: None,
            duration_us: None,
            session_id: "test".into(),
            seq,
        }
    }
}
