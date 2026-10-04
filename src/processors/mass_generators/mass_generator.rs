use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::SystemTime;

use crate::config::{StandardConfig, checksum_fields_of, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processors::{
    Processor, ProcessorBase, check_command_output, format_command, log_command, run_command,
};

/// The manifest schema this processor understands. A tool printing any
/// other `version` is rejected rather than guessed at.
const MANIFEST_VERSION: u32 = 1;

/// Mass generator config.
/// Custom fields: `predict_command`, `predict_args`, `output_dirs`, `loose_manifest`.
/// Unused `StandardConfig` fields: formats, `dep_auto`, `output_dir`, batch, and
/// every scan field — discovery is driven by the tool's manifest, not by a scan.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct MassGeneratorConfig {
    /// The tool's plan command: prints the JSON manifest to stdout and
    /// writes nothing.
    #[serde(default)]
    pub predict_command: String,
    /// Arguments for `predict_command`.
    #[serde(default)]
    pub predict_args: Vec<String>,
    /// Directories the tool writes into. Every manifest path must fall
    /// inside one of them; the plan-vs-build check walks them.
    #[serde(default)]
    pub output_dirs: Vec<String>,
    /// Downgrade plan-vs-build mismatches from errors to warnings. For
    /// developing the tool itself; a production config leaves it false.
    #[serde(default)]
    pub loose_manifest: bool,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

/// The JSON document `predict_command` prints.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    outputs: Vec<ManifestEntry>,
}

/// One predicted output file and the inputs whose content determines it.
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManifestEntry {
    path: PathBuf,
    sources: Vec<PathBuf>,
}

/// Whether this build has already run the tool, and how it went. Every
/// product of the instance asks before executing, so the tool runs at most
/// once per build and a failure is reported once per product instead of
/// re-running a build that just failed.
enum ToolRun {
    NotRun,
    Succeeded,
    Failed(String),
}

/// A data-driven processor for tools that enumerate their outputs before
/// producing them: the transparent counterpart of the opaque Creator.
///
/// `predict_command` prints a manifest at discovery time; each entry becomes
/// one product (`inputs` = the entry's sources, `outputs` = its path), so
/// every generated file has an owner, a blob cache entry, and dependency
/// edges of its own. `command` then runs at most once per build — the first
/// dirty product triggers it, the rest find it done — and the files it
/// wrote are compared against the plan.
///
/// Example config:
/// ```toml
/// [processor.mass_generator.site]
/// command = "rssite"
/// args = ["build"]
/// predict_command = "rssite"
/// predict_args = ["plan"]
/// output_dirs = ["_site"]
/// ```
pub struct MassGeneratorProcessor {
    config: MassGeneratorConfig,
    /// The parsed plan, obtained once per process. Discovery is a fixed-point
    /// loop that calls `discover` several times per build; the source tree
    /// does not change between passes, so neither does the plan. Watch mode
    /// builds a fresh processor per rebuild, which clears this.
    manifest: Mutex<Option<Arc<Vec<ManifestEntry>>>>,
    tool_run: Mutex<ToolRun>,
}

impl MassGeneratorProcessor {
    pub fn new(config: MassGeneratorConfig) -> Result<Self> {
        for dir in &config.output_dirs {
            if let Some(why) = plain_relative_path_error(Path::new(dir)) {
                anyhow::bail!("output_dirs entry '{dir}' {why}");
            }
        }
        // The manifest decides the inputs; a scan field here would be read
        // by nobody, and silently accepting it would hide a config error
        // from someone migrating a creator (`src_extensions = [...]`).
        let std = &config.standard;
        let scan_set = [
            ("src_dirs", &std.src_dirs),
            ("src_extensions", &std.src_extensions),
            ("src_files", &std.src_files),
        ]
        .into_iter()
        .find(|(_, v)| v.as_deref().is_some_and(|v| !v.is_empty()));
        if let Some((field, _)) = scan_set {
            anyhow::bail!(
                "'{field}' is not used by mass_generator: the inputs of every product come from \
                 the manifest's `sources`, so remove the field"
            );
        }
        Ok(Self {
            config,
            manifest: Mutex::new(None),
            tool_run: Mutex::new(ToolRun::NotRun),
        })
    }

