use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{CargoConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, SiblingFilter, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct CargoProcessor {
    config: CargoConfig,
}

impl CargoProcessor {
    pub fn new(config: CargoConfig) -> Self {
        Self { config }
    }

    /// Run cargo build in the Cargo.toml's directory with the given profile
    fn execute_cargo(&self, cargo_toml: &Path, profile: &str) -> Result<()> {
        let mut cmd = Command::new(&self.config.cargo);
        cmd.arg(&self.config.command);
        cmd.args(["--profile", profile]);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, cargo_toml)?;
        check_command_output(&output, format_args!("cargo {} --profile {} in {}", self.config.command, profile, anchor_display_dir(cargo_toml)))
    }
}

impl ProductDiscovery for CargoProcessor {
    fn description(&self) -> &str {
        "Build Rust projects using Cargo"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::MassGenerator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.cargo.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        let Some(files) = crate::processors::scan_or_skip(&self.config.scan, file_index) else {
            return Ok(());
        };

        let siblings = SiblingFilter {
            extensions: &[".rs", ".toml"],
            excludes: &["/.git/", "/target/", "/.rsconstruct/"],
        };
        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;

        for anchor in files {
            let anchor_dir = anchor.parent().map(|p| p.to_path_buf()).unwrap_or_default();

            let sibling_files = file_index.query(
                &anchor_dir,
                siblings.extensions,
                siblings.excludes,
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
                        crate::processors::names::CARGO,
                        hash.clone(),
                        output_dir,
                        Some(profile),
                    )?;
                } else {
                    graph.add_product_with_variant(
                        inputs,
                        vec![],
                        crate::processors::names::CARGO,
                        hash.clone(),
                        Some(profile),
                    )?;
                }
            }
        }

        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        let profile = product.variant.as_deref().unwrap_or("dev");
        self.execute_cargo(product.primary_input(), profile)
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        crate::processors::clean_output_dir(product, crate::processors::names::CARGO, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}
