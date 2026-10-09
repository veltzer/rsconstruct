//! itidy checker: `tidy -errors -q`, in-process.
//!
//! The engine is `crate::tidy`, a port of HTML Tidy 5.8.0's lexer, parser,
//! attribute checks and clean-up passes: it reports what tidy reports, at
//! the same line and column, with the same warning and error totals. This
//! file is the processor around it: which tidy options apply, what fails a
//! product, how problems are printed.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::graph::Product;
use crate::tidy::{self, Options};

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ItidyConfig {
    /// A tidy configuration file (`name: value` lines, `#` comments), read
    /// the way `tidy -config FILE` reads it; relative to the project root.
    /// Empty means none.
    #[serde(default)]
    pub config_file: String,
    /// Further tidy options as `name: value` strings, applied after the
    /// config file in order, the way `--name value` arguments are. For
    /// example `["custom-tags: blocklevel", "drop-empty-elements: no"]`.
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct ItidyProcessor {
    config: ItidyConfig,
    /// The tidy options, built once per build. The error is kept as text
    /// so every product reports the same configuration failure.
    options: OnceLock<Result<Options, String>>,
}

impl ItidyProcessor {
    pub const fn new(config: ItidyConfig) -> Self {
        Self {
            config,
            options: OnceLock::new(),
        }
    }

    fn build_options(&self) -> Result<Options> {
        // What `tidy -errors -q` prints: the reports, not the chatter.
        let mut opts = Options {
            quiet: true,
            ..Options::default()
        };
        if !self.config.config_file.is_empty() {
            let path = Path::new(&self.config.config_file);
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read tidy config file {}", path.display()))?;
            opts.apply_config_text(&text)
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        }
        for setting in &self.config.options {
            let Some((name, value)) = setting.split_once(':') else {
                bail!("itidy option {setting:?} is not of the form \"name: value\"");
            };
            opts.set(name, value)
                .map_err(|e| anyhow::anyhow!("itidy option {setting:?}: {e}"))?;
        }
        Ok(opts)
    }

    fn options(&self) -> Result<&Options> {
        let built = self
            .options
            .get_or_init(|| self.build_options().map_err(|e| format!("{e:#}")));
        match built {
            Ok(opts) => Ok(opts),
            Err(e) => bail!("{e}"),
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_file(product.primary_input())
    }

    fn check_file(&self, file: &Path) -> Result<()> {
        let opts = self.options()?;
        let bytes =
            std::fs::read(file).with_context(|| format!("Failed to read {}", file.display()))?;
        let report =
            tidy::check(&bytes, opts).map_err(|e| anyhow::anyhow!("{}: {e}", file.display()))?;
        if report.warnings == 0 && report.errors == 0 {
            return Ok(());
        }
        let lines: Vec<String> = report
            .lines
            .iter()
            .map(|line| format!("{}: {}", file.display(), line.trim_end()))
            .collect();
        bail!("{}", lines.join("\n"))
    }
}

impl crate::processor::Processor for ItidyProcessor {
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
        crate::processor::execute_checker_batch_per_file(products, |file| self.check_file(file))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(ItidyProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "itidy",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<ItidyConfig>,
        fields: &[
            crate::config::FieldSpec { name: "config_file", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "A tidy configuration file (name: value lines) read like tidy -config; relative to the project root. Empty: none" },
            crate::config::FieldSpec { name: "options", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Further tidy options as \"name: value\" strings, applied after the config file, like --name value arguments" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".html", ".htm"], src_exclude_dirs: &[] }),
        defaults: None,
        keywords: &["html", "validator", "web", "tidy", "rust"],
        description: "Check HTML the way tidy -errors does (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
