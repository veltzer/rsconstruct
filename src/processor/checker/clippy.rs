use anyhow::Result;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{
    DirectoryProductOpts, Processor, SiblingFilter, anchor_display_dir, check_command_output,
    discover_directory_products, run_in_anchor_dir,
};

fn default_cargo() -> String {
    "cargo".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
/// Clippy config. Custom: cargo.
pub struct ClippyConfig {
    #[serde(default = "default_cargo")]
    pub cargo: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for ClippyConfig {
    fn default() -> Self {
        Self {
            cargo: "cargo".into(),
            standard: StandardConfig::default(),
        }
    }
}

pub struct ClippyProcessor {
    config: ClippyConfig,
}

impl ClippyProcessor {
    pub const fn new(config: ClippyConfig) -> Self {
        Self { config }
    }

    /// Run cargo clippy in the Cargo.toml's directory
    fn execute_clippy(
        &self,
        ctx: &crate::build_context::BuildContext,
        cargo_toml: &Path,
    ) -> Result<()> {
        let subcommand = self
            .config
            .standard
            .require_command(crate::processor::names::CLIPPY)?;
        let mut cmd = cargo_command(
            &self.config.cargo,
            subcommand,
            self.project_subcommand().map(|(_, exe)| exe),
        );
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, cargo_toml)?;
        check_command_output(
            &output,
            format_args!("cargo {} in {}", subcommand, anchor_display_dir(cargo_toml)),
        )
    }

    /// The project's own `cargo-<subcommand>` executable, by name and path,
    /// when its tool directory holds one (see [`cargo_command`]).
    fn project_subcommand(&self) -> Option<(String, std::path::PathBuf)> {
        let subcommand = self
            .config
            .standard
            .require_command(crate::processor::names::CLIPPY)
            .ok()?;
        let name = format!("cargo-{subcommand}");
        crate::tool_store::repo_tool(&name).map(|exe| (name, exe))
    }
}

/// The command for `cargo <subcommand>`. When the project's tool directory
/// holds `cargo-<subcommand>` (`project_exe`, a crate from `[dependencies]
/// cargo`), that binary is run directly, exactly as cargo itself would run
/// it: as `cargo-<subcommand> <subcommand> ...`, with `CARGO` naming the
/// cargo to call back. Going through cargo would not do: cargo looks for
/// external subcommands in `$CARGO_HOME/bin` before PATH, so a globally
/// installed cargo-deny would win over the project's locked one.
fn cargo_command(
    cargo: &str,
    subcommand: &str,
    project_exe: Option<std::path::PathBuf>,
) -> Command {
    let mut cmd = match project_exe {
        Some(exe) => {
            let mut cmd = Command::new(exe);
            cmd.env("CARGO", cargo);
            cmd
        }
        None => Command::new(cargo),
    };
    cmd.arg(subcommand);
    cmd
}

impl Processor for ClippyProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    /// cargo, plus the project's `cargo-<subcommand>` when that is what runs
    /// — so it is declared, and its identity is part of the cache key: a
    /// locked cargo-deny moving to a new release re-runs `cargo deny`.
    fn required_tools(&self) -> Vec<String> {
        let mut builtin = vec![self.config.cargo.clone()];
        if let Some((name, _)) = self.project_subcommand() {
            builtin.push(name);
        }
        crate::processor::with_declared_tools(builtin, &self.config.standard)
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        discover_directory_products(
            graph,
            DirectoryProductOpts {
                scan: &self.config.standard,
                file_index,
                dep_inputs: &self.config.standard.dep_inputs,
                cfg_hash: &self.config,
                checksum_fields: crate::config::checksum_fields_of(instance_name),
                siblings: &SiblingFilter {
                    extensions: &[".rs", ".toml", "Cargo.lock"],
                    excludes: &["/.git/", "/target/", "/.rsconstruct/"],
                },
                processor_name: instance_name,
            },
        )
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_clippy(ctx, product.primary_input())
    }
}

#[cfg(test)]
mod tests {
    use super::cargo_command;

    fn argv(cmd: &std::process::Command) -> Vec<String> {
        std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    /// A project-installed subcommand is run directly; anything else goes
    /// through cargo.
    #[test]
    fn project_subcommands_bypass_cargo_home() {
        let exe = std::path::PathBuf::from("/p/.rsconstruct/tools/bin/cargo-deny");
        let cmd = cargo_command("cargo", "deny", Some(exe));
        assert_eq!(
            argv(&cmd),
            vec!["/p/.rsconstruct/tools/bin/cargo-deny", "deny"]
        );
        let cargo_env = cmd
            .get_envs()
            .find(|(k, _)| *k == "CARGO")
            .and_then(|(_, v)| v);
        assert_eq!(cargo_env, Some(std::ffi::OsStr::new("cargo")));

        assert_eq!(
            argv(&cargo_command("cargo", "clippy", None)),
            vec!["cargo", "clippy"]
        );
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(ClippyProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "clippy",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "cargo", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Path to the cargo executable" },
        ],
        omit_standard_fields: &["formats", "output_dir", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["Cargo.toml"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "clippy", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<ClippyConfig>,
        keywords: &["rust", "linter", "cargo", "rs"],
        description: "Lint Rust projects using Cargo Clippy",
        is_builtin: false,
        is_rust: true,
        supports_batch: false,
        caches_outputs: true,
        max_jobs_cap: None,
    }
}
