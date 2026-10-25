//! The sphinx mass generator: builds Sphinx HTML documentation with
//! `sphinx-build`, one cached product per file it writes.
//!
//! Sphinx has no plan mode, but it knows every file before it writes one:
//! after its read phase the environment holds the documents, the domain
//! indices, the pages extensions add (viewcode's `_modules/`) and the
//! theme's static files. The planner below is a small Python program, run
//! against the same Sphinx installation as `sphinx-build`, that drives
//! Sphinx through exactly that much — reading and asset copying into a
//! scratch directory, never rendering a page — and prints the list.
//! Nothing about Sphinx's output layout is reimplemented in Rust, and the
//! plan-vs-build check fails the build if Sphinx ever writes something else.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use super::{
    OnceTool, PlannedOutput, add_planned_products, files_under, fresh_state_dir, install_staged,
    plan_cache_path, read_plan_cache, remove_dir_if_present, require_planned_output,
    write_plan_cache,
};
use crate::config::{StandardConfig, checksum_fields_of, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{
    Processor, ProcessorBase, check_command_output, format_command, log_command, run_command,
};

/// The plan format the planner prints; any other version is rejected.
const PLAN_VERSION: u32 = 1;

/// `sphinx-build` options a config may not pass in `args`: the builder,
/// `-E`, `-a` and `-d` are the processor's own, and `-D`, `-A`, `-t`, `-c`
/// and `-C` change the configuration behind the planner's back — it reads
/// `conf.py` and nothing else.
const OWNED_ARGS: &[&str] = &[
    "-a",
    "--write-all",
    "-b",
    "--builder",
    "-c",
    "--conf-dir",
    "-C",
    "--isolated",
    "-d",
    "--doctree-dir",
    "-D",
    "--define",
    "-A",
    "--html-define",
    "-E",
    "--fresh-env",
    "-M",
    "-t",
    "--tag",
];

/// The planner. `argv[1]` is the Sphinx source directory, `argv[2]` an
/// empty scratch directory; it prints `{"version": 1, "files": [...]}` with
/// paths relative to the output directory, and writes nothing else.
///
/// It follows `sphinx.builders.Builder.build` up to, but not including,
/// `write_documents`: `read()` (which also fires `env-updated`, where
/// `sphinx.ext.githubpages` drops `.nojekyll`), `prepare_writing()` (the
/// search indexer, the domain indices), the image pass `write_doc` would do
/// per page, run on the pickled doctrees instead, and the asset copies into
/// the scratch output directory. The pages are then what `finish()` writes: the
/// documents, the general index, the domain indices, every page an
/// extension yields on `html-collect-pages`, `html_additional_pages` and
/// the search page. Sphinx's own `get_outfilename` and the source-copy rule
/// of `get_doc_context` turn those into file names.
const PLAN_SCRIPT: &str = r#"
import json
import os
import sys

from sphinx.application import Sphinx
from sphinx.environment.adapters.indexentries import IndexEntries
from sphinx.util.docutils import docutils_namespace
from sphinx.util.parallel import SerialTasks


def main():
    srcdir, scratch = sys.argv[1], sys.argv[2]
    outdir = os.path.join(scratch, "out")
    doctreedir = os.path.join(scratch, "doctrees")
    # Only the plan may reach stdout; conf.py and extensions print freely.
    real_stdout = sys.stdout
    sys.stdout = sys.stderr
    with docutils_namespace():
        app = Sphinx(srcdir, srcdir, outdir, doctreedir, "html",
                     status=None, warning=sys.stderr, freshenv=True)
        builder = app.builder
        builder.read()
        docnames = set(app.env.found_docs)
        builder.finish_tasks = SerialTasks()
        builder.prepare_writing(docnames)
        # write_doc would pick each page's images into builder.images;
        # the same step on the pickled doctrees, without rendering.
        for docname in docnames:
            builder.post_process_images(app.env.get_doctree(docname))
        builder.copy_assets()
        builder.copy_image_files()
        pages = list(docnames)
        if builder.use_index:
            if app.config.html_split_index:
                pages += ["genindex", "genindex-all"]
                genindex = IndexEntries(app.env).create_index(builder)
                pages += ["genindex-" + key for key, _ in genindex]
            else:
                pages.append("genindex")
        pages += [name for name, _, _, _ in builder.domain_indices]
        for pagelist in app.events.emit("html-collect-pages"):
            pages += [pagename for pagename, _, _ in pagelist]
        pages += list(app.config.html_additional_pages)
        if builder.search:
            pages.append("search")
        files = set()
        for dirpath, _, filenames in os.walk(outdir):
            for name in filenames:
                files.add(os.path.relpath(os.path.join(dirpath, name), outdir))
        for page in pages:
            files.add(os.path.relpath(builder.get_outfilename(page), outdir))
        if app.config.html_use_opensearch and builder.search:
            files.add(os.path.join("_static", "opensearch.xml"))
        if builder.copysource:
            for docname in docnames:
                suffix = str(app.env.doc2path(docname, False))[len(docname):]
                sourcename = docname + suffix
                if suffix != app.config.html_sourcelink_suffix:
                    sourcename += app.config.html_sourcelink_suffix
                files.add(os.path.join("_sources", sourcename))
        files.add("objects.inv")
        files.add(".buildinfo")
        if builder.indexer is not None:
            files.add("searchindex.js")
    json.dump({"version": 1, "files": sorted(files)}, real_stdout)
    real_stdout.write("\n")


main()
"#;

/// Sphinx processor config.
/// Custom fields: `root`, `python`, `loose_manifest`. `output_dir`
/// (standard) is where the HTML goes. Unused `StandardConfig` fields:
/// formats, `dep_auto`, batch, and the scan fields — Sphinx decides what it
/// reads, and every file under `root` is a source of every product.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SphinxConfig {
    /// The Sphinx source directory (holding `conf.py`), relative to the
    /// project root.
    #[serde(default = "default_root")]
    pub root: String,
    /// The interpreter the planner runs with. It must import the same
    /// Sphinx that `command` runs: the active environment's.
    #[serde(default = "default_python")]
    pub python: String,
    /// Report plan-vs-build mismatches as warnings instead of errors.
    #[serde(default)]
    pub loose_manifest: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

fn default_root() -> String {
    "sphinx".to_string()
}

fn default_python() -> String {
    "python3".to_string()
}

impl Default for SphinxConfig {
    fn default() -> Self {
        Self {
            root: default_root(),
            python: default_python(),
            loose_manifest: false,
            standard: StandardConfig::default(),
        }
    }
}

/// The computed plan of one instance.
struct Plan {
    /// Planned outputs, paths relative to the project root.
    outputs: Vec<PlannedOutput>,
    /// The same files relative to the output directory, as Sphinx writes them.
    relative: BTreeSet<String>,
    /// The output directory, relative to the project root.
    output_dir: PathBuf,
}

/// What the planner prints.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanFile {
    version: u32,
    files: Vec<String>,
}

