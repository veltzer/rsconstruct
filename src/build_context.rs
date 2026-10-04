use redb::Database;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Max-arg-length threshold for `run_checker` (sourced from `build.max_arg_len`).
    /// Stored on the context so any processor can read it without having to be
    /// handed the full `Config`.
    pub(crate) max_arg_len: std::sync::atomic::AtomicUsize,
    /// TTL for cached HTTP responses (sourced from `cache.webcache_ttl_secs`).
    /// On the context for the same reason as `max_arg_len`: the iyamlschema
    /// checker needs it and has no route to the full `Config`.
    pub(crate) webcache_ttl_secs: std::sync::atomic::AtomicU64,
    /// `[build] command_timeout_secs`; 0 means no limit (the default).
    pub(crate) command_timeout_secs: std::sync::atomic::AtomicU64,
    /// Every path some product of the current graph declares as an output.
    /// Filled by the executor before the first product runs. A processor
    /// that inspects a shared output directory after its tool ran (the
    /// mass generator's plan-vs-build check) needs it to tell a neighbor's
    /// declared file from a file its tool wrote off-plan — `execute` has no
    /// other route to the graph.
    pub(crate) declared_outputs: Mutex<HashSet<PathBuf>>,
    /// In-session memo of combined input checksums for large input lists.
    /// See `checksum::combined_input_checksum`.
    pub(crate) combined_memo: Mutex<crate::checksum::CombinedMemo>,
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
            max_arg_len: std::sync::atomic::AtomicUsize::new(1_000_000),
            webcache_ttl_secs: std::sync::atomic::AtomicU64::new(7 * 24 * 60 * 60),
            command_timeout_secs: std::sync::atomic::AtomicU64::new(0),
            declared_outputs: Mutex::new(HashSet::new()),
            combined_memo: Mutex::new(crate::checksum::CombinedMemo::default()),
        }
    }

    /// Record the declared outputs of every product in the graph about to
    /// be executed. Replaces any earlier set: a context outlives one graph
    /// in watch mode.
    pub(crate) fn set_declared_outputs(&self, paths: HashSet<PathBuf>) {
        *self.declared_outputs.lock().unwrap() = paths;
    }

    /// Whether some product of the current graph declares `path` as an output.
    pub fn is_declared_output(&self, path: &std::path::Path) -> bool {
        self.declared_outputs.lock().unwrap().contains(path)
    }

    pub(crate) fn set_command_timeout_secs(&self, n: u64) {
        self.command_timeout_secs.store(n, Ordering::Relaxed);
    }

    /// The build-wide command timeout, or None when `[build]
    /// command_timeout_secs` is 0 (the default: no limit).
    pub fn command_timeout(&self) -> Option<std::time::Duration> {
        match self.command_timeout_secs.load(Ordering::Relaxed) {
            0 => None,
            secs => Some(std::time::Duration::from_secs(secs)),
        }
    }

    pub(crate) fn set_mtime_check(&self, enabled: bool) {
        self.mtime_enabled.store(enabled, Ordering::Relaxed);
    }

    pub(crate) fn set_max_arg_len(&self, n: usize) {
        self.max_arg_len.store(n, Ordering::Relaxed);
    }

    pub(crate) fn set_webcache_ttl_secs(&self, n: u64) {
        self.webcache_ttl_secs.store(n, Ordering::Relaxed);
    }

    pub(crate) fn webcache_ttl_secs(&self) -> u64 {
        self.webcache_ttl_secs.load(Ordering::Relaxed)
    }

    pub fn max_arg_len(&self) -> usize {
        self.max_arg_len.load(Ordering::Relaxed)
    }

    pub(crate) const fn runtime(&self) -> &Runtime {
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
