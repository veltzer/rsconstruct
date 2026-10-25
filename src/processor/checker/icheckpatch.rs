//! icheckpatch checker: the Linux kernel's `checkpatch.pl`, in-process.
//!
//! The engine is `crate::engines::checkpatch`, a port of `checkpatch.pl`
//! as `--no-tree -f` runs it on one file. This file is the processor
//! around it: each file is checked on its own, as `checkpatch.pl --no-tree
//! -f FILE` checks it, and a file fails on any ERROR, WARNING or CHECK,
//! with checkpatch's report as the message.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::config::StandardConfig;
use crate::engines::checkpatch::{Checker, Outcome, timestamp};
use crate::graph::Product;

pub struct IcheckpatchProcessor {
    config: StandardConfig,
}

impl IcheckpatchProcessor {
    pub const fn new(config: StandardConfig) -> Self {
        Self { config }
    }

    fn check_file(file: &Path) -> Result<()> {
        let bytes =
            std::fs::read(file).with_context(|| format!("Failed to read {}", file.display()))?;
        let metadata = std::fs::metadata(file)
            .with_context(|| format!("Failed to stat {}", file.display()))?;
        let block = usize::try_from(crate::platform::block_size(&metadata)).unwrap_or(usize::MAX);
        let mtime = metadata
            .modified()
            .with_context(|| format!("Failed to read the mtime of {}", file.display()))?;
        let name = file.as_os_str().as_encoded_bytes();
        match Checker::check_file(name, &bytes, block, &timestamp(mtime)) {
            Outcome::Report { clean: true, .. } => Ok(()),
            Outcome::Report {
                output,
                clean: false,
            } => {
                bail!(
                    "{}",
                    String::from_utf8_lossy(&output).trim_end_matches('\n')
                )
            }
            Outcome::Fatal(message) => bail!("checkpatch died on {}: {message}", file.display()),
        }
    }
}

impl crate::processor::Processor for IcheckpatchProcessor {
    fn scan_config(&self) -> &StandardConfig {
        &self.config
    }

    /// One product per scanned file (the default `discover`).
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        Self::check_file(product.primary_input())
    }

    fn execute_batch(
        &self,
        _ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        crate::processor::execute_checker_batch_per_file(products, Self::check_file)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IcheckpatchProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::RustAlternative { replaces: "processor.checker.checkpatch", by: "processor.checker.icheckpatch" }
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "icheckpatch",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<StandardConfig>,
        fields: &[],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".c", ".h"], src_exclude_dirs: &[] }),
        defaults: None,
        keywords: &["c", "linux", "kernel", "checker", "patch", "checkpatch", "rust"],
        description: "Check C source files with the Linux kernel's checkpatch.pl rules (in-process)",
        is_builtin: true,
        is_rust: true,
        supports_batch: true,
        caches_outputs: true,
        max_jobs_cap: None,
    }
}