pub struct SphinxProcessor {
    config: SphinxConfig,
    plan: Mutex<Option<Arc<Plan>>>,
    tool_run: OnceTool,
}

impl SphinxProcessor {
    pub fn new(config: SphinxConfig) -> Result<Self> {
        if let Some(arg) = config.standard.args.iter().find(|a| is_owned_arg(a)) {
            anyhow::bail!(
                "args entry '{arg}' is not allowed: the sphinx processor chooses the builder, \
                 -E, -a and -d itself, and -D/-A/-t/-c/-C would change the configuration behind \
                 the planner's back; put configuration in conf.py"
            );
        }
        if config.root.is_empty() {
            anyhow::bail!("root must name the Sphinx source directory (the one holding conf.py)");
        }
        if config.standard.output_dir.is_empty() {
            anyhow::bail!("output_dir must name the directory the HTML is written to");
        }
        Ok(Self {
            config,
            plan: Mutex::new(None),
            tool_run: OnceTool::new(),
        })
    }

    fn root(&self) -> PathBuf {
        PathBuf::from(&self.config.root)
    }

    fn output_dir(&self) -> PathBuf {
        PathBuf::from(&self.config.standard.output_dir)
    }

    /// The plan, computed once per process (discovery runs several passes):
    /// from the plan cache when nothing it was computed from changed,
    /// otherwise by running the planner.
    fn plan(&self, instance_name: &str) -> Result<Arc<Plan>> {
        let mut guard = self.plan.lock();
        if let Some(plan) = guard.as_ref() {
            return Ok(Arc::clone(plan));
        }
        let root = self.root();
        if !root.join("conf.py").is_file() {
            anyhow::bail!(
                "[{instance_name}] {} has no conf.py; set `root` to the Sphinx source directory",
                root.display()
            );
        }
        // Every file under the source directory is a source of every
        // product: a toctree, a cross-reference or the index can change any
        // page when any other changes, so finer edges would be a guess.
        let sources = files_under(&root)?;
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;
        let key = self.plan_key(instance_name, &sources, &extra)?;
        let cache_path = plan_cache_path(instance_name);
        let invalid = || format!("[{instance_name}] the sphinx planner printed an invalid plan");
        let raw = if let Some(raw) = read_plan_cache(&cache_path, &key)? {
            raw
        } else {
            let raw = self.run_planner(instance_name)?;
            // Validate before caching: an invalid plan must fail now and
            // again next time, never be replayed as cached.
            parse_plan(&raw).with_context(invalid)?;
            write_plan_cache(&cache_path, &key, &raw)?;
            raw
        };
        let relative = parse_plan(&raw).with_context(invalid)?;
        let output_dir = self.output_dir();
        let outputs = relative
            .iter()
            .map(|rel| PlannedOutput {
                path: output_dir.join(rel),
                sources: sources.clone(),
            })
            .collect();
        let plan = Arc::new(Plan {
            outputs,
            relative,
            output_dir,
        });
        *guard = Some(Arc::clone(&plan));
        Ok(plan)
    }

