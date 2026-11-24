//! Centralized runtime flags set once at startup and read throughout execution.
//!
//! Replaces individual `AtomicBool` statics scattered across modules with a single
//! struct stored in a `OnceLock`. All flags are immutable after initialization.

use std::sync::OnceLock;

/// Runtime flags set once at startup from CLI arguments.
#[derive(Debug)]
pub struct RuntimeFlags {
    /// What to print about the build's own working.
    pub diagnostics: Diagnostics,
    /// How output is produced.
    pub output: OutputFlags,
    /// What tables show.
    pub tables: TableFlags,
    /// Whether to emit ANSI color escape sequences.
    /// Resolved from --color (auto/always/never) and the `NO_COLOR` env var.
    pub color_enabled: bool,
}

/// Diagnostic output about the build's own working.
#[derive(Debug, Clone, Copy)]
pub struct Diagnostics {
    /// Print each child process command before execution (--show-child-processes)
    pub show_child_processes: bool,
    /// Print phase messages during graph building (--phases)
    pub phases_debug: bool,
    /// Print graph size at each major build stage (--graph-stats)
    pub graph_stats: bool,
}

/// How output is produced.
#[derive(Debug, Clone, Copy)]
pub struct OutputFlags {
    /// Show tool output even on success (--show-output)
    pub show_output: bool,
    /// Output JSON instead of human-readable text (--json)
    pub json_mode: bool,
    /// Suppress all output except errors (--quiet)
    pub quiet: bool,
}

/// What tables show.
#[derive(Debug, Clone, Copy)]
pub struct TableFlags {
    /// Show every column of every table (--all)
    pub all_fields: bool,
    /// Print each table's name and column names only, no data (--fields)
    pub fields_only: bool,
}

static FLAGS: OnceLock<RuntimeFlags> = OnceLock::new();

/// Initialize runtime flags. Must be called exactly once from main before any reads.
pub fn init(flags: RuntimeFlags) {
    FLAGS.set(flags).expect("runtime flags already initialized");
}

/// Initialize with defaults if not already initialized. Test-only: unit
/// tests share one process, so whichever test runs first would win a plain
/// `init()` and the rest would panic on the double-set.
#[cfg(test)]
pub fn init_for_test() {
    let _ = FLAGS.set(RuntimeFlags {
        diagnostics: Diagnostics {
            show_child_processes: false,
            phases_debug: false,
            graph_stats: false,
        },
        output: OutputFlags {
            show_output: false,
            json_mode: false,
            quiet: true,
        },
        tables: TableFlags {
            all_fields: false,
            fields_only: false,
        },
        color_enabled: false,
    });
}

/// Get the runtime flags. Panics if called before `init()`.
fn get() -> &'static RuntimeFlags {
    FLAGS.get().expect("runtime flags not initialized")
}

pub fn show_child_processes() -> bool {
    get().diagnostics.show_child_processes
}

pub fn show_output() -> bool {
    get().output.show_output
}

pub fn phases_debug() -> bool {
    get().diagnostics.phases_debug
}

pub fn graph_stats() -> bool {
    get().diagnostics.graph_stats
}

pub fn json_mode() -> bool {
    get().output.json_mode
}

pub fn quiet() -> bool {
    get().output.quiet
}

/// `--all`: every table shows every column it has. Safe before `init()`
/// (false), since tables print only after startup.
pub fn all_fields() -> bool {
    FLAGS.get().is_some_and(|f| f.tables.all_fields)
}

/// `--fields`: every table prints its name and column names only.
pub fn fields_only() -> bool {
    FLAGS.get().is_some_and(|f| f.tables.fields_only)
}

pub fn color_enabled() -> bool {
    // If flags aren't initialized yet, fall back to "no color". This can happen
    // during very early startup (e.g., CLI parse errors from clap).
    FLAGS.get().is_some_and(|f| f.color_enabled)
}

/// Non-panicking read of `quiet`, safe to call before `init()` — used by the
/// final exit-status line in `main()`, which can run even when CLI parsing
/// failed before flags were set up.
pub fn quiet_or_default() -> bool {
    FLAGS.get().is_some_and(|f| f.output.quiet)
}

/// Non-panicking read of `json_mode`. Same rationale as `quiet_or_default`.
pub fn json_mode_or_default() -> bool {
    FLAGS.get().is_some_and(|f| f.output.json_mode)
}
