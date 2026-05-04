use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use redb::Database;
use tokio::runtime::Runtime;
use tokio::sync::watch;

use crate::errors;

/// Owns the per-build runtime state that was previously stored in process
/// globals: the tokio runtime, the interrupt flag, the interrupt broadcast
/// channel, and the checksum/mtime caches. Creating a fresh `BuildContext`
/// gives an isolated build environment — the prerequisite for daemon mode,
/// LSP integration, and parallel test harnesses.
pub struct BuildContext {
    runtime: Runtime,
    interrupted: AtomicBool,
    interrupt_tx: watch::Sender<bool>,
    interrupt_rx: watch::Receiver<bool>,
    /// In-memory checksum cache — avoids re-reading and re-hashing the same
    /// file multiple times within a single build run.
    pub(crate) checksum_cache: Mutex<Option<HashMap<PathBuf, String>>>,
    /// Persistent mtime database — maps (path, mtime) → checksum across builds.
    pub(crate) mtime_db: Mutex<Option<Database>>,
    /// Whether mtime pre-check is enabled. Set to false by `--no-mtime-cache`.
    pub(crate) mtime_enabled: AtomicBool,
}

impl BuildContext {
    pub(crate) fn new() -> Self {
        let runtime = Runtime::new().expect(errors::TOKIO_RUNTIME);
        let (interrupt_tx, interrupt_rx) = watch::channel(false);
        Self {
            runtime,
            interrupted: AtomicBool::new(false),
            interrupt_tx,
            interrupt_rx,
            checksum_cache: Mutex::new(None),
            mtime_db: Mutex::new(None),
            mtime_enabled: AtomicBool::new(true),
        }
    }

    pub(crate) fn set_mtime_check(&self, enabled: bool) {
        self.mtime_enabled.store(enabled, Ordering::Relaxed);
    }

    pub(crate) fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    pub(crate) fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }

    pub(crate) fn interrupt(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
        let _ = self.interrupt_tx.send(true);
    }

    pub(crate) fn interrupt_receiver(&self) -> watch::Receiver<bool> {
        self.interrupt_rx.clone()
    }
}
