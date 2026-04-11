use std::fs;
use std::process::Command;
use anyhow::{Context, Result};

use crate::config::StandardConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, ProcessorType,
    run_command, run_command_capture, check_command_output, ensure_output_dir};

use super::DiscoverParams;

/// How a simple generator discovers its products.
#[derive(Copy, Clone)]
pub(crate) enum DiscoverMode {
    /// Discover one product per source x format (uses config.formats).
    MultiFormat,
    /// Discover one product per source file with a fixed output extension.
    SingleFormat(&'static str),
}

/// Data-driven generator processor. Replaces identical boilerplate across
/// generators that use StandardConfig with standard discover logic.
pub struct SimpleGenerator {
    base: ProcessorBase,
    config: StandardConfig,
    params: SimpleGeneratorParams,
}

#[derive(Copy, Clone)]
pub(crate) struct SimpleGeneratorParams {
    pub description: &'static str,
    pub extra_tools: &'static [&'static str],
    pub discover_mode: DiscoverMode,
    pub execute_fn: fn(&StandardConfig, &Product) -> Result<()>,
    pub is_native: bool,
}

impl SimpleGenerator {
    pub fn new(config: StandardConfig, params: SimpleGeneratorParams) -> Self {
        Self {
            base: ProcessorBase::generator("", params.description),
            config,
            params,
        }
    }
}

impl Processor for SimpleGenerator {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }

    fn standard_config(&self) -> Option<&StandardConfig> {
        Some(&self.config)
    }

    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> ProcessorType {
        self.base.processor_type()
    }

    fn config_json(&self) -> Option<String> {
        ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn is_native(&self) -> bool {
        self.params.is_native
    }

    fn required_tools(&self) -> Vec<String> {
        if self.params.is_native {
            self.params.extra_tools.iter().map(|t| t.to_string()).collect()
        } else {
            let mut tools = vec![self.config.command.clone()];
            for t in self.params.extra_tools {
                tools.push(t.to_string());
            }
            tools
        }
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let params = DiscoverParams {
            scan: &self.config.scan,
            dep_inputs: &self.config.dep_inputs,
            config: &self.config,
            output_dir: &self.config.output_dir,
            processor_name: instance_name,
        };
        match &self.params.discover_mode {
            DiscoverMode::MultiFormat => {
                super::discover_multi_format(graph, file_index, &params, &self.config.formats)
            }
            DiscoverMode::SingleFormat(ext) => {
                super::discover_single_format(graph, file_index, &params, ext)
            }
        }
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        (self.params.execute_fn)(&self.config, product)
    }
}

// --- Execute functions for each simple generator ---

fn execute_mermaid(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let mut cmd = Command::new(&config.command);
    cmd.arg("-i").arg(input);
    cmd.arg("-o").arg(output);
    for arg in &config.args { cmd.arg(arg); }
    let out = run_command(&mut cmd)?;
    check_command_output(&out, format_args!("mmdc {}", input.display()))
}

fn execute_drawio(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    let format = output.extension()
        .context("drawio output has no extension")?
        .to_string_lossy();
    ensure_output_dir(output)?;
    let mut cmd = Command::new(&config.command);
    cmd.arg("--export");
    cmd.arg("--format").arg(format.as_ref());
    cmd.arg("--output").arg(output);
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    let out = run_command(&mut cmd)?;
    check_command_output(&out, format_args!("drawio {}", input.display()))
}

fn execute_sass(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let mut cmd = Command::new(&config.command);
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input).arg(output);
    let out = run_command(&mut cmd)?;
    check_command_output(&out, format_args!("sass {}", input.display()))
}

fn execute_protobuf(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    let output_dir = output.parent().unwrap_or(std::path::Path::new("."));
    ensure_output_dir(output)?;
    let mut cmd = Command::new(&config.command);
    if let Some(parent) = input.parent() {
        cmd.arg(format!("--proto_path={}", parent.display()));
    }
    cmd.arg(format!("--cpp_out={}", output_dir.display()));
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    let out = run_command(&mut cmd)?;
    check_command_output(&out, format_args!("protoc {}", input.display()))
}

