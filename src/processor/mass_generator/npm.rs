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

const STAMP_DIR: &str = "out/processor.mass_generator.npm";
const ROOT_STAMP: &str = "out/processor.mass_generator.npm/root.stamp";

fn stamp_path(anchor_dir: &Path) -> PathBuf {
    if anchor_dir.as_os_str().is_empty() {
        return PathBuf::from(ROOT_STAMP);
    }
    let flat = anchor_dir.to_string_lossy().replace('/', "_");
    Path::new(STAMP_DIR).join(format!("{flat}.stamp"))
}

/// The files `npm install` reads besides `package.json`, all in the package
/// directory itself: the lockfiles and the per-package npm config.
const INSTALL_SIBLINGS: &[&str] = &["package-lock.json", "npm-shrinkwrap.json", ".npmrc"];

/// The inputs of the install stamp for `package_json`: the manifest and
/// whichever of [`INSTALL_SIBLINGS`] the index has next to it. The package's
/// own `.js`/`.ts` sources are not inputs — editing them changes nothing
/// about what `npm install` does — and neither is anything in a
/// subdirectory.
fn install_inputs(package_json: &Path, file_index: &FileIndex) -> Vec<PathBuf> {
    let anchor_dir = crate::processor::parent_dir_or_empty(package_json);
    let mut inputs = vec![package_json.to_path_buf()];
    for name in INSTALL_SIBLINGS {
        let path = crate::processor::resolve_anchor_path(anchor_dir, name);
        if file_index.contains(&path) {
            inputs.push(path);
        }
    }
    inputs
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
/// Npm config.
pub struct NpmConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct NpmProcessor {
    config: NpmConfig,
}

impl NpmProcessor {
    pub const fn new(config: NpmConfig) -> Self {
        Self { config }
    }

    /// Run npm install in the package.json's directory
    fn execute_npm(
        &self,
        ctx: &crate::build_context::BuildContext,
        package_json: &Path,
    ) -> Result<()> {
        let subcommand = "install";
        let mut cmd = Command::new(&self.config.standard.command);
        cmd.arg(subcommand);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, package_json)?;
        check_command_output(
            &output,
            format_args!("npm {} in {}", subcommand, anchor_display_dir(package_json)),
        )
    }
}

impl Processor for NpmProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processor::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        crate::processor::with_declared_tools(
            vec![self.config.standard.command.clone(), "node".to_string()],
            &self.config.standard,
        )
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let Some(files) = crate::processor::scan_or_skip(&self.config.standard, file_index) else {
            return Ok(());
        };

        let config_hash = output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        );
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        let mut planned = Vec::with_capacity(files.len());
        for anchor in files {
            let anchor_dir = crate::processor::parent_dir_or_empty(&anchor);
            let inputs = install_inputs(&anchor, file_index);
            planned.push(PlannedOutput {
                path: stamp_path(anchor_dir),
                sources: inputs,
            });
        }

        add_planned_products(graph, &planned, instance_name, &config_hash, &extra)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_npm(ctx, product.primary_input())?;
        let stamp = product.primary_output();
        ensure_output_dir(stamp)?;
        std::fs::write(stamp, "installed\n")
            .with_context(|| format!("Failed to write npm install stamp {}", stamp.display()))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(NpmProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 2,
        name: "npm",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[],
        omit_standard_fields: &["formats", "dep_auto", "output_dir", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["package.json"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "npm", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<NpmConfig>,
        keywords: &["javascript", "typescript", "node", "npm", "package-manager", "web", "frontend"],
        description: "Install Node.js dependencies using npm",
        is_builtin: false,
        is_rust: false,
        supports_batch: false,
        caches_outputs: false,
        max_jobs_cap: Some(1),
    }
}
inventory::submit! { crate::tools::ToolInfo {
    name: "npm", runtime: "node",
    project: None,
    machine: Some(crate::tools::InstallMethod::new("apt", "npm")),
    alternatives: &[], prefer: None,
} }

#[cfg(test)]
mod tests {
    use super::*;

    /// `npm install` reads the manifest, the lockfiles and `.npmrc`; the
    /// package's own sources and nested packages do not change what it
    /// installs, so editing them must not re-run it.
    #[test]
    fn install_inputs_are_the_manifest_lockfiles_and_npmrc_only() {
        let files: Vec<PathBuf> = [
            "web/package.json",
            "web/package-lock.json",
            "web/.npmrc",
            "web/index.js",
            "web/src/app.ts",
            "web/tsconfig.json",
            "web/packages/inner/package.json",
            "web/packages/inner/npm-shrinkwrap.json",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        let index = FileIndex::from_paths(files);
        assert_eq!(
            install_inputs(Path::new("web/package.json"), &index),
            ["web/package.json", "web/package-lock.json", "web/.npmrc"]
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            install_inputs(Path::new("web/packages/inner/package.json"), &index),
            [
                "web/packages/inner/package.json",
                "web/packages/inner/npm-shrinkwrap.json"
            ]
            .iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>()
        );

        // Through the registry, so the scan fields carry their defaults;
        // the scan root is configured, as every project does.
        let processor = crate::builder::create_processor_for_instance(
            "processor.mass_generator.npm",
            &toml::from_str("src_dirs = [\".\"]").unwrap(),
        )
        .unwrap()
        .expect("npm is registered");
        let mut graph = BuildGraph::new();
        processor
            .discover(&mut graph, &index, "processor.mass_generator.npm")
            .unwrap();
        assert_eq!(graph.products().len(), 2, "one stamp per package.json");
        let root = graph
            .products()
            .iter()
            .find(|p| p.primary_input() == Path::new("web/package.json"))
            .unwrap();
        assert_eq!(
            root.inputs,
            ["web/package.json", "web/package-lock.json", "web/.npmrc"]
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        );
    }
}
