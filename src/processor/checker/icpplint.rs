//! icpplint checker: cpplint's rules, in-process.
//!
//! The engine is `crate::engines::cpplint`, a port of cpplint 2.0.2. This file is the
//! processor around it: cpplint's command-line options as TOML fields, the
//! per-directory `CPPLINT.cfg` chain (or the file `config_file` names), and
//! findings printed in cpplint's own `file:line:  message  [category]
//! [confidence]` format. A file with any finding fails, as `cpplint` exits 1.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::engines::cpplint::{IncludeOrder, Options, Outcome};
use crate::graph::Product;

const fn default_verbose() -> u32 {
    1
}

const fn default_line_length() -> usize {
    80
}

fn default_include_order() -> String {
    "default".to_string()
}

fn default_config_file() -> String {
    "CPPLINT.cfg".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IcpplintConfig {
    /// Category filters, as `--filter`: `-whitespace`, `+whitespace/braces`,
    /// `-runtime/printf:foo.cc:14`; an entry may hold several, comma-separated.
    #[serde(default)]
    pub filters: Vec<String>,
    /// Report only findings with at least this confidence (0-5), as `--verbose`.
    #[serde(default = "default_verbose")]
    pub verbose: u32,
    /// The allowed line length, as `--linelength`.
    #[serde(default = "default_line_length")]
    pub line_length: usize,
    /// The directory header guards are derived from, as `--root`. Empty: unset.
    #[serde(default)]
    pub root: String,
    /// The repository top directory, as `--repository`. Empty: found by the
    /// nearest `.git`, `.hg` or `.svn`.
    #[serde(default)]
    pub repository: String,
    /// Extensions (without the dot) treated as headers, as `--headers`.
    #[serde(default)]
    pub headers: Vec<String>,
    /// Extensions (without the dot) cpplint accepts, as `--extensions`.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// `default` or `standardcfirst`, as `--includeorder`.
    #[serde(default = "default_include_order")]
    pub include_order: String,
    /// The name of the per-directory configuration file, as `--config`.
    #[serde(default = "default_config_file")]
    pub config_file: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for IcpplintConfig {
    fn default() -> Self {
        Self {
            filters: Vec::new(),
            verbose: default_verbose(),
            line_length: default_line_length(),
            root: String::new(),
            repository: String::new(),
            headers: Vec::new(),
            extensions: Vec::new(),
            include_order: default_include_order(),
            config_file: default_config_file(),
            standard: StandardConfig::default(),
        }
    }
}

pub struct IcpplintProcessor {
    config: IcpplintConfig,
    /// cpplint's options, validated once per build. The error is kept as
    /// text so every product reports the same configuration failure.
    options: OnceLock<Result<Options, String>>,
}

impl IcpplintProcessor {
    pub const fn new(config: IcpplintConfig) -> Self {
        Self {
            config,
            options: OnceLock::new(),
        }
    }

    fn build_options(&self) -> Result<Options> {
        let include_order = IncludeOrder::parse(&self.config.include_order)
            .map_err(|e| anyhow::anyhow!("icpplint include_order: {e}"))?;
        if self.config.config_file.is_empty() || self.config.config_file.contains('/') {
            bail!(
                "icpplint config_file: Config file name must not include directory components ({:?})",
                self.config.config_file
            );
        }
        let optional = |s: &str| {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        };
        Ok(Options {
            filters: self.config.filters.clone(),
            verbose: self.config.verbose,
            line_length: self.config.line_length,
            root: optional(&self.config.root),
            repository: optional(&self.config.repository),
            headers: self.config.headers.clone(),
            extensions: self.config.extensions.clone(),
            include_order,
            config_filename: self.config.config_file.clone(),
        })
    }

    fn options(&self) -> Result<&Options> {
        let built = self
            .options
            .get_or_init(|| self.build_options().map_err(|e| format!("{e:#}")));
        match built {
            Ok(options) => Ok(options),
            Err(e) => bail!("{e}"),
        }
    }

    fn execute_product(&self, product: &Product) -> Result<()> {
        self.check_file(product.primary_input())
    }

    fn check_file(&self, file: &Path) -> Result<()> {
        let options = self.options()?;
        let name = file.display().to_string();
        let outcome = crate::engines::cpplint::lint_file(&name, options)
            .with_context(|| format!("cpplint failed on {name}"))?;
        match outcome {
            Outcome::Excluded => Ok(()),
            Outcome::Linted(findings) if findings.is_empty() => Ok(()),
            Outcome::Linted(findings) => {
                let lines: Vec<String> = findings.iter().map(|f| f.render(&name)).collect();
                bail!("{}", lines.join("\n"))
            }
        }
    }
}

impl crate::processor::Processor for IcpplintProcessor {
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
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IcpplintProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "icpplint",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IcpplintConfig>,
        fields: &[
            crate::config::FieldSpec { name: "filters", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Category filters as --filter: -whitespace, +whitespace/braces, -runtime/printf:foo.cc:14 (an entry may hold several, comma-separated)" },
            crate::config::FieldSpec { name: "verbose", ty: crate::config::FieldType::Integer,
                affects_output: true, required: false,
                doc: "Report only findings with at least this confidence (0-5), as --verbose" },
            crate::config::FieldSpec { name: "line_length", ty: crate::config::FieldType::Integer,
                affects_output: true, required: false,
                doc: "The allowed line length, as --linelength" },
            crate::config::FieldSpec { name: "root", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The directory header guards are derived from, as --root. Empty: unset" },
            crate::config::FieldSpec { name: "repository", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The repository top directory, as --repository. Empty: the nearest .git, .hg or .svn" },
            crate::config::FieldSpec { name: "headers", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Extensions (without the dot) treated as headers, as --headers" },
            crate::config::FieldSpec { name: "extensions", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Extensions (without the dot) cpplint accepts, as --extensions" },
            crate::config::FieldSpec { name: "include_order", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "default or standardcfirst, as --includeorder" },
            crate::config::FieldSpec { name: "config_file", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The name of the per-directory configuration file, as --config (CPPLINT.cfg)" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".c", ".cc", ".cpp", ".cxx", ".c++", ".cu", ".h", ".hh", ".hpp", ".hxx", ".h++", ".cuh"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { dep_auto: &["CPPLINT.cfg"], ..crate::config::ProcessorDefaults::EMPTY }),
        keywords: &["c", "cpp", "linter", "google", "cpplint", "cc", "h", "hpp", "rust"],
        description: "Lint C/C++ files with cpplint's rules (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: true,
        max_jobs_cap: None,
    }
}
