use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{OnceTool, require_planned_output};
use crate::config::{StandardConfig, output_config_hash, resolve_extra_inputs};
use crate::file_index::FileIndex;
use crate::graph::{BuildGraph, Product};
use crate::processor::{
    Processor, SiblingFilter, anchor_display_dir, check_command_output, run_in_anchor_dir,
};

fn default_cargo() -> String {
    "cargo".into()
}

fn default_cargo_profiles() -> Vec<String> {
    vec!["dev".into(), "release".into()]
}

/// Cargo config. Custom: cargo, profiles.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CargoConfig {
    #[serde(default = "default_cargo")]
    pub cargo: String,
    #[serde(default = "default_cargo_profiles")]
    pub profiles: Vec<String>,
    #[serde(flatten)]
    pub standard: StandardConfig,
}

impl Default for CargoConfig {
    fn default() -> Self {
        Self {
            cargo: "cargo".into(),
            profiles: default_cargo_profiles(),
            standard: StandardConfig::default(),
        }
    }
}

/// Map a Cargo `--profile` name to the subdirectory under `target/`.
fn profile_dir_name(profile: &str) -> &str {
    match profile {
        "dev" | "test" => "debug",
        "release" | "bench" => "release",
        other => other,
    }
}

/// Read and parse a Cargo manifest.
fn read_manifest(cargo_toml: &Path) -> Result<toml::Value> {
    let text = fs::read_to_string(cargo_toml)
        .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;
    toml::from_str(&text).with_context(|| format!("Failed to parse {}", cargo_toml.display()))
}

/// `path` with its `.` components dropped and each `..` folded into the
/// component before it, so that `crates/foo/../bar` and `./crates/bar`
/// both compare equal to `crates/bar`. Lexical only: nothing is looked up
/// on disk, which is right for the graph's project-relative paths.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Whether a `[workspace]` path entry is a glob pattern rather than a
/// literal path.
fn is_glob(entry: &str) -> bool {
    entry.contains(['*', '?', '['])
}

/// The directories a list of `[workspace]` path entries (`members`,
/// `default-members`, `exclude`) names, relative to the project root like
/// every graph path: each entry resolved against the workspace directory,
/// a glob (`crates/*`) expanded to the directories it matches. A literal
/// entry must be a directory; a glob match that is not one (a README next
/// to the crates) is passed over.
fn workspace_paths(ws_dir: &Path, entries: &[toml::Value], what: &str) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in entries.iter().filter_map(toml::Value::as_str) {
        let pattern = crate::processor::resolve_anchor_path(ws_dir, entry);
        if is_glob(entry) {
            let pattern_text = pattern.to_string_lossy();
            let matches = glob::glob(&pattern_text).with_context(|| {
                format!("[workspace] {what} entry '{entry}' is not a valid glob")
            })?;
            for found in matches {
                let found = crate::errors::ctx(
                    found,
                    &format!("Failed to expand [workspace] {what} entry '{entry}'"),
                )?;
                if found.is_dir() {
                    dirs.push(lexical_normalize(&found));
                }
            }
        } else {
            dirs.push(lexical_normalize(&pattern));
        }
    }
    dirs.sort();
    dirs.dedup();
    Ok(dirs)
}

