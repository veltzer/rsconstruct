//! The sandbox execution model (`[build] execution_model = "sandbox"`).
//!
//! A correctness check, not a security boundary: see
//! docs/src/internal/sandbox.md. Each external tool a product runs is
//! wrapped in bubblewrap with the whole filesystem read-only except `$HOME`
//! and `/tmp` (writable, as natively), the network unshared unless the
//! processor sets `sandbox_network`, and an overlayfs on the project root.
//! Every write the tool makes inside the project lands in the overlay's
//! upper directory instead of the real tree. When the tool exits:
//!
//! - on success, the upper directory is inspected: changes to the product's
//!   declared outputs and output directories are applied to the real tree,
//!   and any other change fails the product, naming each path;
//! - on failure, everything the tool wrote is discarded, so a failed product
//!   leaves no partial outputs behind.
//!
//! The executor records which product is running in a thread-local
//! ([`set_spec`]); `processor::run_command_inner` asks [`prepare`] to wrap
//! the command and calls [`Run::finish`] afterwards. Commands run outside a
//! product (discovery planners, version probes, `tool install`) see no spec
//! and run natively.

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::config::{ExecutionModel, StandardConfig};

/// The executable the sandbox runs tools under.
pub const SANDBOX_TOOL: &str = "bwrap";

/// Where each sandboxed run keeps its overlay directories, relative to the
/// project root. Inside the project so that moving an output from the
/// upper directory into the tree is a rename, not a copy.
const SANDBOX_DIR: &str = ".rsconstruct/sandbox";

/// The xattr overlayfs sets on an upper directory that hides the lower
/// directory's contents: the tool removed the directory and created a new
/// one in its place. bwrap mounts the overlay with `userxattr`, so the
/// attribute lives in the `user.` namespace.
const OPAQUE_XATTR: &str = "user.overlay.opaque";

/// What the running product may change in the project tree.
#[derive(Clone, Debug)]
pub struct Spec {
    /// The processor instance, for messages.
    pub label: String,
    /// Declared output files, relative to the project root.
    pub outputs: Vec<PathBuf>,
    /// Declared output directories: everything beneath them is the product's.
    pub output_dirs: Vec<PathBuf>,
    /// Whether the tool may reach the network.
    pub network: bool,
    /// Where the tools may write their own state: the `sandbox_writable`
    /// entries of every tool the processor runs plus its own config, as
    /// written (parsed by [`Writable::parse`] in [`prepare`]).
    pub writable: Vec<String>,
}

/// A place a sandboxed tool may write its own state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Writable {
    /// A path relative to the project root: it and everything beneath it.
    /// When it already exists it is bind-mounted over the overlay, so the
    /// tool writes the real tree directly (no copy-up of a large `target`);
    /// when it does not, what the tool creates there is moved into the tree
    /// after a successful run like an output.
    Path(PathBuf),
    /// `**/NAME`: every directory called NAME, anywhere in the tree
    /// (`**/__pycache__`). What the tool writes beneath one is moved into
    /// the tree after a successful run.
    Named(String),
}

impl Writable {
    /// Parse one `sandbox_writable` entry: a relative path without `..` or
    /// wildcards, or `**/NAME` with NAME a single wildcard-free component.
    pub fn parse(entry: &str) -> std::result::Result<Self, String> {
        let bad = |why: &str| Err(format!("sandbox_writable entry '{entry}': {why}"));
        if let Some(name) = entry.strip_prefix("**/") {
            if name.is_empty() || name.contains('/') || name.contains('*') {
                return bad("after '**/' must come one directory name, without '/' or '*'");
            }
            return Ok(Self::Named(name.to_string()));
        }
        if entry.is_empty() {
            return bad("empty");
        }
        if entry.contains('*') {
            return bad("wildcards are only allowed as a leading '**/'");
        }
        let path = Path::new(entry);
        if path.is_absolute() {
            return bad("must be relative to the project root");
        }
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return bad("must not contain '..'");
        }
        let clean: PathBuf = path
            .components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .collect();
        if clean.as_os_str().is_empty() {
            return bad("names the project root itself");
        }
        Ok(Self::Path(clean))
    }

    /// Whether `path` (relative to the project root) lies in this place.
    fn covers(&self, path: &Path) -> bool {
        match self {
            Self::Path(p) => path.starts_with(p),
            Self::Named(name) => path.components().any(|c| c.as_os_str() == name.as_str()),
        }
    }
}

