//! The undeclared-reads report (`[build] undeclared_reads = "report"`).
//!
//! rsconstruct discovers a product's inputs; when discovery misses a file
//! the tool reads, editing that file rebuilds nothing. With the report on,
//! every external tool a product runs is started under strace, which logs
//! each file the tool and its children open. After a successful run the
//! log is compared with the product's declaration, and each project file
//! read that is neither an input, an output, nor a place the tool keeps
//! its own state is reported as a warning. See
//! docs/src/internal/undeclared-reads.md.
//!
//! Like the sandbox, it hangs off the product running on the thread: the
//! executor records it with [`set_spec`], `processor::run_command_inner`
//! wraps the command with [`prepare`] and calls [`Trace::finish`] after
//! the run. Builtin processors are not watched: their work is rsconstruct's
//! own code, which reads exactly the inputs it is handed.

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// The executable the report runs tools under.
pub const TRACE_TOOL: &str = "strace";

/// Where each traced run writes its log, relative to the project root.
const TRACE_DIR: &str = ".rsconstruct/read-trace";

/// Top-level directories whose reads are never reported: rsconstruct's own
/// state and the tools' installations (a Python tool reads its whole
/// `.venv`, a node tool its `node_modules`), and git's metadata, which
/// tools probe for the project root or a version string.
const IGNORED_DIRS: &[&str] = &[".rsconstruct", ".venv", "node_modules", ".git"];

/// What the running product (or batch) declares.
#[derive(Clone, Debug)]
pub struct Spec {
    /// The processor instance, for messages.
    pub label: String,
    /// Declared inputs and outputs, relative to the project root.
    pub declared: Vec<PathBuf>,
    /// Declared output directories: everything beneath them is the product's.
    pub declared_dirs: Vec<PathBuf>,
    /// Where the tools keep their own state, as `sandbox_writable` entries
    /// (`crate::sandbox::writable_for`): reading it back is not an input.
    pub writable: Vec<String>,
}

thread_local! {
    static SPEC: RefCell<Option<Spec>> = const { RefCell::new(None) };
}

/// Record the product (or batch) about to run on this thread, or clear it
/// with `None`. Set and cleared by the executor next to the sandbox spec.
pub fn set_spec(spec: Option<Spec>) {
    SPEC.with(|s| *s.borrow_mut() = spec);
}

/// The spec for running `products` of a processor whose config is
/// `standard` and which runs `tools`.
pub fn spec_for(
    standard: &crate::config::StandardConfig,
    products: &[&crate::graph::Product],
    tools: &[String],
) -> Spec {
    let label = products
        .first()
        .map(|p| p.processor.clone())
        .unwrap_or_default();
    Spec {
        label,
        declared: products
            .iter()
            .flat_map(|p| p.inputs.iter().chain(&p.outputs).cloned())
            .collect(),
        declared_dirs: products
            .iter()
            .flat_map(|p| p.output_dirs.iter().map(|d| d.as_ref().clone()))
            .collect(),
        writable: crate::sandbox::writable_for(standard, tools),
    }
}

/// The enabled processors among `active` whose tools a build with `build`
/// runs under strace, sorted: every non-builtin one when the report is on.
pub fn traced_processors(
    processors: &crate::processor::ProcessorMap,
    active: &[&String],
    build: &crate::config::BuildConfig,
) -> Vec<String> {
    if build.undeclared_reads != crate::config::UndeclaredReads::Report {
        return Vec::new();
    }
    let mut names: Vec<String> = active
        .iter()
        .filter(|name| processors.contains_key(name.as_str()))
        .filter(|name| !crate::registries::processor::is_builtin(name))
        .map(|name| (*name).clone())
        .collect();
    names.sort();
    names
}

/// A command wrapped in strace for one run, plus what is needed to read
/// its log afterwards.
pub struct Trace {
    /// The strace invocation to execute instead of the original command.
    pub command: Command,
    log: PathBuf,
    root: PathBuf,
    spec: Spec,
}