/// The `[workspace] members` of the workspace rooted at `ws_toml`: the
/// member directories (globs expanded) minus `exclude` (an excluded
/// directory takes everything below it with it). Every member must hold a
/// `Cargo.toml`: cargo refuses such a workspace, and predicting nothing for
/// the member would silently drop its artifacts from the plan.
fn workspace_member_dirs(ws_toml: &Path, doc: &toml::Value) -> Result<Vec<PathBuf>> {
    let ws_dir = crate::processor::parent_dir_or_empty(ws_toml);
    let workspace = doc.get("workspace");
    let list = |key: &str| -> Vec<toml::Value> {
        workspace
            .and_then(|w| w.get(key))
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let excluded = workspace_paths(ws_dir, &list("exclude"), "exclude")?;
    let mut members = Vec::new();
    for dir in workspace_paths(ws_dir, &list("members"), "members")? {
        if excluded.iter().any(|ex| dir.starts_with(ex)) {
            continue;
        }
        if !dir.join("Cargo.toml").is_file() {
            anyhow::bail!(
                "{}: [workspace] member '{}' has no Cargo.toml",
                ws_toml.display(),
                dir.display()
            );
        }
        members.push(dir);
    }
    Ok(members)
}

/// What a plain `cargo build` in the workspace directory of `ws_toml`
/// builds: whether the root package is in, and which member directories
/// are. With `--workspace` (or `--all`) in `args` (`all_members`), every
/// member and the root; otherwise `default-members` when the manifest sets
/// it (`.` standing for the root); otherwise every member of a virtual
/// manifest, or the root package alone of a manifest that has one.
fn workspace_build_selection(
    ws_toml: &Path,
    doc: &toml::Value,
    all_members: bool,
) -> Result<(bool, Vec<PathBuf>)> {
    let ws_dir = lexical_normalize(crate::processor::parent_dir_or_empty(ws_toml));
    let members = workspace_member_dirs(ws_toml, doc)?;
    let has_root_package = doc.get("package").is_some();
    if all_members {
        return Ok((has_root_package, members));
    }
    if let Some(defaults) = doc
        .get("workspace")
        .and_then(|w| w.get("default-members"))
        .and_then(toml::Value::as_array)
    {
        let selected = workspace_paths(
            crate::processor::parent_dir_or_empty(ws_toml),
            defaults,
            "default-members",
        )?;
        let root_selected = has_root_package && selected.contains(&ws_dir);
        let member_dirs = members
            .into_iter()
            .filter(|m| selected.contains(m))
            .collect();
        return Ok((root_selected, member_dirs));
    }
    if has_root_package {
        Ok((true, Vec::new()))
    } else {
        Ok((false, members))
    }
}

/// The root manifest of the workspace `cargo_toml` is a member of, if any:
/// the one `[package] workspace` names, else the nearest ancestor manifest
/// (up to the project root) with a `[workspace]` — a member when that
/// workspace lists it, standalone when it is excluded, as cargo decides.
/// A workspace rooted above the project root is out of sight: paths are
/// project-relative, and such a layout is not one rsconstruct builds.
fn enclosing_workspace_root(cargo_toml: &Path, doc: &toml::Value) -> Result<Option<PathBuf>> {
    let crate_dir = lexical_normalize(crate::processor::parent_dir_or_empty(cargo_toml));
    if let Some(named) = doc
        .get("package")
        .and_then(|p| p.get("workspace"))
        .and_then(toml::Value::as_str)
    {
        let root_toml =
            lexical_normalize(&crate::processor::resolve_anchor_path(&crate_dir, named))
                .join("Cargo.toml");
        if !root_toml.is_file() {
            anyhow::bail!(
                "{}: [package] workspace = '{named}' but {} does not exist",
                cargo_toml.display(),
                root_toml.display()
            );
        }
        return Ok(Some(root_toml));
    }
    let mut dir = crate_dir.parent();
    while let Some(candidate_dir) = dir {
        let candidate = candidate_dir.join("Cargo.toml");
        if candidate.is_file() {
            let candidate_doc = read_manifest(&candidate)?;
            if candidate_doc.get("workspace").is_some() {
                let members = workspace_member_dirs(&candidate, &candidate_doc)?;
                return Ok(members.contains(&crate_dir).then_some(candidate));
            }
        }
        dir = candidate_dir.parent();
    }
    Ok(None)
}

/// The directory cargo keeps `target/` in for `cargo_toml`: the workspace
/// root's when the crate is a member, the crate's own otherwise. Cargo runs
/// without `--target-dir`, so this is where it writes.
fn target_root(cargo_toml: &Path, doc: &toml::Value) -> Result<PathBuf> {
    let root_toml = enclosing_workspace_root(cargo_toml, doc)?;
    let dir = root_toml.as_deref().map_or_else(
        || crate::processor::parent_dir_or_empty(cargo_toml),
        crate::processor::parent_dir_or_empty,
    );
    Ok(dir.to_path_buf())
}

/// Enumerate the top-level artifact filenames (`<bin>`, `lib<name>.rlib`,
/// `lib<name>.so`/`.dylib`, `lib<name>.a`) that `cargo build` run in the
/// directory of `cargo_toml` writes into the workspace's
/// `target/<profile_dir>/`: the manifest's own package when cargo's
/// selection includes it, and the workspace members it builds
/// ([`workspace_build_selection`]).
fn planned_cargo_artifacts(
    cargo_toml: &Path,
    doc: &toml::Value,
    all_members: bool,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let (root_selected, member_dirs) = workspace_build_selection(cargo_toml, doc, all_members)?;
    if root_selected {
        collect_package_artifacts(cargo_toml, doc, &mut out)?;
    }
    for member_dir in member_dirs {
        let member_toml = member_dir.join("Cargo.toml");
        let member_doc = read_manifest(&member_toml)?;
        collect_package_artifacts(&member_toml, &member_doc, &mut out)?;
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The artifacts of the `[package]` in `doc` (none for a virtual manifest).
fn collect_package_artifacts(
    cargo_toml: &Path,
    doc: &toml::Value,
    out: &mut Vec<String>,
) -> Result<()> {
    let crate_dir = crate::processor::parent_dir_or_empty(cargo_toml);

    if let Some(pkg) = doc.get("package").and_then(toml::Value::as_table) {
        let pkg_name = pkg
            .get("name")
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{} [package] is missing `name`", cargo_toml.display()))?;
        let autolib = pkg
            .get("autolib")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);
        let autobins = pkg
            .get("autobins")
            .and_then(toml::Value::as_bool)
            .unwrap_or(true);

        let lib_table = doc.get("lib").and_then(toml::Value::as_table);
        let has_lib = lib_table.is_some()
            || (autolib
                && crate::processor::resolve_anchor_path(crate_dir, "src/lib.rs").is_file());
        if has_lib {
            let raw_name = lib_table
                .and_then(|t| t.get("name"))
                .and_then(toml::Value::as_str)
                .unwrap_or(pkg_name);
            let lib_name = raw_name.replace('-', "_");
            let proc_macro = lib_table
                .and_then(|t| t.get("proc-macro").or_else(|| t.get("proc_macro")))
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);
            let crate_types: Vec<&str> = lib_table
                .and_then(|t| t.get("crate-type").or_else(|| t.get("crate_type")))
                .and_then(toml::Value::as_array)
                .map_or_else(
                    || {
                        if proc_macro {
                            vec!["proc-macro"]
                        } else {
                            vec!["rlib"]
                        }
                    },
                    |arr| arr.iter().filter_map(toml::Value::as_str).collect(),
                );
            for ct in crate_types {
                match ct {
                    "lib" | "rlib" => out.push(format!("lib{lib_name}.rlib")),
                    "staticlib" => out.push(format!("lib{lib_name}.a")),
                    "dylib" | "cdylib" | "proc-macro" => out.push(format!(
                        "{}{lib_name}{}",
                        std::env::consts::DLL_PREFIX,
                        std::env::consts::DLL_SUFFIX
                    )),
                    _ => {}
                }
            }
        }

        let mut bin_names = Vec::new();
        let mut main_rs_claimed = false;
        if let Some(bins) = doc.get("bin").and_then(toml::Value::as_array) {
            for bin in bins {
                if let Some(name) = bin.get("name").and_then(toml::Value::as_str) {
                    bin_names.push(name.to_string());
                }
                if bin
                    .get("path")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|p| p == "src/main.rs")
                {
                    main_rs_claimed = true;
                }
            }
        }
        if autobins {
            if !main_rs_claimed
                && !bin_names.iter().any(|n| n == pkg_name)
                && crate::processor::resolve_anchor_path(crate_dir, "src/main.rs").is_file()
            {
                bin_names.push(pkg_name.to_string());
            }
            let src_bin = crate::processor::resolve_anchor_path(crate_dir, "src/bin");
            if src_bin.is_dir() {
                let entries = fs::read_dir(&src_bin)
                    .with_context(|| format!("Failed to read {}", src_bin.display()))?;
                for entry in entries {
                    let entry =
                        entry.with_context(|| format!("Failed to read {}", src_bin.display()))?;
                    let path = entry.path();
                    if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("rs") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                            && !bin_names.iter().any(|n| n == stem)
                        {
                            bin_names.push(stem.to_string());
                        }
                    } else if path.is_dir()
                        && path.join("main.rs").is_file()
                        && let Some(stem) = path.file_name().and_then(|s| s.to_str())
                        && !bin_names.iter().any(|n| n == stem)
                    {
                        bin_names.push(stem.to_string());
                    }
                }
            }
        }
        out.extend(bin_names);
    }

    Ok(())
}

