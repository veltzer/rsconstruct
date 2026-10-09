//! idockerfile checker: hadolint's DL rules, in-process.
//!
//! The engine is `crate::engines::hadolint`, a port of hadolint 2.15.1's parser,
//! pragmas and rules. This file is the processor around it: where the
//! configuration comes from (`.hadolint.yaml` found like hadolint finds it,
//! or a named file, plus TOML fields), what fails a product (hadolint's
//! failure threshold), and how findings are printed (hadolint's own
//! `file:line CODE severity: message`).

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::engines::hadolint::{self, config::Config, config::LabelType, config::Severity};
use crate::graph::Product;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct IdockerfileConfig {
    /// A hadolint configuration file (YAML). Empty: `.hadolint.yaml` or
    /// `.hadolint.yml` in the project root when one exists, as hadolint
    /// looks for them.
    #[serde(default)]
    pub config_file: String,
    /// Rule codes to ignore, added to the configuration file's `ignored`.
    #[serde(default)]
    pub ignored: Vec<String>,
    /// Registries FROM images may come from (DL3026); `*` wildcards allowed.
    #[serde(default)]
    pub trusted_registries: Vec<String>,
    /// Labels that must be present, with their type: `text`, `url`,
    /// `semver`, `hash`, `rfc3339`, `spdx`, `email`.
    #[serde(default)]
    pub label_schema: toml::Table,
    /// Forbid labels outside `label_schema` (DL3050).
    #[serde(default)]
    pub strict_labels: bool,
    /// Ignore `# hadolint ignore=...` pragmas.
    #[serde(default)]
    pub disable_ignore_pragma: bool,
    /// Fail only on findings at least this severe: `error`, `warning`,
    /// `info`, `style`, `none`. Empty: the configuration file's, else `info`.
    #[serde(default)]
    pub failure_threshold: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct IdockerfileProcessor {
    config: IdockerfileConfig,
    /// The hadolint configuration, built once per build. The error is kept
    /// as text so every product reports the same configuration failure.
    hadolint: OnceLock<Result<Config, String>>,
}

impl IdockerfileProcessor {
    pub const fn new(config: IdockerfileConfig) -> Self {
        Self {
            config,
            hadolint: OnceLock::new(),
        }
    }

    fn build_config(&self) -> Result<Config> {
        let mut cfg = Config::default();
        let path = if self.config.config_file.is_empty() {
            [".hadolint.yaml", ".hadolint.yml"]
                .iter()
                .map(Path::new)
                .find(|p| p.is_file())
                .map(Path::to_path_buf)
        } else {
            Some(Path::new(&self.config.config_file).to_path_buf())
        };
        if let Some(path) = path {
            let text = std::fs::read_to_string(&path).with_context(|| {
                format!("Failed to read hadolint config file {}", path.display())
            })?;
            cfg.apply_yaml(&text)
                .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        }
        cfg.ignore_rules.extend(self.config.ignored.iter().cloned());
        cfg.trusted_registries
            .extend(self.config.trusted_registries.iter().cloned());
        for (name, ty) in &self.config.label_schema {
            let Some(ty) = ty.as_str() else {
                bail!("idockerfile label_schema {name:?}: the type must be a string");
            };
            let ty = LabelType::parse(ty)
                .map_err(|e| anyhow::anyhow!("idockerfile label_schema {name:?}: {e}"))?;
            cfg.set_label(name.clone(), ty);
        }
        if self.config.strict_labels {
            cfg.strict_labels = true;
        }
        if self.config.disable_ignore_pragma {
            cfg.disable_ignore_pragma = true;
        }
        if !self.config.failure_threshold.is_empty() {
            cfg.failure_threshold = Severity::parse(&self.config.failure_threshold)
                .map_err(|e| anyhow::anyhow!("idockerfile failure_threshold: {e}"))?;
        }
        Ok(cfg)
    }

    fn hadolint_config(&self) -> Result<&Config> {
        let built = self
            .hadolint
            .get_or_init(|| self.build_config().map_err(|e| format!("{e:#}")));
        match built {
            Ok(cfg) => Ok(cfg),
            Err(e) => bail!("{e}"),
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_file(product.primary_input())
    }

    fn check_file(&self, file: &Path) -> Result<()> {
        let cfg = self.hadolint_config()?;
        let bytes =
            std::fs::read(file).with_context(|| format!("Failed to read {}", file.display()))?;
        let source = hadolint::decode(&bytes);
        let report = match hadolint::lint(&source, cfg) {
            Ok(r) => r,
            Err(e) => bail!("{}:{}:{} {}", file.display(), e.line, e.column, e.message),
        };
        if !report.fails {
            return Ok(());
        }
        let lines: Vec<String> = report
            .failures
            .iter()
            .map(|f| {
                format!(
                    "{}:{} {} {}: {}",
                    file.display(),
                    f.line,
                    f.code,
                    f.severity.text(),
                    f.message
                )
            })
            .collect();
        bail!("{}", lines.join("\n"))
    }
}

impl crate::processor::Processor for IdockerfileProcessor {
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
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IdockerfileProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "idockerfile",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IdockerfileConfig>,
        fields: &[
            crate::config::FieldSpec { name: "config_file", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "A hadolint configuration file (YAML). Empty: .hadolint.yaml or .hadolint.yml in the project root when present" },
            crate::config::FieldSpec { name: "ignored", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Rule codes to ignore, added to the configuration file's ignored list" },
            crate::config::FieldSpec { name: "trusted_registries", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Registries FROM images may come from (DL3026); * wildcards allowed" },
            crate::config::FieldSpec { name: "label_schema", ty: crate::config::FieldType::Table,
                affects_output: true, required: false,
                doc: "Labels that must be present, name = type (text, url, semver, hash, rfc3339, spdx, email)" },
            crate::config::FieldSpec { name: "strict_labels", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Forbid labels outside label_schema (DL3050)" },
            crate::config::FieldSpec { name: "disable_ignore_pragma", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Ignore # hadolint ignore=... pragmas in the files" },
            crate::config::FieldSpec { name: "failure_threshold", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Fail only on findings at least this severe: error, warning, info, style, none. Empty: the config file's, else info" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["Dockerfile"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { dep_auto: &[".hadolint.yaml", ".hadolint.yml"], ..crate::config::ProcessorDefaults::EMPTY }),
        keywords: &["docker", "dockerfile", "linter", "hadolint", "rust"],
        description: "Lint Dockerfiles with hadolint's rules (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