    fn output_dirs(&self) -> Vec<PathBuf> {
        self.config.output_dirs.iter().map(PathBuf::from).collect()
    }

    fn predict_command(&self) -> Command {
        let mut cmd = Command::new(&self.config.predict_command);
        cmd.args(&self.config.predict_args);
        cmd
    }

    /// The plan, running `predict_command` on first use. `discover` has no
    /// `BuildContext`, so this is a plain spawn: the plan command is by
    /// contract cheap and side-effect free, and nothing else runs alongside
    /// it at discovery time.
    fn manifest(&self, instance_name: &str) -> Result<Arc<Vec<ManifestEntry>>> {
        let mut guard = self.manifest.lock();
        if let Some(entries) = guard.as_ref() {
            return Ok(Arc::clone(entries));
        }
        if self.config.predict_command.is_empty() {
            anyhow::bail!("'predict_command' is not set for processor '{instance_name}'");
        }
        let mut cmd = self.predict_command();
        log_command(&cmd);
        let output = cmd.output().with_context(|| {
            format!("Failed to spawn predict_command: {}", format_command(&cmd))
        })?;
        check_command_output(
            &output,
            format_args!("[{instance_name}] predict_command {}", format_command(&cmd)),
        )?;
        let entries = parse_manifest(&output.stdout, &self.output_dirs()).with_context(|| {
            format!(
                "[{instance_name}] predict_command {} printed an invalid manifest",
                format_command(&cmd)
            )
        })?;
        let entries = Arc::new(entries);
        *guard = Some(Arc::clone(&entries));
        Ok(entries)
    }

    /// Run `command` if this build has not yet; replay the earlier outcome
    /// if it has. Serialized on `tool_run`, so concurrent products of one
    /// instance cannot start the tool twice.
    fn run_tool_once(
        &self,
        ctx: &crate::build_context::BuildContext,
        instance_name: &str,
    ) -> Result<()> {
        let mut state = self.tool_run.lock();
        match &*state {
            ToolRun::Succeeded => return Ok(()),
            ToolRun::Failed(msg) => {
                anyhow::bail!(
                    "{} already failed earlier in this build: {msg}",
                    self.config.standard.command
                )
            }
            ToolRun::NotRun => {}
        }
        let result = self.run_tool(ctx, instance_name);
        *state = match &result {
            Ok(()) => ToolRun::Succeeded,
            Err(e) => ToolRun::Failed(format!("{e:#}")),
        };
        result
    }

    /// One invocation of `command`, bracketed by the plan-vs-build check.
    fn run_tool(
        &self,
        ctx: &crate::build_context::BuildContext,
        instance_name: &str,
    ) -> Result<()> {
        let entries = self.manifest(instance_name)?;
        let predicted: BTreeSet<&Path> = entries.iter().map(|e| e.path.as_path()).collect();

        // The tool rewrites every predicted file, including the ones whose
        // products are clean this build. A clean file may be a read-only
        // hardlink left by a cache restore, which the tool cannot open for
        // writing; unlinking first gives it a clear path. The executor only
        // unlinks the outputs of the products it is about to build.
        for path in &predicted {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(anyhow::Error::from(e)
                        .context(format!("Failed to remove stale output {}", path.display())));
                }
            }
        }

        let output_dirs = self.output_dirs();
        let before = snapshot_mtimes(&output_dirs)?;

        let command = self.config.standard.require_command(instance_name)?;
        let mut cmd = Command::new(command);
        cmd.args(&self.config.standard.args);
        let output = run_command(ctx, &cmd)?;
        check_command_output(
            &output,
            format_args!("[{instance_name}] {}", format_command(&cmd)),
        )?;

        let after = snapshot_mtimes(&output_dirs)?;
        let foreign = |path: &Path| ctx.is_declared_output(path);
        let mismatches = plan_mismatches(&predicted, &before, &after, &foreign);
        if !mismatches.is_empty() {
            let msg = format!(
                "[{instance_name}] the files {} wrote do not match the plan from {}:\n  {}",
                format_command(&cmd),
                format_command(&self.predict_command()),
                mismatches.join("\n  ")
            );
            if self.config.loose_manifest {
                crate::output::warn(&format!("{msg}\n  (loose_manifest = true: continuing)"));
            } else {
                anyhow::bail!("{msg}");
            }
        }
        Ok(())
    }
}