thread_local! {
    static SPEC: RefCell<Option<Spec>> = const { RefCell::new(None) };
}

/// Record the product (or batch) about to run on this thread, or clear it
/// with `None`. Set and cleared by the executor around `execute` /
/// `execute_batch`, next to the declared-tools record.
pub fn set_spec(spec: Option<Spec>) {
    SPEC.with(|s| *s.borrow_mut() = spec);
}

/// Add `dir` to the output directories of the product running on this
/// thread, if it runs sandboxed. For the private directories rsconstruct
/// itself gives a tool to write into during a product — a mass
/// generator's staging and doctrees directories under `.rsconstruct/` —
/// whose contents rsconstruct checks against its own plan afterwards. A
/// no-op natively and outside a product.
pub fn declare_output_dir(dir: &Path) {
    SPEC.with(|s| {
        if let Some(spec) = s.borrow_mut().as_mut() {
            spec.output_dirs.push(dir.to_path_buf());
        }
    });
}

fn current_spec() -> Option<Spec> {
    SPEC.with(|s| s.borrow().clone())
}

/// The enabled instances among `instances` that would run sandboxed under
/// `build_default` although their processor is not `sandboxable`, sorted.
/// Read from the raw instance tables, before any processor is created, so a
/// build can refuse them before doing any work.
pub fn unsandboxable_instances(
    instances: &[crate::config::ProcessorInstance],
    build_default: ExecutionModel,
) -> Vec<String> {
    let mut out: Vec<String> = instances
        .iter()
        .filter(|inst| {
            let table = &inst.config_toml;
            let enabled = table
                .get("enabled")
                .and_then(toml::Value::as_bool)
                .unwrap_or(true);
            let model = table
                .get(crate::config::EXECUTION_MODEL)
                .and_then(|v| v.clone().try_into::<ExecutionModel>().ok())
                .unwrap_or(build_default);
            enabled
                && model == ExecutionModel::Sandbox
                && crate::registries::find_plugin(&inst.pname).is_some_and(|p| !p.sandboxable)
        })
        .map(|inst| inst.instance_name.clone())
        .collect();
    out.sort();
    out
}

/// Fail when any configured, enabled instance would run sandboxed although
/// its processor cannot run in a sandbox. Called at the start of a build.
pub fn check_sandboxable(
    instances: &[crate::config::ProcessorInstance],
    build_default: ExecutionModel,
) -> Result<()> {
    let bad = unsandboxable_instances(instances, build_default);
    if bad.is_empty() {
        return Ok(());
    }
    let msg = format!(
        "These processors would run under execution_model = \"sandbox\" but cannot run in a sandbox:\n  {}\nSet execution_model = \"native\" on each of them, or disable them.",
        bad.join("\n  ")
    );
    Err(crate::exit_code::RsconstructError::new(
        crate::exit_code::RsconstructExitCode::ConfigError,
        msg,
    )
    .into())
}

/// Whether a processor with this config runs sandboxed under the build-wide
/// `build_default`.
pub fn is_sandboxed(standard: &StandardConfig, build_default: ExecutionModel) -> bool {
    standard.execution_model.unwrap_or(build_default) == ExecutionModel::Sandbox
}