fn execute_chromium(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let abs_input = fs::canonicalize(input)
        .with_context(|| format!("Failed to resolve absolute path for: {}", input.display()))?;
    let input_url = format!("file://{}", abs_input.display());
    let mut cmd = Command::new(&config.command);
    cmd.arg("--headless");
    cmd.arg("--disable-gpu");
    cmd.arg("--no-sandbox");
    cmd.arg(format!("--print-to-pdf={}", output.display()));
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(&input_url);
    let out = run_command(&mut cmd)?;
    check_command_output(&out, format_args!("chromium {}", input.display()))
}

fn execute_markdown2html(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let mut cmd = Command::new(&config.command);
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    let out = run_command_capture(&mut cmd)?;
    check_command_output(&out, format_args!("markdown {}", input.display()))?;
    fs::write(output, &out.stdout)
        .with_context(|| format!("Failed to write markdown output: {}", output.display()))?;
    Ok(())
}

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
    let mut cmd = Command::new("flock");
    cmd.arg("/tmp/rsconstruct_libreoffice");
    cmd.arg(&config.command);
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
fn cleanup_marp_tmp_dirs() {
    let Ok(entries) = fs::read_dir("/tmp") else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        if entry.file_name().to_string_lossy().starts_with("marp-cli-") {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn execute_marp(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    let format = output.extension()
        .context("marp output has no extension")?
        .to_string_lossy();
    ensure_output_dir(output)?;
    let mut cmd = Command::new(&config.command);
    if format != "html" {
        cmd.arg(format!("--{}", format));
    }
    cmd.arg("--output").arg(output);
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    let out = run_command(&mut cmd)?;
    let result = check_command_output(&out, format_args!("marp {}", input.display()));
    cleanup_marp_tmp_dirs();
    result
}

fn execute_pandoc(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    let format = output.extension()
        .context("pandoc output has no extension")?
        .to_string_lossy();
    ensure_output_dir(output)?;
    let mut cmd = Command::new(&config.command);
    cmd.env("SOURCE_DATE_EPOCH", "0");
    cmd.arg("--to").arg(format.as_ref());
    if format.as_ref() == "pdf" {
        cmd.arg("-V").arg(r"header-includes=\pdftrailerid{}");
    }
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    cmd.arg("-o").arg(output);
    let out = run_command(&mut cmd)?;
    check_command_output(&out, format_args!("pandoc {}", input.display()))
}

fn execute_a2x(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let mut cmd = Command::new(&config.command);
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    let out = run_command(&mut cmd)?;
    check_command_output(&out, format_args!("a2x {}", input.display()))?;
    // a2x generates the PDF next to the input file — move it to the output path
    let stem = input.file_stem()
        .context("a2x input has no file stem")?;
    let generated = input.with_file_name(format!("{}.pdf", stem.to_string_lossy()));
    if generated != *output && generated.exists() {
        fs::rename(&generated, output)
            .with_context(|| format!("Failed to move a2x output from {} to {}", generated.display(), output.display()))?;
    }
    Ok(())
}

fn execute_objdump(config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let mut cmd = Command::new(&config.command);
    cmd.arg("--disassemble").arg("--source");
    for arg in &config.args { cmd.arg(arg); }
    cmd.arg(input);
    let out = run_command_capture(&mut cmd)?;
    check_command_output(&out, format_args!("objdump {}", input.display()))?;
    fs::write(output, &out.stdout)
        .with_context(|| format!("Failed to write objdump output: {}", output.display()))?;
    Ok(())
}

fn execute_imarkdown2html(_config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let contents = fs::read_to_string(input)
        .with_context(|| format!("Failed to read {}", input.display()))?;
    let parser = pulldown_cmark::Parser::new(&contents);
    let mut html_output = String::new();
    pulldown_cmark::html::push_html(&mut html_output, parser);
    fs::write(output, &html_output)
        .with_context(|| format!("Failed to write {}", output.display()))?;
    Ok(())
}

fn execute_isass(_config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let css = grass::from_path(input, &grass::Options::default())
        .map_err(|e| anyhow::anyhow!("Failed to compile {}: {}", input.display(), e))?;
    fs::write(output, &css)
        .with_context(|| format!("Failed to write {}", output.display()))?;
    Ok(())
}

fn execute_yaml2json(_config: &StandardConfig, product: &Product) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let contents = fs::read_to_string(input)
        .with_context(|| format!("Failed to read {}", input.display()))?;
    let value: serde_json::Value = serde_yml::from_str(&contents)
        .with_context(|| format!("Failed to parse YAML from {}", input.display()))?;
    let json = serde_json::to_string_pretty(&value)
        .with_context(|| format!("Failed to serialize JSON for {}", input.display()))?;
    fs::write(output, json)
        .with_context(|| format!("Failed to write {}", output.display()))?;
    Ok(())
}


// --- Plugin registrations ---

// --- Plugin registrations ---

fn create_mermaid(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &["node"], discover_mode: DiscoverMode::MultiFormat, execute_fn: execute_mermaid, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "mermaid", processor_type: crate::processors::ProcessorType::Generator, create: create_mermaid,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_drawio(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &[], discover_mode: DiscoverMode::MultiFormat, execute_fn: execute_drawio, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "drawio", processor_type: crate::processors::ProcessorType::Generator, create: create_drawio,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_sass(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &[], discover_mode: DiscoverMode::SingleFormat("css"), execute_fn: execute_sass, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "sass", processor_type: crate::processors::ProcessorType::Generator, create: create_sass,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_protobuf(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &[], discover_mode: DiscoverMode::SingleFormat("pb.cc"), execute_fn: execute_protobuf, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "protobuf", processor_type: crate::processors::ProcessorType::Generator, create: create_protobuf,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_chromium(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &[], discover_mode: DiscoverMode::SingleFormat("pdf"), execute_fn: execute_chromium, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "chromium", processor_type: crate::processors::ProcessorType::Generator, create: create_chromium,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_markdown2html(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &["perl"], discover_mode: DiscoverMode::SingleFormat("html"), execute_fn: execute_markdown2html, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "markdown2html", processor_type: crate::processors::ProcessorType::Generator, create: create_markdown2html,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_libreoffice(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &["flock"], discover_mode: DiscoverMode::MultiFormat, execute_fn: execute_libreoffice, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "libreoffice", processor_type: crate::processors::ProcessorType::Generator, create: create_libreoffice,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_marp(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &["node"], discover_mode: DiscoverMode::MultiFormat, execute_fn: execute_marp, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "marp", processor_type: crate::processors::ProcessorType::Generator, create: create_marp,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_pandoc(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &[], discover_mode: DiscoverMode::MultiFormat, execute_fn: execute_pandoc, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "pandoc", processor_type: crate::processors::ProcessorType::Generator, create: create_pandoc,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_a2x(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &["python3"], discover_mode: DiscoverMode::SingleFormat("pdf"), execute_fn: execute_a2x, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "a2x", processor_type: crate::processors::ProcessorType::Generator, create: create_a2x,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_objdump(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &[], discover_mode: DiscoverMode::SingleFormat("dis"), execute_fn: execute_objdump, is_native: false })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "objdump", processor_type: crate::processors::ProcessorType::Generator, create: create_objdump,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_imarkdown2html(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &[], discover_mode: DiscoverMode::SingleFormat("html"), execute_fn: execute_imarkdown2html, is_native: true })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "imarkdown2html", processor_type: crate::processors::ProcessorType::Generator, create: create_imarkdown2html,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_isass(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &[], discover_mode: DiscoverMode::SingleFormat("css"), execute_fn: execute_isass, is_native: true })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "isass", processor_type: crate::processors::ProcessorType::Generator, create: create_isass,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

fn create_yaml2json(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SimpleGenerator::new(cfg, SimpleGeneratorParams { description: "", extra_tools: &[], discover_mode: DiscoverMode::SingleFormat("json"), execute_fn: execute_yaml2json, is_native: true })))
}
inventory::submit! { crate::registry::ProcessorPlugin {
    name: "yaml2json", processor_type: crate::processors::ProcessorType::Generator, create: create_yaml2json,
    known_fields: crate::registry::typed_known_fields::<crate::config::StandardConfig>,
    output_fields: crate::registry::typed_output_fields::<crate::config::StandardConfig>,
    must_fields: crate::registry::typed_must_fields::<crate::config::StandardConfig>,
    field_descriptions: crate::registry::typed_field_descriptions::<crate::config::StandardConfig>,
    defconfig_json: crate::registry::default_config_json::<crate::config::StandardConfig>,
} }