pub struct CargoProcessor {
    config: CargoConfig,
    runs: Mutex<BTreeMap<(PathBuf, String), Arc<OnceTool>>>,
}

impl CargoProcessor {
    pub const fn new(config: CargoConfig) -> Self {
        Self {
            config,
            runs: Mutex::new(BTreeMap::new()),
        }
    }

    fn run_guard(&self, cargo_toml: &Path, profile: &str) -> Arc<OnceTool> {
        let mut guard = self.runs.lock();
        Arc::clone(
            guard
                .entry((cargo_toml.to_path_buf(), profile.to_string()))
                .or_insert_with(|| Arc::new(OnceTool::new())),
        )
    }

    /// Whether `args` select the whole workspace (`--workspace`, or its old
    /// spelling `--all`).
    fn builds_all_members(&self) -> bool {
        self.config
            .standard
            .args
            .iter()
            .any(|arg| arg == "--workspace" || arg == "--all")
    }

    /// The files one `cargo <command> --profile <profile>` run in the
    /// directory of `cargo_toml` produces, as graph paths, and whether they
    /// are stamps rsconstruct writes itself. For `build`: the artifacts of
    /// the crate (and of the workspace members a root builds) under the
    /// workspace's `target/<profile_dir>/` — the workspace root's for a
    /// member crate, since cargo runs without `--target-dir`. For a command
    /// that produces nothing (`check`, `clippy`) or a package with no
    /// artifact: one stamp under the crate's own `target/<profile_dir>/`,
    /// so every crate of a workspace keeps a stamp of its own.
    fn planned_outputs(&self, cargo_toml: &Path, profile: &str) -> Result<(Vec<PathBuf>, bool)> {
        let subcommand = self
            .config
            .standard
            .require_command(crate::processor::names::CARGO)?;
        let doc = read_manifest(cargo_toml)?;
        let profile_dir = profile_dir_name(profile);
        if subcommand == "build" {
            let artifacts = planned_cargo_artifacts(cargo_toml, &doc, self.builds_all_members())?;
            if !artifacts.is_empty() {
                let target_dir = target_root(cargo_toml, &doc)?
                    .join("target")
                    .join(profile_dir);
                return Ok((
                    artifacts.iter().map(|a| target_dir.join(a)).collect(),
                    false,
                ));
            }
        }
        let own_target_dir = crate::processor::parent_dir_or_empty(cargo_toml)
            .join("target")
            .join(profile_dir);
        Ok((
            vec![own_target_dir.join(format!(".rsconstruct-{subcommand}.stamp"))],
            true,
        ))
    }

