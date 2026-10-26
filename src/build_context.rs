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
    /// Mtime entries computed this session and not yet written to `mtime_db`.
    /// See `checksum::flush_mtime_cache`.
    pub(crate) mtime_pending: Mutex<crate::checksum::PendingMtime>,
    /// Whether mtime pre-check is enabled by configuration (`[cache]
    /// mtime_check`). Re-applied from the config on every builder
    /// construction, so a config edit under `rsconstruct watch` takes effect
    /// on the next rebuild.
    mtime_config_enabled: AtomicBool,
    /// Whether the mtime pre-check was forced off (`--no-mtime-cache`, or a
    /// caller that must not touch the CWD-relative mtime database). Wins
    /// over the config value whatever order the two are set in.
    mtime_forced_off: AtomicBool,
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
    /// Every product's declared output directories: a path under one of
    /// them belongs to that product, whatever its name.
    pub(crate) declared_output_dirs: Mutex<Vec<PathBuf>>,
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
            mtime_pending: Mutex::new(crate::checksum::PendingMtime::default()),
            mtime_config_enabled: AtomicBool::new(true),
            mtime_forced_off: AtomicBool::new(false),
            max_arg_len: std::sync::atomic::AtomicUsize::new(1_000_000),
            webcache_ttl_secs: std::sync::atomic::AtomicU64::new(7 * 24 * 60 * 60),
            command_timeout_secs: std::sync::atomic::AtomicU64::new(0),
            declared_outputs: Mutex::new(HashSet::new()),
            declared_output_dirs: Mutex::new(Vec::new()),
            combined_memo: Mutex::new(crate::checksum::CombinedMemo::default()),
        }
    }

    /// Record the declared outputs of every product in the graph about to
    /// be executed. Replaces any earlier set: a context outlives one graph
    /// in watch mode.
    pub(crate) fn set_declared_outputs(&self, paths: HashSet<PathBuf>, dirs: Vec<PathBuf>) {
        *self.declared_outputs.lock().unwrap() = paths;
        *self.declared_output_dirs.lock().unwrap() = dirs;
    }

    /// Whether some product of the current graph declares `path` as an output.
    /// Whether some product owns `path`: it is a declared output file, or it
    /// lies under a declared output directory. A step that writes a
    /// directory of files (`output_dirs = ["_site/logos"]`) owns every file
    /// in it, so a mass generator sharing `_site/` must not report them as
    /// written off its plan when the two run at the same time.
    pub fn is_declared_output(&self, path: &std::path::Path) -> bool {
        if self.declared_outputs.lock().unwrap().contains(path) {
            return true;
        }
        self.declared_output_dirs
            .lock()
            .unwrap()
            .iter()
            .any(|dir| path.starts_with(dir))
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

    /// Force the mtime pre-check off (`false`) or lift that override
    /// (`true`). This is the CLI's `--no-mtime-cache` and the handle unit
    /// tests use to stay away from the CWD-relative mtime database; it is
    /// authoritative over [`set_mtime_check_from_config`](Self::set_mtime_check_from_config).
    pub(crate) fn set_mtime_check(&self, enabled: bool) {
        self.mtime_forced_off.store(!enabled, Ordering::Relaxed);
    }

    /// Apply `[cache] mtime_check` from the project config. Called on every
    /// builder construction, so the value tracks the config across watch
    /// rebuilds; a forced-off override still wins.
    pub(crate) fn set_mtime_check_from_config(&self, enabled: bool) {
        self.mtime_config_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Whether the mtime pre-check runs: enabled by config and not forced off.
    pub(crate) fn mtime_check_enabled(&self) -> bool {
        self.mtime_config_enabled.load(Ordering::Relaxed)
            && !self.mtime_forced_off.load(Ordering::Relaxed)
    }

    /// Drop every cache that assumes file content is stable for one build
    /// run: the in-session checksum cache and the combined-input memo.
    ///
    /// Both are keyed by path alone and consulted before any stat, so they
    /// are only valid while no source file changes underneath them. One
    /// build run satisfies that; `rsconstruct watch`, which reuses one
    /// context across rebuilds triggered by exactly such changes, does not —
    /// it calls this before every rebuild. The executor's `forget_in_session`
    /// covers the files a build writes itself (product outputs); this covers
    /// the files the user edits. The persistent mtime database and its staged
    /// entries stay: they are validated against the file stamp on every
    /// lookup, so an edited file misses them and is re-hashed.
    pub(crate) fn clear_session_caches(&self) {
        *self.checksum_cache.lock().unwrap() = None;
        *self.combined_memo.lock().unwrap() = crate::checksum::CombinedMemo::default();
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

#[cfg(test)]
mod declared_output_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_file_under_a_declared_output_dir_is_owned() {
        let ctx = BuildContext::new();
        ctx.set_declared_outputs(
            HashSet::from([PathBuf::from("_site/data/companies.json.gz")]),
            vec![PathBuf::from("_site/logos")],
        );
        assert!(ctx.is_declared_output(Path::new("_site/data/companies.json.gz")));
        assert!(ctx.is_declared_output(Path::new("_site/logos/redhat.svg")));
        assert!(ctx.is_declared_output(Path::new("_site/logos/deep/er.svg")));
        assert!(!ctx.is_declared_output(Path::new("_site/logos-old/x.svg")));
        assert!(!ctx.is_declared_output(Path::new("_site/en/index.html")));
    }
}

#[cfg(test)]
mod mtime_precedence_tests {
    use super::*;

    /// `--no-mtime-cache` must win over `[cache] mtime_check = true`
    /// whichever is applied first, and the config value must be able to
    /// flip both ways while no override is in force (watch mode re-applies
    /// it on every rebuild).
    #[test]
    fn cli_override_beats_config_in_either_order() {
        let ctx = BuildContext::new();
        assert!(ctx.mtime_check_enabled(), "on by default");

        ctx.set_mtime_check_from_config(false);
        assert!(!ctx.mtime_check_enabled(), "config turns it off");
        ctx.set_mtime_check_from_config(true);
        assert!(ctx.mtime_check_enabled(), "config turns it back on");

        // CLI first, config after: the config must not re-enable it.
        ctx.set_mtime_check(false);
        ctx.set_mtime_check_from_config(true);
        assert!(
            !ctx.mtime_check_enabled(),
            "forced off beats a later config=true"
        );

        // Config first, CLI after.
        let ctx = BuildContext::new();
        ctx.set_mtime_check_from_config(true);
        ctx.set_mtime_check(false);
        assert!(
            !ctx.mtime_check_enabled(),
            "forced off beats an earlier config=true"
        );

        ctx.set_mtime_check(true);
        assert!(
            ctx.mtime_check_enabled(),
            "lifting the override restores the config value"
        );
    }
}
