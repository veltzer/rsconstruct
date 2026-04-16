//! pandoc generator — registered as a SimpleGenerator with a custom execute fn.

use std::process::Command;
use anyhow::{Context, Result};

use crate::config::StandardConfig;
use crate::graph::Product;
use crate::processors::{run_command, check_command_output, ensure_output_dir};

use crate::processors::{SimpleGenerator, SimpleGeneratorParams, DiscoverMode};

fn execute_pandoc(ctx: &crate::build_context::BuildContext, config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    let format = output.extension()
        .context("pandoc output has no extension")?
        .to_string_lossy();
    ensure_output_dir(output)?;
    let command = config.require_command("pandoc")?;
    let mut cmd = Command::new(command);
    cmd.env("SOURCE_DATE_EPOCH", "0");
    cmd.arg("--to").arg(format.as_ref());
    if format.as_ref() == "pdf" {
        cmd.arg("-V").arg(r"header-includes=\pdftrailerid{}");
    }
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    cmd.arg("-o").arg(output);
    let out = run_command(ctx, &mut cmd)?;
    check_command_output(&out, format_args!("pandoc {}", input.display()))
}


fn create_pandoc(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "Convert documents using pandoc", extra_tools: &[], discover_mode: DiscoverMode::MultiFormat, execute_fn: execute_pandoc, is_native: false })))
}
inventory::submit! { crate::registries::ProcessorPlugin {
    version: 1,
    name: "pandoc", processor_type: crate::processors::ProcessorType::Generator, create: create_pandoc,
    known_fields: crate::registries::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registries::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registries::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registries::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registries::default_config_json::<crate::config::StandardConfig>,
} }
