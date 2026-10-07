use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{
    Processor, SiblingFilter, anchor_display_dir, check_command_output, ensure_output_dir,
    run_in_anchor_dir,
};

fn default_gem_home() -> String {
    "gems".into()
}

/// Where the install stamps live: one file per `Gemfile`, written after a
/// successful `bundle install`. The stamp is the product's declared output,
/// so a processor that needs the gems installed first (mdl with
/// `local_repo`) lists it as an input and the graph orders the two. A bare
/// `gem_home` directory cannot carry that edge: dependencies connect
/// declared output *files* to inputs.
const STAMP_DIR: &str = "out/processor.creator.gem";

/// The stamp of a `Gemfile` at the project root — the default `gem_stamp`
/// of the mdl processor.
pub const ROOT_STAMP: &str = "out/processor.creator.gem/root.stamp";

/// The install stamp for the `Gemfile` in `anchor_dir`: `root.stamp` for the
/// project root, the directory path with `/` turned into `_` otherwise.
fn stamp_path(anchor_dir: &Path) -> PathBuf {
    if anchor_dir.as_os_str().is_empty() {
        return PathBuf::from(ROOT_STAMP);
    }
    let flat = anchor_dir.to_string_lossy().replace('/', "_");
    Path::new(STAMP_DIR).join(format!("{flat}.stamp"))
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GemConfig {
    #[serde(default = "default_gem_home")]
    pub gem_home: String,
    #[serde(default = "crate::config::default_true")]
    pub cache_output_dir: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for GemConfig {
    fn default() -> Self {
        Self {
            gem_home: "gems".into(),
            cache_output_dir: true,
            standard: StandardConfig::default(),
        }
    }
}

pub struct GemProcessor {
    config: GemConfig,
}

impl GemProcessor {
    pub const fn new(config: GemConfig) -> Self {
        Self { config }
    }

    /// Run bundle install in the Gemfile's directory
    fn execute_gem(&self, ctx: &crate::build_context::BuildContext, gemfile: &Path) -> Result<()> {
        let subcommand = "install";
        let mut cmd = Command::new(&self.config.standard.command);
        cmd.arg(subcommand);
        cmd.env("GEM_HOME", &self.config.gem_home);
        cmd.env("GEM_PATH", &self.config.gem_home);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, gemfile)?;
        check_command_output(
            &output,
            format_args!("bundle {} in {}", subcommand, anchor_display_dir(gemfile)),
        )
    }
}

impl Processor for GemProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    // Serialize the FULL config (the trait default covers StandardConfig
    // only), so the extra fields reach config-change detection.
    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        let stamps = crate::processor::ProcessorBase::clean(product, &product.processor, verbose)?;
        let dirs = crate::processor::ProcessorBase::clean_output_dir(
            product,
            &product.processor,
            verbose,
        )?;
        Ok(stamps + dirs)
    }

    fn required_tools(&self) -> Vec<String> {
        vec![self.config.standard.command.clone(), "ruby".to_string()]
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let Some(files) = crate::processor::scan_or_skip(&self.config.standard, file_index) else {
            return Ok(());
        };

        let hash = Some(output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        ));
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        let siblings = SiblingFilter {
            extensions: &[".gemspec"],
            excludes: &["/.git/", "/out/", "/.rsconstruct/", "/gems/"],
        };

        for anchor in files {
            let anchor_dir = anchor
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();

            let sibling_files = file_index.query(
                &anchor_dir,
                siblings.extensions,
                siblings.excludes,
                &[],
                &[],
                &[],
            );

            let inputs = crate::processor::build_anchor_inputs(&anchor, &sibling_files, &extra);
            let outputs = vec![stamp_path(&anchor_dir)];

            let output_dirs = if self.config.cache_output_dir {
                vec![anchor_dir.join(&self.config.gem_home)]
            } else {
                Vec::new()
            };
            graph.add_product_with(
                inputs,
                outputs,
                instance_name,
                hash.clone(),
                None,
                output_dirs,
            )?;
        }

        Ok(())
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.execute_gem(ctx, product.primary_input())?;
        let stamp = product.primary_output();
        ensure_output_dir(stamp)?;
        // Fixed content: the stamp marks "installed", and a byte-identical
        // stamp keeps the cache entry stable across rebuilds.
        std::fs::write(stamp, "installed\n")
            .with_context(|| format!("Failed to write gem install stamp {}", stamp.display()))
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(GemProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 2,
        name: "gem",
        processor_type: crate::processor::ProcessorType::Creator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "gem_home", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Directory where gems are installed" },
            crate::config::FieldSpec { name: "cache_output_dir", ty: crate::config::FieldType::Bool,
                affects_output: false, required: false,
                doc: "Cache the entire output directory as a unit" },
        ],
        omit_standard_fields: &["formats", "dep_auto", "output_dir"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["Gemfile"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "bundle", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<GemConfig>,
        keywords: &["ruby", "gem", "package-manager", "rb"],
        description: "Install Ruby dependencies using Bundler",
        is_native: false,
        is_rust: false,
        can_fix: false,
        supports_batch: false,
        max_jobs_cap: Some(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_paths_are_one_per_gemfile_directory() {
        assert_eq!(stamp_path(Path::new("")), PathBuf::from(ROOT_STAMP));
        assert_eq!(
            stamp_path(Path::new("tools/lint")),
            PathBuf::from("out/processor.creator.gem/tools_lint.stamp")
        );
    }
}
