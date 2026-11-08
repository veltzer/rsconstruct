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

    /// The `cargo-<subcommand>` tool to declare: the project's own, or a
    /// tool rsconstruct knows wherever it is installed (a global
    /// cargo-deny still needs the network under the sandbox). A rustup
    /// component such as `cargo-clippy` is not a known tool, so plain
    /// `cargo clippy` declares only cargo.
    fn subcommand_tool(&self) -> Option<String> {
        if let Some((name, _)) = self.project_subcommand() {
            return Some(name);
        }
        let subcommand = self
            .config
            .standard
            .require_command(crate::processor::names::CLIPPY)
            .ok()?;
        known_subcommand_tool(subcommand)
    }
}

/// `cargo-<subcommand>` when rsconstruct knows it as a tool.
fn known_subcommand_tool(subcommand: &str) -> Option<String> {
    let name = format!("cargo-{subcommand}");
    crate::tools::tool_info(&name).map(|_| name)
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

    /// cargo, plus the `cargo-<subcommand>` that runs (`subcommand_tool`)
    /// — so it is declared, its identity is part of the cache key (a
    /// cargo-deny moving to a new release re-runs `cargo deny`), and a
    /// sandboxed run gets the network it declares.
    fn required_tools(&self) -> Vec<String> {
        let mut builtin = vec![self.config.cargo.clone()];
        if let Some(name) = self.subcommand_tool() {
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
    use super::{cargo_command, known_subcommand_tool};

    /// A known subcommand tool is declared wherever it is installed; a
    /// rustup component is not a tool rsconstruct knows.
    #[test]
    fn known_subcommands_are_declared() {
        assert_eq!(known_subcommand_tool("deny"), Some("cargo-deny".into()));
        assert_eq!(known_subcommand_tool("clippy"), None);
    }

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
        sandboxable: true,
        max_jobs_cap: None,
    }
}
// `cargo deny`, run through this processor: the project's own when its
// tool directory holds one (`project_subcommand`), otherwise the one cargo
// finds. Either way it is declared (`subcommand_tool`). It fetches the RustSec
// advisory database on every run.
inventory::submit! { crate::tools::ToolInfo {
    name: "cargo-deny", runtime: "rust",
    project: Some(crate::tools::InstallMethod::new("cargo", "cargo-deny")),
    machine: Some(crate::tools::InstallMethod::new("cargo", "cargo-deny")),
    alternatives: &[], prefer: None, sandbox_writable: &[], needs_network: true,
} }
