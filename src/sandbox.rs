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
    /// The outputs the previous build left, which must go before the tool
    /// writes (a restored output is a read-only hardlink). A native run
    /// removes them from the tree beforehand; a sandboxed run removes them
    /// inside its overlay, as the first step of its first command, so that
    /// a run that fails changes nothing. Cleared once a run is applied:
    /// later commands of the same product must not remove what it wrote.
    pub stale: Vec<PathBuf>,
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

/// Declare `paths` as outputs of the product running on this thread and
/// have the sandbox remove the previous copies inside its overlay before
/// the tool starts, if it runs sandboxed. For a tool that writes the files
/// of many products in one run (a mass generator's planned outputs): the
/// run is one product's, but it rewrites them all. Returns false natively
/// or outside a product, where the caller removes the old copies itself.
pub fn declare_replaced_outputs(paths: &[&Path]) -> bool {
    SPEC.with(|s| {
        let mut spec = s.borrow_mut();
        let Some(spec) = spec.as_mut() else {
            return false;
        };
        for path in paths {
            spec.outputs.push(path.to_path_buf());
            spec.stale.push(path.to_path_buf());
        }
        true
    })
}

/// Set in the environment of every sandboxed run, so everything it starts,
/// however deep, knows it is inside bwrap.
const INSIDE_ENV: &str = "RSCONSTRUCT_IN_SANDBOX";

/// Whether the product running on this thread runs inside bwrap: it runs
/// sandboxed, or this whole process does (an rsconstruct started by a
/// sandboxed tool, such as a test run by a sandboxed `cargo nextest`, runs
/// its own products natively but still inside the outer bwrap). A processor
/// whose tool runs Chromium asks this to turn Chromium's own sandbox off,
/// which cannot start inside bwrap; natively it stays on. See
/// docs/src/internal/sandbox-chromium.md.
pub fn active() -> bool {
    SPEC.with(|s| s.borrow().is_some()) || std::env::var_os(INSIDE_ENV).is_some()
}

fn current_spec() -> Option<Spec> {
    SPEC.with(|s| s.borrow().clone())
}

/// Let the product running on this thread, when it runs sandboxed, write
/// anything beneath `dirs`. A mass generator calls this with its
/// `output_dirs`: it checks what its tool wrote there against its plan
/// itself (an error, or a warning with `loose_manifest`), with a message
/// that says which files are off the plan, so the sandbox leaves those
/// directories to that check. pdflatex calls it with the scratch directory
/// its passes share and it removes afterwards. Natively it does nothing.
pub fn allow_output_dirs(dirs: &[PathBuf]) {
    SPEC.with(|s| {
        if let Some(spec) = s.borrow_mut().as_mut() {
            for dir in dirs {
                if !spec.output_dirs.contains(dir) {
                    spec.output_dirs.push(dir.clone());
                }
            }
        }
    });
}

/// Forget the stale outputs of the product running on this thread: a run
/// that removed them was applied, so they are gone from the tree.
fn clear_stale() {
    SPEC.with(|s| {
        if let Some(spec) = s.borrow_mut().as_mut() {
            spec.stale.clear();
        }
    });
}

/// The shell script a sandboxed command runs before the tool, with its
/// arguments: remove each stale output, create each output directory (a
/// tool may assume they exist), then `exec` the tool. The paths are
/// absolute, so none of them is the `--` that ends each list.
const PRELUDE: &str = "while [ \"$1\" != -- ]; do rm -f -- \"$1\" || exit; shift; done; shift; \
                       while [ \"$1\" != -- ]; do mkdir -p -- \"$1\" || exit; shift; done; shift; \
                       exec \"$@\"";

/// Whether the processor named `pname` can run in a sandbox, by its plugin's
/// `sandboxable` flag.
fn plugin_is_sandboxable(pname: &str) -> bool {
    crate::registries::find_plugin(pname).is_none_or(|p| p.sandboxable)
}

