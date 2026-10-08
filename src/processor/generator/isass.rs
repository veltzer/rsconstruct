//! isass generator — registered as a `SimpleGenerator` with a custom execute fn.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::config::StandardConfig;
use crate::graph::Product;
use crate::processor::ensure_output_dir;

use crate::processor::{DiscoverMode, SimpleGenerator, SimpleGeneratorParams};

const fn default_skip_partials() -> bool {
    true
}

/// isass config. Custom field: `skip_partials` — leave files whose name
/// starts with `_` out of discovery. Such a file is a Sass partial, written
/// to be `@use`d by other stylesheets, and the `sass` CLI compiles none of
/// them to a `.css` of their own; this processor follows it by default.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IsassConfig {
    #[serde(default = "default_skip_partials")]
    pub skip_partials: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for IsassConfig {
    fn default() -> Self {
        Self {
            skip_partials: default_skip_partials(),
            standard: StandardConfig::default(),
        }
    }
}

impl AsRef<StandardConfig> for IsassConfig {
    fn as_ref(&self) -> &StandardConfig {
        &self.standard
    }
}

/// A partial is a stylesheet whose file name starts with `_`.
fn is_partial(source: &Path) -> bool {
    source
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('_'))
}

/// Discovery keeps a scanned file unless it is a partial and partials are
/// skipped.
fn keep_source(config: &IsassConfig, source: &Path) -> bool {
    !(config.skip_partials && is_partial(source))
}

fn execute_isass(
    _ctx: &crate::build_context::BuildContext,
    _config: &IsassConfig,
    product: &Product,
) -> Result<()> {
    let input = product.primary_input();
    let output = product.primary_output();
    ensure_output_dir(output)?;
    let css = grass::from_path(input, &grass::Options::default())
        .map_err(|e| anyhow::anyhow!("Failed to compile {}: {}", input.display(), e))?;
    fs::write(output, &css).with_context(|| format!("Failed to write {}", output.display()))?;
    Ok(())
}

fn create_isass(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| {
        Box::new(SimpleGenerator::new(
            cfg,
            SimpleGeneratorParams {
                extra_tools: &[],
                extra_tools_fn: None,
                source_filter: Some(keep_source),
                discover_mode: DiscoverMode::SingleFormat("css"),
                execute_fn: execute_isass,
                is_native: true,
            },
        ))
    })
}
inventory::submit! { crate::registries::ProcessorPlugin {
    // v2: partials (`_*.scss`) are no longer discovered by default.
    version: 2,
    name: "isass", processor_type: crate::processor::ProcessorType::Generator, create: create_isass,
    fields: &[
        crate::config::FieldSpec { name: "skip_partials", ty: crate::config::FieldType::Bool,
            affects_output: false, required: false,
            doc: "Leave files whose name starts with _ (Sass partials) out of discovery, as the sass CLI does. false compiles every matching file." },
    ],
    omit_standard_fields: &[],
    scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".scss", ".sass"], src_exclude_dirs: &[] }),
    defaults: Some(crate::config::ProcessorDefaults { output_dir: "out/processor.generator.isass", ..crate::config::ProcessorDefaults::EMPTY }),
    defconfig_json: crate::registries::default_config_json::<IsassConfig>,
    keywords: &["sass", "scss", "css", "converter", "web", "frontend"],
    description: "Compile Sass/SCSS to CSS (in-process)",
    is_native: true,
    is_rust: true,
    can_fix: false,
    supports_batch: false,
    max_jobs_cap: None,
} }
