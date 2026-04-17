use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{CargoConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, SiblingFilter, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct CargoProcessor {
    config: CargoConfig,
}

impl CargoProcessor {
    pub fn new(config: CargoConfig) -> Self {
        Self {
            config,
        }
    }

    /// Run cargo build in the Cargo.toml's directory with the given profile
    fn execute_cargo(&self, ctx: &crate::build_context::BuildContext, cargo_toml: &Path, profile: &str) -> Result<()> {
        let subcommand = self.config.standard.require_command(crate::processors::names::CARGO)?;
        let mut cmd = Command::new(&self.config.cargo);
        cmd.arg(subcommand);
        cmd.args(["--profile", profile]);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, cargo_toml)?;
        check_command_output(&output, format_args!("cargo {} --profile {} in {}", subcommand, profile, anchor_display_dir(cargo_toml)))
    }
}

impl Processor for CargoProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }


    fn config_json(&self) -> Option<String> {
        crate::processors::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::ProcessorBase::clean_output_dir(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.cargo.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let Some(files) = crate::processors::scan_or_skip(&self.config.standard, file_index) else {
            return Ok(());
        };

        let siblings = SiblingFilter {
            extensions: &[".rs", ".toml"],
            excludes: &["/.git/", "/target/", "/.rsconstruct/"],
        };
        let hash = Some(output_config_hash(&self.config, <crate::config::CargoConfig as crate::config::KnownFields>::checksum_fields()));
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        for anchor in files {
            let anchor_dir = anchor.parent().map(|p| p.to_path_buf()).unwrap_or_default();

            let sibling_files = file_index.query(
                &anchor_dir,
                siblings.extensions,
                siblings.excludes,
                &[],
                &[],
                &[],
            );

            let base_inputs = crate::processors::build_anchor_inputs(&anchor, &sibling_files, &extra);

            for profile in &self.config.profiles {
                let inputs = base_inputs.clone();
                if self.config.cache_output_dir {
                    let output_dir = if anchor_dir.as_os_str().is_empty() {
                        PathBuf::from("target")
                    } else {
                        anchor_dir.join("target")
                    };
                    graph.add_product_with_output_dir_and_variant(
                        inputs,
                        vec![],
                        instance_name,
                        hash.clone(),
                        output_dir,
                        Some(profile),
                    )?;
                } else {
                    graph.add_product_with_variant(
                        inputs,
                        vec![],
                        instance_name,
                        hash.clone(),
                        Some(profile),
                    )?;
                }
            }
        }

        Ok(())
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let profile = product.variant.as_deref().unwrap_or("dev");
        self.execute_cargo(ctx, product.primary_input(), profile)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(CargoProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "cargo",
        processor_type: crate::processors::ProcessorType::Creator,
        create: plugin_create,
        defconfig_json: crate::registries::default_config_json::<crate::config::CargoConfig>,
        known_fields: crate::registries::typed_known_fields::<crate::config::CargoConfig>,
        checksum_fields: crate::registries::typed_checksum_fields::<crate::config::CargoConfig>,
        must_fields: crate::registries::typed_must_fields::<crate::config::CargoConfig>,
        field_descriptions: crate::registries::typed_field_descriptions::<crate::config::CargoConfig>,
        keywords: &["rust", "builder", "cargo", "rs", "package-manager"],
        description: "Build Rust projects using Cargo",
        is_native: false,
        can_fix: false,
        supports_batch: false,
        max_jobs_cap: Some(1),
    }
}
