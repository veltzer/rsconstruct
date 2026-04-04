use anyhow::Result;
use std::path::Path;
use std::process::Command;

use crate::config::MdbookConfig;
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, SiblingFilter, DirectoryProductOpts, discover_directory_products, scan_root_valid, run_in_anchor_dir, anchor_display_dir, check_command_output};

pub struct MdbookProcessor {
    config: MdbookConfig,
}

impl MdbookProcessor {
    pub fn new(config: MdbookConfig) -> Self {
        Self { config }
    }

    /// Run mdbook build in the book.toml's directory
    fn execute_mdbook(&self, book_toml: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.mdbook);
        cmd.arg("build");
        cmd.arg(".");
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(&mut cmd, book_toml)?;
        check_command_output(&output, format_args!("mdbook build in {}", anchor_display_dir(book_toml)))
    }
}

impl ProductDiscovery for MdbookProcessor {
    fn description(&self) -> &str {
        "Build mdbook documentation"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::MassGenerator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.mdbook.clone()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !scan_root_valid(&self.config.scan) {
            return Ok(());
        }

        discover_directory_products(graph, DirectoryProductOpts {
            scan: &self.config.scan,
            file_index,
            extra_inputs: &self.config.extra_inputs,
            cfg_hash: &self.config,
            siblings: &SiblingFilter {
                extensions: &[".md", ".toml"],
                excludes: &["/.git/", "/out/", "/.rsconstruct/", "/book/"],
            },
            processor_name: crate::processors::names::MDBOOK,
            output_dir_name: if self.config.cache_output_dir {
                Some(self.config.output_dir.as_str())
            } else {
                None
            },
        })
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_mdbook(product.primary_input())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        crate::processors::clean_output_dir(product, crate::processors::names::MDBOOK, verbose)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }
}