/// The `sandbox_writable` entries a processor running `tools` gets: what
/// each tool declares (`ToolInfo::sandbox_writable`), then the processor's
/// own config, without duplicates.
pub fn writable_for(standard: &StandardConfig, tools: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let declared = tools
        .iter()
        .filter_map(|t| crate::tools::tool_info(t))
        .flat_map(|info| info.sandbox_writable.iter().map(|s| (*s).to_string()));
    for entry in declared.chain(standard.sandbox_writable.iter().cloned()) {
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

/// Whether a processor running `tools` gets the network: when any of them
/// declares that it needs it (`ToolInfo::needs_network`), or when its
/// config says so (`sandbox_network`, for a script whose tool is unknown).
pub fn network_for(standard: &StandardConfig, tools: &[String]) -> bool {
    standard.sandbox_network
        || tools
            .iter()
            .filter_map(|t| crate::tools::tool_info(t))
            .any(|info| info.needs_network)
}

/// The spec for running `products` (one, or a batch) of a processor whose
/// config is `standard` and which runs `tools`, or `None` when the
/// processor runs natively.
pub fn spec_for(
    standard: &StandardConfig,
    build_default: ExecutionModel,
    products: &[&crate::graph::Product],
    tools: &[String],
) -> Option<Spec> {
    if !is_sandboxed(standard, build_default) {
        return None;
    }
    let label = products
        .first()
        .map(|p| p.processor.clone())
        .unwrap_or_default();
    let outputs = products
        .iter()
        .flat_map(|p| p.outputs.iter().cloned())
        .collect();
    let output_dirs = products
        .iter()
        .flat_map(|p| p.output_dirs.iter().map(|d| d.as_ref().clone()))
        .collect();
    Some(Spec {
        label,
        outputs,
        output_dirs,
        network: network_for(standard, tools),
        writable: writable_for(standard, tools),
    })
}

/// A command wrapped for one sandboxed run, plus what is needed to settle
/// the run afterwards.
pub struct Run {
    /// The bwrap invocation to execute instead of the original command.
    pub command: Command,
    root: PathBuf,
    dir: PathBuf,
    upper: PathBuf,
    spec: Spec,
    /// `spec.writable`, parsed.
    writable: Vec<Writable>,
}

/// Wrap `cmd` for the product running on this thread, or return `None`
/// when no product is running or its processor runs natively.
pub fn prepare(cmd: &Command) -> Result<Option<Run>> {
    let Some(mut spec) = current_spec() else {
        return Ok(None);
    };
    let root =
        std::env::current_dir().context("Failed to determine the project root for the sandbox")?;
    spec.outputs = spec.outputs.iter().map(|p| relative_to(&root, p)).collect();
    spec.output_dirs = spec
        .output_dirs
        .iter()
        .map(|p| relative_to(&root, p))
        .collect();
    let writable = spec
        .writable
        .iter()
        .map(|entry| Writable::parse(entry).map_err(|e| anyhow::anyhow!("[{}] {e}", spec.label)))
        .collect::<Result<Vec<_>>>()?;

    static NEXT_RUN: AtomicU64 = AtomicU64::new(0);
    let dir = root.join(SANDBOX_DIR).join(format!(
        "{}-{}",
        std::process::id(),
        NEXT_RUN.fetch_add(1, Ordering::Relaxed)
    ));
    let upper = dir.join("upper");
    let work = dir.join("work");
    for d in [&upper, &work] {
        fs::create_dir_all(d)
            .with_context(|| format!("Failed to create sandbox directory {}", d.display()))?;
    }

    let mut command = Command::new(SANDBOX_TOOL);
    command.args(["--die-with-parent", "--ro-bind", "/", "/"]);
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        command.arg("--bind").arg(&home).arg(&home);
    }
    command.args([
        "--bind",
        "/tmp",
        "/tmp",
        "--dev-bind",
        "/dev",
        "/dev",
        "--proc",
        "/proc",
    ]);
    if !spec.network {
        command.arg("--unshare-net");
    }
    command
        .arg("--overlay-src")
        .arg(&root)
        .arg("--overlay")
        .arg(&upper)
        .arg(&work)
        .arg(&root);
    // A state path that already exists goes on top of the overlay, straight
    // to the real tree. One that does not is left to the overlay: creating
    // it here would leave an empty directory in the tree (an empty `.git`
    // turns a directory into a broken repository), and what the tool
    // creates there is moved in after the run like an output.
    for place in &writable {
        if let Writable::Path(rel) = place {
            let path = root.join(rel);
            if fs::symlink_metadata(&path).is_ok() {
                command.arg("--bind").arg(&path).arg(&path);
            }
        }
    }
    // `join` keeps an absolute current_dir as is and anchors a relative one
    // at the root, which is where a native run would resolve it too.
    let cwd = cmd
        .get_current_dir()
        .map_or_else(|| root.clone(), |d| root.join(d));
    command.arg("--chdir").arg(&cwd);
    command
        .arg("--")
        .arg(cmd.get_program())
        .args(cmd.get_args());
    for (key, value) in cmd.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    Ok(Some(Run {
        command,
        root,
        dir,
        upper,
        spec,
        writable,
    }))
}

/// A path relative to the project root, as declared outputs are compared.
fn relative_to(root: &Path, path: &Path) -> PathBuf {
    let path = path.strip_prefix(root).unwrap_or(path);
    path.strip_prefix(".").unwrap_or(path).to_path_buf()
}

/// One change a tool made to the project tree, as its overlay recorded it.
#[derive(Debug, PartialEq, Eq)]
enum Change {
    /// A file or symlink was created or modified.
    Written(PathBuf),
    /// A file or directory of the real tree was removed.
    Removed(PathBuf),
    /// A directory of the real tree was removed and created anew.
    Replaced(PathBuf),
}

impl Change {
    fn path(&self) -> &Path {
        match self {
            Self::Written(p) | Self::Removed(p) | Self::Replaced(p) => p,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::Written(p) => format!("wrote    {}", p.display()),
            Self::Removed(p) => format!("removed  {}", p.display()),
            Self::Replaced(p) => format!("replaced {}", p.display()),
        }
    }
}

impl Run {
    /// Settle the run: on success apply the declared changes (or fail on
    /// undeclared ones), on failure discard everything; then remove the
    /// run's directory.
    pub fn finish(self, tool_succeeded: bool) -> Result<()> {
        let applied = if tool_succeeded { self.apply() } else { Ok(()) };
        let cleaned = remove_run_dir(&self.dir);
        applied?;
        cleaned
    }

    fn declares(&self, path: &Path) -> bool {
        self.spec.outputs.iter().any(|o| o == path)
            || self.spec.output_dirs.iter().any(|d| path.starts_with(d))
            || self.writable.iter().any(|w| w.covers(path))
    }

    /// What `change` touched that the product does not declare, described.
    /// A removed or replaced directory touched every file the real tree had
    /// beneath it, so each of those is checked rather than the directory's
    /// own path: a tool that runs `rm -rf docx` and writes `docx/` anew
    /// changes nothing undeclared when every file `docx` held is a declared
    /// output. The new contents of a replaced directory are separate
    /// `Written` changes.
    fn undeclared_in(&self, change: &Change) -> Result<Vec<String>> {
        let path = change.path();
        if self.declares(path) {
            return Ok(Vec::new());
        }
        let (Change::Removed(_) | Change::Replaced(_)) = change else {
            return Ok(vec![change.describe()]);
        };
        let real = self.root.join(path);
        if !fs::symlink_metadata(&real).is_ok_and(|m| m.is_dir()) {
            return Ok(vec![change.describe()]);
        }
        let mut files = Vec::new();
        real_files_under(&self.root, path, &mut files)?;
        Ok(files
            .into_iter()
            .filter(|f| !self.declares(f))
            .map(|f| Change::Removed(f).describe())
            .collect())
    }

    fn apply(&self) -> Result<()> {
        let mut changes = Vec::new();
        collect_changes(&self.upper, Path::new(""), &self.root, &mut changes)?;
        let mut undeclared: Vec<String> = Vec::new();
        for change in &changes {
            undeclared.extend(self.undeclared_in(change)?);
        }
        if !undeclared.is_empty() {
            anyhow::bail!(
                "[{}] the tool changed paths its product does not declare (execution_model = \"sandbox\"):\n  {}\n\
                 Nothing it wrote was kept. Declare these paths as outputs, stop the tool from writing them, \
                 or set execution_model = \"native\" on this processor.",
                self.spec.label,
                undeclared.join("\n  ")
            );
        }
        // Removals first, so a replaced directory is emptied before the new
        // contents are moved in.
        for change in &changes {
            if let Change::Removed(p) | Change::Replaced(p) = change {
                remove_real(&self.root.join(p))?;
            }
        }
        for change in &changes {
            if let Change::Written(p) = change {
                let from = self.upper.join(p);
                let to = self.root.join(p);
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("Failed to create directory {}", parent.display())
                    })?;
                }
                fs::rename(&from, &to).with_context(|| {
                    format!(
                        "Failed to move sandbox output {} to {}",
                        from.display(),
                        to.display()
                    )
                })?;
            }
        }
        Ok(())
    }
}

