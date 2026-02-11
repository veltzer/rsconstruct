//! Centralized runtime flags set once at startup and read throughout execution.
//!
//! Replaces individual `AtomicBool` statics scattered across modules with a single
//! struct stored in a `OnceLock`. All flags are immutable after initialization.

use std::sync::OnceLock;

/// Runtime flags set once at startup from CLI arguments.
#[derive(Debug)]
pub(crate) struct RuntimeFlags {
    /// Print each child process command before execution (--show-child-processes)
    pub show_child_processes: bool,
    /// Show tool output even on success (--show-output)
    pub show_output: bool,
    /// Print phase messages during graph building (--phases)
    pub phases_debug: bool,
    /// Output JSON instead of human-readable text (--json)
    pub json_mode: bool,
}

static FLAGS: OnceLock<RuntimeFlags> = OnceLock::new();

/// Initialize runtime flags. Must be called exactly once from main before any reads.
pub(crate) fn init(flags: RuntimeFlags) {
    FLAGS.set(flags).expect("runtime flags already initialized");
}

/// Get the runtime flags. Panics if called before `init()`.
fn get() -> &'static RuntimeFlags {
    FLAGS.get().expect("runtime flags not initialized")
}

pub(crate) fn show_child_processes() -> bool {
    get().show_child_processes
}

pub(crate) fn show_output() -> bool {
    get().show_output
}

pub(crate) fn phases_debug() -> bool {
    get().phases_debug
}

pub(crate) fn json_mode() -> bool {
    get().json_mode
}
