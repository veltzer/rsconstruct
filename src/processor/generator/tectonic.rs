//! tectonic generator: `.tex` to PDF, the Rust counterpart of
//! `processor.generator.pdflatex`. tectonic reruns the engine itself until
//! cross-references settle and keeps its intermediate files in memory, so
//! there is no pass count and no scratch directory to clean. It carries no
//! TeX distribution: the files a document needs are downloaded on first use
//! and cached under `~/.cache/tectonic`.

use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use crate::config::StandardConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{Processor, check_command_output, run_command};

use super::DiscoverParams;

pub struct TectonicProcessor {
    config: StandardConfig,
}

impl TectonicProcessor {
    pub const fn new(config: StandardConfig) -> Self {
        Self { config }
    }
}

impl Processor for TectonicProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config
    }

    /// One product per scanned file (`discover_single_format`).
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn required_tools(&self) -> Vec<String> {
        crate::processor::with_declared_tools(vec![self.config.command.clone()], &self.config)
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: instance_name,
            checksum_fields: crate::config::checksum_fields_of(instance_name),
            source_filter: None,
        };
        super::discover_single_format(graph, file_index, &params, "pdf", &[])
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let final_output = product.primary_output();
        let input_stem = input
            .file_stem()
            .context("tectonic input has no file stem")?
            .to_string_lossy();
        crate::processor::ensure_output_dir(final_output)?;

        // tectonic names its output `<stem>.pdf` in the directory it is
        // given. A directory of the product's own keeps two same-stem
        // sources that share an output parent apart under -j.
        let scratch = crate::processor::parent_dir(final_output).join(format!(
            ".tectonic-{}",
            &crate::checksum::bytes_checksum(input.display().to_string().as_bytes())[..12]
        ));
        fs::create_dir_all(&scratch).with_context(|| {
            format!(
                "Failed to create tectonic scratch dir: {}",
                scratch.display()
            )
        })?;
        crate::sandbox::allow_output_dirs(std::slice::from_ref(&scratch));

        let mut cmd = Command::new(&self.config.command);
        // The PDF's creation date and ID come from the clock otherwise, so
        // the same source would give a different file on every build.
        cmd.env("SOURCE_DATE_EPOCH", "0");
        cmd.arg("--outdir").arg(&scratch);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        cmd.arg(input);
        let out = run_command(ctx, &cmd)?;
        check_command_output(&out, format_args!("tectonic {}", input.display()))?;

        let pdf = scratch.join(format!("{input_stem}.pdf"));
        fs::rename(&pdf, final_output).with_context(|| {
            format!(
                "Failed to move tectonic output into place: {}",
                final_output.display()
            )
        })?;
        fs::remove_dir_all(&scratch).with_context(|| {
            format!(
                "Failed to remove tectonic scratch dir: {}",
                scratch.display()
            )
        })
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(TectonicProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "tectonic",
        processor_type: crate::processor::ProcessorType::Generator,
        create: plugin_create,
        fields: &[],
        omit_standard_fields: &["formats", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_extensions: &[".tex"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "tectonic", output_dir: "out/processor.generator.tectonic", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<StandardConfig>,
        keywords: &["latex", "tex", "pdf", "generator", "typesetting", "rust"],
        description: "Compile LaTeX documents using tectonic",
        is_builtin: false,
        is_rust: true,
        supports_batch: false,
        caches_outputs: true,
        sandboxable: true,
        max_jobs_cap: None,
    }
}
inventory::submit! { crate::tools::ToolInfo {
    name: "tectonic", runtime: "rust",
    project: Some(crate::tools::InstallMethod::new("binary", "tectonic")),
    machine: Some(crate::tools::InstallMethod::new("binary", "tectonic")),
    alternatives: &[crate::tools::InstallMethod::new("cargo", "tectonic")],
    prefer: None,
    sandbox_writable: &[],
    // tectonic downloads the TeX files a document needs on first use.
    needs_network: true,
} }
