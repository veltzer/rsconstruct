//! ishellcheck checker: `ShellCheck`'s analysis, in-process.
//!
//! The engine is `crate::engines::shellcheck`, a port of `ShellCheck` 0.11.0.
//! This file is the processor around it: shellcheck's command-line options
//! as TOML fields, and findings printed as `shellcheck --format=gcc` prints
//! them. Any finding fails the product, as shellcheck exits non-zero on any
//! finding. Each file is checked on its own, as `shellcheck FILE` checks
//! it: `source`d files are followed only with `external_sources`.

use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::StandardConfig;
use crate::engines::shellcheck::{
    self, CheckSpec, IoOptions, data::shell_for_executable, interface::Severity,
};
use crate::graph::Product;

fn default_severity() -> String {
    "style".to_string()
}

/// The parser recurses once per nesting level of the script; a generous
/// stack keeps deeply nested scripts from overflowing it.
const CHECK_STACK_SIZE: usize = 256 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IshellcheckConfig {
    /// The shell dialect, as `--shell` (`sh`, `bash`, `dash`, `ksh`,
    /// `busybox`). Empty: from the shebang, directives or file extension.
    #[serde(default)]
    pub shell: String,
    /// Follow `source`d files that are not inputs, as `--external-sources`.
    #[serde(default)]
    pub external_sources: bool,
    /// Where to look for `source`d files, as `--source-path` (`SCRIPTDIR`
    /// is the script's directory).
    #[serde(default)]
    pub source_path: Vec<String>,
    /// The minimum severity reported, as `--severity`: `error`, `warning`,
    /// `info`, `style`.
    #[serde(default = "default_severity")]
    pub severity: String,
    /// Codes to exclude, as `--exclude` (`SC2034` or `2034`).
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Codes to report exclusively, as `--include`. Empty: all codes.
    #[serde(default)]
    pub include: Vec<String>,
    /// Optional checks to enable, as `--enable` (`all` for every one).
    #[serde(default)]
    pub enable: Vec<String>,
    /// A `.shellcheckrc` to use instead of searching for one, as `--rcfile`.
    #[serde(default)]
    pub rcfile: String,
    /// Ignore `.shellcheckrc` files, as `--norc`.
    #[serde(default)]
    pub norc: bool,
    /// Report findings in `source`d files too, as `--check-sourced`.
    #[serde(default)]
    pub check_sourced: bool,
    /// Extended dataflow analysis, as `--extended-analysis`: `true`,
    /// `false`, or empty for the default (directives decide).
    #[serde(default)]
    pub extended_analysis: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for IshellcheckConfig {
    fn default() -> Self {
        Self {
            shell: String::new(),
            external_sources: false,
            source_path: Vec::new(),
            severity: default_severity(),
            exclude: Vec::new(),
            include: Vec::new(),
            enable: Vec::new(),
            rcfile: String::new(),
            norc: false,
            check_sourced: false,
            extended_analysis: String::new(),
            standard: StandardConfig::default(),
        }
    }
}

pub struct IshellcheckProcessor {
    config: IshellcheckConfig,
}

/// A code as `--exclude`/`--include` take it: `SC2034` or `2034`.
fn parse_code(s: &str) -> Result<i64> {
    let digits = s
        .strip_prefix("SC")
        .or_else(|| s.strip_prefix("sc"))
        .unwrap_or(s);
    digits.parse::<i64>().map_err(|_| {
        anyhow::anyhow!("ishellcheck: invalid code {s:?} (expected e.g. SC2034 or 2034)")
    })
}

impl IshellcheckProcessor {
    pub const fn new(config: IshellcheckConfig) -> Self {
        Self { config }
    }

    fn template(&self) -> Result<(CheckSpec, IoOptions)> {
        let c = &self.config;
        let mut spec = CheckSpec::new("", String::new());
        if !c.shell.is_empty() {
            match shell_for_executable(&c.shell) {
                Some(sh) => spec.shell_type_override = Some(sh),
                None => bail!("ishellcheck: unknown shell {:?}", c.shell),
            }
        }
        spec.min_severity = match c.severity.as_str() {
            "error" => Severity::ErrorC,
            "warning" => Severity::WarningC,
            "info" => Severity::InfoC,
            "style" => Severity::StyleC,
            other => bail!("ishellcheck: unknown severity {other:?} (error, warning, info, style)"),
        };
        spec.excluded_warnings = c
            .exclude
            .iter()
            .map(|s| parse_code(s))
            .collect::<Result<_>>()?;
        if !c.include.is_empty() {
            spec.included_warnings = Some(
                c.include
                    .iter()
                    .map(|s| parse_code(s))
                    .collect::<Result<_>>()?,
            );
        }
        spec.optional_checks.clone_from(&c.enable);
        spec.ignore_rc = c.norc;
        spec.check_sourced = c.check_sourced;
        spec.extended_analysis = match c.extended_analysis.as_str() {
            "" => None,
            "true" => Some(true),
            "false" => Some(false),
            other => {
                bail!("ishellcheck: extended_analysis must be true, false or empty, not {other:?}")
            }
        };
        let io = IoOptions {
            external_sources: c.external_sources,
            source_paths: c.source_path.clone(),
            rcfile: if c.rcfile.is_empty() {
                None
            } else {
                Some(c.rcfile.clone())
            },
        };
        Ok((spec, io))
    }

    fn check_file(&self, file: &Path) -> Result<()> {
        let (spec, io) = self.template()?;
        let name = file.to_string_lossy().into_owned();
        let handle = std::thread::Builder::new()
            .stack_size(CHECK_STACK_SIZE)
            .spawn(move || shellcheck::check_files(&[name], io, &spec))
            .map_err(|e| anyhow::anyhow!("ishellcheck: failed to start the check thread: {e}"))?;
        let Ok(lines) = handle.join() else {
            bail!(
                "ishellcheck: internal error while checking {}",
                file.display()
            );
        };
        if lines.is_empty() {
            return Ok(());
        }
        bail!("{}", lines.join("\n"))
    }
}

impl crate::processor::Processor for IshellcheckProcessor {
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
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IshellcheckProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "ishellcheck",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IshellcheckConfig>,
        fields: &[
            crate::config::FieldSpec { name: "shell", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The shell dialect, as --shell (sh, bash, dash, ksh, busybox). Empty: from the shebang, directives or extension" },
            crate::config::FieldSpec { name: "external_sources", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Follow sourced files that are not inputs, as --external-sources" },
            crate::config::FieldSpec { name: "source_path", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Where to look for sourced files, as --source-path (SCRIPTDIR is the script's directory)" },
            crate::config::FieldSpec { name: "severity", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The minimum severity reported, as --severity: error, warning, info, style" },
            crate::config::FieldSpec { name: "exclude", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Codes to exclude, as --exclude (SC2034 or 2034)" },
            crate::config::FieldSpec { name: "include", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Codes to report exclusively, as --include. Empty: all codes" },
            crate::config::FieldSpec { name: "enable", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Optional checks to enable, as --enable (all for every one)" },
            crate::config::FieldSpec { name: "rcfile", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "A .shellcheckrc to use instead of searching for one, as --rcfile" },
            crate::config::FieldSpec { name: "norc", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Ignore .shellcheckrc files, as --norc" },
            crate::config::FieldSpec { name: "check_sourced", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Report findings in sourced files too, as --check-sourced" },
            crate::config::FieldSpec { name: "extended_analysis", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Extended dataflow analysis, as --extended-analysis: true, false, or empty for the default" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".sh", ".bash"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { dep_auto: &[".shellcheckrc"], ..crate::config::ProcessorDefaults::EMPTY }),
        keywords: &["shell", "bash", "sh", "linter", "script", "shellcheck", "rust"],
        description: "Lint shell scripts with ShellCheck's analysis (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        // In-process, a batch has no startup cost to share and would only
        // check its files one after another: products run in parallel instead.
        supports_batch: false,
        max_jobs_cap: None,
    }
}
