//! isvglint checker: svglint's built-in rules, in-process.
//!
//! The engine is `crate::engines::svglint`, a port of svglint 4.2.1's lenient SVG
//! parser, CSS selector matching, XML validity check and the `elm` and
//! `attr` rules. This file is the processor around it: the rules come from
//! TOML fields instead of a JavaScript config file, and every finding
//! fails the file, as svglint's exit status does.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::engines::svglint::{self, Rules};
use crate::graph::Product;

const fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IsvglintConfig {
    /// The `valid` rule: the file must be well-formed XML.
    #[serde(default = "default_true")]
    pub valid: bool,
    /// The `elm` rule: CSS selector to `true` (at least one), `false`
    /// (none), a count, or `[min, max]`.
    #[serde(default)]
    pub elm: toml::Table,
    /// The `attr` rule: one table per element selector, naming the
    /// attributes allowed, required or forbidden and their values.
    #[serde(default)]
    pub attr: Vec<toml::Table>,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for IsvglintConfig {
    fn default() -> Self {
        Self {
            valid: true,
            elm: toml::Table::new(),
            attr: Vec::new(),
            standard: StandardConfig::default(),
        }
    }
}

pub struct IsvglintProcessor {
    config: IsvglintConfig,
    /// The compiled rules, built once per build. The error is kept as text
    /// so every product reports the same configuration failure.
    rules: OnceLock<Result<Rules, String>>,
}

impl IsvglintProcessor {
    pub const fn new(config: IsvglintConfig) -> Self {
        Self {
            config,
            rules: OnceLock::new(),
        }
    }

    fn rules(&self) -> Result<&Rules> {
        let built = self.rules.get_or_init(|| {
            Rules::from_config(self.config.valid, &self.config.elm, &self.config.attr)
                .map_err(|e| format!("isvglint: {e}"))
        });
        match built {
            Ok(rules) => Ok(rules),
            Err(e) => bail!("{e}"),
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_file(product.primary_input())
    }

    fn check_file(&self, file: &Path) -> Result<()> {
        let rules = self.rules()?;
        let bytes =
            std::fs::read(file).with_context(|| format!("Failed to read {}", file.display()))?;
        // svglint reads the file as UTF-8 text, replacing invalid bytes.
        let source = String::from_utf8_lossy(&bytes);
        let reports = svglint::lint(&source, rules)
            .map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
        if reports.is_empty() {
            return Ok(());
        }
        let lines: Vec<String> = reports
            .iter()
            .map(|r| match &r.at {
                Some(at) => format!(
                    "{}:{}:{}: {}: {} (<{}>)",
                    file.display(),
                    at.line,
                    at.column,
                    r.rule,
                    r.text,
                    at.name
                ),
                None => format!("{}: {}: {}", file.display(), r.rule, r.text),
            })
            .collect();
        bail!("{}", lines.join("\n"))
    }
}

impl crate::processor::Processor for IsvglintProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    // Serialize the FULL config (the trait default covers StandardConfig
    // only), so the rules reach config-change detection.
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
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IsvglintProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "isvglint",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IsvglintConfig>,
        fields: &[
            crate::config::FieldSpec { name: "valid", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "The valid rule: every file must be well-formed XML (fast-xml-parser's validator)" },
            crate::config::FieldSpec { name: "elm", ty: crate::config::FieldType::Table,
                affects_output: true, required: false,
                doc: "The elm rule: CSS selector = true (at least one), false (none), a count, or [min, max]" },
            crate::config::FieldSpec { name: "attr", ty: crate::config::FieldType::TableArray,
                affects_output: true, required: false,
                doc: "The attr rule: one table per \"rule::selector\" naming allowed, required and forbidden attributes and their values" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".svg"], src_exclude_dirs: &[] }),
        defaults: None,
        keywords: &["svg", "linter", "xml", "validator", "rust"],
        description: "Lint SVG files with svglint's rules (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
