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

const STAMP_DIR: &str = "out/processor.mass_generator.pip";
const ROOT_STAMP: &str = "out/processor.mass_generator.pip/root.stamp";

fn stamp_path(anchor_dir: &Path) -> PathBuf {
    if anchor_dir.as_os_str().is_empty() {
        return PathBuf::from(ROOT_STAMP);
    }
    let flat = anchor_dir.to_string_lossy().replace('/', "_");
    Path::new(STAMP_DIR).join(format!("{flat}.stamp"))
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
/// Pip config. Custom: none (uses standard.command as the pip executable).
pub struct PipConfig {
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct PipProcessor {
    config: PipConfig,
}

impl PipProcessor {
    pub const fn new(config: PipConfig) -> Self {
        Self { config }
    }

    /// Run pip install -r requirements.txt in the file's directory
    fn execute_pip(
        &self,
        ctx: &crate::build_context::BuildContext,
        requirements_txt: &Path,
    ) -> Result<()> {
        let mut cmd = Command::new(&self.config.standard.command);
        cmd.arg("install");
        cmd.arg("-r").arg(
            requirements_txt
                .file_name()
                .context("requirements.txt path has no filename")?,
        );
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, requirements_txt)?;
        check_command_output(
            &output,
            format_args!("pip install in {}", anchor_display_dir(requirements_txt)),
        )
    }
}

impl Processor for PipProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product per scanned requirements file; no sibling lookup.
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.standard, false).is_empty()
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processor::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        crate::processor::with_declared_tools(
            vec![self.config.standard.command.clone(), "python3".to_string()],
            &self.config.standard,
        )
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let files = file_index.scan(&self.config.standard, false);
        if files.is_empty() {
            return Ok(());
        }

        let config_hash = output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        );
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        let mut planned = Vec::with_capacity(files.len());
        for anchor in files {
            let anchor_dir = anchor
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            planned.push(PlannedOutput {
                path: stamp_path(&anchor_dir),
                sources: vec![anchor],
            });
        }

        add_planned_products(graph, &planned, instance_name, &config_hash, &extra)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_pip(ctx, product.primary_input())?;
        let stamp = product.primary_output();
        ensure_output_dir(stamp)?;
        std::fs::write(stamp, "installed\n")
            .with_context(|| format!("Failed to write pip install stamp {}", stamp.display()))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(PipProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 2,
        name: "pip",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[],
        omit_standard_fields: &["formats", "dep_auto", "output_dir", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["requirements.txt"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "pip", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<PipConfig>,
        keywords: &["python", "pip", "package-manager", "py"],
        description: "Install Python dependencies using pip",
        is_builtin: false,
        is_rust: false,
        supports_batch: false,
        caches_outputs: false,
        max_jobs_cap: Some(1),
    }
}