    /// The key a cached plan is valid for: the rsconstruct version, the
    /// instance's whole config, and the path and content of every file the
    /// plan was computed from — the source directory and `dep_inputs` (the
    /// package autodoc imports, the lock file that pins Sphinx). A new,
    /// removed or edited file all mean a fresh plan.
    fn plan_key(
        &self,
        instance_name: &str,
        sources: &[PathBuf],
        extra: &[PathBuf],
    ) -> Result<String> {
        let config = serde_json::to_string(&self.config)
            .context("Failed to serialize sphinx config for the plan key")?;
        let mut parts: Vec<String> = vec![
            env!("CARGO_PKG_VERSION").to_string(),
            instance_name.to_string(),
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

    /// Run the planner in a fresh scratch directory and return what it
    /// printed. Discovery has no `BuildContext`, so this is a plain spawn.
    fn run_planner(&self, instance_name: &str) -> Result<String> {
        let scratch = fresh_state_dir(instance_name, ".plan")?;
        let mut cmd = Command::new(&self.config.python);
        cmd.arg("-c")
            .arg(PLAN_SCRIPT)
            .arg(&self.config.root)
            .arg(&scratch);
        log_command(&cmd);
        let what = format!(
            "[{instance_name}] the sphinx planner ({} -c <planner> {} {})",
            self.config.python,
            self.config.root,
            scratch.display()
        );
        let output = cmd
            .output()
            .with_context(|| format!("Failed to run {what}"))?;
        remove_dir_if_present(&scratch)?;
        check_command_output(&output, &what)?;
        String::from_utf8(output.stdout).with_context(|| format!("{what} printed non-UTF-8 output"))
    }

    /// One `sphinx-build` into a fresh staging directory, with a fresh
    /// doctree directory kept out of the output, then the plan check and
    /// the move of the planned files into place.
    fn run_tool(
        &self,
        ctx: &crate::build_context::BuildContext,
        instance_name: &str,
    ) -> Result<()> {
        let plan =
            self.plan.lock().clone().with_context(|| {
                format!("[{instance_name}] executed before its plan was loaded")
            })?;
        let staging = fresh_state_dir(instance_name, ".staging")?;
        let doctrees = fresh_state_dir(instance_name, ".doctrees")?;

        let mut cmd = Command::new(&self.config.standard.command);
        cmd.args(["-b", "html", "-E", "-a", "-d"]).arg(&doctrees);
        cmd.args(&self.config.standard.args);
        cmd.arg(&self.config.root).arg(&staging);
        let output = run_command(ctx, &cmd)?;
        check_command_output(
            &output,
            format_args!("[{instance_name}] {}", format_command(&cmd)),
        )?;
        remove_dir_if_present(&doctrees)?;
        install_staged(
            &staging,
            &plan.relative,
            &plan.output_dir,
            instance_name,
            &self.config.standard.command,
            self.config.loose_manifest,
        )
    }
}

/// Whether `arg` is, or starts with, one of `OWNED_ARGS` (`-Dkey=value`,
/// `--define=key=value` and `-Ea` count).
fn is_owned_arg(arg: &str) -> bool {
    OWNED_ARGS.iter().any(|owned| {
        if arg == *owned {
            return true;
        }
        if owned.starts_with("--") {
            arg.starts_with(&format!("{owned}="))
        } else {
            arg.len() > owned.len() && arg.starts_with(owned)
        }
    })
}

/// The planner's output as the set of files relative to the output
/// directory; every entry must be a plain relative path, listed once.
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

impl Processor for SphinxProcessor {
    fn scan_config(&self) -> &StandardConfig {
        &self.config.standard
    }

    fn auto_detect(&self, _file_index: &FileIndex) -> bool {
        self.root().join("conf.py").is_file()
    }

    fn required_tools(&self) -> Vec<String> {
        let mut tools = vec![
            self.config.standard.command.clone(),
            self.config.python.clone(),
        ];
        tools.extend(self.config.standard.required_tools.iter().cloned());
        tools
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        _file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let plan = self.plan(instance_name)?;
        let config_hash = output_config_hash(&self.config, &checksum_fields_of(instance_name));
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;
        add_planned_products(graph, &plan.outputs, instance_name, &config_hash, &extra)
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let instance_name = &product.processor;
        self.tool_run.run(&self.config.standard.command, || {
            self.run_tool(ctx, instance_name)
        })?;
        require_planned_output(product, &self.config.standard.command)
    }

    /// `run_tool` builds into a staging directory and unlinks each planned
    /// file before moving Sphinx's version into place.
    fn replaces_own_outputs(&self) -> bool {
        true
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn config_json(&self) -> Option<String> {
        ProcessorBase::config_json(&self.config)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_try_create(toml, |cfg| {
        SphinxProcessor::new(cfg).map(|p| Box::new(p) as Box<dyn Processor>)
    })
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "sphinx",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "root", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "The Sphinx source directory (holding conf.py), relative to the project root" },
            crate::config::FieldSpec { name: "python", ty: crate::config::FieldType::String,
                affects_output: false, required: false,
                doc: "The interpreter the planner runs with; it must import the same Sphinx as `command`" },
            crate::config::FieldSpec { name: "output_dir", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Where the HTML is written" },
            crate::config::FieldSpec { name: "loose_manifest", ty: crate::config::FieldType::Bool,
                affects_output: false, required: false,
                doc: "Report plan-vs-build mismatches as warnings instead of failing the build" },
        ],
        omit_standard_fields: &["formats", "dep_auto", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "sphinx-build", output_dir: "docs", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<SphinxConfig>,
        keywords: &["python", "sphinx", "documentation", "rst", "html", "mass", "pip"],
        description: "Build Sphinx HTML documentation; one cached product per file Sphinx writes, planned by Sphinx itself",
        is_builtin: false,
        is_rust: false,
        supports_batch: false,
        caches_outputs: true,
        max_jobs_cap: Some(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_args_are_refused_in_every_spelling() {
        let refused = "-E -a -Ea -b -bhtml -d -dout/doctrees -D -Dproject=x --define \
                       --define=project=x -A -t -tdraft -c -C -M";
        for arg in refused.split_whitespace() {
            assert!(is_owned_arg(arg), "{arg} must be refused");
        }
        let allowed = "-W -q -n -j auto --keep-going -T -w warnings.log";
        for arg in allowed.split_whitespace() {
            assert!(!is_owned_arg(arg), "{arg} must be allowed");
        }
        let mut cfg = SphinxConfig::default();
        cfg.standard.output_dir = "docs".to_string();
        cfg.standard.args = vec!["-W".to_string(), "-E".to_string()];
        let err = SphinxProcessor::new(cfg)
            .err()
            .expect("-E must be rejected");
        assert!(format!("{err:#}").contains("args entry '-E' is not allowed"));
    }

    #[test]
    fn plan_parsing_validates_version_paths_and_duplicates() {
        let ok =
            parse_plan("{\"version\":1,\"files\":[\"index.html\",\"_static/a.css\"]}").unwrap();
        assert_eq!(
            ok.into_iter().collect::<Vec<_>>(),
            ["_static/a.css", "index.html"]
        );
        for (raw, why) in [
            (
                "{\"version\":2,\"files\":[\"a\"]}",
                "version 2 is not supported",
            ),
            ("{\"version\":1,\"files\":[]}", "lists no files"),
            (
                "{\"version\":1,\"files\":[\"/abs.html\"]}",
                "not a plain relative path",
            ),
            (
                "{\"version\":1,\"files\":[\"a/../b.html\"]}",
                "not a plain relative path",
            ),
            (
                "{\"version\":1,\"files\":[\"a.html\",\"a.html\"]}",
                "lists 'a.html' twice",
            ),
            (
                "{\"version\":1,\"files\":[\"a\"],\"extra\":1}",
                "not valid JSON",
            ),
        ] {
            let err = parse_plan(raw)
                .err()
                .unwrap_or_else(|| panic!("{raw} must fail"));
            assert!(format!("{err:#}").contains(why), "{raw}: {err:#}");
        }
    }

    #[test]
    fn required_tools_lists_sphinx_build_and_the_interpreter() {
        let mut cfg = SphinxConfig::default();
        cfg.standard.command = "sphinx-build".to_string();
        cfg.standard.output_dir = "docs".to_string();
        cfg.standard.required_tools = vec!["dot".to_string()];
        let proc = SphinxProcessor::new(cfg).unwrap();
        assert_eq!(
            proc.required_tools(),
            vec!["sphinx-build", "python3", "dot"]
        );
    }
}
