//! uv mass generator: runs `uv sync` beside each `uv.lock`, the Rust
//! counterpart of `processor.mass_generator.pip`. The lock and the
//! `pyproject.toml` beside it are the inputs; the output is an install
//! stamp, so the environment is synced again only when one of them changes.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::{PlannedOutput, add_planned_products};
use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{
    Processor, anchor_display_dir, check_command_output, ensure_output_dir, run_in_anchor_dir,
};

const STAMP_DIR: &str = "out/processor.mass_generator.uv";
const ROOT_STAMP: &str = "out/processor.mass_generator.uv/root.stamp";

fn stamp_path(anchor_dir: &Path) -> PathBuf {
    if anchor_dir.as_os_str().is_empty() {
        return PathBuf::from(ROOT_STAMP);
    }
    let flat = anchor_dir.to_string_lossy().replace('/', "_");
    Path::new(STAMP_DIR).join(format!("{flat}.stamp"))
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
/// Uv config. Custom: none (uses standard.command as the uv executable).
pub struct UvConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct UvProcessor {
    config: UvConfig,
}

impl UvProcessor {
    pub const fn new(config: UvConfig) -> Self {
        Self { config }
    }

    /// Run `uv sync` in the lock file's directory.
    fn execute_uv(&self, ctx: &crate::build_context::BuildContext, lock: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.standard.command);
        cmd.arg("sync");
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, lock)?;
        check_command_output(
            &output,
            format_args!("uv sync in {}", anchor_display_dir(lock)),
        )
    }
}

impl Processor for UvProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product per scanned lock file. The `pyproject.toml` beside it is
    /// named by path, not looked up in the index.
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.standard, true).is_empty()
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn required_tools(&self) -> Vec<String> {
        crate::processor::with_declared_tools(
            vec![self.config.standard.command.clone()],
            &self.config.standard,
        )
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        // Every lock below the scanned directories, at any depth: a project
        // nested in another has an environment of its own.
        let files = file_index.scan(&self.config.standard, true);
        if files.is_empty() {
            return Ok(());
        }

        let config_hash = output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        );
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        let mut planned = Vec::with_capacity(files.len());
        for lock in files {
            let anchor_dir = lock
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            // uv.lock is written from the pyproject.toml beside it, and
            // `uv sync` reads both.
            let pyproject = anchor_dir.join("pyproject.toml");
            planned.push(PlannedOutput {
                path: stamp_path(&anchor_dir),
                sources: vec![lock, pyproject],
            });
        }

        add_planned_products(graph, &planned, instance_name, &config_hash, &extra)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_uv(ctx, product.primary_input())?;
        let stamp = product.primary_output();
        ensure_output_dir(stamp)?;
        std::fs::write(stamp, "synced\n")
            .with_context(|| format!("Failed to write uv sync stamp {}", stamp.display()))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(UvProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "uv",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[],
        omit_standard_fields: &["formats", "dep_auto", "output_dir", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_extensions: &["uv.lock"], src_exclude_dirs: &[] }),
        // `--locked` fails when uv.lock is out of date with pyproject.toml
        // instead of quietly re-resolving it during a build.
        defaults: Some(crate::config::ProcessorDefaults { command: "uv", args: &["--locked"], ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<UvConfig>,
        keywords: &["python", "uv", "package-manager", "py", "venv", "rust"],
        description: "Install Python dependencies using uv sync",
        is_builtin: false,
        is_rust: true,
        supports_batch: false,
        caches_outputs: false,
        sandboxable: true,
        max_jobs_cap: Some(1),
    }
}
// uv builds the project's .venv (`tool install project` runs `uv sync`), so
// it cannot come from that venv: its release binary goes into the project's
// tool directory, installed ahead of the sync.
inventory::submit! { crate::tools::ToolInfo {
    name: "uv", runtime: "python",
    project: Some(crate::tools::InstallMethod::new("binary", "uv")),
    machine: Some(crate::tools::InstallMethod::new("pip", "uv")),
    alternatives: &[], prefer: None,
    // `uv sync` builds the project's environment.
    sandbox_writable: &[".venv"],
    // `uv sync` downloads the packages it installs.
    needs_network: true,
} }