    /// Run cargo build in the Cargo.toml's directory with the given profile
    fn execute_cargo(
        &self,
        ctx: &crate::build_context::BuildContext,
        cargo_toml: &Path,
        profile: &str,
    ) -> Result<()> {
        let subcommand = self
            .config
            .standard
            .require_command(crate::processor::names::CARGO)?;
        let (planned, is_stamp) = self.planned_outputs(cargo_toml, profile)?;
        for path in &planned {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(anyhow::Error::from(e)
                        .context(format!("Failed to remove stale output {}", path.display())));
                }
            }
        }

        let mut cmd = Command::new(&self.config.cargo);
        cmd.arg(subcommand);
        cmd.args(["--profile", profile]);
        for arg in &self.config.standard.args {
            cmd.arg(arg);
        }
        let output = run_in_anchor_dir(ctx, &mut cmd, cargo_toml)?;
        check_command_output(
            &output,
            format_args!(
                "cargo {} --profile {} in {}",
                subcommand,
                profile,
                anchor_display_dir(cargo_toml)
            ),
        )?;
        if is_stamp {
            for path in &planned {
                crate::processor::ensure_output_dir(path)?;
                fs::write(path, format!("{subcommand} {profile}\n"))
                    .with_context(|| format!("Failed to write {}", path.display()))?;
            }
        }
        Ok(())
    }
}