/// The enabled instances among `instances` that would run sandboxed under
/// `build_default` although `sandboxable` says their processor cannot,
/// sorted. Read from the raw instance tables, before any processor is
/// created, so a build can refuse them before doing any work.
fn unsandboxable_instances(
    instances: &[crate::config::ProcessorInstance],
    build_default: ExecutionModel,
    sandboxable: &dyn Fn(&str) -> bool,
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
            enabled && model == ExecutionModel::Sandbox && !sandboxable(&inst.pname)
        })
        .map(|inst| inst.instance_name.clone())
        .collect();
    out.sort();
    out
}

/// Fail when any configured, enabled instance would run sandboxed although
/// its processor cannot run in a sandbox. Called at the start of a build. No
/// processor is marked unsandboxable today (see
/// docs/src/internal/sandbox-chromium.md); the check stays for one that is.
pub fn check_sandboxable(
    instances: &[crate::config::ProcessorInstance],
    build_default: ExecutionModel,
) -> Result<()> {
    check_sandboxable_by(instances, build_default, &plugin_is_sandboxable)
}

/// [`check_sandboxable`] with what counts as sandboxable given, so the
/// refusal can be tested while every real processor is sandboxable.
fn check_sandboxable_by(
    instances: &[crate::config::ProcessorInstance],
    build_default: ExecutionModel,
    sandboxable: &dyn Fn(&str) -> bool,
) -> Result<()> {
    let bad = unsandboxable_instances(instances, build_default, sandboxable);
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

/// The enabled processors (by `is_enabled`) that run under the sandbox, so
/// need [`SANDBOX_TOOL`], sorted. Builtin processors count too: a sandboxed
/// builtin runs in a child rsconstruct inside bwrap (`execute_builtin`).
/// Shared by the build's preflight and `tool install machine`, so the tool
/// the build demands is the tool the install provides.
pub fn sandboxed_processors(
    processors: &crate::processor::ProcessorMap,
    is_enabled: &dyn Fn(&str) -> bool,
    build_default: ExecutionModel,
) -> Vec<String> {
    let mut names: Vec<String> = processors
        .iter()
        .filter(|(name, p)| is_enabled(name) && is_sandboxed(p.scan_config(), build_default))
        .map(|(name, _)| name.clone())
        .collect();
    names.sort();
    names
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
        stale: Vec::new(),
    })
}

// ─── Builtin processors ──────────────────────────────────────────────────────
//
// A builtin processor does its work in rsconstruct's own code, not in a
// tool, so there is no command to wrap. When one runs sandboxed, the build
// runs rsconstruct itself under bwrap instead: the hidden `sandbox-run`
// subcommand recreates the processor from the same config, executes the
// products it is handed, and reports each one's result. Being an ordinary
// command, it goes through the same sandbox as any tool: its writes land in
// the overlay and are checked against the products' declared outputs.

/// One builtin processor instance, as `sandbox-run` recreates it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct BuiltinInstance {
    pub instance: String,
    pub pname: String,
    /// The instance's config table, overrides (`--iset`, `--pset`, CLI
    /// flags) already applied.
    pub config: toml::Value,
}

/// What `sandbox-run` needs besides its products: the same for every
/// product of a build. Built from the build's config by the builder and
/// handed to the executor.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct BuiltinSetup {
    /// Every builtin processor instance of the build.
    pub instances: Vec<BuiltinInstance>,
    /// How the build walks the tree (`Config::file_index_walk_dirs`), so the
    /// child's discovery sees the same files.
    pub force_dirs: Vec<String>,
    pub exclude_roots: Vec<String>,
    pub warn_symlinks: bool,
    /// `[build] python`.
    pub python: String,
    /// The settings the build puts on its `BuildContext`.
    pub max_arg_len: usize,
    pub command_timeout_secs: u64,
    /// `[cache] webcache_ttl_secs`.
    pub webcache_ttl_secs: u64,
    /// `[build] worker_stack_mib`: the child runs its products on a thread
    /// with this stack, as the build's workers do natively, so a deeply
    /// nested file that a worker checks does not overflow the child.
    pub worker_stack_mib: usize,
}

