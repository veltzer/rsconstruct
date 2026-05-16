//! marp generator — custom Processor impl backed by [`MarpConfig`].

use std::fs;
use std::process::Command;
use std::time::Duration;
use anyhow::{Context, Result};

use crate::config::MarpConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{run_command_with_timeout, check_command_output, ensure_output_dir, Processor};

fn cleanup_marp_tmp_dirs() {
    let Ok(entries) = fs::read_dir("/tmp") else { return };
    for entry in entries.filter_map(std::result::Result::ok) {
        if entry.file_name().to_string_lossy().starts_with("marp-cli-") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn is_timeout_error(err: &anyhow::Error) -> bool {
    err.to_string().contains("Command timed out after")
}

pub struct MarpProcessor {
    config: MarpConfig,
}

impl MarpProcessor {
    pub const fn new(config: MarpConfig) -> Self {
        Self { config }
    }
}

impl Processor for MarpProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn standard_config(&self) -> Option<&crate::config::StandardConfig> {
        Some(&self.config.standard)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        crate::processors::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.standard.command.clone(), "node".into()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let params = super::DiscoverParams {
            scan: &self.config.standard,
            dep_inputs: &self.config.standard.dep_inputs,
            config: &self.config,
            output_dir: &self.config.standard.output_dir,
            processor_name: instance_name,
            checksum_fields: <crate::config::MarpConfig as crate::config::KnownFields>::checksum_fields(),
        };
        super::discover_multi_format(graph, file_index, &params, &self.config.standard.formats)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let input = product.primary_input();
        let output = product.primary_output();
        let format = output.extension()
            .context("marp output has no extension")?
            .to_string_lossy();
        ensure_output_dir(output)?;
        let command = self.config.standard.require_command("marp")?;
        let timeout = Duration::from_secs(self.config.timeout_secs);
        let max_attempts = self.config.max_attempts.max(1);

        for attempt in 1..=max_attempts {
            let mut cmd = Command::new(command);
            if format != "html" {
                cmd.arg(format!("--{format}"));
            }
            cmd.arg("--output").arg(output);
            for arg in &self.config.standard.args { cmd.arg(arg); }
            cmd.arg(input);
            let result = run_command_with_timeout(ctx, &cmd, timeout)
                .and_then(|out| check_command_output(&out, format_args!("marp {}", input.display())));
            cleanup_marp_tmp_dirs();
            match result {
                Ok(()) => return Ok(()),
                Err(err) => {
                    if !is_timeout_error(&err) || attempt == max_attempts {
                        return Err(err);
                    }
                    eprintln!(
                        "[marp] {} timed out (attempt {}/{}), retrying",
                        input.display(), attempt, max_attempts
                    );
                }
            }
        }
        unreachable!("loop exits via return on every iteration")
    }
}

fn create_marp(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(MarpProcessor::new(cfg)))
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "marp", processor_type: crate::processors::ProcessorType::Generator, create: create_marp,
    known_fields: crate::registries::typed_known_fields::<crate::config::MarpConfig>,
    checksum_fields: crate::registries::typed_checksum_fields::<crate::config::MarpConfig>,
    must_fields: crate::registries::typed_must_fields::<crate::config::MarpConfig>,
    field_descriptions: crate::registries::typed_field_descriptions::<crate::config::MarpConfig>,
    defconfig_json: crate::registries::default_config_json::<crate::config::MarpConfig>,
    keywords: &["markdown", "presentation", "slides", "pdf", "html"],
    description: "Convert Marp Markdown presentations to PDF/HTML",
    is_native: false,
    can_fix: false,
    supports_batch: false,
    max_jobs_cap: None,
} }

/// CI cap for marp: GitHub Actions runners (and similar 2-vCPU CI hosts) OOM
/// when running marp at full project parallelism. Equivalent to the user
/// passing `--pset marp.max_jobs=2`; only applies when the user hasn't set
/// `max_jobs` themselves and `CI=true` is in the environment.
#[allow(clippy::unnecessary_wraps)] // Result<()> is required by PhaseHook::run signature.
fn marp_ci_cap(config: &mut crate::config::Config) -> anyhow::Result<()> {
    if !std::env::var("CI").is_ok_and(|v| v == "true") {
        return Ok(());
    }
    for inst in config.processor.instances.iter_mut().filter(|i| i.type_name == "marp") {
        let Some(table) = inst.config_toml.as_table_mut() else { continue };
        if !table.contains_key("max_jobs") {
            table.insert("max_jobs".to_string(), toml::Value::Integer(2));
        }
    }
    Ok(())
}

inventory::submit! { crate::phases::PhaseHook {
    name: "marp_ci_cap",
    phase: crate::phases::Phase::PostConfig,
    description: "When CI=true and marp.max_jobs is unset, cap it at 2",
    function: concat!(module_path!(), "::marp_ci_cap"),
    location: concat!(file!(), ":", line!()),
    run: marp_ci_cap,
} }