/// Why `path` is not a plain relative path, or `None` if it is one. Plain
/// means every component is a name: no root, no `.`, no `..`. Graph paths
/// are compared literally against other processors' declared outputs, so
/// `_site/./index.html` would silently fail to match `_site/index.html`.
fn plain_relative_path_error(path: &Path) -> Option<&'static str> {
    // Split the raw text rather than using `Path::components()`, which
    // normalizes an interior `./` away and would let `_site/./x.html`
    // through with its literal `./` intact.
    let text = path.to_string_lossy();
    if text.is_empty() {
        return Some("is empty");
    }
    if text.starts_with('/') {
        return Some("is absolute");
    }
    for piece in text.split('/') {
        match piece {
            "" => return Some("contains an empty component (`//` or a trailing `/`)"),
            "." => return Some("contains a `.` component"),
            ".." => return Some("contains a `..` component"),
            _ => {}
        }
    }
    None
}

/// Parse and validate the manifest `predict_command` printed. Entries come
/// back sorted by path, so product ids do not depend on the tool's output
/// order.
fn parse_manifest(json: &[u8], output_dirs: &[PathBuf]) -> Result<Vec<ManifestEntry>> {
    let manifest: Manifest = serde_json::from_slice(json).context("manifest is not valid JSON")?;
    if manifest.version != MANIFEST_VERSION {
        anyhow::bail!(
            "manifest version {} is not supported (this rsconstruct understands version {MANIFEST_VERSION})",
            manifest.version
        );
    }
    let mut entries = manifest.outputs;
    for entry in &entries {
        let path = &entry.path;
        if let Some(why) = plain_relative_path_error(path) {
            anyhow::bail!("manifest output path '{}' {why}", path.display());
        }
        if !output_dirs.iter().any(|dir| path.starts_with(dir)) {
            anyhow::bail!(
                "manifest output path '{}' is outside every output_dirs entry ({})",
                path.display(),
                output_dirs
                    .iter()
                    .map(|d| d.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if entry.sources.is_empty() {
            anyhow::bail!(
                "manifest output '{}' lists no sources; every output must name at least one \
                 file whose content determines it (the tool's own config file, if nothing else)",
                path.display()
            );
        }
        for source in &entry.sources {
            if let Some(why) = plain_relative_path_error(source) {
                anyhow::bail!(
                    "manifest output '{}' has source '{}' which {why}",
                    path.display(),
                    source.display()
                );
            }
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    for pair in entries.windows(2) {
        if pair[0].path == pair[1].path {
            anyhow::bail!(
                "manifest lists output path '{}' twice",
                pair[0].path.display()
            );
        }
    }
    Ok(entries)
}

/// Modification time of every regular file under `dirs`. A directory that
/// does not exist yet contributes nothing.
fn snapshot_mtimes(dirs: &[PathBuf]) -> Result<BTreeMap<PathBuf, SystemTime>> {
    let mut snapshot = BTreeMap::new();
    for dir in dirs {
        for path in crate::object_store::walk_files(dir) {
            let mtime = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .with_context(|| format!("Failed to stat {}", path.display()))?;
            snapshot.insert(path, mtime);
        }
    }
    Ok(snapshot)
}

/// Compare what the tool wrote against what it predicted. A predicted file
/// that is absent afterwards is missing. A file that is new or rewritten
/// since the `before` snapshot, is not predicted, and is not another
/// product's declared output (`foreign`) was written off-plan. Files the
/// tool left untouched — a neighbor processor's output, a committed CNAME,
/// a page from an older plan — are not its business and are not reported.
fn plan_mismatches(
    predicted: &BTreeSet<&Path>,
    before: &BTreeMap<PathBuf, SystemTime>,
    after: &BTreeMap<PathBuf, SystemTime>,
    foreign: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let mut mismatches = Vec::new();
    for path in predicted {
        if !after.contains_key(*path) {
            mismatches.push(format!(
                "missing: {} (predicted but not produced)",
                path.display()
            ));
        }
    }
    for (path, mtime) in after {
        if predicted.contains(path.as_path()) || foreign(path) {
            continue;
        }
        if before.get(path) != Some(mtime) {
            mismatches.push(format!(
                "unexpected: {} (written but not in the plan)",
                path.display()
            ));
        }
    }
    mismatches
}

impl Processor for MassGeneratorProcessor {
    fn scan_config(&self) -> &StandardConfig {
        &self.config.standard
    }

    fn auto_detect(&self, _file_index: &FileIndex) -> bool {
        !self.config.standard.command.is_empty()
            && !self.config.predict_command.is_empty()
            && !self.config.output_dirs.is_empty()
    }

    fn required_tools(&self) -> Vec<String> {
        let mut tools = Vec::new();
        for tool in [&self.config.standard.command, &self.config.predict_command]
            .into_iter()
            .chain(self.config.standard.required_tools.iter())
        {
            if !tool.is_empty() && !tools.contains(tool) {
                tools.push(tool.clone());
            }
        }
        tools
    }

    fn discover(
        &self,
        graph: &mut BuildGraph,
        _file_index: &FileIndex,
        instance_name: &str,
    ) -> Result<()> {
        let entries = self.manifest(instance_name)?;
        let config_hash = output_config_hash(&self.config, &checksum_fields_of(instance_name));
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;
        for entry in entries.iter() {
            let mut inputs = entry.sources.clone();
            for input in &extra {
                if !inputs.contains(input) {
                    inputs.push(input.clone());
                }
            }
            // A product's cache key is processor + config hash + input
            // checksum; the output path is not part of it, because for every
            // other processor the output path follows from the input. Here
            // many outputs can share one input set — a tag's index page, its
            // `page/1/` redirect and its feed all depend on exactly the same
            // posts — and without the path in the key the second would be
            // "restored" from the first's blob, with the first's content.
            let hash = crate::checksum::hash_parts(&[
                config_hash.as_str(),
                &entry.path.display().to_string(),
            ]);
            graph.add_product(inputs, vec![entry.path.clone()], instance_name, Some(hash))?;
        }
        Ok(())
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        self.run_tool_once(ctx, &product.processor)?;
        // The plan check already reported this in strict mode; in loose mode
        // it was a warning, and this product still has nothing to cache.
        let output = product.primary_output();
        if !output.is_file() {
            anyhow::bail!(
                "{} did not produce the predicted output {}",
                self.config.standard.command,
                output.display()
            );
        }
        Ok(())
    }

    fn clean(&self, product: &Product, verbose: bool) -> Result<usize> {
        ProcessorBase::clean(product, &product.processor, verbose)
    }

    fn config_json(&self) -> Option<String> {
        ProcessorBase::config_json(&self.config)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processors::Processor>> {
    crate::registries::deserialize_and_try_create(toml, |cfg| {
        MassGeneratorProcessor::new(cfg).map(|p| Box::new(p) as Box<dyn Processor>)
    })
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 1,
        name: "mass_generator",
        processor_type: crate::processors::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "command", ty: crate::config::FieldType::String,
                affects_output: true, required: true,
                doc: "The tool's build command; runs at most once per build, when any predicted file is dirty" },
            crate::config::FieldSpec { name: "predict_command", ty: crate::config::FieldType::String,
                affects_output: true, required: true,
                doc: "The tool's plan command; prints the JSON manifest of predicted outputs to stdout" },
            crate::config::FieldSpec { name: "predict_args", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Arguments passed to predict_command" },
            crate::config::FieldSpec { name: "output_dirs", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: true,
                doc: "Directories the tool writes into; every manifest path must fall inside one" },
            crate::config::FieldSpec { name: "loose_manifest", ty: crate::config::FieldType::Bool,
                affects_output: false, required: false,
                doc: "Report plan-vs-build mismatches as warnings instead of failing the build" },
        ],
        omit_standard_fields: &["formats", "dep_auto", "output_dir", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &[], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults::EMPTY),
        defconfig_json: crate::registries::default_config_json::<MassGeneratorConfig>,
        keywords: &["mass", "generator", "predict", "manifest", "site", "transparent", "creator"],
        description: "Run a tool that enumerates its outputs in advance; one cached product per predicted file",
        is_native: false,
        is_rust: false,
        can_fix: false,
        supports_batch: false,
        // Products of one instance execute one at a time: the first runs the
        // tool, the rest find it done. Letting them run in parallel would
        // only park threads on the tool_run lock.
        max_jobs_cap: Some(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn dirs(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn parse_sorts_entries_by_path() {
        let json = br#"{"version":1,"outputs":[
            {"path":"_site/b.html","sources":["docs/b.md"]},
            {"path":"_site/a.html","sources":["docs/a.md","templates/t.html"]}
        ]}"#;
        let entries = parse_manifest(json, &dirs(&["_site"])).unwrap();
        let paths: Vec<_> = entries
            .iter()
            .map(|e| e.path.display().to_string())
            .collect();
        assert_eq!(paths, vec!["_site/a.html", "_site/b.html"]);
        assert_eq!(entries[0].sources, dirs(&["docs/a.md", "templates/t.html"]));
    }

    #[test]
    fn parse_accepts_an_empty_plan() {
        let entries = parse_manifest(br#"{"version":1,"outputs":[]}"#, &dirs(&["_site"])).unwrap();
        assert_eq!(entries.len(), 0);
    }

    /// Each rejection names the offending path, so the tool author can
    /// find it in a long manifest.
    #[test]
    fn parse_rejects_malformed_manifests() {
        let cases: &[(&[u8], &str)] = &[
            (b"not json", "not valid JSON"),
            (
                br#"{"version":2,"outputs":[]}"#,
                "version 2 is not supported",
            ),
            (br#"{"version":1,"outputs":[],"extra":1}"#, "unknown field"),
            (
                br#"{"version":1,"outputs":[{"path":"elsewhere/x.html","sources":["a.md"]}]}"#,
                "'elsewhere/x.html' is outside every output_dirs entry (_site)",
            ),
            (
                br#"{"version":1,"outputs":[{"path":"/_site/x.html","sources":["a.md"]}]}"#,
                "'/_site/x.html' is absolute",
            ),
            (
                br#"{"version":1,"outputs":[{"path":"_site/../x.html","sources":["a.md"]}]}"#,
                "contains a `..` component",
            ),
            (
                br#"{"version":1,"outputs":[{"path":"_site/./x.html","sources":["a.md"]}]}"#,
                "contains a `.` component",
            ),
            (
                br#"{"version":1,"outputs":[{"path":"_site/x.html","sources":[]}]}"#,
                "'_site/x.html' lists no sources",
            ),
            (
                br#"{"version":1,"outputs":[{"path":"_site/x.html","sources":["../a.md"]}]}"#,
                "source '../a.md' which contains a `..` component",
            ),
            (
                br#"{"version":1,"outputs":[
                    {"path":"_site/x.html","sources":["a.md"]},
                    {"path":"_site/x.html","sources":["b.md"]}]}"#,
                "lists output path '_site/x.html' twice",
            ),
        ];
        for (json, expected) in cases {
            let err = parse_manifest(json, &dirs(&["_site"]))
                .err()
                .unwrap_or_else(|| panic!("accepted: {}", String::from_utf8_lossy(json)));
            let msg = format!("{err:#}");
            assert!(msg.contains(expected), "expected '{expected}' in: {msg}");
        }
    }

    #[test]
    fn output_dir_match_is_by_component() {
        // `_site2/x.html` starts with the string `_site` but is not inside it.
        let json = br#"{"version":1,"outputs":[{"path":"_site2/x.html","sources":["a.md"]}]}"#;
        assert!(parse_manifest(json, &dirs(&["_site"])).is_err());
    }

    fn snapshot(entries: &[(&str, u64)]) -> BTreeMap<PathBuf, SystemTime> {
        entries
            .iter()
            .map(|(p, secs)| {
                (
                    PathBuf::from(p),
                    SystemTime::UNIX_EPOCH + Duration::from_secs(*secs),
                )
            })
            .collect()
    }

    /// The decision table of the plan-vs-build check: missing predicted
    /// files and off-plan writes are reported; untouched strangers, files
    /// another product declares, and the predicted files themselves are not.
    #[test]
    fn plan_mismatch_decision_table() {
        let predicted_paths = [Path::new("_site/a.html"), Path::new("_site/b.html")];
        let predicted: BTreeSet<&Path> = predicted_paths.into_iter().collect();
        let before = snapshot(&[
            ("_site/stale.html", 10),
            ("_site/CNAME", 10),
            ("_site/about.html", 10),
            ("_site/a.html", 10),
        ]);
        let after = snapshot(&[
            ("_site/stale.html", 10), // untouched stranger: not reported
            ("_site/CNAME", 10),      // untouched stranger: not reported
            ("_site/about.html", 20), // rewritten but foreign-owned: not reported
            ("_site/a.html", 20),     // predicted and produced
            ("_site/extra.html", 20), // new and off-plan: reported
            ("_site/stray.txt", 20),  // new and off-plan: reported
                                      // _site/b.html predicted but absent: reported
        ]);
        let foreign = |p: &Path| p == Path::new("_site/about.html");
        let mismatches = plan_mismatches(&predicted, &before, &after, &foreign);
        assert_eq!(
            mismatches,
            vec![
                "missing: _site/b.html (predicted but not produced)",
                "unexpected: _site/extra.html (written but not in the plan)",
                "unexpected: _site/stray.txt (written but not in the plan)",
            ]
        );
    }

    #[test]
    fn plan_mismatch_flags_rewritten_stranger() {
        let predicted_paths = [Path::new("_site/a.html")];
        let predicted: BTreeSet<&Path> = predicted_paths.into_iter().collect();
        let before = snapshot(&[("_site/old.html", 10), ("_site/a.html", 10)]);
        let after = snapshot(&[("_site/old.html", 11), ("_site/a.html", 11)]);
        let mismatches = plan_mismatches(&predicted, &before, &after, &|_| false);
        assert_eq!(
            mismatches,
            vec!["unexpected: _site/old.html (written but not in the plan)"]
        );
    }

    #[test]
    fn constructor_rejects_scan_fields_and_bad_output_dirs() {
        let mut cfg = MassGeneratorConfig::default();
        cfg.standard.src_dirs = Some(vec!["docs".to_string()]);
        let err = MassGeneratorProcessor::new(cfg)
            .err()
            .expect("src_dirs must be rejected");
        assert!(format!("{err:#}").contains("'src_dirs' is not used by mass_generator"));

        let cfg = MassGeneratorConfig {
            output_dirs: vec!["../out".to_string()],
            ..Default::default()
        };
        let err = MassGeneratorProcessor::new(cfg)
            .err()
            .expect("`..` must be rejected");
        assert!(
            format!("{err:#}").contains("output_dirs entry '../out' contains a `..` component")
        );

        let mut cfg = MassGeneratorConfig::default();
        cfg.standard.src_dirs = Some(Vec::new());
        cfg.standard.src_extensions = Some(Vec::new());
        assert!(
            MassGeneratorProcessor::new(cfg).is_ok(),
            "resolved-empty scan fields are fine"
        );
    }

    #[test]
    fn required_tools_lists_both_commands_once() {
        let mut cfg = MassGeneratorConfig {
            predict_command: "rssite".to_string(),
            ..Default::default()
        };
        cfg.standard.command = "rssite".to_string();
        cfg.standard.required_tools = vec!["pandoc".to_string()];
        let proc = MassGeneratorProcessor::new(cfg).unwrap();
        assert_eq!(proc.required_tools(), vec!["rssite", "pandoc"]);

        let mut cfg = MassGeneratorConfig {
            predict_command: "./plan.sh".to_string(),
            ..Default::default()
        };
        cfg.standard.command = "./build.sh".to_string();
        let proc = MassGeneratorProcessor::new(cfg).unwrap();
        assert_eq!(proc.required_tools(), vec!["./build.sh", "./plan.sh"]);
    }
}
