//! icheckstyle checker: checkstyle's Java checks, in-process.
//!
//! The engine is `crate::engines::checkstyle`, a port of checkstyle 8.36.1
//! (its ANTLR Java grammar, AST and the checks the fleet's configuration
//! enables). This file is the processor around it: the `checkstyle.xml`
//! the run uses, read once per build, and each file's messages printed as
//! `checkstyle` prints them. A file fails when it has a message of severity
//! `error` (checkstyle's exit status counts those) or when checkstyle would
//! stop on it (a parse error); `warning` and `info` messages are printed and
//! the file passes.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::engines::checkstyle::message::Severity;
use crate::engines::checkstyle::{Config, Outcome};
use crate::graph::Product;

fn default_config_file() -> String {
    "checkstyle.xml".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IcheckstyleConfig {
    /// The checkstyle configuration, as `checkstyle -c`; relative to the
    /// project root.
    #[serde(default = "default_config_file")]
    pub config_file: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for IcheckstyleConfig {
    fn default() -> Self {
        Self {
            config_file: default_config_file(),
            standard: StandardConfig::default(),
        }
    }
}

pub struct IcheckstyleProcessor {
    config: IcheckstyleConfig,
    /// The checkstyle configuration, read once per build. The error is
    /// kept as text so every product reports the same failure.
    checks: OnceLock<Result<Config, String>>,
}

impl IcheckstyleProcessor {
    pub const fn new(config: IcheckstyleConfig) -> Self {
        Self {
            config,
            checks: OnceLock::new(),
        }
    }

    fn load_checks(&self) -> Result<Config> {
        let path = Path::new(&self.config.config_file);
        let text = std::fs::read_to_string(path).with_context(|| {
            format!("Failed to read checkstyle configuration {}", path.display())
        })?;
        crate::engines::checkstyle::config::parse(&text)
            .with_context(|| format!("icheckstyle: {}", path.display()))
    }

    fn checks(&self) -> Result<&Config> {
        let loaded = self
            .checks
            .get_or_init(|| self.load_checks().map_err(|e| format!("{e:#}")));
        match loaded {
            Ok(config) => Ok(config),
            Err(e) => bail!("{e}"),
        }
    }

    fn check_file(&self, file: &Path) -> Result<()> {
        let config = self.checks()?;
        let bytes =
            std::fs::read(file).with_context(|| format!("Failed to read {}", file.display()))?;
        let name = file.display().to_string();
        match crate::engines::checkstyle::check_file(&name, &bytes, config) {
            Outcome::Skipped => Ok(()),
            Outcome::Failed(failure) => bail!("{}", failure.render(&name)),
            Outcome::Checked(messages) => {
                let shown: Vec<_> = messages
                    .iter()
                    .filter(|m| m.severity != Severity::Ignore)
                    .collect();
                if shown.iter().any(|m| m.severity == Severity::Error) {
                    let lines: Vec<String> = shown.iter().map(|m| m.render(&name)).collect();
                    bail!("{}", lines.join("\n"));
                }
                for m in shown {
                    crate::output::warn(&m.render(&name));
                }
                Ok(())
            }
        }
    }
}

impl crate::processor::Processor for IcheckstyleProcessor {
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
        self.check_file(product.primary_input())
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
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IcheckstyleProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "icheckstyle",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IcheckstyleConfig>,
        fields: &[
            crate::config::FieldSpec { name: "config_file", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The checkstyle configuration, as checkstyle -c; relative to the project root (checkstyle.xml)" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".java"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { dep_auto: &["checkstyle.xml"], ..crate::config::ProcessorDefaults::EMPTY }),
        keywords: &["java", "linter", "checker", "style", "checkstyle", "rust"],
        description: "Check Java code with checkstyle's checks (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
