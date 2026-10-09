//! iluacheck checker: luacheck's analysis, in-process.
//!
//! The engine is `crate::luacheck`, a port of luacheck 1.2.0. This file is
//! the processor around it: luacheck's command-line options as TOML fields,
//! the `.luacheckrc` configuration, and findings printed as `luacheck
//! --formatter plain --codes` prints them. Any finding fails the build, as
//! `luacheck` exits non-zero on any warning.
//!
//! luacheck resolves implicitly defined globals (`allow_defined`,
//! `allow_defined_top`) across all files checked in one run, so a file's
//! findings depend on the other files. The processor therefore makes one
//! product of all its files, checked together on every change.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::luacheck::{CliOptions, FileResult, Limit};

fn default_config_file() -> String {
    ".luacheckrc".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct IluacheckConfig {
    /// The configuration file, looked up from the project root upwards, as
    /// `--config`. Empty: no configuration file, as `--no-config`.
    #[serde(default = "default_config_file")]
    pub config_file: String,
    /// Standard globals, as `--std` (`max`, `lua51`, `lua54+busted`, ...).
    /// Empty: unset.
    #[serde(default)]
    pub std: String,
    #[serde(default)]
    pub globals: Option<Vec<String>>,
    #[serde(default)]
    pub read_globals: Option<Vec<String>>,
    #[serde(default)]
    pub new_globals: Option<Vec<String>>,
    #[serde(default)]
    pub new_read_globals: Option<Vec<String>>,
    #[serde(default)]
    pub not_globals: Option<Vec<String>>,
    #[serde(default)]
    pub ignore: Option<Vec<String>>,
    #[serde(default)]
    pub enable: Option<Vec<String>>,
    #[serde(default)]
    pub only: Option<Vec<String>>,
    #[serde(default)]
    pub operators: Option<Vec<String>>,
    #[serde(default)]
    pub allow_defined: bool,
    #[serde(default)]
    pub allow_defined_top: bool,
    #[serde(default)]
    pub module: bool,
    #[serde(default)]
    pub no_global: bool,
    #[serde(default)]
    pub no_unused: bool,
    #[serde(default)]
    pub no_redefined: bool,
    #[serde(default)]
    pub no_unused_args: bool,
    #[serde(default)]
    pub no_unused_secondaries: bool,
    #[serde(default)]
    pub no_self: bool,
    #[serde(default)]
    pub max_line_length: Option<u64>,
    #[serde(default)]
    pub max_code_line_length: Option<u64>,
    #[serde(default)]
    pub max_string_line_length: Option<u64>,
    #[serde(default)]
    pub max_comment_line_length: Option<u64>,
    #[serde(default)]
    pub max_cyclomatic_complexity: Option<u64>,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for IluacheckConfig {
    fn default() -> Self {
        Self {
            config_file: default_config_file(),
            std: String::new(),
            globals: None,
            read_globals: None,
            new_globals: None,
            new_read_globals: None,
            not_globals: None,
            ignore: None,
            enable: None,
            only: None,
            operators: None,
            allow_defined: false,
            allow_defined_top: false,
            module: false,
            no_global: false,
            no_unused: false,
            no_redefined: false,
            no_unused_args: false,
            no_unused_secondaries: false,
            no_self: false,
            max_line_length: None,
            max_code_line_length: None,
            max_string_line_length: None,
            max_comment_line_length: None,
            max_cyclomatic_complexity: None,
            standard: StandardConfig::default(),
        }
    }
}

/// A limit field: a number, with 0 meaning no limit (`--no-max-...`).
const fn limit(n: u64) -> Limit {
    if n == 0 {
        Limit::Unlimited
    } else {
        Limit::Value(n as f64)
    }
}

impl IluacheckConfig {
    fn cli_options(&self) -> CliOptions {
        let mut disabled = Vec::new();
        for (set, name) in [
            (self.no_global, "global"),
            (self.no_unused, "unused"),
            (self.no_redefined, "redefined"),
            (self.no_unused_args, "unused_args"),
            (self.no_unused_secondaries, "unused_secondaries"),
            (self.no_self, "self"),
        ] {
            if set {
                disabled.push(name);
            }
        }
        CliOptions {
            config: if self.config_file.is_empty() {
                None
            } else {
                Some(self.config_file.clone())
            },
            std: if self.std.is_empty() {
                None
            } else {
                Some(self.std.clone())
            },
            globals: self.globals.clone(),
            read_globals: self.read_globals.clone(),
            new_globals: self.new_globals.clone(),
            new_read_globals: self.new_read_globals.clone(),
            not_globals: self.not_globals.clone(),
            ignore: self.ignore.clone(),
            enable: self.enable.clone(),
            only: self.only.clone(),
            operators: self.operators.clone(),
            allow_defined: self.allow_defined,
            allow_defined_top: self.allow_defined_top,
            module: self.module,
            disabled,
            max_line_length: self.max_line_length.map(limit),
            max_code_line_length: self.max_code_line_length.map(limit),
            max_string_line_length: self.max_string_line_length.map(limit),
            max_comment_line_length: self.max_comment_line_length.map(limit),
            max_cyclomatic_complexity: self.max_cyclomatic_complexity.map(limit),
        }
    }

    fn dep_inputs(&self) -> Result<Vec<PathBuf>> {
        let scan = &self.standard;
        let mut all_dep_inputs = scan.dep_inputs.clone();
        for ai in &scan.dep_auto {
            all_dep_inputs.extend(crate::processor::config_file_inputs(ai));
        }
        resolve_extra_inputs(&all_dep_inputs)
    }
}

pub struct IluacheckProcessor {
    config: IluacheckConfig,
}

impl IluacheckProcessor {
    pub const fn new(config: IluacheckConfig) -> Self {
        Self { config }
    }

    fn check_files(&self, files: &[&Path]) -> Result<()> {
        let names: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
        let results = crate::luacheck::run(&names, &self.config.cli_options())?;
        let mut lines = Vec::new();
        for (_, result) in results {
            match result {
                FileResult::Checked(findings) => lines.extend(findings),
                FileResult::Fatal(message) => lines.push(message),
            }
        }
        if lines.is_empty() {
            Ok(())
        } else {
            bail!("{}", lines.join("\n"))
        }
    }
}

impl crate::processor::Processor for IluacheckProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    // Serialize the FULL config (the trait default covers StandardConfig
    // only), so the extra fields reach config-change detection.
    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    /// One product spanning every scanned file: luacheck's implicit globals
    /// are resolved across all files checked together.
    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let mut inputs = file_index.scan(&self.config.standard, true);
        if inputs.is_empty() {
            return Ok(());
        }
        inputs.extend(self.config.dep_inputs()?);
        let hash = Some(output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        ));
        graph.add_product(inputs, vec![], instance_name, hash)?;
        Ok(())
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let deps = self.config.dep_inputs()?;
        let files: Vec<&Path> = product
            .inputs
            .iter()
            .filter(|p| !deps.contains(p))
            .map(PathBuf::as_path)
            .collect();
        self.check_files(&files)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IluacheckProcessor::new(cfg)))
}

inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "iluacheck",
        processor_type: crate::processor::ProcessorType::Checker,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<IluacheckConfig>,
        fields: &[
            crate::config::FieldSpec { name: "config_file", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The configuration file, looked up from the project root upwards, as --config (.luacheckrc); empty for --no-config" },
            crate::config::FieldSpec { name: "std", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Standard globals, as --std (max, lua51, lua54+busted, ...); empty: unset" },
            crate::config::FieldSpec { name: "globals", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Custom globals or fields (foo, foo.bar), as --globals" },
            crate::config::FieldSpec { name: "read_globals", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Read-only globals or fields, as --read-globals" },
            crate::config::FieldSpec { name: "new_globals", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Custom globals replacing those set before, as --new-globals" },
            crate::config::FieldSpec { name: "new_read_globals", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Read-only globals replacing those set before, as --new-read-globals" },
            crate::config::FieldSpec { name: "not_globals", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Globals or fields to remove, as --not-globals" },
            crate::config::FieldSpec { name: "ignore", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Warning patterns to filter out (code, name, or code/name), as --ignore" },
            crate::config::FieldSpec { name: "enable", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Warning patterns not to filter out, as --enable" },
            crate::config::FieldSpec { name: "only", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Report only warnings matching these patterns, as --only" },
            crate::config::FieldSpec { name: "operators", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Compound operators to allow (+=, ...), as --operators" },
            crate::config::FieldSpec { name: "allow_defined", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Allow defining globals implicitly by setting them, as --allow-defined" },
            crate::config::FieldSpec { name: "allow_defined_top", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Allow defining globals implicitly by setting them in the main chunk, as --allow-defined-top" },
            crate::config::FieldSpec { name: "module", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Limit implicitly defined globals to their files, as --module" },
            crate::config::FieldSpec { name: "no_global", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Filter out warnings about globals, as --no-global" },
            crate::config::FieldSpec { name: "no_unused", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Filter out warnings about unused variables and values, as --no-unused" },
            crate::config::FieldSpec { name: "no_redefined", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Filter out warnings about redefined variables, as --no-redefined" },
            crate::config::FieldSpec { name: "no_unused_args", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Filter out warnings about unused arguments and loop variables, as --no-unused-args" },
            crate::config::FieldSpec { name: "no_unused_secondaries", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Filter out warnings about unused variables set together with used ones, as --no-unused-secondaries" },
            crate::config::FieldSpec { name: "no_self", ty: crate::config::FieldType::Bool,
                affects_output: true, required: false,
                doc: "Filter out warnings about the implicit self argument, as --no-self" },
            crate::config::FieldSpec { name: "max_line_length", ty: crate::config::FieldType::Integer,
                affects_output: true, required: false,
                doc: "Maximum line length, as --max-line-length; 0 for no limit (default 120, or the configuration's)" },
            crate::config::FieldSpec { name: "max_code_line_length", ty: crate::config::FieldType::Integer,
                affects_output: true, required: false,
                doc: "Maximum length of lines ending in code, as --max-code-line-length; 0 for no limit" },
            crate::config::FieldSpec { name: "max_string_line_length", ty: crate::config::FieldType::Integer,
                affects_output: true, required: false,
                doc: "Maximum length of lines inside a string, as --max-string-line-length; 0 for no limit" },
            crate::config::FieldSpec { name: "max_comment_line_length", ty: crate::config::FieldType::Integer,
                affects_output: true, required: false,
                doc: "Maximum length of comment lines, as --max-comment-line-length; 0 for no limit" },
            crate::config::FieldSpec { name: "max_cyclomatic_complexity", ty: crate::config::FieldType::Integer,
                affects_output: true, required: false,
                doc: "Maximum cyclomatic complexity of functions, as --max-cyclomatic-complexity; 0 for no limit (the default)" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".lua"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { dep_auto: &[".luacheckrc"], output_depends_on_input_name: true, ..crate::config::ProcessorDefaults::EMPTY }),
        keywords: &["lua", "linter", "checker", "luacheck", "rust"],
        description: "Lint Lua files with luacheck's analysis (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: false,
        max_jobs_cap: None,
    }
}
