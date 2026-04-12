//! libreoffice generator — registered as a SimpleGenerator with a custom execute fn.

use std::fs;
use std::process::Command;
use anyhow::{Context, Result};

use crate::config::StandardConfig;
use crate::graph::Product;
use crate::processors::{run_command, check_command_output};

use crate::processors::{SimpleGenerator, SimpleGeneratorParams, DiscoverMode};

fn execute_libreoffice(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    let format = output.extension()
        .context("libreoffice output has no extension")?
        .to_string_lossy();
    let output_dir = output.parent()
        .context("libreoffice output has no parent directory")?;
    fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create libreoffice output directory: {}", output_dir.display()))?;
    let command = config.require_command("libreoffice")?;
    let mut cmd = Command::new("flock");
    cmd.arg("/tmp/rsconstruct_libreoffice");
    cmd.arg(command);
    cmd.arg("--headless");
    cmd.arg("--convert-to").arg(format.as_ref());
    cmd.arg("--outdir").arg(output_dir);
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    let out = run_command(&mut cmd)?;
    check_command_output(&out, format_args!("libreoffice {}", input.display()))
}

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

fn create_libreoffice(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "Convert documents using LibreOffice", extra_tools: &["flock"], discover_mode: DiscoverMode::MultiFormat, execute_fn: execute_libreoffice, is_native: false })))
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "libreoffice", processor_type: crate::processors::ProcessorType::Generator, create: create_libreoffice,
    known_fields: crate::registries::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registries::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registries::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registries::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
} }
