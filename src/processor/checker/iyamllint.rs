//! iyamllint checker: yamllint's rules, in-process.
//!
//! The linting engine is `crate::engines::yamllint`, a port of yamllint's parser,
//! configuration and 23 rules. This file is the processor around it: which
//! config file is read, what fails a product, how problems are reported.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::engines::yamllint::{self, Config as LintConfig, Level};
use crate::graph::Product;

/// The config files yamllint looks for in the working directory, in its
/// order of preference. The user-level `~/.config/yamllint/config` yamllint
/// also consults is deliberately not read: a build must not depend on the
/// machine it runs on.
const CONFIG_FILES: &[&str] = &[".yamllint", ".yamllint.yaml", ".yamllint.yml"];

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct IyamllintConfig {
    /// The yamllint config file to read. Empty means yamllint's own lookup:
    /// `.yamllint`, `.yamllint.yaml` or `.yamllint.yml` in the project
    /// root, else yamllint's `default` configuration.
    #[serde(default)]
    pub config_file: String,
    /// Fail on warnings too, like `yamllint --strict`. Off, warnings are
    /// printed and the product passes, like a plain `yamllint` run.
    #[serde(default)]
    pub strict: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct IyamllintProcessor {
    config: IyamllintConfig,
    /// The yamllint configuration, read once per build. The error is kept
    /// as text so every product reports the same config failure.
    lint_config: OnceLock<Result<LintConfig, String>>,
}

impl IyamllintProcessor {
    pub const fn new(config: IyamllintConfig) -> Self {
        Self {
            config,
            lint_config: OnceLock::new(),
        }
    }

    fn read_lint_config(&self) -> Result<LintConfig> {
        if !self.config.config_file.is_empty() {
            return LintConfig::from_file(Path::new(&self.config.config_file));
        }
        for name in CONFIG_FILES {
            let path = Path::new(name);
            if path.is_file() {
                return LintConfig::from_file(path);
            }
        }
        Ok(LintConfig::default_conf())
    }

    fn lint_config(&self) -> Result<&LintConfig> {
        let loaded = self
            .lint_config
            .get_or_init(|| self.read_lint_config().map_err(|e| format!("{e:#}")));
        match loaded {
            Ok(conf) => Ok(conf),
            Err(msg) => bail!("{msg}"),
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let conf = self.lint_config()?;
        let mut failures = Vec::new();

        for file in files {
            let contents = std::fs::read_to_string(file)
                .with_context(|| format!("Failed to read {}", file.display()))?;
            // yamllint decodes a BOM-prefixed file as `utf_8_sig`, dropping
            // the BOM; `PyYAML` would otherwise count it as a character.
            let contents = contents.strip_prefix('\u{feff}').unwrap_or(&contents);
            for problem in yamllint::lint(contents, conf, Some(file)) {
                let line = format!("{}:{problem}", file.display());
                if problem.level == Level::Error || self.config.strict {
                    failures.push(line);
                } else {
                    crate::output::warn(&line);
                }
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            bail!("{}", failures.join("\n"))
        }
    }
}

impl crate::processor::Processor for IyamllintProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    // Serialize the FULL config (the trait default covers StandardConfig
    // only), so the extra fields reach config-change detection.
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
        crate::processor::execute_checker_batch_per_file(products, |file| self.check_files(&[file]))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IyamllintProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        // v2: yamllint's rules and `.yamllint.yaml` are applied; v1 only
        // parsed the file.
        version: 2,
        name: "iyamllint",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IyamllintConfig>,
        fields: &[
            crate::config::FieldSpec { name: "config_file", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "yamllint config file to read; empty means .yamllint, .yamllint.yaml or .yamllint.yml in the project root, else yamllint's default configuration" },
            crate::config::FieldSpec { name: "strict", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Fail on warnings too, like yamllint --strict (default: warnings are printed and the file passes)" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { dep_auto: CONFIG_FILES, ..crate::config::ProcessorDefaults::EMPTY }),
        keywords: &["yaml", "yml", "linter", "validator", "yamllint", "rust"],
        description: "Lint YAML files with yamllint's rules (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