/// Walk the upper directory below `rel` and record every change it holds.
/// Directories themselves are not changes: overlayfs copies a directory up
/// whenever something inside it changes, and an empty new directory is
/// left alone. A directory marked opaque is a change only when the real
/// tree has something there to replace.
fn collect_changes(upper: &Path, rel: &Path, root: &Path, out: &mut Vec<Change>) -> Result<()> {
    let dir = upper.join(rel);
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .with_context(|| format!("Failed to read sandbox directory {}", dir.display()))?
        .collect::<std::io::Result<_>>()
        .with_context(|| format!("Failed to read sandbox directory {}", dir.display()))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let child = rel.join(entry.file_name());
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)
            .with_context(|| format!("Failed to stat {}", path.display()))?;
        if crate::platform::is_overlay_whiteout(&meta) {
            out.push(Change::Removed(child));
        } else if meta.is_dir() {
            let opaque = crate::platform::get_xattr(&path, OPAQUE_XATTR)
                .with_context(|| format!("Failed to read xattrs of {}", path.display()))?
                .is_some_and(|v| v == b"y");
            if opaque && fs::symlink_metadata(root.join(&child)).is_ok() {
                out.push(Change::Replaced(child.clone()));
            }
            collect_changes(upper, &child, root, out)?;
        } else {
            out.push(Change::Written(child));
        }
    }
    Ok(())
}