impl Processor for CargoProcessor {
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

    fn required_tools(&self) -> Vec<String> {
        crate::processor::with_declared_tools(
            vec![self.config.cargo.clone()],
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

        let siblings = SiblingFilter {
            extensions: &[".rs", ".toml", "Cargo.lock"],
            excludes: &["/.git/", "/target/", "/.rsconstruct/"],
        };
        let config_hash = output_config_hash(
            &self.config,
            &crate::config::checksum_fields_of(instance_name),
        );
        let extra = resolve_extra_inputs(&self.config.standard.dep_inputs)?;
        let scanned: std::collections::BTreeSet<&PathBuf> = files.iter().collect();

        for anchor in &files {
            let anchor_dir = crate::processor::parent_dir_or_empty(anchor);

            // A member crate whose workspace root is scanned too, and whose
            // root `cargo build` builds it, is the root's business: the root
            // already plans its artifacts (under the one shared `target/`),
            // and a second run from the member directory would race it.
            let doc = read_manifest(anchor)?;
            if let Some(root_toml) = enclosing_workspace_root(anchor, &doc)?
                && scanned.contains(&root_toml)
            {
                let root_doc = read_manifest(&root_toml)?;
                let (_, built) =
                    workspace_build_selection(&root_toml, &root_doc, self.builds_all_members())?;
                if built.contains(&lexical_normalize(anchor_dir)) {
                    continue;
                }
            }

            let sibling_files = file_index.query(
                anchor_dir,
                siblings.extensions,
                siblings.excludes,
                &[],
                &[],
                &[],
            );

            let base_inputs = crate::processor::build_anchor_inputs(anchor, &sibling_files, &extra);

            for profile in &self.config.profiles {
                let (outputs, _) = self.planned_outputs(anchor, profile)?;
                for output_path in outputs {
                    let hash = crate::checksum::hash_parts(&[
                        &config_hash,
                        profile,
                        &output_path.display().to_string(),
                    ]);
                    graph.add_product_with(
                        base_inputs.clone(),
                        vec![output_path],
                        instance_name,
                        Some(hash),
                        Some(profile),
                        Vec::new(),
                    )?;
                }
            }
        }

        Ok(())
    }

    fn execute(&self, ctx: &crate::build_context::BuildContext, product: &Product) -> Result<()> {
        let profile = product.variant.as_deref().unwrap_or("dev");
        let cargo_toml = product.primary_input();
        let guard = self.run_guard(cargo_toml, profile);
        guard.run(&self.config.cargo, || {
            self.execute_cargo(ctx, cargo_toml, profile)
        })?;
        require_planned_output(product, &self.config.cargo)
    }
}

fn plugin_create(toml: &toml::Value) -> anyhow::Result<Box<dyn crate::processor::Processor>> {
    crate::registries::deserialize_and_create(toml, |cfg| Box::new(CargoProcessor::new(cfg)))
}
inventory::submit! {
    crate::registries::ProcessorPlugin {
        version: 2,
        name: "cargo",
        processor_type: crate::processor::ProcessorType::MassGenerator,
        create: plugin_create,
        fields: &[
            crate::config::FieldSpec { name: "cargo", ty: crate::config::FieldType::String,
                affects_output: true, required: false,
                doc: "Path to the cargo executable" },
            crate::config::FieldSpec { name: "profiles", ty: crate::config::FieldType::StringArray,
                affects_output: true, required: false,
                doc: "Build profiles to run (e.g. dev, release)" },
        ],
        omit_standard_fields: &["formats", "dep_auto", "output_dir", "batch"],
        scan_defaults: Some(crate::config::ScanDefaultsData { src_dirs: &[], src_extensions: &["Cargo.toml"], src_exclude_dirs: &[] }),
        defaults: Some(crate::config::ProcessorDefaults { command: "build", ..crate::config::ProcessorDefaults::EMPTY }),
        defconfig_json: crate::registries::default_config_json::<CargoConfig>,
        keywords: &["rust", "builder", "cargo", "rs", "package-manager"],
        description: "Build Rust projects using Cargo",
        is_builtin: false,
        is_rust: true,
        supports_batch: false,
        caches_outputs: true,
        max_jobs_cap: Some(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A package manifest with a `src/lib.rs` in `dir`.
    fn write_lib_crate(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
    }

    /// `tmp/ws`: a workspace whose members are `crates/*` minus
    /// `crates/skipped`, with crates `foo`, `bar`, `skipped` and a README
    /// under `crates/`, plus `tools/standalone` outside the workspace.
    fn write_workspace(tmp: &Path, root_manifest: &str) -> PathBuf {
        let ws = tmp.join("ws");
        std::fs::create_dir_all(ws.join("crates")).unwrap();
        std::fs::write(ws.join("Cargo.toml"), root_manifest).unwrap();
        std::fs::write(ws.join("crates/README.md"), "not a crate\n").unwrap();
        write_lib_crate(&ws.join("crates/foo"), "foo");
        write_lib_crate(&ws.join("crates/bar"), "bar");
        write_lib_crate(&ws.join("crates/skipped"), "skipped");
        write_lib_crate(&ws.join("tools/standalone"), "standalone");
        ws
    }

    const VIRTUAL_ROOT: &str =
        "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/skipped\"]\n";

    /// A `cargo build` processor with `args`. The `build` command is the
    /// plugin default the registry applies; `CargoConfig::default()` alone
    /// has none.
    fn build_processor(args: &[&str]) -> CargoProcessor {
        let mut cfg = CargoConfig::default();
        cfg.standard.command = "build".to_string();
        cfg.standard.args = args.iter().map(|a| (*a).to_string()).collect();
        CargoProcessor::new(cfg)
    }

    #[test]
    fn workspace_members_expand_globs_and_honour_exclude() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ws = write_workspace(tmp.path(), VIRTUAL_ROOT);
        let root_toml = ws.join("Cargo.toml");
        let members =
            workspace_member_dirs(&root_toml, &read_manifest(&root_toml).unwrap()).unwrap();
        assert_eq!(members, vec![ws.join("crates/bar"), ws.join("crates/foo")]);

        // A literal member without a manifest is an error, not a silent gap.
        std::fs::write(
            &root_toml,
            "[workspace]\nmembers = [\"crates/foo\", \"crates/missing\"]\n",
        )
        .unwrap();
        let err =
            workspace_member_dirs(&root_toml, &read_manifest(&root_toml).unwrap()).unwrap_err();
        assert!(
            format!("{err:#}").contains("member") && format!("{err:#}").contains("crates/missing"),
            "{err:#}"
        );
    }

    #[test]
    fn member_crates_find_their_workspace_root_and_its_target_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ws = write_workspace(tmp.path(), VIRTUAL_ROOT);
        let root_toml = ws.join("Cargo.toml");

        let foo = ws.join("crates/foo/Cargo.toml");
        let foo_doc = read_manifest(&foo).unwrap();
        assert_eq!(
            enclosing_workspace_root(&foo, &foo_doc).unwrap(),
            Some(root_toml.clone())
        );
        assert_eq!(target_root(&foo, &foo_doc).unwrap(), ws);

        // Excluded by the nearest workspace: standalone, its own target dir.
        let skipped = ws.join("crates/skipped/Cargo.toml");
        let skipped_doc = read_manifest(&skipped).unwrap();
        assert_eq!(
            enclosing_workspace_root(&skipped, &skipped_doc).unwrap(),
            None
        );
        assert_eq!(
            target_root(&skipped, &skipped_doc).unwrap(),
            ws.join("crates/skipped")
        );

        // `[package] workspace` names the root outright.
        let standalone = ws.join("tools/standalone/Cargo.toml");
        let mut text = std::fs::read_to_string(&standalone).unwrap();
        text.push_str("workspace = \"../..\"\n");
        std::fs::write(&standalone, text).unwrap();
        let standalone_doc = read_manifest(&standalone).unwrap();
        assert_eq!(
            enclosing_workspace_root(&standalone, &standalone_doc).unwrap(),
            Some(root_toml)
        );

        // Planned outputs of a member land under the workspace's target dir.
        let processor = build_processor(&[]);
        let (outputs, is_stamp) = processor.planned_outputs(&foo, "dev").unwrap();
        assert!(!is_stamp);
        assert_eq!(outputs, vec![ws.join("target/debug/libfoo.rlib")]);
    }

    /// What a root `cargo build` builds: every member of a virtual
    /// workspace, only the root package of a root-package workspace (all
    /// members with `--workspace`), and `default-members` when set.
    #[test]
    fn root_build_predicts_what_cargo_selects() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ws = write_workspace(tmp.path(), VIRTUAL_ROOT);
        let root_toml = ws.join("Cargo.toml");
        let artifacts = |processor: &CargoProcessor| -> Vec<String> {
            let (outputs, is_stamp) = processor.planned_outputs(&root_toml, "dev").unwrap();
            assert!(!is_stamp);
            outputs
                .iter()
                .map(|p| {
                    assert_eq!(p.parent().unwrap(), ws.join("target/debug"));
                    p.file_name().unwrap().to_string_lossy().into_owned()
                })
                .collect()
        };
        let plain = build_processor(&[]);
        assert_eq!(artifacts(&plain), vec!["libbar.rlib", "libfoo.rlib"]);

        // A root package: alone by default, with every member on --workspace.
        std::fs::write(
            &root_toml,
            "[package]\nname = \"root\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/skipped\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/main.rs"), "fn main() {}\n").unwrap();
        assert_eq!(artifacts(&plain), vec!["root"]);
        let whole = build_processor(&["--workspace"]);
        assert_eq!(
            artifacts(&whole),
            vec!["libbar.rlib", "libfoo.rlib", "root"]
        );

        // default-members narrows the plain build; `.` would keep the root.
        std::fs::write(
            &root_toml,
            "[package]\nname = \"root\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/skipped\"]\ndefault-members = [\"crates/foo\"]\n",
        )
        .unwrap();
        assert_eq!(artifacts(&plain), vec!["libfoo.rlib"]);
    }

    #[test]
    fn lexical_normalize_folds_dot_and_dotdot() {
        assert_eq!(
            lexical_normalize(Path::new("./crates/foo")),
            PathBuf::from("crates/foo")
        );
        assert_eq!(
            lexical_normalize(Path::new("crates/foo/../bar")),
            PathBuf::from("crates/bar")
        );
        assert_eq!(
            lexical_normalize(Path::new("crates/foo/../..")),
            PathBuf::from("")
        );
    }
}