impl BuiltinSetup {
    pub fn from_config(config: &crate::config::Config) -> Self {
        let (exclude_roots, force_dirs) = config.file_index_walk_dirs();
        Self {
            instances: config
                .processor
                .instances
                .iter()
                .filter(|inst| crate::registries::processor::is_builtin(&inst.pname))
                .map(|inst| BuiltinInstance {
                    instance: inst.instance_name.clone(),
                    pname: inst.pname.clone(),
                    config: inst.config_toml.clone(),
                })
                .collect(),
            force_dirs,
            exclude_roots,
            warn_symlinks: config.build.warn_symlinks,
            python: config.build.python.clone(),
            max_arg_len: config.build.max_arg_len,
            command_timeout_secs: config.build.command_timeout_secs,
            webcache_ttl_secs: config.cache.webcache_ttl_secs,
            worker_stack_mib: config.build.worker_stack_mib,
        }
    }
}

/// What `sandbox-run` reports after [`RESULTS_MARKER`].
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct JobResults {
    /// One per product, in order: `None` for success, else its error.
    results: Vec<Option<String>>,
    /// What the child downloaded into no cache (see `crate::webcache`),
    /// for the build to store.
    fetched: Vec<crate::webcache::Download>,
}

/// One product as `sandbox-run` receives it.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct JobProduct {
    id: usize,
    inputs: Vec<PathBuf>,
    outputs: Vec<PathBuf>,
    output_dirs: Vec<PathBuf>,
    variant: Option<String>,
}

/// What the build sends `sandbox-run` on its standard input.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct BuiltinJob {
    setup: BuiltinSetup,
    instance: BuiltinInstance,
    products: Vec<JobProduct>,
    /// Run the products through `execute_batch` (one invocation, as the
    /// executor would natively), not `execute`.
    batch: bool,
}

/// The line `sandbox-run` prints before its results, so they can be told
/// from whatever the processor printed to standard output before them.
const RESULTS_MARKER: &str = "\u{1e}rsconstruct-sandbox-results\u{1e}";

/// Execute `products` of the builtin processor instance `instance` in a
/// sandbox, as `execute` (one product, `batch` false) or `execute_batch`
/// would natively; one result per product, in order. The executor has set
/// the sandbox spec for these products, so the command below runs wrapped.
pub fn execute_builtin(
    ctx: &crate::build_context::BuildContext,
    setup: &BuiltinSetup,
    instance: &str,
    products: &[&crate::graph::Product],
    batch: bool,
) -> Vec<Result<()>> {
    let all = |e: &anyhow::Error| {
        products
            .iter()
            .map(|_| Err(anyhow::anyhow!("{e:#}")))
            .collect()
    };
    match run_builtin_job(ctx, setup, instance, products, batch) {
        Ok(results) => results
            .into_iter()
            .map(|r| r.map_or(Ok(()), |e| Err(anyhow::anyhow!(e))))
            .collect(),
        Err(e) => all(&e),
    }
}