/// Every file and symlink of the real tree beneath directory `rel`
/// (relative to `root`), as root-relative paths, sorted. Directories
/// themselves are not listed: removing an empty one changes no file.
fn real_files_under(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let dir = root.join(rel);
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .with_context(|| format!("Failed to read directory {}", dir.display()))?
        .collect::<std::io::Result<_>>()
        .with_context(|| format!("Failed to read directory {}", dir.display()))?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let child = rel.join(entry.file_name());
        let meta = fs::symlink_metadata(entry.path())
            .with_context(|| format!("Failed to stat {}", entry.path().display()))?;
        if meta.is_dir() {
            real_files_under(root, &child, out)?;
        } else {
            out.push(child);
        }
    }
    Ok(())
}

/// Remove `path` from the real tree, whatever it is; absent is fine.
fn remove_real(path: &Path) -> Result<()> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .with_context(|| format!("Failed to remove {}", path.display()))
}

/// Remove a run's directory. overlayfs leaves `work/work` with mode 000,
/// which must be opened up before it can be deleted.
fn remove_run_dir(dir: &Path) -> Result<()> {
    let inner = dir.join("work").join("work");
    if inner.exists() {
        crate::platform::set_permissions_mode(&inner, 0o700)
            .with_context(|| format!("Failed to open up {}", inner.display()))?;
    }
    fs::remove_dir_all(dir)
        .with_context(|| format!("Failed to remove sandbox directory {}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(
        instance_name: &str,
        pname: &str,
        toml_src: &str,
    ) -> crate::config::ProcessorInstance {
        crate::config::ProcessorInstance {
            instance_name: instance_name.into(),
            pname: pname.into(),
            config_toml: toml::from_str(toml_src).expect("instance toml"),
            provenance: crate::config::ProvenanceMap::new(),
        }
    }

    #[test]
    fn unsandboxable_processors_are_refused_only_when_sandboxed() {
        let marp = instance("processor.generator.marp", "processor.generator.marp", "");
        let ruff = instance("processor.checker.ruff", "processor.checker.ruff", "");
        let native_marp = instance(
            "processor.generator.marp.a",
            "processor.generator.marp",
            "execution_model = \"native\"",
        );
        let disabled_marp = instance(
            "processor.generator.marp.b",
            "processor.generator.marp",
            "enabled = false",
        );
        let sandboxed_marp = instance(
            "processor.generator.marp.c",
            "processor.generator.marp",
            "execution_model = \"sandbox\"",
        );
        let all = [marp, ruff, native_marp, disabled_marp, sandboxed_marp];
        // Build-wide sandbox: marp and its explicitly sandboxed instance are
        // refused; ruff, the native override and the disabled one are not.
        assert_eq!(
            unsandboxable_instances(&all, ExecutionModel::Sandbox),
            ["processor.generator.marp", "processor.generator.marp.c"]
        );
        // Build-wide native: only the instance that opts into the sandbox.
        assert_eq!(
            unsandboxable_instances(&all, ExecutionModel::Native),
            ["processor.generator.marp.c"]
        );
        assert!(check_sandboxable(&all[1..3], ExecutionModel::Sandbox).is_ok());
        let err = check_sandboxable(&all, ExecutionModel::Sandbox)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("processor.generator.marp")
                && err.contains("execution_model = \"native\""),
            "{err}"
        );
    }

    /// The processors marked as unable to run in a sandbox are exactly the
    /// ones that launch Chromium (measured: each aborts inside bwrap).
    #[test]
    fn only_chromium_launchers_are_unsandboxable() {
        let mut names: Vec<String> = crate::registries::processor::all_plugins()
            .filter(|p| !p.sandboxable)
            .map(crate::registries::ProcessorPlugin::pname)
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "processor.generator.chromium",
                "processor.generator.drawio",
                "processor.generator.marp",
                "processor.generator.mermaid",
            ]
        );
    }

    fn run_with(outputs: &[&str], dirs: &[&str]) -> Run {
        Run {
            command: Command::new("true"),
            root: PathBuf::from("/p"),
            dir: PathBuf::from("/p/.rsconstruct/sandbox/x"),
            upper: PathBuf::from("/p/.rsconstruct/sandbox/x/upper"),
            spec: Spec {
                label: "processor.checker.x".into(),
                outputs: outputs.iter().map(PathBuf::from).collect(),
                output_dirs: dirs.iter().map(PathBuf::from).collect(),
                network: false,
                writable: Vec::new(),
            },
            writable: Vec::new(),
        }
    }

    #[test]
    fn writable_entries_parse_as_paths_or_named_directories() {
        assert_eq!(
            Writable::parse("target"),
            Ok(Writable::Path(PathBuf::from("target")))
        );
        assert_eq!(
            Writable::parse("./sub/.cache"),
            Ok(Writable::Path(PathBuf::from("sub/.cache")))
        );
        assert_eq!(
            Writable::parse("**/__pycache__"),
            Ok(Writable::Named("__pycache__".into()))
        );
        for bad in [
            "", ".", "/abs", "../up", "a/../b", "*.pyc", "a/**/b", "**/", "**/a/b", "**/*",
        ] {
            assert!(Writable::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn writable_places_are_declared() {
        let mut run = run_with(&["out/a"], &[]);
        run.writable = vec![
            Writable::Path(PathBuf::from("target")),
            Writable::Named("__pycache__".into()),
        ];
        assert!(run.declares(Path::new("target/debug/x")));
        assert!(run.declares(Path::new(
            ".venv/lib/python3/site-packages/m/__pycache__/m.pyc"
        )));
        assert!(run.declares(Path::new("__pycache__/a.pyc")));
        assert!(!run.declares(Path::new("targets/x")));
        assert!(!run.declares(Path::new("src/m.py")));
    }

    /// Every tool's declared state places must parse; a bad entry would
    /// otherwise fail every sandboxed run of every processor using it.
    #[test]
    fn every_tool_declares_valid_writable_places() {
        for tool in crate::tools::all_tools() {
            for entry in tool.sandbox_writable {
                assert!(
                    Writable::parse(entry).is_ok(),
                    "tool {}: {:?}",
                    tool.name,
                    Writable::parse(entry)
                );
            }
        }
    }

    /// A processor gets the places of every tool it runs, then its config.
    #[test]
    fn writable_for_unions_tools_and_config() {
        let standard = StandardConfig {
            sandbox_writable: vec!["extra".into(), "target".into()],
            ..Default::default()
        };
        let tools = [
            "cargo".to_string(),
            "python".to_string(),
            "no-such-tool".to_string(),
        ];
        assert_eq!(
            writable_for(&standard, &tools),
            ["target", "**/__pycache__", "extra"]
        );
    }

    /// The network comes from a tool that declares it needs it, or from the
    /// processor's config; otherwise it is cut off.
    #[test]
    fn network_comes_from_the_tools_or_the_config() {
        let standard = StandardConfig::default();
        let tools = |names: &[&str]| names.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
        assert!(!network_for(&standard, &tools(&["cargo", "ruff"])));
        assert!(network_for(&standard, &tools(&["cargo", "cargo-deny"])));
        assert!(network_for(&standard, &tools(&["taplo"])));
        let configured = StandardConfig {
            sandbox_network: true,
            ..Default::default()
        };
        assert!(network_for(&configured, &tools(&[])));
    }

    /// The tools that reach the network are exactly the ones that install
    /// packages or fetch remote data on every run.
    #[test]
    fn network_tools_are_the_installers_and_fetchers() {
        let mut names: Vec<&str> = crate::tools::all_tools()
            .filter(|t| t.needs_network)
            .map(|t| t.name)
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["bundle", "cargo-deny", "npm", "pip", "taplo", "uv"]);
    }

    /// A mass generator's staging directory becomes an output directory of
    /// the running product, and only while a sandboxed product runs.
    #[test]
    fn declared_output_dirs_join_the_running_spec() {
        declare_output_dir(Path::new(".rsconstruct/mass_generator/x.staging"));
        assert!(
            current_spec().is_none(),
            "no product running: nothing to join"
        );
        set_spec(Some(run_with(&[], &[]).spec));
        declare_output_dir(Path::new(".rsconstruct/mass_generator/x.staging"));
        let spec = current_spec().unwrap();
        set_spec(None);
        assert_eq!(
            spec.output_dirs,
            [PathBuf::from(".rsconstruct/mass_generator/x.staging")]
        );
    }

    /// A removed or replaced directory is judged by the real files it held:
    /// fine when each is a declared output, refused for the ones that are not.
    #[test]
    fn replaced_directory_is_checked_file_by_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        fs::create_dir_all(root.join("docx/sub")).unwrap();
        fs::write(root.join("docx/a.docx"), "a").unwrap();
        fs::write(root.join("docx/sub/b.docx"), "b").unwrap();
        let mut run = run_with(&["docx/a.docx", "docx/sub/b.docx"], &[]);
        run.root = root.clone();
        let replaced = Change::Replaced(PathBuf::from("docx"));
        let none: Vec<String> = Vec::new();
        assert_eq!(run.undeclared_in(&replaced).unwrap(), none);
        assert_eq!(
            run.undeclared_in(&Change::Removed(PathBuf::from("docx")))
                .unwrap(),
            none
        );
        fs::write(root.join("docx/notes.txt"), "n").unwrap();
        assert_eq!(
            run.undeclared_in(&replaced).unwrap(),
            ["removed  docx/notes.txt"]
        );
        // A removed file is its own path.
        assert_eq!(
            run.undeclared_in(&Change::Removed(PathBuf::from("other")))
                .unwrap(),
            ["removed  other"]
        );
    }

    #[test]
    fn declares_outputs_and_everything_under_output_dirs() {
        let run = run_with(&["out/a.html"], &["docs"]);
        assert!(run.declares(Path::new("out/a.html")));
        assert!(run.declares(Path::new("docs")));
        assert!(run.declares(Path::new("docs/x/y.html")));
        assert!(!run.declares(Path::new("out/b.html")));
        assert!(!run.declares(Path::new("out")));
        // Path-component prefix, not string prefix.
        assert!(!run.declares(Path::new("docsx/y.html")));
    }

    #[test]
    fn relative_to_strips_the_root_and_a_leading_dot() {
        let root = Path::new("/p");
        assert_eq!(
            relative_to(root, Path::new("/p/out/a")),
            PathBuf::from("out/a")
        );
        assert_eq!(
            relative_to(root, Path::new("./out/a")),
            PathBuf::from("out/a")
        );
        assert_eq!(
            relative_to(root, Path::new("out/a")),
            PathBuf::from("out/a")
        );
    }

    #[test]
    fn native_unless_the_processor_or_the_build_says_sandbox() {
        let mut standard = StandardConfig::default();
        assert!(!is_sandboxed(&standard, ExecutionModel::Native));
        assert!(is_sandboxed(&standard, ExecutionModel::Sandbox));
        standard.execution_model = Some(ExecutionModel::Native);
        assert!(!is_sandboxed(&standard, ExecutionModel::Sandbox));
        standard.execution_model = Some(ExecutionModel::Sandbox);
        assert!(is_sandboxed(&standard, ExecutionModel::Native));
    }

    #[test]
    fn collect_changes_reports_files_and_ignores_plain_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let upper = tmp.path().join("upper");
        let root = tmp.path().join("root");
        fs::create_dir_all(upper.join("out/sub")).unwrap();
        fs::create_dir_all(upper.join("empty")).unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::write(upper.join("out/sub/a.txt"), "a").unwrap();
        fs::write(upper.join("stray"), "s").unwrap();
        let mut changes = Vec::new();
        collect_changes(&upper, Path::new(""), &root, &mut changes).unwrap();
        assert_eq!(
            changes,
            vec![
                Change::Written(PathBuf::from("out/sub/a.txt")),
                Change::Written(PathBuf::from("stray")),
            ]
        );
    }
}
