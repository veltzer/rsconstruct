use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{PdfuniteConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, run_command, check_command_output};

use super::find_dirs_with_ext;

pub struct PdfuniteProcessor {
    base: ProcessorBase,
    config: PdfuniteConfig,
}

impl PdfuniteProcessor {
    pub fn new(config: PdfuniteConfig) -> Self {
        Self {
            base: ProcessorBase::generator(
                crate::processors::names::PDFUNITE,
                "Merge PDFs from subdirectories into course bundles",
            ),
            config,
        }
    }
}

impl Processor for PdfuniteProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.standard.scan
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config.standard)
    }

    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn auto_detect(&self, _file_index: &FileIndex) -> bool {
        let base = Path::new(&self.config.source_dir);
        if !base.exists() {
            return false;
        }
        let ext = self.config.source_ext.strip_prefix('.').unwrap_or(&self.config.source_ext);
        !find_dirs_with_ext(base, ext).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.pdfunite_bin.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, _file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let base = Path::new(&self.config.source_dir);
        if !base.exists() {
            return Ok(());
        }

        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;
        let ext = self.config.source_ext.strip_prefix('.').unwrap_or(&self.config.source_ext);

        let dirs = find_dirs_with_ext(base, ext);

        // Compute upstream scan dir once
        let upstream_scan_dir = Path::new(&self.config.source_dir)
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        let upstream_scan_dirs = [upstream_scan_dir];

        for dir_path in dirs {
            // Find all source files in this directory
            let mut source_files: Vec<PathBuf> = fs::read_dir(&dir_path)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == ext))
                .collect();
            if source_files.is_empty() {
                continue;
            }
            source_files.sort();

            let inputs: Vec<PathBuf> = source_files.iter().map(|src| {
                super::output_path(src, &upstream_scan_dirs, &self.config.source_output_dir, "pdf")
            }).chain(extra.iter().cloned()).collect();

            // Mirror the directory structure from source_dir into output_dir,
            // naming each merged PDF after its leaf directory.
            let relative = dir_path.strip_prefix(base).unwrap_or(&dir_path);
            let parent = relative.parent().unwrap_or(Path::new(""));
            let leaf = relative.file_name().unwrap_or(relative.as_os_str());
            let outputs = vec![
                Path::new(&self.config.standard.output_dir).join(parent).join(format!("{}.pdf", leaf.to_string_lossy())),
            ];

            graph.add_product(inputs, outputs, instance_name, hash.clone())?;
        }

        Ok(())
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        let output = product.primary_output();

        crate::processors::ensure_output_dir(output)?;

        let mut cmd = Command::new(&self.config.pdfunite_bin);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        // pdfunite takes input files followed by output file
        for input in &product.inputs {
            cmd.arg(input);
        }
        cmd.arg(output);

        let out = run_command(&mut cmd)?;
        check_command_output(&out, format_args!("pdfunite {}", output.display()))
    }

}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(PdfuniteProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "pdfunite",
        processor_type: crate::processors::ProcessorType::Generator,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::PdfuniteConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::PdfuniteConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::PdfuniteConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::PdfuniteConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::PdfuniteConfig>,
    }
}
