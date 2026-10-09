//! iprotobuf generator: Protocol Buffer definitions compiled in-process.
//!
//! The Rust alternative to the `protobuf` generator (which runs `protoc`):
//! the `.proto` files under `src_dirs` are compiled with protox, a Rust
//! implementation of the protobuf compiler, and prost-build turns the
//! result into Rust code, one `<package>.rs` per protobuf package in the
//! output directory. Optionally the compiled descriptor set is written as
//! well, the file `protoc --descriptor_set_out` produces.
//!
//! All the files of a stanza are compiled together as one product, since
//! a package's generated file gathers every `.proto` that declares it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::Processor;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct IprotobufConfig {
    /// Directories `import` statements are resolved against. Empty: the
    /// `src_dirs`.
    #[serde(default)]
    pub include_paths: Vec<String>,
    /// Where to write the compiled `FileDescriptorSet` (what
    /// `protoc --include_imports --descriptor_set_out` writes). Empty: not
    /// written.
    #[serde(default)]
    pub descriptor_set: String,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

pub struct IprotobufProcessor {
    config: IprotobufConfig,
}

impl IprotobufProcessor {
    pub const fn new(config: IprotobufConfig) -> Self {
        Self { config }
    }

    fn output_dir(&self) -> &str {
        &self.config.standard.output_dir
    }

    fn include_paths(&self) -> Vec<PathBuf> {
        let configured: Vec<PathBuf> = if self.config.include_paths.is_empty() {
            self.config
                .standard
                .src_dirs()
                .iter()
                .filter(|d| !d.is_empty())
                .map(PathBuf::from)
                .collect()
        } else {
            self.config
                .include_paths
                .iter()
                .map(PathBuf::from)
                .collect()
        };
        if configured.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            configured
        }
    }

    /// The Rust file prost-build writes for the package a `.proto` declares:
    /// the package's dotted path with each component in snake case, or `_`
    /// for a file with no `package` statement.
    fn generated_file_name(proto: &Path) -> Result<String> {
        let text = std::fs::read_to_string(proto)
            .with_context(|| format!("Failed to read {}", proto.display()))?;
        let re = Regex::new(r"(?m)^\s*package\s+([A-Za-z_][A-Za-z0-9_.]*)\s*;")
            .context("package regex")?;
        let name = re.captures(&text).map_or_else(
            || "_".to_string(),
            |c| {
                c[1].split('.')
                    .filter(|s| !s.is_empty())
                    .map(to_snake)
                    .collect::<Vec<_>>()
                    .join(".")
            },
        );
        Ok(format!("{name}.rs"))
    }

    fn compile(&self, files: &[PathBuf]) -> Result<()> {
        let out_dir = Path::new(self.output_dir());
        std::fs::create_dir_all(out_dir)
            .with_context(|| format!("Failed to create output directory {}", out_dir.display()))?;
        let mut compiler =
            protox::Compiler::new(self.include_paths()).map_err(|e| anyhow!("iprotobuf: {e}"))?;
        compiler.include_imports(true).include_source_info(true);
        compiler
            .open_files(files)
            .map_err(|e| anyhow!("iprotobuf: {e}"))?;
        let fds = compiler.file_descriptor_set();
        if !self.config.descriptor_set.is_empty() {
            // What `protoc --include_imports --descriptor_set_out` writes:
            // the source locations stay out unless asked for, and they are
            // only needed here for the comments in the generated code.
            let mut plain = fds.clone();
            for file in &mut plain.file {
                file.source_code_info = None;
            }
            let path = Path::new(&self.config.descriptor_set);
            crate::processor::ensure_output_dir(path)?;
            std::fs::write(path, prost::Message::encode_to_vec(&plain))
                .with_context(|| format!("Failed to write descriptor set {}", path.display()))?;
        }
        prost_build::Config::new()
            .out_dir(out_dir)
            .compile_fds(fds)
            .with_context(|| format!("Failed to generate Rust code into {}", out_dir.display()))
    }
}

/// heck's snake case, as prost-build applies it to package components.
fn to_snake(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            let prev_lower =
                i > 0 && (chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit());
            let next_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            let prev_upper = i > 0 && chars[i - 1].is_uppercase();
            if !out.is_empty() && !out.ends_with('_') && (prev_lower || (prev_upper && next_lower))
            {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

impl Processor for IprotobufProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    /// One product over all the stanza's files.
    fn discovery(&self) -> crate::processor::Discovery {
        crate::processor::Discovery::WholeIndex
    }

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        crate::processor::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.standard, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        Vec::new()
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let files = file_index.scan(&self.config.standard, true);
        if files.is_empty() {
            return Ok(());
        }
        let out_dir = Path::new(self.output_dir());
        let mut outputs: BTreeSet<PathBuf> = BTreeSet::new();
        for f in &files {
            outputs.insert(out_dir.join(Self::generated_file_name(f)?));
        }
        if !self.config.descriptor_set.is_empty() {
            outputs.insert(PathBuf::from(&self.config.descriptor_set));
        }
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;
        let mut inputs = files;
        inputs.extend_from_slice(&extra);
        graph.add_product(
            inputs,
            outputs.into_iter().collect(),
            instance_name,
            Some(output_config_hash(
                &self.config,
                &crate::config::checksum_fields_of(instance_name),
            )),
        )?;
        Ok(())
    }

    fn execute(&self, _ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let files: Vec<PathBuf> = product
            .inputs
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "proto"))
            .cloned()
            .collect();
        self.compile(&files)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(IprotobufProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "iprotobuf",
        processor_type: crate::processor::ProcessorType::Generator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "include_paths", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Directories import statements are resolved against. Empty: the src_dirs" },
            crate::config::FieldSpec { name: "descriptor_set", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Where to write the compiled FileDescriptorSet (protoc --descriptor_set_out). Empty: not written" },
        ],
        omit_standard_fields: &["command"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[".proto"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { output_dir: "out/processor.generator.iprotobuf", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<IprotobufConfig>,
        keywords: &["protobuf", "proto", "generator", "grpc", "serialization", "rust"],
        description: "Compile Protocol Buffer definitions to Rust (in-process)",
        is_native: true,
        is_rust: true,
        can_fix: false,
        supports_batch: false,
        max_jobs_cap: None,
    }
}