/// The parent's side of `execute_builtin`: send the job, relay what the
/// child printed, read its results (`None` for a product that succeeded,
/// its error otherwise).
fn run_builtin_job(
    ctx: &crate::build_context::BuildContext,
    setup: &BuiltinSetup,
    instance: &str,
    products: &[&crate::graph::Product],
    batch: bool,
) -> Result<Vec<Option<String>>> {
    let instance = setup
        .instances
        .iter()
        .find(|i| i.instance == instance)
        .cloned()
        .with_context(|| format!("internal error: no config for builtin processor '{instance}'"))?;
    let label = instance.instance.clone();
    let job = BuiltinJob {
        setup: BuiltinSetup {
            instances: Vec::new(),
            ..setup.clone()
        },
        instance,
        products: products
            .iter()
            .map(|p| JobProduct {
                id: p.id,
                inputs: p.inputs.clone(),
                outputs: p.outputs.clone(),
                output_dirs: p.output_dirs.iter().map(|d| d.as_ref().clone()).collect(),
                variant: p.variant.clone(),
            })
            .collect(),
        batch,
    };
    let payload = serde_json::to_vec(&job).context("Failed to encode the sandboxed job")?;

    let exe = std::env::current_exe().context("Failed to locate the rsconstruct executable")?;
    let mut cmd = Command::new(&exe);
    cmd.arg("--color")
        .arg(if crate::runtime_flags::color_enabled() {
            "always"
        } else {
            "never"
        });
    for (on, flag) in [
        (crate::runtime_flags::json_mode(), "--json"),
        (crate::runtime_flags::quiet(), "--quiet"),
        (crate::runtime_flags::show_output(), "--show-output"),
        (
            crate::runtime_flags::show_child_processes(),
            "--show-child-processes",
        ),
    ] {
        if on {
            cmd.arg(flag);
        }
    }
    cmd.arg("sandbox-run");
    let output = crate::processor::run_command_with_stdin(ctx, &cmd, &payload)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let (printed, results) = match stdout.rfind(RESULTS_MARKER) {
        Some(at) => (&stdout[..at], Some(&stdout[at + RESULTS_MARKER.len()..])),
        None => (&*stdout, None),
    };
    relay(printed, &String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        anyhow::bail!(
            "[{label}] the sandboxed run of rsconstruct failed ({})",
            output.status
        );
    }
    let results = results.with_context(|| {
        format!("[{label}] the sandboxed run of rsconstruct reported no results")
    })?;
    let JobResults { results, fetched } = serde_json::from_str(results.trim())
        .with_context(|| format!("[{label}] Failed to read the sandboxed run's results"))?;
    crate::webcache::defer_store(fetched);
    anyhow::ensure!(
        results.len() == products.len(),
        "[{label}] the sandboxed run reported {} results for {} products",
        results.len(),
        products.len()
    );
    Ok(results)
}

/// Pass on what the child printed, as the processor would have printed it
/// running in the build's own process.
fn relay(stdout: &str, stderr: &str) {
    use std::io::Write;
    if !stdout.is_empty() {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(stdout.as_bytes());
        let _ = out.flush();
    }
    if !stderr.is_empty() {
        let _ = std::io::stderr().lock().write_all(stderr.as_bytes());
    }
}

/// `rsconstruct sandbox-run`: the child's side of [`execute_builtin`]. Reads
/// the job from standard input, recreates the processor and runs its
/// discovery (a processor may keep state from it for execute: tera keeps
/// the templates `{% include %}` can name), executes the products, and
/// prints [`RESULTS_MARKER`] and their results. An error here (bad job,
/// config the processor rejects) fails every product of the job.
pub fn sandbox_run(ctx: &crate::build_context::BuildContext) -> Result<()> {
    let input = std::io::read_to_string(std::io::stdin())
        .context("Failed to read the sandboxed job from standard input")?;
    let job: BuiltinJob =
        serde_json::from_str(&input).context("Failed to decode the sandboxed job")?;
    let setup = &job.setup;

    // Inside the sandbox the build's own caches are out of reach (the
    // parent holds them), and nothing here is sandboxed again: commands the
    // processor runs execute directly, inside this sandbox. The webcache
    // (`.rsconstruct/webcache.redb`, iyamlschema's schemas) is only read:
    // storing into it would be a write the products do not declare, and
    // children running side by side cannot share a writer's lock. What this
    // child downloads goes back with its results for the build to store.
    ctx.set_mtime_check(false);
    ctx.set_max_arg_len(setup.max_arg_len);
    ctx.set_command_timeout_secs(setup.command_timeout_secs);
    ctx.set_webcache_ttl_secs(setup.webcache_ttl_secs);
    crate::webcache::set_child_mode();
    crate::python_env::set_interpreter(&setup.python);

    let processor =
        crate::builder::create_processor_for_instance(&job.instance.pname, &job.instance.config)?
            .with_context(|| format!("Unknown processor type: '{}'", job.instance.pname))?;
    let force_refs: Vec<&str> = setup.force_dirs.iter().map(String::as_str).collect();
    let file_index = crate::file_index::FileIndex::build_with_force_dirs(
        &force_refs,
        &setup.exclude_roots,
        setup.warn_symlinks,
    )?;
    processor.discover(
        &mut crate::graph::BuildGraph::new(),
        &file_index,
        &job.instance.instance,
    )?;

    let products: Vec<crate::graph::Product> = job
        .products
        .into_iter()
        .map(|p| {
            let mut product =
                crate::graph::Product::new(p.inputs, p.outputs, &job.instance.instance, p.id, None);
            product.output_dirs = p.output_dirs.into_iter().map(std::sync::Arc::new).collect();
            product.variant = p.variant;
            product
        })
        .collect();
    let refs: Vec<&crate::graph::Product> = products.iter().collect();

    let results: Vec<Result<()>> = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("sandbox-run".to_string())
            .stack_size(setup.worker_stack_mib * 1024 * 1024)
            .spawn_scoped(scope, || {
                crate::processor::set_declared_tools(Some(processor.required_tools()));
                let results = if job.batch {
                    processor.execute_batch(ctx, &refs)
                } else {
                    refs.iter().map(|p| processor.execute(ctx, p)).collect()
                };
                crate::processor::set_declared_tools(None);
                results
            })
            .context("Failed to start the sandboxed run's worker thread")?
            .join()
            .map_err(|_| anyhow::anyhow!("the sandboxed run's worker thread panicked"))
    })?;

    let results = JobResults {
        results: results
            .into_iter()
            .map(|r| r.err().map(|e| format!("{e:#}")))
            .collect(),
        fetched: crate::webcache::take_fetched(),
    };
    let encoded = serde_json::to_string(&results).context("Failed to encode the results")?;
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{RESULTS_MARKER}{encoded}")
        .and_then(|()| out.flush())
        .context("Failed to write the sandboxed run's results")?;
    Ok(())
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
    spec.stale = spec.stale.iter().map(|p| relative_to(&root, p)).collect();
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
    command.arg("--").args(inner_command(&root, &spec, cmd));
    command.env(INSIDE_ENV, "1");
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

