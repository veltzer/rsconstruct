use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{
    OnceTool, PlannedOutput, add_planned_products, fresh_state_dir, install_staged,
    plan_cache_path, read_plan_cache, remove_dir_if_present, require_planned_output,
    write_plan_cache,
};
use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{
    Processor, SiblingFilter, anchor_display_dir, check_command_output, log_command,
    resolve_anchor_path, run_in_anchor_dir,
};

const PLAN_VERSION: u32 = 1;

/// Embedded Ruby planner that loads Jekyll (`Jekyll::Site`), runs `reset`,
/// `read` and `generate` without rendering or writing, and prints the
/// destination files relative to the output directory.
const PLAN_SCRIPT: &str = r#"
require "json"
require "rubygems"

source_dir, dest_dir, jekyll_cmd = ARGV[0], ARGV[1], ARGV[2]
real_stdout = $stdout.dup
$stdout.reopen($stderr)

begin
  require "jekyll"
rescue LoadError
  bin = if jekyll_cmd.include?("/")
    jekyll_cmd
  else
    ENV.fetch("PATH", "").split(File::PATH_SEPARATOR)
      .map { |d| File.join(d, jekyll_cmd) }
      .find { |p| File.executable?(p) && !File.directory?(p) }
  end
  if bin && File.exist?(bin)
    gem_root = File.expand_path("../..", File.realpath(bin))
    ENV["GEM_PATH"] = [gem_root, ENV["GEM_PATH"]].compact.reject(&:empty?).join(File::PATH_SEPARATOR)
    Gem.clear_paths
  end
  require "jekyll"
end

Jekyll.logger.log_level = :error
cfg = Jekyll.configuration({
  "source" => source_dir,
  "destination" => dest_dir,
  "quiet" => true,
})
site = Jekyll::Site.new(cfg)
site.reset
site.read
site.generate

prefix = File.expand_path(dest_dir) + "/"
files = []
(site.pages + site.docs_to_write + site.static_files).each do |item|
  d = File.expand_path(item.destination(dest_dir))
  files << d.delete_prefix(prefix) if d.start_with?(prefix)
end
files.uniq!
files.sort!
real_stdout.puts(JSON.generate({ "version" => 1, "files" => files }))
"#;

