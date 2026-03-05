use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{SphinxConfig, config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProductDiscovery, ProcessorType, SiblingFilter, scan_root_valid, run_command, anchor_display_dir, check_command_output};

pub struct SphinxProcessor {
    config: SphinxConfig,
}

impl SphinxProcessor {
    pub fn new(config: SphinxConfig) -> Self {
        Self { config }
    }

    /// Run sphinx-build from the project root.
    /// Source dir is the directory containing conf.py (e.g. "sphinx"),
    /// output dir is at project root level (e.g. "docs").
    fn execute_sphinx(&self, conf_py: &Path) -> Result<()> {
        let mut cmd = Command::new(&self.config.sphinx_build);
        let anchor_dir = conf_py.parent().unwrap_or(Path::new(""));
        // Source dir is the directory containing conf.py (e.g. "sphinx")
        if anchor_dir.as_os_str().is_empty() {
            cmd.arg(".");
        } else {
            cmd.arg(anchor_dir);
        }
        // Output dir at project root level (e.g. "docs")
        cmd.arg(&self.config.output_dir);
        for arg in &self.config.args {
            cmd.arg(arg);
        }
        if let Some(ref dir) = self.config.working_dir {
            cmd.current_dir(dir);
        }
        let output = run_command(&mut cmd)?;
        check_command_output(&output, format_args!("sphinx-build in {}", anchor_display_dir(conf_py)))
    }
}

impl ProductDiscovery for SphinxProcessor {
    fn description(&self) -> &str {
        "Build Sphinx documentation"
    }

    fn processor_type(&self) -> ProcessorType {
        ProcessorType::MassGenerator
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        scan_root_valid(&self.config.scan) && !file_index.scan(&self.config.scan, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.sphinx_build.clone(), "python3".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex) -> Result<()> {
        if !scan_root_valid(&self.config.scan) {
            return Ok(());
        }
        let files = file_index.scan(&self.config.scan, true);
        if files.is_empty() {
            return Ok(());
        }
        let hash = Some(config_hash(&self.config));
        let extra = resolve_extra_inputs(&self.config.extra_inputs)?;
        let siblings = SiblingFilter {
            extensions: &[".rst", ".py", ".md"],
            excludes: &["/.git/", "/out/", "/.rsbuild/", "/_build/", "/docs/"],
        };
        for anchor in files {
            let anchor_dir = anchor.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let sibling_files = file_index.query(
                &anchor_dir, siblings.extensions, siblings.excludes, &[], &[],
            );
            let mut inputs: Vec<PathBuf> = Vec::with_capacity(1 + sibling_files.len() + extra.len());
            inputs.push(anchor.clone());
            for file in &sibling_files {
                if *file != anchor {
                    inputs.push(file.clone());
                }
            }
            inputs.extend_from_slice(&extra);
            if self.config.cache_output_dir {
                // output_dir is at project root, NOT joined with anchor_dir
                let output_dir = PathBuf::from(&self.config.output_dir);
                graph.add_product_with_output_dir(inputs, vec![], crate::processors::names::SPHINX, hash.clone(), output_dir)?;
            } else {
                graph.add_product(inputs, vec![], crate::processors::names::SPHINX, hash.clone())?;
            }
        }
        Ok(())
    }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_sphinx(product.primary_input())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        if let Some(ref output_dir) = product.output_dir
            && output_dir.exists()
        {
            if verbose {
                println!("Removing sphinx output directory: {}", output_dir.display());
            }
            fs::remove_dir_all(output_dir.as_ref())?;
            return Ok(1);
        }
        Ok(0)
    }

    fn config_json(&self) -> Option<String> {
        serde_json::to_string(&self.config).ok()
    }
}
