//! Phase hook system.
//!
//! Phases are well-known points in rsconstruct's execution flow. Code can register
//! a [`PhaseHook`] via `inventory::submit!` to run when a phase fires. The
//! `PostConfig` phase fires once at the end of [`crate::config::Config::load`],
//! before any command consumes the config. Build-pipeline phases (`Discover`
//! through `Build`) fire at the corresponding points in the build pipeline.
//!
//! Hooks within a phase run in inventory registration order.

use anyhow::Result;
use clap::ValueEnum;

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum)]
pub enum Phase {
    /// After `Config::load` returns, before any command uses the config.
    PostConfig,
    /// Build pipeline: products discovered (before dependency scanning).
    Discover,
    /// Build pipeline: dependencies added (before graph resolution).
    AddDependencies,
    /// Build pipeline: graph resolved (before classification).
    Resolve,
    /// Build pipeline: products classified into skip/restore/build.
    Classify,
    /// Build pipeline: build execution finished.
    Build,
}

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::PostConfig      => "post-config",
            Phase::Discover        => "discover",
            Phase::AddDependencies => "add-dependencies",
            Phase::Resolve         => "resolve",
            Phase::Classify        => "classify",
            Phase::Build           => "build",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Phase::PostConfig      => "Fires after Config::load, before any command uses the config",
            Phase::Discover        => "Build pipeline: products discovered",
            Phase::AddDependencies => "Build pipeline: dependencies added to graph",
            Phase::Resolve         => "Build pipeline: graph resolved",
            Phase::Classify        => "Build pipeline: products classified (skip/restore/build)",
            Phase::Build           => "Build pipeline: build execution finished",
        }
    }

    pub fn all() -> &'static [Phase] {
        &[
            Phase::PostConfig,
            Phase::Discover,
            Phase::AddDependencies,
            Phase::Resolve,
            Phase::Classify,
            Phase::Build,
        ]
    }
}

/// A hook registered against a [`Phase`]. Submit one with `inventory::submit!`.
///
/// `function` and `location` carry the registered function's fully-qualified
/// path and source location. They are populated by hand at each registration
/// site (no macro), via `concat!(module_path!(), "::", stringify!(fn))` and
/// `concat!(file!(), ":", line!())`. They are surfaced by
/// `rsconstruct phases hooks --verbose`.
pub struct PhaseHook {
    pub name: &'static str,
    pub phase: Phase,
    pub description: &'static str,
    pub function: &'static str,
    pub location: &'static str,
    pub run: fn(&mut Config) -> Result<()>,
}

inventory::collect!(PhaseHook);

pub fn all_hooks() -> impl Iterator<Item = &'static PhaseHook> {
    inventory::iter::<PhaseHook>()
}

pub fn hooks_for(phase: Phase) -> impl Iterator<Item = &'static PhaseHook> {
    all_hooks().filter(move |h| h.phase == phase)
}

/// Run every hook registered for `phase`, in inventory order.
pub fn run_phase(phase: Phase, config: &mut Config) -> Result<()> {
    for hook in hooks_for(phase) {
        (hook.run)(config)
            .map_err(|e| e.context(format!("phase hook '{}' (phase {})", hook.name, phase.name())))?;
    }
    Ok(())
}
