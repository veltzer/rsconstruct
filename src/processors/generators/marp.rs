//! marp generator — registered as a SimpleGenerator with a custom execute fn.

use std::fs;
use std::process::Command;
use std::time::Duration;
use anyhow::{Context, Result};

use crate::config::StandardConfig;
use crate::graph::Product;
use crate::processors::{run_command_with_timeout, check_command_output, ensure_output_dir};

use crate::processors::{SimpleGenerator, SimpleGeneratorParams, DiscoverMode};

/// marp occasionally hangs (chromium-headless / sandbox issues). Kill it after this long
/// rather than letting the build sit forever.
const MARP_TIMEOUT: Duration = Duration::from_secs(10);

fn cleanup_marp_tmp_dirs() {
    let Ok(entries) = fs::read_dir("/tmp") else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy().starts_with("marp-cli-") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn execute_marp(ctx: &crate::build_context::BuildContext, config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    let format = output.extension()
        .context("marp output has no extension")?
        .to_string_lossy();
    ensure_output_dir(output)?;
    let command = config.require_command("marp")?;
    let mut cmd = Command::new(command);
    if format != "html" {
        cmd.arg(format!("--{}", format));
    }
    cmd.arg("--output").arg(output);
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    let result = run_command_with_timeout(ctx, &mut cmd, MARP_TIMEOUT)
        .and_then(|out| check_command_output(&out, format_args!("marp {}", input.display())));
    cleanup_marp_tmp_dirs();
    result
}


fn create_marp(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { extra_tools: &["node"], discover_mode: DiscoverMode::MultiFormat, execute_fn: execute_marp, is_native: false })))
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "marp", processor_type: crate::processors::ProcessorType::Generator, create: create_marp,
    known_fields: crate::registries::typed_known_fields::<crate::config::StandardConfig>,
    checksum_fields: crate::registries::typed_checksum_fields::<crate::config::StandardConfig>,
    must_fields: crate::registries::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registries::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
    keywords: &["markdown", "presentation", "slides", "pdf", "html"],
    description: "Convert Marp Markdown presentations to PDF/HTML",
    is_native: false,
    can_fix: false,
    supports_batch: false,
    max_jobs_cap: None,
} }
