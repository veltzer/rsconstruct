//! iactionlint checker: actionlint's rules, in-process.
//!
//! The engine is `crate::actionlint`, a port of actionlint v1.7.12. This
//! file is the processor around it: which config file is read, how the
//! repository root is found, how problems are reported.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::actionlint::{Config as LintConfig, Linter};
use crate::config::StandardConfig;
use crate::graph::Product;

/// Where actionlint looks for its configuration, in order.
const CONFIG_FILES: &[&str] = &[".github/actionlint.yaml", ".github/actionlint.yml"];

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct IactionlintConfig {
    /// The actionlint config file to read. Empty means actionlint's own
    /// lookup: `.github/actionlint.yaml`, then `.github/actionlint.yml`,
    /// else no configuration.
    #[serde(default)]
    pub config_file: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct IactionlintProcessor {
    config: IactionlintConfig,
    /// The linter with its configuration and per-repository caches, built
    /// once per build. The error is kept as text so every product reports
    /// the same config failure.
    linter: OnceLock<Result<Linter, String>>,
}

impl IactionlintProcessor {
    pub const fn new(config: IactionlintConfig) -> Self {
        Self {
            config,
            linter: OnceLock::new(),
        }
    }

    fn read_config(&self) -> Result<Option<LintConfig>> {
        let path = if self.config.config_file.is_empty() {
            match CONFIG_FILES.iter().map(Path::new).find(|p| p.is_file()) {
                Some(p) => p.to_path_buf(),
                None => return Ok(None),
            }
        } else {
            PathBuf::from(&self.config.config_file)
        };
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read actionlint config {}", path.display()))?;
        let config = LintConfig::parse(&text)
            .map_err(|e| anyhow::anyhow!("could not parse config file {}: {e}", path.display()))?;
        Ok(Some(config))
    }

    fn build_linter(&self) -> Result<Linter> {
        let config = self.read_config()?;
        // rsconstruct runs in the project root, which is the repository
        // root actionlint resolves `./.github/actions/...` against.
        let root = std::env::current_dir().context("Failed to get the current directory")?;
        Ok(Linter::new(Some(root.clone()), root, config))
    }

    fn linter(&self) -> Result<&Linter> {
        let built = self
            .linter
            .get_or_init(|| self.build_linter().map_err(|e| format!("{e:#}")));
        match built {
            Ok(linter) => Ok(linter),
            Err(msg) => bail!("{msg}"),
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_files(&[product.primary_input()])
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let linter = self.linter()?;
        let mut failures = Vec::new();
        for file in files {
            let bytes = std::fs::read(file)
                .with_context(|| format!("Failed to read {}", file.display()))?;
            let path = file.display().to_string();
            for error in linter.lint(&path, &bytes) {
                failures.push(format!("{path}:{error}"));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            bail!("{}", failures.join("\n"))
        }
    }
}

impl crate::processor::Processor for IactionlintProcessor {
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
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IactionlintProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "iactionlint",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IactionlintConfig>,
        fields: &[
            crate::config::FieldSpec { name: "config_file", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "actionlint config file to read; empty means .github/actionlint.yaml, then .github/actionlint.yml, else no configuration" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".yml", ".yaml"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { dep_auto: CONFIG_FILES, ..crate::config::ProcessorDefaults::EMPTY }),
        keywords: &["github", "actions", "workflow", "ci", "linter", "yaml", "actionlint", "rust"],
        description: "Lint GitHub Actions workflow files with actionlint's rules (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
