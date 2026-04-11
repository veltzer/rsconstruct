use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{SphinxConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{ProcessorBase, Processor, SiblingFilter, run_command, anchor_display_dir, check_command_output};

pub struct SphinxProcessor {
    base: ProcessorBase,
    config: SphinxConfig,
}

impl SphinxProcessor {
    pub fn new(config: SphinxConfig) -> Self {
        Self {
            base: ProcessorBase::creator(
                crate::processors::names::SPHINX,
                "Build Sphinx documentation",
            ),
            config,
        }
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

impl Processor for SphinxProcessor {
    fn scan_config(&self) -> &crate::config::ScanConfig {
        &self.config.scan
    }


    fn description(&self) -> &str {
        self.base.description()
    }

    fn processor_type(&self) -> crate::processors::ProcessorType {
        self.base.processor_type()
    }


    fn config_json(&self) -> Option<String> {
        crate::processors::ProcessorBase::config_json(&self.config)
    }

    fn max_jobs(&self) -> Option<usize> {
        self.config.max_jobs
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processors::ProcessorBase::clean_output_dir(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.sphinx_build.clone(), "python3".to_string()]
    }

    fn discover(&self, graph: &mut BuildGraph, file_index: &FileIndex, instance_name: &str) -> Result<()> {
        let Some(files) = crate::processors::scan_or_skip(&self.config.scan, file_index) else {
            return Ok(());
        };
        let hash = Some(output_config_hash(&self.config, &[]));
        let extra = resolve_extra_inputs(&self.config.dep_inputs)?;
        let siblings = SiblingFilter {
            extensions: &[".rst", ".py", ".md"],
            excludes: &["/.git/", "/out/", "/.rsconstruct/", "/_build/", "/docs/"],
        };
        for anchor in files {
            let anchor_dir = anchor.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let sibling_files = file_index.query(
                &anchor_dir, siblings.extensions, siblings.excludes, &[], &[], &[],
            );
            let inputs = crate::processors::build_anchor_inputs(&anchor, &sibling_files, &extra);
            if self.config.cache_output_dir {
                // output_dir is at project root, NOT joined with anchor_dir
                let output_dir = PathBuf::from(&self.config.output_dir);
                graph.add_product_with_output_dir(inputs, vec![], instance_name, hash.clone(), output_dir)?;
            } else {
                graph.add_product(inputs, vec![], instance_name, hash.clone())?;
            }
        }
        Ok(())
    }

    fn supports_batch(&self) -> bool { false }

    fn execute(&self, product: &Product) -> Result<()> {
        self.execute_sphinx(product.primary_input())
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registry::deserialize_and_create(toml, |cfg| Box::new(SphinxProcessor::new(cfg)))
}
inventory::submit! {
    crate::registry::ProcessorPlugin {
        name: "sphinx",
        processor_type: crate::processors::ProcessorType::Creator,
        create: plugin_create,
        defconfig_json: crate::registry::default_config_json::<crate::config::SphinxConfig>,
        known_fields: crate::registry::typed_known_fields::<crate::config::SphinxConfig>,
        output_fields: crate::registry::typed_output_fields::<crate::config::SphinxConfig>,
        must_fields: crate::registry::typed_must_fields::<crate::config::SphinxConfig>,
        field_descriptions: crate::registry::typed_field_descriptions::<crate::config::SphinxConfig>,
    }
}
