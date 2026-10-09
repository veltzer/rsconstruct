//! iyq checker: `yq FILTER file` in-process.
//!
//! The Rust alternative to the `yq` checker, which runs `yq .` over every
//! YAML file: the file is read as YAML (every document of it), turned into
//! JSON, and the jq filter is evaluated over each document with jaq. A file
//! that is not valid YAML, or on which the filter raises an error, fails;
//! the filter's outputs are not kept (this is a checker).

use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use jaq_core::load::{Arena, File, Loader};
use jaq_core::{Compiler, Ctx, Vars, data, unwrap_valr};
use jaq_json::Val;
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::graph::Product;

fn default_filter() -> String {
    ".".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IyqConfig {
    /// The jq filter to evaluate over every document; `.` just checks
    /// that the YAML parses.
    #[serde(default = "default_filter")]
    pub filter: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for IyqConfig {
    fn default() -> Self {
        Self {
            filter: default_filter(),
            standard: StandardConfig::default(),
        }
    }
}

pub struct IyqProcessor {
    config: IyqConfig,
}

impl IyqProcessor {
    pub const fn new(config: IyqConfig) -> Self {
        Self { config }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_file(product.primary_input())
    }

    /// The documents of a YAML file as JSON values, as `yq` hands them to
    /// `jq` one by one.
    fn documents(file: &Path) -> Result<Vec<serde_json::Value>> {
        let text = std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read {}", file.display()))?;
        // A byte order mark is not YAML content; PyYAML (and so yq) skips it.
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let mut docs = Vec::new();
        for document in serde_yaml_ng::Deserializer::from_str(text) {
            let value = serde_json::Value::deserialize(document)
                .map_err(|e| anyhow!("{}: {e}", file.display()))?;
            docs.push(value);
        }
        Ok(docs)
    }

    fn check_file(&self, file: &Path) -> Result<()> {
        let docs = Self::documents(file)?;
        let filter_text = self.config.filter.as_str();
        let arena = Arena::default();
        let program = File {
            code: filter_text,
            path: (),
        };
        let loader = Loader::new(
            jaq_core::defs()
                .chain(jaq_std::defs())
                .chain(jaq_json::defs()),
        );
        let modules = loader
            .load(&arena, program)
            .map_err(|errs| anyhow!("iyq filter {filter_text:?}: {}", load_errors(&errs)))?;
        let filter = Compiler::default()
            .with_funs(
                jaq_core::funs()
                    .chain(jaq_std::funs())
                    .chain(jaq_json::funs()),
            )
            .compile(modules)
            .map_err(|errs| anyhow!("iyq filter {filter_text:?}: {}", load_errors(&errs)))?;
        for (index, doc) in docs.iter().enumerate() {
            let bytes = serde_json::to_vec(doc).with_context(|| {
                format!(
                    "{}: document {}: not representable as JSON",
                    file.display(),
                    index + 1
                )
            })?;
            let input = jaq_json::read::parse_single(&bytes)
                .map_err(|e| anyhow!("{}: document {}: {e}", file.display(), index + 1))?;
            let ctx = Ctx::<data::JustLut<Val>>::new(&filter.lut, Vars::new([]));
            for out in filter.id.run((ctx, input)).map(unwrap_valr) {
                if let Err(e) = out {
                    bail!("{}: document {}: jq: {e}", file.display(), index + 1);
                }
            }
        }
        Ok(())
    }
}

/// jaq's load or compile errors (a list of per-file errors), one per line.
fn load_errors<F, E: std::fmt::Debug>(errs: &[(F, E)]) -> String {
    errs.iter()
        .map(|(_, err)| format!("{err:?}"))
        .collect::<Vec<_>>()
        .join("\n")
}

impl crate::processor::Processor for IyqProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    // Serialize the FULL config (the trait default covers StandardConfig
    // only), so `filter` reaches config-change detection.
    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    /// One product per scanned file (the default `discover`).
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::PerFile
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_product(product)
    }

    fn execute_batch(
        &self,
        _ctx: &crate::build_context::BuildContext,
        products: &[&Product],
    ) -> Vec<Result<()>> {
        crate::processor::execute_checker_batch_per_file(products, |file| self.check_file(file))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IyqProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "iyq",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IyqConfig>,
        fields: &[
            crate::config::FieldSpec { name: "filter", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The jq filter evaluated over every YAML document; \".\" only checks that the YAML parses" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] }),
        defaults: None,
        keywords: &["yaml", "yml", "jq", "checker", "validator", "rust"],
        description: "Check YAML files with a jq filter (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
