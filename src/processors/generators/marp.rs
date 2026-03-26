use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::config::MarpConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, clean_outputs, scan_root_valid, run_command, check_command_output};

use super::DiscoverParams;

/// Remove all marp-cli-* temp directories from /tmp.
///
/// marp-cli creates a unique browser profile directory (named `marp-cli-<random>`)
/// in /tmp for each invocation. These are Chromium user-data-dirs needed to isolate
/// the browser environment from the user's regular profile. marp-cli intentionally
/// does not delete them because the browser may still use the directory for
/// post-processing after the main conversion finishes (puppeteer/puppeteer#6291).
/// The marp-cli maintainer considers this the OS's responsibility to clean up.
///
/// In practice they accumulate (~18 MB each) and are never cleaned up on Linux.
/// Since rsconstruct waits for the marp process to fully exit before reaching this point,
/// it is safe to remove them here.
///
/// See: https://github.com/marp-team/marp-cli/issues/678
/// See: https://github.com/puppeteer/puppeteer/issues/6414
fn cleanup_marp_tmp_dirs() {
    let Ok(entries) = fs::read_dir("/tmp") else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy().starts_with("marp-cli-") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

pub struct MarpProcessor {
    config: MarpConfig,
}

impl MarpProcessor {
    pub fn new(config: MarpConfig) -> Self {
        Self { config }
    }
}

impl ProductDiscovery for MarpProcessor {
    fn description(&self) -> &str {
        "Convert Marp slides to PDF/PPTX/HTML"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::Generator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.marp_bin.clone(), "node".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            extra_inputs: &self.config.extra_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: crate::processors::names::MARP,
        };
        super::discover_multi_format(graph, file_index, &params, &self.config.formats)
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();

        let format = output.extension()
            .context("marp output has no extension")?
            .to_string_lossy();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.marp_bin);
        // HTML is marp's default output, so no format flag needed for it
        if format != "html" {
            cmd.arg(format!("--{}", format));
        }
        cmd.arg("--output").arg(output);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);

        let out = run_command(&mut cmd)?;
        let result = check_command_output(&out, format_args!("marp {}", input.display()));

        cleanup_marp_tmp_dirs();

        result
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        clean_outputs(product, crate::processors::names::MARP, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