/// Wrap `cmd` in strace for the product running on this thread, or return
/// `None` when no product is watched.
pub fn prepare(cmd: &Command) -> Result<Option<Trace>> {
    let Some(spec) = SPEC.with(|s| s.borrow().clone()) else {
        return Ok(None);
    };
    let root = std::env::current_dir()
        .context("Failed to determine the project root for the read trace")?;
    let dir = root.join(TRACE_DIR);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create read-trace directory {}", dir.display()))?;
    static NEXT_RUN: AtomicU64 = AtomicU64::new(0);
    let log = dir.join(format!(
        "{}-{}.log",
        std::process::id(),
        NEXT_RUN.fetch_add(1, Ordering::Relaxed)
    ));

    // -f follows children (a sandboxed tool runs under bwrap, a script
    // runs its interpreter); -z logs successful calls only; -y prints the
    // path each returned descriptor refers to, so a relative open resolves
    // without knowing the caller's directory; --kill-on-exit takes the
    // tool down with strace when a timeout or an interrupt kills it.
    let mut command = Command::new(TRACE_TOOL);
    command
        .args(["-f", "-qq", "-z", "-y", "--kill-on-exit"])
        .args(["-e", "trace=open,openat,openat2", "-e", "signal=none", "-o"])
        .arg(&log)
        .arg("--")
        .arg(cmd.get_program())
        .args(cmd.get_args());
    if let Some(dir) = cmd.get_current_dir() {
        command.current_dir(dir);
    }
    for (key, value) in cmd.get_envs() {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
    Ok(Some(Trace {
        command,
        log,
        root,
        spec,
    }))
}

impl Trace {
    /// Read the log of a finished run and remove it. When the tool
    /// succeeded, warn about each project file it read that its product
    /// does not declare; a failed run's reads are not reported.
    pub fn finish(self, tool_succeeded: bool) -> Result<()> {
        let text = fs::read_to_string(&self.log)
            .with_context(|| format!("Failed to read the read trace {}", self.log.display()));
        let _ = fs::remove_file(&self.log);
        if !tool_succeeded {
            return Ok(());
        }
        let undeclared = undeclared_reads(&self.root, &self.spec, &text?)?;
        if !undeclared.is_empty() {
            let mut msg = format!(
                "[{}] the tool read files its product does not declare:",
                self.spec.label
            );
            for path in &undeclared {
                msg.push_str("\n  read     ");
                msg.push_str(&path.display().to_string());
            }
            crate::output::warn(&msg);
        }
        Ok(())
    }
}

/// The project files `log` shows were read that `spec` does not declare,
/// relative to `root`, sorted.
fn undeclared_reads(root: &Path, spec: &Spec, log: &str) -> Result<Vec<PathBuf>> {
    let writable = spec
        .writable
        .iter()
        .map(|entry| {
            crate::sandbox::Writable::parse(entry)
                .map_err(|e| anyhow::anyhow!("[{}] {e}", spec.label))
        })
        .collect::<Result<Vec<_>>>()?;
    let declared = declared_set(root, &spec.declared);
    let mut out = BTreeSet::new();
    for path in read_paths(log) {
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let ignored = rel
            .components()
            .next()
            .is_some_and(|c| IGNORED_DIRS.iter().any(|d| c.as_os_str() == *d));
        if rel.as_os_str().is_empty()
            || ignored
            || declared.contains(rel)
            || spec.declared_dirs.iter().any(|d| rel.starts_with(d))
            || writable.iter().any(|w| w.covers(rel))
            // A file gone after the run was the tool's scratch, and a
            // directory read is a listing, not the content of an input.
            || !path.is_file()
        {
            continue;
        }
        out.insert(rel.to_path_buf());
    }
    Ok(out.into_iter().collect())
}

/// The declared paths, plus where each symlink among them points: strace
/// reports the file a descriptor refers to, which is the link's target.
fn declared_set(root: &Path, declared: &[PathBuf]) -> HashSet<PathBuf> {
    let mut set = HashSet::new();
    for path in declared {
        let path = path.strip_prefix(".").unwrap_or(path);
        let abs = root.join(path);
        if fs::symlink_metadata(&abs).is_ok_and(|m| m.file_type().is_symlink())
            && let Ok(target) = fs::canonicalize(&abs)
            && let Ok(rel) = target.strip_prefix(root)
        {
            set.insert(rel.to_path_buf());
        }
        set.insert(path.to_path_buf());
    }
    set
}

/// The absolute paths of the files opened for reading in a strace log
/// written with `-f -z -y`. A call split by another process's output
/// (`<unfinished ...>`, then `<... openat resumed>`) is joined first.
fn read_paths(log: &str) -> Vec<PathBuf> {
    let mut pending: HashMap<&str, &str> = HashMap::new();
    let mut paths = Vec::new();
    for line in log.lines() {
        let Some((pid, call)) = line.split_once(' ') else {
            continue;
        };
        let call = call.trim_start();
        if let Some(start) = call.strip_suffix(" <unfinished ...>") {
            pending.insert(pid, start);
            continue;
        }
        let joined;
        let call = if let Some(rest) = call.strip_prefix("<... ") {
            let Some((_, tail)) = rest.split_once(" resumed>") else {
                continue;
            };
            let Some(start) = pending.remove(pid) else {
                continue;
            };
            joined = format!("{start}{tail}");
            joined.as_str()
        } else {
            call
        };
        if let Some(path) = read_path(call) {
            paths.push(path);
        }
    }
    paths
}

/// The file one logged `open`/`openat`/`openat2` call opened for reading,
/// or `None` when it opened for writing, created, truncated, opened a
/// directory or only a path, or returned no descriptor.
fn read_path(call: &str) -> Option<PathBuf> {
    const NOT_READS: &[&str] = &["O_WRONLY", "O_CREAT", "O_TRUNC", "O_DIRECTORY", "O_PATH"];
    if !(call.starts_with("open(") || call.starts_with("openat(") || call.starts_with("openat2(")) {
        return None;
    }
    let (args, result) = call.rsplit_once(") = ")?;
    if NOT_READS.iter().any(|flag| args.contains(flag)) {
        return None;
    }
    // `3</abs/path>`: the descriptor, then the path it refers to.
    let path = result.split_once('<')?.1.strip_suffix('>')?;
    path.starts_with('/').then(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(declared: &[&str], dirs: &[&str], writable: &[&str]) -> Spec {
        Spec {
            label: "processor.explicit.generic".into(),
            declared: declared.iter().map(PathBuf::from).collect(),
            declared_dirs: dirs.iter().map(PathBuf::from).collect(),
            writable: writable.iter().map(|w| (*w).to_string()).collect(),
        }
    }

    #[test]
    fn read_path_takes_reads_and_skips_the_rest() {
        assert_eq!(
            read_path(r#"openat(AT_FDCWD</p>, "a.txt", O_RDONLY|O_CLOEXEC) = 3</p/a.txt>"#),
            Some(PathBuf::from("/p/a.txt"))
        );
        assert_eq!(
            read_path(r#"open("/p/b", O_RDWR) = 4</p/b>"#),
            Some(PathBuf::from("/p/b"))
        );
        assert_eq!(
            read_path(r#"openat2(AT_FDCWD</p>, "c", {flags=O_RDONLY, resolve=0}, 24) = 5</p/c>"#),
            Some(PathBuf::from("/p/c"))
        );
        for call in [
            r#"openat(AT_FDCWD</p>, "o", O_WRONLY|O_CREAT|O_TRUNC, 0666) = 3</p/o>"#,
            r#"openat(AT_FDCWD</p>, "l", O_RDWR|O_CREAT, 0666) = 3</p/l>"#,
            r#"openat(AT_FDCWD</p>, ".", O_RDONLY|O_DIRECTORY) = 3</p>"#,
            r#"openat(AT_FDCWD</p>, "x", O_RDONLY|O_PATH) = 3</p/x>"#,
            r#"stat("/p/a.txt", {st_mode=S_IFREG|0644, st_size=3, ...}) = 0"#,
        ] {
            assert_eq!(read_path(call), None, "{call}");
        }
    }

    #[test]
    fn read_paths_joins_calls_split_by_another_process() {
        let log = "\
11 openat(AT_FDCWD</p>, \"a\", O_RDONLY <unfinished ...>
12 openat(AT_FDCWD</p>, \"b\", O_RDONLY) = 3</p/b>
11 <... openat resumed>) = 4</p/a>
";
        assert_eq!(
            read_paths(log),
            vec![PathBuf::from("/p/b"), PathBuf::from("/p/a")]
        );
    }

    #[test]
    fn only_undeclared_project_files_are_reported() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        for f in [
            "in.txt",
            "extra.txt",
            "out.txt",
            "gen/x.txt",
            ".venv/lib/m.py",
            "target/c",
            "pkg/__pycache__/m.pyc",
        ] {
            let path = r.join(f);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        let line = |f: &str| {
            format!(
                "1 openat(AT_FDCWD</>, \"{p}\", O_RDONLY) = 3<{p}>\n",
                p = r.join(f).display()
            )
        };
        let mut log: String = [
            "in.txt",
            "extra.txt",
            "out.txt",
            "gen/x.txt",
            ".venv/lib/m.py",
            "target/c",
            "pkg/__pycache__/m.pyc",
            "gone.txt",
        ]
        .iter()
        .map(|f| line(f))
        .collect();
        log.push_str("1 openat(AT_FDCWD</>, \"/etc/hosts\", O_RDONLY) = 3</etc/hosts>\n");
        let found = undeclared_reads(
            r,
            &spec(
                &["in.txt", "out.txt"],
                &["gen"],
                &["target", "**/__pycache__"],
            ),
            &log,
        )
        .unwrap();
        assert_eq!(found, vec![PathBuf::from("extra.txt")]);
    }
}