/// What bwrap runs inside the sandbox: `cmd`, behind [`PRELUDE`] when there
/// are stale outputs to remove or output directories to create. Both happen
/// inside the overlay, never in the real tree: a failed run changes nothing.
fn inner_command(root: &Path, spec: &Spec, cmd: &Command) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    if !spec.stale.is_empty() || !spec.output_dirs.is_empty() {
        args.extend(["sh", "-c", PRELUDE, "sh"].map(Into::into));
        args.extend(spec.stale.iter().map(|p| root.join(p).into_os_string()));
        args.push("--".into());
        args.extend(
            spec.output_dirs
                .iter()
                .map(|d| root.join(d).into_os_string()),
        );
        args.push("--".into());
    }
    args.push(cmd.get_program().to_os_string());
    args.extend(cmd.get_args().map(std::ffi::OsStr::to_os_string));
    args
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
        if tool_succeeded && applied.is_ok() {
            clear_stale();
        }
        let cleaned = remove_run_dir(&self.dir);
        applied?;
        cleaned
    }

    /// Whether the product may change `path`. The stale outputs are
    /// included: removing them was rsconstruct's own first step.
    fn declares(&self, path: &Path) -> bool {
        self.spec.stale.iter().any(|s| s == path)
            || self.spec.outputs.iter().any(|o| o == path)
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
    collect_changes_in(upper, rel, root, false, out)
}

/// [`collect_changes`], knowing whether `rel` lies in a directory that
/// replaces a real one (`replacing`). Applying the replacement removes the
/// real directory first, so every file in it must be moved in, even one
/// identical to the real file it replaces.
fn collect_changes_in(
    upper: &Path,
    rel: &Path,
    root: &Path,
    replacing: bool,
    out: &mut Vec<Change>,
) -> Result<()> {
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
            let replaces = opaque && fs::symlink_metadata(root.join(&child)).is_ok();
            if replaces {
                out.push(Change::Replaced(child.clone()));
            }
            collect_changes_in(upper, &child, root, replacing || replaces, out)?;
        } else if replacing || !same_as_real(&path, &meta, &root.join(&child))? {
            out.push(Change::Written(child));
        }
    }
    Ok(())
}