fn default_ruby() -> String {
    "ruby".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JekyllConfig {
    /// The Ruby interpreter the planner runs with.
    #[serde(default = "default_ruby")]
    pub ruby: String,
    /// Report plan-vs-build mismatches as warnings instead of errors.
    #[serde(default)]
    pub loose_manifest: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for JekyllConfig {
    fn default() -> Self {
        Self {
            ruby: default_ruby(),
            loose_manifest: false,
            standard: StandardConfig::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanFile {
    version: u32,
    files: Vec<String>,
}

struct SitePlan {
    output_dir: PathBuf,
    outputs: Vec<PlannedOutput>,
    relative: BTreeSet<String>,
}

pub struct JekyllProcessor {
    config: JekyllConfig,
    plans: Mutex<BTreeMap<PathBuf, Arc<SitePlan>>>,
    runs: Mutex<BTreeMap<PathBuf, Arc<OnceTool>>>,
}

impl JekyllProcessor {
    pub const fn new(config: JekyllConfig) -> Self {
        Self {
            config,
            plans: Mutex::new(BTreeMap::new()),
            runs: Mutex::new(BTreeMap::new()),
        }
    }

    fn run_guard(&self, config_yml: &Path) -> Arc<OnceTool> {
        let mut guard = self.runs.lock();
        Arc::clone(
            guard
                .entry(config_yml.to_path_buf())
                .or_insert_with(|| Arc::new(OnceTool::new())),
        )
    }

    fn plan_site(
        &self,
        config_yml: &Path,
        sources: &[PathBuf],
        extra: &[PathBuf],
        instance_name: &str,
    ) -> Result<Arc<SitePlan>> {
        let mut guard = self.plans.lock();
        if let Some(plan) = guard.get(config_yml) {
            return Ok(Arc::clone(plan));
        }

        let anchor_dir = crate::processor::parent_dir_or_empty(config_yml);
        let output_dir = resolve_anchor_path(anchor_dir, &self.config.standard.output_dir);
        let flat = config_yml.to_string_lossy().replace('/', "_");
        let cache_id = format!("{instance_name}.{flat}");
        let key = self.plan_key(&cache_id, sources, extra)?;
        let cache_path = plan_cache_path(&cache_id);
        let invalid = || format!("[{instance_name}] the jekyll planner printed an invalid plan");

        let raw = if let Some(raw) = read_plan_cache(&cache_path, &key)? {
            raw
        } else {
            let raw = self.run_planner(anchor_dir, &cache_id)?;
            parse_plan(&raw).with_context(invalid)?;
            write_plan_cache(&cache_path, &key, &raw)?;
            raw
        };
        let relative = parse_plan(&raw).with_context(invalid)?;
        let outputs = relative
            .iter()
            .map(|rel| PlannedOutput {
                path: output_dir.join(rel),
                sources: sources.to_vec(),
            })
            .collect();
        let plan = Arc::new(SitePlan {
            output_dir,
            outputs,
            relative,
        });
        guard.insert(config_yml.to_path_buf(), Arc::clone(&plan));
        Ok(plan)
    }

    fn plan_key(&self, cache_id: &str, sources: &[PathBuf], extra: &[PathBuf]) -> Result<String> {
        let config = serde_json::to_string(&self.config)
            .context("Failed to serialize jekyll config for the plan key")?;
        let mut parts: Vec<String> = vec![
            env!("CARGO_PKG_VERSION").to_string(),
            cache_id.to_string(),
            config,
        ];
        let mut all: Vec<&PathBuf> = sources.iter().chain(extra).collect();
        all.sort();
        all.dedup();
        for path in all {
            let data = std::fs::read(path)
                .with_context(|| format!("Failed to read plan input {}", path.display()))?;
            parts.push(path.display().to_string());
            parts.push(crate::checksum::bytes_checksum(&data));
        }
        let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
        Ok(crate::checksum::hash_parts(&refs))
    }

    fn run_planner(&self, anchor_dir: &Path, cache_id: &str) -> Result<String> {
        let scratch = fresh_state_dir(cache_id, ".plan")?;
        let source_arg = if anchor_dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            anchor_dir
        };
        let mut cmd = Command::new(&self.config.ruby);
        cmd.arg("-e")
            .arg(PLAN_SCRIPT)
            .arg(source_arg)
            .arg(&scratch)
            .arg(&self.config.standard.command);
        log_command(&cmd);
        let what = format!(
            "[{cache_id}] the jekyll planner ({} -e <planner> {} {})",
            self.config.ruby,
            source_arg.display(),
            scratch.display()
        );
        let output = cmd
            .output()
            .with_context(|| format!("Failed to run {what}"))?;
        remove_dir_if_present(&scratch)?;
        check_command_output(&output, &what)?;
        String::from_utf8(output.stdout).with_context(|| format!("{what} printed non-UTF-8 output"))
    }

    fn execute_jekyll(
        &self,
        ctx: &crate::build_context::BuildContext,
        config_yml: &Path,
        instance_name: &str,
    ) -> Result<()> {
        let plan = self
            .plans
            .lock()
            .get(config_yml)
            .cloned()
            .with_context(|| format!("[{instance_name}] executed before its plan was loaded"))?;
        let flat = config_yml.to_string_lossy().replace('/', "_");
        let staging = fresh_state_dir(instance_name, &format!(".{flat}.staging"))?;
        let cwd =
            std::env::current_dir().context("Failed to get current directory for jekyll build")?;
        let staging_abs = cwd.join(&staging);

        let command = self.config.standard.require_command("jekyll")?;
        let mut cmd = Command::new(command);
        cmd.args(["build", "--disable-disk-cache", "-d"])
            .arg(&staging_abs);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, config_yml)?;
        check_command_output(
            &output,
            format_args!("jekyll build in {}", anchor_display_dir(config_yml)),
        )?;

        install_staged(
            &staging,
            &plan.relative,
            &plan.output_dir,
            instance_name,
            command,
            self.config.loose_manifest,
        )
    }
}

fn parse_plan(raw: &str) -> Result<BTreeSet<String>> {
    let parsed: PlanFile = serde_json::from_str(raw).context("plan is not valid JSON")?;
    if parsed.version != PLAN_VERSION {
        anyhow::bail!(
            "plan version {} is not supported (this rsconstruct understands version {PLAN_VERSION})",
            parsed.version
        );
    }
    let mut files = BTreeSet::new();
    for file in parsed.files {
        let plain = !file.is_empty()
            && !file.starts_with('/')
            && file
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != "..");
        if !plain {
            anyhow::bail!("plan entry '{file}' is not a plain relative path");
        }
        if !files.insert(file.clone()) {
            anyhow::bail!("plan lists '{file}' twice");
        }
    }
    if files.is_empty() {
        anyhow::bail!("plan lists no files");
    }
    Ok(files)
}

impl Processor for JekyllProcessor {
    fn scan_config(&self) -> &crate::config::StandardConfig {
        &self.config.standard
    }

    fn config_json(&self) -> Option<String> {
        crate::processor::ProcessorBase::config_json(&self.config)
    }

    fn clean(&self, product: &crate::graph::Product, verbose: bool) -> anyhow::Result<usize> {
        crate::processor::ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn replaces_own_outputs(&self) -> bool {
        true
    }

    fn auto_detect(&self, file_index: &FileIndex) -> bool {
        !file_index.scan(&self.config.standard, true).is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        vec![
            self.config.standard.command.clone(),
            self.config.ruby.clone(),
        ]
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
        let siblings = SiblingFilter {
            extensions: &[""],
            excludes: &[
                "/.git/",
                "/out/",
                "/.rsconstruct/",
                "/_site/",
                "/.jekyll-cache/",
            ],
        };

        let mut all_planned = Vec::new();
        for config_yml in files {
            let anchor_dir = config_yml
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
            let sources = crate::processor::build_anchor_inputs(&config_yml, &sibling_files, &[]);
            let plan = self.plan_site(&config_yml, &sources, &extra, instance_name)?;
            all_planned.extend(plan.outputs.clone());
        }

        add_planned_products(graph, &all_planned, instance_name, &config_hash, &extra)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let config_yml = product.primary_input();
        let instance_name = &product.processor;
        let guard = self.run_guard(config_yml);
        guard.run(&self.config.standard.command, || {
            self.execute_jekyll(ctx, config_yml, instance_name)
        })?;
        require_planned_output(product, &self.config.standard.command)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(JekyllProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 2,
        name: "jekyll",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "ruby", ty: crate::config::FieldType::String,
                affects_output: false, required: false,
                doc: "The Ruby interpreter the planner runs with; it must load the same Jekyll gem as `command`" },
            crate::config::FieldSpec { name: "loose_manifest", ty: crate::config::FieldType::Bool,
                affects_output: false, required: false,
                doc: "Report plan-vs-build mismatches as warnings instead of failing the build" },
        ],
        omit_standard_fields: &[],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["_config.yml"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "jekyll", output_dir: "_site", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<JekyllConfig>,
        keywords: &["ruby", "jekyll", "static-site", "html", "markdown", "web", "gem"],
        description: "Build Jekyll sites",
        is_native: false,
        is_rust: false,
        supports_batch: false,
        max_jobs_cap: Some(1),
    }
}
