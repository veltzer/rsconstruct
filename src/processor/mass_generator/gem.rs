use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::{PlannedOutput, add_planned_products};
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
/// `local_repo`) lists it as an input and the graph orders the two.
const STAMP_DIR: &str = "out/processor.mass_generator.gem";

/// The stamp of a `Gemfile` at the project root — the default `gem_stamp`
/// of the mdl processor.
pub const ROOT_STAMP: &str = "out/processor.mass_generator.gem/root.stamp";

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
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for GemConfig {
    fn default() -> Self {
        Self {
            gem_home: "gems".into(),
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

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processor::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn required_tools(&self) -> Vec<String> {
        crate::processor::with_declared_tools(
            vec![self.config.standard.command.clone(), "ruby".to_string()],
            &self.config.standard,
        )
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

        let config_hash = output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        );
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;

        // What `bundle install` resolves from besides the Gemfile: the
        // gemspecs it references and the lockfile it pins to. The lockfile
        // is an input even though bundler may rewrite it: a rewrite with the
        // same content keeps the checksum, and one with new content means
        // the install changed and the next build re-runs it once.
        let siblings = SiblingFilter {
            extensions: &[".gemspec", "Gemfile.lock"],
            excludes: &["/.git/", "/out/", "/.rsconstruct/", "/gems/"],
        };

        let mut planned = Vec::with_capacity(files.len());
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

            let inputs = crate::processor::build_anchor_inputs(&anchor, &sibling_files, &[]);
            planned.push(PlannedOutput {
                path: stamp_path(&anchor_dir),
                sources: inputs,
            });
        }

        add_planned_products(graph, &planned, instance_name, &config_hash, &extra)
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
        version: 3,
        name: "gem",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "gem_home", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Directory where gems are installed" },
        ],
        omit_standard_fields: &["formats", "dep_auto", "output_dir", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["Gemfile"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "bundle", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<GemConfig>,
        keywords: &["ruby", "gem", "package-manager", "rb"],
        description: "Install Ruby dependencies using Bundler",
        is_builtin: false,
        is_rust: false,
        supports_batch: false,
        caches_outputs: false,
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
            PathBuf::from("out/processor.mass_generator.gem/tools_lint.stamp")
        );
    }

    /// `Gemfile.lock` pins what `bundle install` installs, so it is an input
    /// of the stamp next to the gemspecs; the installed gems and unrelated
    /// Ruby sources are not.
    #[test]
    fn gemfile_lock_is_an_input_of_the_install_stamp() {
        let files: Vec<PathBuf> = [
            "Gemfile",
            "Gemfile.lock",
            "mylib.gemspec",
            "lib/mylib.rb",
            "gems/gems/rake-13.0.0/rake.gemspec",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        // Through the registry, so the scan fields carry their defaults;
        // the scan root is configured, as every project does.
        let processor = crate::builder::create_processor_for_instance(
            "processor.mass_generator.gem",
            &toml::from_str("src_dirs = [\".\"]").unwrap(),
        )
        .unwrap()
        .expect("gem is registered");
        let mut graph = BuildGraph::new();
        processor
            .discover(
                &mut graph,
                &FileIndex::from_paths(files),
                "processor.mass_generator.gem",
            )
            .unwrap();
        assert_eq!(graph.products().len(), 1);
        let product = &graph.products()[0];
        assert_eq!(product.outputs, vec![PathBuf::from(ROOT_STAMP)]);
        assert_eq!(
            product.inputs,
            ["Gemfile", "Gemfile.lock", "mylib.gemspec"]
                .iter()
                .map(PathBuf::from)
                .collect::<Vec<_>>()
        );
    }
}