/// Whether the upper directory's file at `upper` is the real tree's file at
/// `real` unchanged. overlayfs copies a file up as soon as a tool opens it
/// for writing, even when it writes nothing (Python's `shelve` opens its
/// database read-write to read it), and such a file changed nothing.
fn same_as_real(upper: &Path, upper_meta: &fs::Metadata, real: &Path) -> Result<bool> {
    let Ok(real_meta) = fs::symlink_metadata(real) else {
        return Ok(false);
    };
    if !upper_meta.is_file()
        || !real_meta.is_file()
        || upper_meta.len() != real_meta.len()
        || crate::platform::get_mode(upper_meta) != crate::platform::get_mode(&real_meta)
    {
        return Ok(false);
    }
    same_content(upper, real)
}

/// Whether two files of the same length hold the same bytes, read in
/// chunks so a large file is never held whole.
fn same_content(a: &Path, b: &Path) -> Result<bool> {
    use std::io::Read;
    let open =
        |p: &Path| fs::File::open(p).with_context(|| format!("Failed to open {}", p.display()));
    let (mut fa, mut fb) = (open(a)?, open(b)?);
    let mut ba = vec![0u8; 64 * 1024];
    let mut bb = vec![0u8; 64 * 1024];
    loop {
        let n = fa
            .read(&mut ba)
            .with_context(|| format!("Failed to read {}", a.display()))?;
        if n == 0 {
            return Ok(true);
        }
        fb.read_exact(&mut bb[..n])
            .with_context(|| format!("Failed to read {}", b.display()))?;
        if ba[..n] != bb[..n] {
            return Ok(false);
        }
    }
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
/// Remove what killed builds left under `.rsconstruct/sandbox/` in the
/// project at `root`, and the directory itself; returns how many runs were
/// removed. A run's directory normally goes when the run is settled, but a
/// build killed mid-run (SIGKILL, OOM, power loss) leaves it behind, and
/// overlayfs's `work/work` has mode 000, so a plain `rm -rf` fails on it.
///
/// Called at the start of a build, when nothing can be using them: the
/// build holds the project's cache database open, which redb locks
/// exclusively, so no other rsconstruct process is running in this project,
/// and this one has not started a run yet. Nothing in a leftover run is
/// worth keeping: a run reaches the tree only once it is accepted.
pub fn remove_leftover_runs(root: &Path) -> Result<usize> {
    let sandbox = root.join(SANDBOX_DIR);
    let entries = match fs::read_dir(&sandbox) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => {
            return Err(e).with_context(|| {
                format!("Failed to read sandbox directory {}", sandbox.display())
            });
        }
    };
    let mut removed = 0;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("Failed to read sandbox directory {}", sandbox.display()))?;
        let path = entry.path();
        if fs::symlink_metadata(&path)
            .with_context(|| format!("Failed to stat {}", path.display()))?
            .is_dir()
        {
            remove_run_dir(&path)?;
        } else {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
        removed += 1;
    }
    fs::remove_dir(&sandbox)
        .with_context(|| format!("Failed to remove sandbox directory {}", sandbox.display()))?;
    Ok(removed)
}

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

    /// No real processor is unsandboxable today, so the test treats marp as
    /// one to exercise the refusal.
    fn marp_is_unsandboxable(pname: &str) -> bool {
        pname != "processor.generator.marp"
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
            unsandboxable_instances(&all, ExecutionModel::Sandbox, &marp_is_unsandboxable),
            ["processor.generator.marp", "processor.generator.marp.c"]
        );
        // Build-wide native: only the instance that opts into the sandbox.
        assert_eq!(
            unsandboxable_instances(&all, ExecutionModel::Native, &marp_is_unsandboxable),
            ["processor.generator.marp.c"]
        );
        assert!(
            check_sandboxable_by(&all[1..3], ExecutionModel::Sandbox, &marp_is_unsandboxable)
                .is_ok()
        );
        let err = check_sandboxable_by(&all, ExecutionModel::Sandbox, &marp_is_unsandboxable)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("processor.generator.marp")
                && err.contains("execution_model = \"native\""),
            "{err}"
        );
    }

    /// Every processor can run in a sandbox, the Chromium-based ones
    /// included (they turn Chromium's own sandbox off when sandboxed; see
    /// docs/src/internal/sandbox-chromium.md), and the real build check
    /// refuses none of them.
    #[test]
    fn every_processor_is_sandboxable() {
        let unsandboxable: Vec<String> = crate::registries::processor::all_plugins()
            .filter(|p| !p.sandboxable)
            .map(crate::registries::ProcessorPlugin::pname)
            .collect();
        assert_eq!(unsandboxable, Vec::<String>::new());
        let marp = instance("processor.generator.marp", "processor.generator.marp", "");
        assert!(check_sandboxable(&[marp], ExecutionModel::Sandbox).is_ok());
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
                stale: Vec::new(),
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
            [
                "target",
                "**/target",
                "**/Cargo.lock",
                "**/__pycache__",
                "extra"
            ]
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

    /// The tool runs behind the prelude only when there is something for it
    /// to do, with the stale outputs and output directories made absolute.
    #[test]
    fn the_prelude_runs_only_when_needed() {
        let mut cmd = Command::new("tool");
        cmd.args(["-x", "--", "in"]);
        let root = Path::new("/p");
        let mut spec = run_with(&[], &[]).spec;
        assert_eq!(inner_command(root, &spec, &cmd), ["tool", "-x", "--", "in"]);

        spec.stale = vec![PathBuf::from("out/a"), PathBuf::from("out/b")];
        spec.output_dirs = vec![PathBuf::from("out")];
        assert_eq!(
            inner_command(root, &spec, &cmd),
            [
                "sh", "-c", PRELUDE, "sh", "/p/out/a", "/p/out/b", "--", "/p/out", "--", "tool",
                "-x", "--", "in"
            ]
        );
    }

    /// The prelude removes the stale outputs (read-only ones too: a restored
    /// output is a read-only hardlink), creates the output directories, and
    /// runs the tool with its arguments untouched.
    #[test]
    fn the_prelude_clears_stale_outputs_then_runs_the_tool() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("out")).unwrap();
        fs::write(root.join("out/old"), "old").unwrap();
        fs::write(root.join("out/restored"), "restored").unwrap();
        crate::platform::set_permissions_mode(&root.join("out/restored"), 0o444).unwrap();
        let mut spec = run_with(&[], &["out", "made/here"]).spec;
        spec.stale = vec![
            PathBuf::from("out/old"),
            PathBuf::from("out/restored"),
            PathBuf::from("out/gone"),
        ];
        let mut tool = Command::new("sh");
        tool.args([
            "-c",
            "printf '%s|' \"$@\" > out/ran",
            "sh",
            "a b",
            "--",
            "c",
        ]);
        let args = inner_command(root, &spec, &tool);
        let status = Command::new(&args[0])
            .args(&args[1..])
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!root.join("out/old").exists());
        assert!(!root.join("out/restored").exists());
        assert!(root.join("made/here").is_dir());
        assert_eq!(
            fs::read_to_string(root.join("out/ran")).unwrap(),
            "a b|--|c|"
        );
    }

    /// A killed build's runs are removed, the mode-000 `work/work` that
    /// overlayfs leaves included, and so is the sandbox directory; a project
    /// without one is fine.
    #[test]
    fn leftover_runs_are_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        assert_eq!(remove_leftover_runs(root).unwrap(), 0);

        let run = root.join(SANDBOX_DIR).join("4242-0");
        fs::create_dir_all(run.join("upper/out")).unwrap();
        fs::write(run.join("upper/out/half.html"), "half").unwrap();
        fs::create_dir_all(run.join("work/work")).unwrap();
        crate::platform::set_permissions_mode(&run.join("work/work"), 0o000).unwrap();
        fs::create_dir_all(root.join(SANDBOX_DIR).join("4242-1")).unwrap();

        assert_eq!(remove_leftover_runs(root).unwrap(), 2);
        assert!(!root.join(SANDBOX_DIR).exists());
        assert!(root.join(".rsconstruct").is_dir(), "only the sandbox goes");
    }

    /// Removing a stale output is rsconstruct's own first step, so it is
    /// never reported as undeclared.
    #[test]
    fn stale_outputs_may_be_removed() {
        let mut run = run_with(&[], &[]);
        assert!(!run.declares(Path::new("old/out.html")));
        run.spec.stale = vec![PathBuf::from("old/out.html")];
        assert!(run.declares(Path::new("old/out.html")));
    }

    /// The stale outputs are removed by the first run that is applied, and
    /// by no run after it, which would remove what that run wrote. A run
    /// that fails is discarded, so the next attempt must remove them again.
    #[test]
    fn stale_outputs_are_forgotten_once_a_run_is_applied() {
        let settle = |succeeded: bool| {
            let tmp = tempfile::tempdir().unwrap();
            let mut run = run_with(&[], &[]);
            run.root = tmp.path().to_path_buf();
            run.dir = tmp.path().join("run");
            run.upper = run.dir.join("upper");
            fs::create_dir_all(&run.upper).unwrap();
            let mut spec = run.spec.clone();
            spec.stale = vec![PathBuf::from("out/old")];
            set_spec(Some(spec));
            run.finish(succeeded).unwrap();
            let left = current_spec().unwrap().stale;
            set_spec(None);
            left
        };
        assert_eq!(settle(false), [PathBuf::from("out/old")]);
        let none: Vec<PathBuf> = Vec::new();
        assert_eq!(settle(true), none);
    }

    /// A run that rewrites other products' files declares them and leaves
    /// their removal to the overlay; natively there is nothing to declare.
    #[test]
    fn replaced_outputs_are_declared_and_removed_in_the_overlay() {
        set_spec(None);
        assert!(!declare_replaced_outputs(&[Path::new("_site/a.html")]));
        set_spec(Some(run_with(&["_site/a.html"], &[]).spec));
        assert!(declare_replaced_outputs(&[Path::new("_site/b.html")]));
        let spec = current_spec().unwrap();
        set_spec(None);
        assert!(spec.outputs.contains(&PathBuf::from("_site/b.html")));
        assert_eq!(spec.stale, [PathBuf::from("_site/b.html")]);
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

    /// A file copied up unchanged (opened for writing, nothing written) is
    /// not a change; one whose bytes differ is.
    #[test]
    fn files_copied_up_unchanged_are_not_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let upper = tmp.path().join("upper");
        let root = tmp.path().join("root");
        fs::create_dir_all(&upper).unwrap();
        fs::create_dir_all(&root).unwrap();
        for (name, real, written) in [("same", "abc", "abc"), ("edited", "abc", "abd")] {
            fs::write(root.join(name), real).unwrap();
            fs::write(upper.join(name), written).unwrap();
        }
        let mut changes = Vec::new();
        collect_changes(&upper, Path::new(""), &root, &mut changes).unwrap();
        assert_eq!(changes, vec![Change::Written(PathBuf::from("edited"))]);
    }

    /// In a directory that replaces a real one, an identical file is still
    /// written: applying the replacement removes the real directory first.
    #[test]
    fn identical_files_in_a_replaced_directory_are_written() {
        let tmp = tempfile::tempdir().unwrap();
        let upper = tmp.path().join("upper");
        let root = tmp.path().join("root");
        fs::create_dir_all(upper.join("marp")).unwrap();
        fs::create_dir_all(root.join("marp")).unwrap();
        fs::write(root.join("marp/a.css"), "a").unwrap();
        fs::write(upper.join("marp/a.css"), "a").unwrap();
        xattr::set(upper.join("marp"), OPAQUE_XATTR, b"y").unwrap();
        let mut changes = Vec::new();
        collect_changes(&upper, Path::new(""), &root, &mut changes).unwrap();
        assert_eq!(
            changes,
            vec![
                Change::Replaced(PathBuf::from("marp")),
                Change::Written(PathBuf::from("marp/a.css")),
            ]
        );
    }
}
