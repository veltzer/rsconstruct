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
    /// State directories the tool writes directly, relative to the project
    /// root: bind-mounted read-write over the overlay, never inspected.
    pub writable: Vec<PathBuf>,
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

/// The spec for running `products` (one, or a batch) of a processor whose
/// config is `standard`, or `None` when the processor runs natively.
pub fn spec_for(
    standard: &StandardConfig,
    build_default: ExecutionModel,
    products: &[&crate::graph::Product],
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
        network: standard.sandbox_network,
        writable: standard
            .sandbox_writable
            .iter()
            .map(PathBuf::from)
            .collect(),
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
    // State directories go on top of the overlay, straight to the real
    // tree. A bind needs its target to exist, so they are created first.
    for writable in &spec.writable {
        let dir = root.join(writable);
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create sandbox_writable {}", dir.display()))?;
        command.arg("--bind").arg(&dir).arg(&dir);
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
    }

    fn apply(&self) -> Result<()> {
        let mut changes = Vec::new();
        collect_changes(&self.upper, Path::new(""), &self.root, &mut changes)?;
        let undeclared: Vec<String> = changes
            .iter()
            .filter(|c| !self.declares(c.path()))
            .map(Change::describe)
            .collect();
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
        }
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
