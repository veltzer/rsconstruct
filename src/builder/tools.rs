use std::collections::BTreeMap;
// Aliased: `std::io::Write` is also in scope here (for stdout flushing), and
// both traits define `write!`/`writeln!` methods.
use super::{Builder, sorted_keys};
use crate::cli::{GraphFormat, ToolAction};
use crate::color;
use crate::json_output;
use crate::tables;
use crate::tool_lock;
use anyhow::{Context, Result, bail};
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::process::Command;

/// Check if a system package is installed using the platform's package manager.
/// Tries dpkg-query (Debian/Ubuntu), rpm (Fedora/RHEL), pacman (Arch), and brew (macOS).
///
/// Probes run through the central runner so a `tool check` over a long
/// package list stays interruptible — `dpkg-query` on a large system is not
/// instant, and one probe runs per declared package.
///
/// Shared with `doctor`: a `[dependencies] system` entry is a package, not a
/// tool, so every probe of one must go through the package manager. Checking
/// it with `which()` misreports in both directions — a binary-less package
/// like aspell-en shows as missing forever, and a binary that happens to share
/// the package's name shows as installed without the package being on.
pub(super) fn is_system_package_installed(
    ctx: &crate::build_context::BuildContext,
    pkg: &str,
) -> bool {
    let probe = |program: &str, args: &[&str]| -> bool {
        let mut cmd = Command::new(program);
        cmd.args(args);
        crate::processor::run_command_capture(ctx, &cmd).is_ok_and(|o| o.status.success())
    };

    if which::which("dpkg-query").is_ok() {
        // Exit status alone is not enough. dpkg-query exits 0 for a package it
        // merely knows about -- one removed earlier, or named as a dependency --
        // printing "unknown ok not-installed" or "deinstall ok config-files".
        // Only "install ok installed" means the files are actually on disk.
        //
        // Trusting the exit code made install-deps silently skip aspell-en on a
        // runner that had neither aspell nor aspell-en, so apt was never asked
        // for it and the spellcheck processor then failed on missing
        // /usr/lib/aspell/en.dat. Packages that ship no binary of their own are
        // the ones this hides, because the which() fallback cannot catch them.
        let mut cmd = Command::new("dpkg-query");
        cmd.args(["-W", "-f", "${Status}", pkg]);
        return crate::processor::run_command_capture(ctx, &cmd).is_ok_and(|o| {
            o.status.success() && dpkg_status_means_installed(&String::from_utf8_lossy(&o.stdout))
        });
    }
    if which::which("rpm").is_ok() {
        return probe("rpm", &["-q", pkg]);
    }
    if which::which("pacman").is_ok() {
        return probe("pacman", &["-Q", pkg]);
    }
    if which::which("brew").is_ok() {
        return probe("brew", &["list", pkg]);
    }
    // Fallback: check if the package name is available as a command
    which::which(pkg).is_ok()
}

/// Whether a `dpkg-query -f '${Status}'` line means the package is on disk.
///
/// dpkg's status is three words: want, error, state. Only the `installed`
/// state means the files are present -- `not-installed` and `config-files`
/// both leave nothing usable behind, yet dpkg-query still exits 0 for them.
fn dpkg_status_means_installed(status: &str) -> bool {
    status.split_whitespace().nth(2) == Some("installed")
}

/// Whether a `required_tools()` entry names something the tool registry could
/// ever install.
///
/// A processor `command` that contains a path separator is a repo-local script
/// (`scripts/check_md.py`), not a binary on `$PATH` — see `ToolInfo::name` in
/// `src/tools.rs` for why a name with a `/` cannot serve as a registry key.
/// Such a command is checked into the repo and needs no installing, so
/// `tool install` must skip it rather than fail with "No install method".
/// The build-time preflight in `builder/build.rs` still requires it to exist,
/// so skipping it here does not weaken strictness.
fn is_installable_tool_name(tool: &str) -> bool {
    !tool.contains('/')
}

/// Whether a package manager reports `pkg` as already installed.
///
/// One helper for the pip/npm/gem probes, which were four copies of the same
/// spawn-and-discard-output block. Routed through the central runner so a
/// long `install-deps` preflight stays interruptible.
fn package_probe_succeeds(
    ctx: &crate::build_context::BuildContext,
    program: &str,
    args: &[&str],
) -> bool {
    let mut cmd = Command::new(program);
    cmd.args(args);
    crate::processor::run_command_capture(ctx, &cmd).is_ok_and(|o| o.status.success())
}

/// The project's pinned Python closure as `uv export` renders it from
/// uv.lock — one requirement per line, environment markers included.
/// `--frozen` keeps the lock untouched (and fails when it is out of sync
/// with pyproject.toml instead of silently re-resolving).
fn uv_export_reqs(ctx: &crate::build_context::BuildContext) -> Result<Vec<String>> {
    let mut cmd = Command::new("uv");
    cmd.args([
        "export",
        "--frozen",
        "--no-hashes",
        "--no-annotate",
        "--no-header",
        "--no-emit-project",
    ]);
    let out = crate::processor::run_command_capture(ctx, &cmd).context(
        "failed to run `uv export` — is uv installed? (`rsconstruct tool install uv`, \
                  or set `python_installer = \"pip\"` under [dependencies] to install with pip)",
    )?;
    if !out.status.success() {
        bail!(
            "`uv export` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(crate::config::parse_uv_export(&String::from_utf8_lossy(
        &out.stdout,
    )))
}

/// Return the language runtime category for a tool.
/// Delegates to the central `TOOLS` registry in `src/tools.rs`.
fn tool_runtime(tool: &str) -> &'static str {
    crate::tools::tool_runtime(tool).unwrap_or_else(|| {
        debug_assert!(false, "tool_runtime: unrecognized tool '{tool}'");
        "system"
    })
}

/// Whether the instance `name` takes part in builds: its stanza does not
/// say `enabled = false`.
///
/// `create_processors` instantiates every declared stanza, disabled ones
/// included, so each command that walks the map decides for itself. The
/// tool commands, `doctor` and the build-time lock check all go through
/// this one predicate: a disabled stanza exists precisely to keep its
/// config while its tool is absent, so none of them may demand that tool.
pub(super) fn instance_enabled(processors: &crate::processor::ProcessorMap, name: &str) -> bool {
    processors[name].scan_config().enabled
}

/// How many processors a `tool list` row names before summarizing the rest
/// as "+N more": `python3` alone is run by 13, and naming them all made the
/// table several terminal widths wide. `--json` always carries the full list.
const TOOL_LIST_MAX_PROCESSORS: usize = 2;

/// The Install cell of `tool list` and `tool list-configured`: the command
/// of the tool's preferred method, or with `-M` every method it lists, each
/// labeled with the slots it fills (`alt` for an alternative). "?" for a
/// tool the registry does not know.
fn install_cell(info: Option<&crate::tools::ToolInfo>, show_methods: bool) -> String {
    let Some(info) = info else {
        return "?".to_string();
    };
    if !show_methods {
        return info
            .preferred_method()
            .map_or_else(|| "?".to_string(), crate::tools::InstallMethod::command);
    }
    info.methods()
        .into_iter()
        .map(|(m, targets)| {
            let slots = if targets.is_empty() {
                "alt".to_string()
            } else {
                targets
                    .iter()
                    .map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join("+")
            };
            format!("{slots} {}: {}", m.method, m.command())
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Handle `rsconstruct tool` subcommands without a project config.
/// Uses default processor configs so List, Stats, Install, and Graph work
/// even outside a project directory.
pub fn tools_no_config(
    ctx: &crate::build_context::BuildContext,
    action: ToolAction,
    verbose: bool,
) -> Result<()> {
    let processors = super::create_all_default_processors()?;
    run_tools_command(
        ctx,
        &processors,
        &|name| instance_enabled(&processors, name),
        action,
        verbose,
        None,
    )
}

/// What the project's rsconstruct.lock must hold, for the processors
/// `is_enabled` selects.
fn project_wanted(
    builder: &Builder,
    processors: &crate::processor::ProcessorMap,
    is_enabled: &dyn Fn(&str) -> bool,
) -> Result<tool_lock::Wanted> {
    tool_lock::collect_wanted(
        processors,
        is_enabled,
        &builder.config.dependencies,
        std::path::Path::new("."),
    )
}

/// The project's lock, synced for an install: filled in outside CI, required
/// to match in CI. Also returns the locked tools installed outside the
/// project's tool directory whose installed copy is not the locked one and
/// may be replaced: Python packages when `python3` is the project's own
/// virtualenv (or in CI), `.deb` binaries only in CI — outside it they
/// belong to the machine, and replacing one would change it under every
/// other project. Those left alone are reported, never passed off as
/// installed. The project's tool directory is not judged here: syncing it
/// with the lock is what installing does.
fn lock_for_install(
    ctx: &crate::build_context::BuildContext,
    wanted: &tool_lock::Wanted,
    verbose: bool,
) -> Result<(
    tool_lock::ToolLock,
    std::collections::BTreeSet<(String, String)>,
)> {
    let mode = tool_lock::LockMode::for_environment();
    let synced = tool_lock::sync(ctx, wanted, mode, verbose)?;
    synced.report();
    let in_ci = mode == tool_lock::LockMode::Locked;
    let python_replaceable = in_ci || crate::tools::python_env_is_inside(std::path::Path::new("."));
    let mut drift = std::collections::BTreeSet::new();
    for m in tool_lock::mismatches(ctx, &synced.lock, wanted) {
        let (replaceable, why_not) = match m.method.as_str() {
            "pip" => (
                python_replaceable,
                "python3 is not this project's virtualenv",
            ),
            "binary" if !crate::tools::binary_is_repo_local(&m.name) => {
                (in_ci, "a system package, replaced only in CI")
            }
            _ => continue,
        };
        if replaceable {
            drift.insert((m.method, m.name));
        } else if m.installed.is_some() {
            crate::output::warn(&format!("{m}: left alone ({why_not})"));
        }
    }
    Ok((synced.lock, drift))
}

/// The project's tool directory: what it must hold, and how that compares
/// with what it holds now.
struct ProjectTools {
    packages: std::collections::BTreeSet<crate::tool_store::Package>,
    plan: crate::tool_store::Plan,
}

impl ProjectTools {
    /// Whether the tool directory installs `package` for `method`.
    fn covers(&self, method: &str, package: &str) -> bool {
        self.packages
            .iter()
            .any(|p| p.method == method && p.name == package)
    }
}

/// What the project's tool directory must hold: every locked crate, npm
/// package, gem and non-`.deb` binary at its locked version, plus the
/// `[dependencies]` entries that name their own version.
fn project_tools(
    builder: &Builder,
    wanted: &tool_lock::Wanted,
    lock: &tool_lock::ToolLock,
) -> Result<ProjectTools> {
    use crate::tool_store::Package;
    let mut packages = std::collections::BTreeSet::new();
    for (method, names) in [
        ("cargo", &wanted.cargo),
        ("gem", &wanted.gem),
        ("npm", &wanted.npm),
    ] {
        for name in names {
            let version = lock.version(method, name).with_context(|| {
                format!("{method} {name} has no entry in {}", tool_lock::LOCK_FILE)
            })?;
            packages.insert(Package::new(method, name, version));
        }
    }
    for name in &wanted.binary {
        if !crate::tools::binary_is_repo_local(name) {
            continue;
        }
        let version = lock
            .binary
            .get(name)
            .and_then(tool_lock::binary_version)
            .with_context(|| {
                format!(
                    "binary {name} has no executable hash in {}",
                    tool_lock::LOCK_FILE
                )
            })?;
        packages.insert(Package::new("binary", name, &version));
    }
    let deps = &builder.config.dependencies;
    for (method, list) in [
        ("cargo", &deps.cargo),
        ("gem", &deps.gem),
        ("npm", &deps.npm),
    ] {
        for request in list {
            if let Some((name, version)) = tool_lock::split_versioned(method, request) {
                packages.insert(Package::new(method, &name, &version));
            }
        }
    }
    let plan = crate::tool_store::plan(std::path::Path::new("."), &packages);
    Ok(ProjectTools { packages, plan })
}

/// Print the project's tool directory as an install is about to leave it:
/// `✓` in place, `+` to be linked in, `-` to be removed. Nothing when the
/// project installs nothing there.
fn print_project_tools(tools: &ProjectTools) {
    let plan = &tools.plan;
    if tools.packages.is_empty() && plan.stale.is_empty() {
        return;
    }
    println!(
        "{}:",
        color::bold(&format!(
            "Project tools ({})",
            crate::tool_store::REPO_TOOLS_DIR
        ))
    );
    for pkg in &plan.present {
        println!("  {} {}", color::green("✓"), pkg);
    }
    for pkg in &plan.missing {
        println!("  {} {}", color::bold("+"), pkg);
    }
    for stale in &plan.stale {
        println!("  {} {}", color::bold("-"), stale);
    }
    println!();
}

/// Bring the project's tool directory in line with `tools`, installing into
/// the tool store whatever it lacks.
fn apply_project_tools(
    tools: &ProjectTools,
    lock: &tool_lock::ToolLock,
    verbose: bool,
) -> Result<()> {
    if tools.packages.is_empty() && tools.plan.stale.is_empty() {
        return Ok(());
    }
    let store = crate::tool_store::store_root()?;
    let ctx = crate::tools::InstallCtx {
        verbose,
        use_eatmydata: false,
        binary_pins: lock.binary.clone(),
    };
    let install = |pkg: &crate::tool_store::Package, prefix: &std::path::Path| {
        crate::tools::install_into_prefix(pkg, prefix, &ctx)
    };
    crate::tool_store::sync(
        std::path::Path::new("."),
        &store,
        &tools.packages,
        &install,
        &mut |pkg, fresh| {
            let how = if fresh {
                "installed into the tool store, then linked"
            } else {
                "linked from the tool store"
            };
            println!(
                "{} {} {}",
                color::green("✓"),
                pkg,
                color::dim(&format!("({how})"))
            );
        },
    )
}

/// Ask `Proceed? [y/N]` on the terminal; true on yes.
fn confirm() -> Result<bool> {
    print!("Proceed? [y/N] ");
    std::io::stdout()
        .flush()
        .context("Failed to flush stdout before install prompt")?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("Failed to read install prompt answer")?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// The key a `(method, package)` install has in a mismatch set: Python
/// names are normalized there, like in the lock.
fn drift_key(method: &str, package: &str) -> (String, String) {
    match method {
        "pip" | "uv" => (
            "pip".to_string(),
            crate::config::normalized_distribution_name(package),
        ),
        _ => (method.to_string(), package.to_string()),
    }
}

impl Builder {
    /// Check that rsconstruct.lock covers the processors `is_active` selects
    /// and that their tools are installed at the locked versions. Run by
    /// `build` in CI and with `--verify-tool-versions`, after discovery and
    /// the `-p`/`-x` selection, over the processors with products in the
    /// graph — the same set the missing-tool preflight checks. Entries the
    /// lock holds for other processors are not judged here; `tool check` is
    /// the exact, whole-project test.
    pub(super) fn verify_tool_versions(
        &self,
        ctx: &crate::build_context::BuildContext,
        processors: &crate::processor::ProcessorMap,
        is_active: &dyn Fn(&str) -> bool,
    ) -> Result<()> {
        let wanted = project_wanted(self, processors, is_active)?;
        tool_lock::verify(ctx, &wanted, tool_lock::Coverage::AtLeast).map(|_| ())
    }

    /// Handle `rsconstruct tool` subcommands
    pub fn tools(
        &self,
        ctx: &crate::build_context::BuildContext,
        action: ToolAction,
        verbose: bool,
    ) -> Result<()> {
        let processors = self.create_processors()?;
        run_tools_command(
            ctx,
            &processors,
            &|name| instance_enabled(&processors, name),
            action,
            verbose,
            Some(self),
        )
    }
}

/// Core implementation for `tools` subcommands, shared by `Builder::tools()` and `tools_no_config()`.
/// `builder` is `Some` when running with a project config (needed for `open_file`, `Check`, `Lock`).
/// `is_enabled` selects the processors whose tools count: `list-configured
/// --all` is the one action that looks past it.
fn run_tools_command(
    ctx: &crate::build_context::BuildContext,
    processors: &crate::processor::ProcessorMap,
    is_enabled: &dyn Fn(&str) -> bool,
    action: ToolAction,
    verbose: bool,
    builder: Option<&Builder>,
) -> Result<()> {
    let show_all = matches!(&action, ToolAction::ListConfigured { all: true, .. });
    let show_methods = matches!(
        &action,
        ToolAction::ListConfigured { methods: true, .. } | ToolAction::List { methods: true }
    );
    let install_interactive = matches!(
        &action,
        ToolAction::Install {
            interactive: true,
            ..
        }
    );
    let install_no_eatmydata = matches!(
        &action,
        ToolAction::Install {
            no_eatmydata: true,
            ..
        }
    );
    // eatmydata speeds up apt/dnf/pacman by no-op'ing fsync; the trade-off
    // is loss-on-power-cut. Off by default; the post-config phase hook
    // `eatmydata_ci_default` flips it on when CI=true. Users who want it
    // off in CI should unset CI; users who want it on outside CI should
    // run with CI=true. The CLI flag --no-eatmydata always wins.
    // `--all` is config-independent by design, so there may be no Config for
    // the `eatmydata_ci_default` hook to have run against. Apply the same
    // CI=true policy directly, otherwise the one caller that most wants the
    // speedup (a CI runner with no rsconstruct.toml) would never get it.
    let install_all = matches!(&action, ToolAction::Install { all: true, .. });
    let config_enables_eatmydata = builder.is_some_and(|b| b.config.dependencies.eatmydata)
        || (install_all && crate::config::running_in_ci());
    let install_use_eatmydata =
        !install_no_eatmydata && config_enables_eatmydata && which::which("eatmydata").is_ok();

    let mut tool_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in sorted_keys(processors) {
        if !show_all && !is_enabled(name) {
            continue;
        }
        for tool in processors[name].required_tools() {
            let procs = tool_map.entry(tool).or_default();
            if !procs.contains(name) {
                procs.push(name.clone());
            }
        }
    }

    match action {
        ToolAction::List { .. } => {
            // Registry-wide list: every tool rsconstruct knows how to install,
            // independent of which processors are configured. Mirrors
            // `processor list`, which shows all built-in processors. So the
            // Processors column comes from every processor at its default
            // config, not from the project's: a tool no processor runs is one
            // a project selects through config (`required_tools`, a cc
            // profile) or that rsconstruct runs itself.
            let all_processors = super::create_all_default_processors()?;
            let mut runners: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            for name in sorted_keys(&all_processors) {
                for tool in all_processors[name].required_tools() {
                    if let Some(info) = crate::tools::tool_info(&tool) {
                        runners.entry(info.name).or_default().push(name.clone());
                    }
                }
            }
            if crate::json_output::is_json_mode() {
                let entries: Vec<json_output::ToolListEntry> = crate::tools::all_tools()
                    .map(|info| {
                        json_output::ToolListEntry::new(
                            info.name,
                            Some(info),
                            runners.get(info.name).cloned().unwrap_or_default(),
                        )
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&entries)?);
                return Ok(());
            }

            let mut tools: Vec<&crate::tools::ToolInfo> = crate::tools::all_tools().collect();
            tools.sort_by_key(|t| t.name);

            // The install command is noise for the common "what tools exist?"
            // question, so the Install column is hidden by default — shown
            // with --verbose (or --methods for every method), or when --all
            // or the user's [tables] config asks for it. JSON always carries it.
            let show_install = verbose || show_methods;
            let rows: Vec<Vec<String>> = tools
                .iter()
                .map(|info| {
                    let status = if which::which(info.name).is_ok() {
                        color::green("installed")
                    } else {
                        color::red("missing")
                    };
                    let procs = runners.get(info.name).map_or_else(
                        || color::yellow("(none)").to_string(),
                        |names| tables::processor_names_cell(names, TOOL_LIST_MAX_PROCESSORS),
                    );
                    let target = info
                        .preferred_target()
                        .map_or("?", crate::tools::InstallTarget::as_str);
                    vec![
                        info.name.to_string(),
                        status.to_string(),
                        info.runtime.to_string(),
                        target.to_string(),
                        procs,
                        install_cell(Some(info), show_methods),
                    ]
                })
                .collect();
            tables::print(&tables::defs::TOOL_LIST, show_install, &rows);
        }
        ToolAction::ListConfigured { .. } => {
            if crate::json_output::is_json_mode() {
                let entries: Vec<json_output::ToolListEntry> = tool_map
                    .iter()
                    .map(|(tool, procs)| {
                        json_output::ToolListEntry::new(
                            tool,
                            crate::tools::tool_info(tool),
                            procs.clone(),
                        )
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&entries)?);
                return Ok(());
            }

            for (tool, procs) in &tool_map {
                let installed = if which::which(tool).is_ok() {
                    color::green("installed")
                } else {
                    color::red("missing")
                };
                let info = crate::tools::tool_info(tool);
                let runtime = info.map_or("unknown", |i| i.runtime);
                let install_str = install_cell(info, show_methods);
                println!(
                    "{} [{}] [{}] ({}) — {}",
                    tool,
                    installed,
                    runtime,
                    procs.join(", "),
                    color::dim(&install_str)
                );
            }
        }
        ToolAction::Check => {
            let builder = builder.ok_or_else(|| {
                anyhow::anyhow!("tool check requires a project with rsconstruct.toml")
            })?;
            let wanted = project_wanted(builder, processors, is_enabled)?;
            tool_lock::verify(ctx, &wanted, tool_lock::Coverage::Exact)?;
            if crate::json_output::is_json_mode() {
                let out = serde_json::json!({
                    "lock_file": tool_lock::LOCK_FILE,
                    "match": true,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!(
                    "{}",
                    color::green(&format!("Installed tools match {}.", tool_lock::LOCK_FILE))
                );
            }
        }
        ToolAction::Lock { upgrade } => {
            let builder = builder.ok_or_else(|| {
                anyhow::anyhow!("tool lock requires a project with rsconstruct.toml")
            })?;
            let wanted = project_wanted(builder, processors, is_enabled)?;
            let synced = match upgrade {
                None => tool_lock::sync(ctx, &wanted, tool_lock::LockMode::Fill, verbose)?,
                Some(names) if names.is_empty() => {
                    tool_lock::sync(ctx, &wanted, tool_lock::LockMode::Upgrade, verbose)?
                }
                Some(names) => tool_lock::upgrade_named(ctx, &wanted, &names, verbose)?,
            };
            if crate::json_output::is_json_mode() {
                let out = serde_json::json!({
                    "lock_file": tool_lock::LOCK_FILE,
                    "created": synced.created,
                    "changes": synced.changes,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else if synced.changes.is_empty() && !synced.created {
                println!("{} is up to date.", color::bold(tool_lock::LOCK_FILE));
            } else {
                synced.report();
            }
        }
        ToolAction::Graph { format, view } => {
            if view {
                let html_content = tools_graph_html(&tool_map);
                let html_path = std::env::temp_dir().join("rsconstruct_tools_graph.html");
                std::fs::write(&html_path, &html_content).with_context(|| {
                    format!("Failed to write HTML file: {}", html_path.display())
                })?;
                if let Some(b) = builder {
                    b.open_file(&html_path)?;
                    println!("Opened tools graph in browser: {}", html_path.display());
                } else {
                    println!("Wrote tools graph to: {}", html_path.display());
                }
            } else {
                let effective_format = if crate::json_output::is_json_mode() {
                    GraphFormat::Json
                } else {
                    format
                };
                let output = match effective_format {
                    GraphFormat::Dot => tools_graph_dot(&tool_map),
                    GraphFormat::Mermaid => tools_graph_mermaid(&tool_map),
                    GraphFormat::Text => tools_graph_text(&tool_map),
                    GraphFormat::Json => tools_graph_json(&tool_map)?,
                    GraphFormat::Svg => tools_graph_svg(ctx, &tool_map)?,
                };
                println!("{output}");
            }
        }
        ToolAction::Stats => {
            let mut tool_stats: Vec<json_output::ToolStat> = Vec::new();
            for (tool, procs) in &tool_map {
                let installed = which::which(tool).is_ok();
                let runtime = tool_runtime(tool).to_string();
                let install_command = crate::tools::tool_install_command(tool);
                tool_stats.push(json_output::ToolStat {
                    name: tool.clone(),
                    installed,
                    runtime,
                    processor: procs.clone(),
                    install_command,
                });
            }

            let mut runtime_map: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            for stat in &tool_stats {
                let entry = runtime_map
                    .entry(tool_runtime(&stat.name))
                    .or_insert((0, 0));
                entry.0 += 1;
                if stat.installed {
                    entry.1 += 1;
                }
            }
            let runtime_stats: Vec<json_output::RuntimeStat> = runtime_map
                .iter()
                .map(|(runtime, (total, installed))| json_output::RuntimeStat {
                    runtime: runtime.to_string(),
                    total: *total,
                    installed: *installed,
                    missing: total - installed,
                })
                .collect();

            let total_tools = tool_stats.len();
            let installed_count = tool_stats.iter().filter(|t| t.installed).count();
            let missing_count = total_tools - installed_count;

            if crate::json_output::is_json_mode() {
                let output = json_output::ToolStatsOutput {
                    tools: tool_stats,
                    runtimes: runtime_stats,
                    summary: json_output::StatsSummary {
                        total_tools,
                        installed: installed_count,
                        missing: missing_count,
                    },
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                let rows: Vec<Vec<String>> = tool_stats
                    .iter()
                    .map(|stat| {
                        let status = if stat.installed {
                            color::green("\u{2713}")
                        } else {
                            color::red("\u{2717}")
                        };
                        let procs = stat
                            .processor
                            .iter()
                            .map(|n| tables::short_processor_name(n))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let install = stat.install_command.as_deref().unwrap_or("").to_string();
                        vec![stat.name.clone(), status.to_string(), procs, install]
                    })
                    .collect();
                tables::print(&tables::defs::TOOL_STATS, false, &rows);

                println!();
                println!("Runtime summary:");
                let runtime_display: &[(&str, &str)] = &[
                    ("python", "Python"),
                    ("node", "Node.js"),
                    ("ruby", "Ruby"),
                    ("rust", "Rust"),
                    ("perl", "Perl"),
                    ("jvm", "JVM"),
                    ("system", "System"),
                ];
                let rt_rows: Vec<Vec<String>> = runtime_display
                    .iter()
                    .filter_map(|(key, label)| {
                        runtime_stats.iter().find(|r| r.runtime == *key).map(|rs| {
                            let status_str = format!("{}/{}", rs.installed, rs.total);
                            let line = if rs.missing > 0 {
                                color::yellow(&status_str)
                            } else {
                                color::green(&status_str)
                            };
                            vec![label.to_string(), line.to_string()]
                        })
                    })
                    .collect();
                tables::print(&tables::defs::TOOL_RUNTIMES, false, &rt_rows);

                println!();
                let total_line = format!("Total: {installed_count}/{total_tools} tools installed");
                if missing_count > 0 {
                    println!("{}", color::yellow(&total_line));
                } else {
                    println!("{}", color::green(&total_line));
                }
            }
        }
        ToolAction::Install { name, all, .. } => {
            // In a project, installs follow rsconstruct.lock. `--all` is the
            // config-independent registry sweep and has no lock to follow.
            let (lock, drift, wanted) = match builder {
                Some(b) if !all => {
                    let wanted = project_wanted(b, processors, is_enabled)?;
                    let (lock, drift) = lock_for_install(ctx, &wanted, verbose)?;
                    (Some(lock), drift, Some(wanted))
                }
                _ => (None, std::collections::BTreeSet::new(), None),
            };
            // A plain `tool install` in a project syncs the project's tool
            // directory with the lock (crates, npm packages, gems, release
            // binaries) and installs only the rest onto the machine. A named
            // tool or `--all` is a machine install and does not touch it.
            let project = match (builder, &lock, &wanted) {
                (Some(b), Some(lock), Some(wanted)) if name.is_none() => {
                    let project = project_tools(b, wanted, lock)?;
                    print_project_tools(&project);
                    if install_interactive && project.plan.has_work() && !confirm()? {
                        println!("Aborted.");
                        return Ok(());
                    }
                    apply_project_tools(&project, lock, verbose)?;
                    Some(project)
                }
                _ => None,
            };
            // Collect missing tools with their install info
            let missing_tools: Vec<(&str, &crate::tools::InstallMethod)> = if all {
                // Registry-wide: every tool rsconstruct knows how to install,
                // independent of any config. Mirrors `tool list`, which is the
                // registry-wide counterpart of `tool list-configured`.
                //
                // Nothing is skipped. A tool with no automatable method is a
                // hard error, not a warning to scroll past: --all claiming
                // success while a tool is absent is exactly the silent-gap
                // failure the strict-by-default rule exists to prevent.
                let mut missing = Vec::new();
                let mut manual_only: Vec<&crate::tools::ToolInfo> = Vec::new();
                for info in crate::tools::all_tools() {
                    if which::which(info.name).is_ok() {
                        continue;
                    }
                    // The same choice every other install path makes: the
                    // method in the slot of the tool's preferred target. A
                    // tool with no slot has none (the test
                    // `every_slot_holds_a_method_for_its_target` rules that
                    // out).
                    match info.preferred_method() {
                        Some(method) => missing.push((info.name, method)),
                        None => manual_only.push(info),
                    }
                }
                if !manual_only.is_empty() {
                    eprintln!(
                        "{}: {} tool(s) have no automatable install method:",
                        color::red("Error"),
                        manual_only.len(),
                    );
                    for info in &manual_only {
                        let instructions = info
                            .alternatives
                            .first()
                            .map_or("no install method known", |m| m.package);
                        eprintln!("  {} — {}", color::bold(info.name), instructions);
                    }
                    return Err(crate::exit_code::RsconstructError::new(
                        crate::exit_code::RsconstructExitCode::ToolError,
                        format!(
                            "Cannot install {} tool(s) automatically: {}",
                            manual_only.len(),
                            manual_only
                                .iter()
                                .map(|i| i.name)
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                    )
                    .into());
                }
                if missing.is_empty() {
                    println!(
                        "{}",
                        color::green("All registry tools are already installed.")
                    );
                    return Ok(());
                }
                missing
            } else if let Some(ref name) = name {
                // Install a specific tool
                if let Some(info) = crate::tools::tool_info(name) {
                    if let Some(method) = info.preferred_method() {
                        vec![(info.name, method)]
                    } else {
                        eprintln!("{}: No install method for '{}'", color::red("Error"), name);
                        return Err(crate::exit_code::RsconstructError::new(
                            crate::exit_code::RsconstructExitCode::ToolError,
                            format!("No install method known for tool '{name}'"),
                        )
                        .into());
                    }
                } else {
                    eprintln!("{}: Unknown tool '{}'", color::red("Error"), name);
                    return Err(crate::exit_code::RsconstructError::new(
                        crate::exit_code::RsconstructExitCode::ToolError,
                        format!("Unknown tool '{name}'"),
                    )
                    .into());
                }
            } else {
                // Build tool list from the enabled processors. A stanza with
                // `enabled = false` is skipped: it is kept precisely so its
                // tool need not be installed. auto_detect is not checked
                // because the tool itself may need to be installed before
                // detection can succeed.
                let mut install_tools: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for name in sorted_keys(processors) {
                    if !is_enabled(name) {
                        continue;
                    }
                    for tool in processors[name].required_tools() {
                        let procs = install_tools.entry(tool).or_default();
                        if !procs.contains(name) {
                            procs.push(name.clone());
                        }
                    }
                }

                // Collect missing tools
                let mut missing = Vec::new();
                let mut any_unknown = false;
                for tool_name in install_tools.keys() {
                    if !is_installable_tool_name(tool_name) {
                        continue;
                    }
                    // Present on PATH counts as installed, unless it was
                    // found at a version other than the locked one that may
                    // be replaced (see `lock_for_install`). A tool the
                    // project's tool directory holds was synced above.
                    let info = crate::tools::tool_info(tool_name);
                    let method = info.and_then(crate::tools::ToolInfo::preferred_method);
                    if method.is_some_and(|m| {
                        project
                            .as_ref()
                            .is_some_and(|p| p.covers(m.method, m.package))
                    }) {
                        continue;
                    }
                    let drifted =
                        method.is_some_and(|m| drift.contains(&drift_key(m.method, m.package)));
                    if which::which(tool_name).is_ok() && !drifted {
                        continue;
                    }
                    if let (Some(info), Some(method)) = (info, method) {
                        missing.push((info.name, method));
                    } else {
                        eprintln!(
                            "{}: No install method for '{}'",
                            color::red("Error"),
                            tool_name
                        );
                        any_unknown = true;
                    }
                }
                if any_unknown {
                    return Err(crate::exit_code::RsconstructError::new(
                        crate::exit_code::RsconstructExitCode::ToolError,
                        "Some tools have no known install procedure",
                    )
                    .into());
                }
                if missing.is_empty() {
                    println!("{}", color::green("All tools are already installed."));
                    return Ok(());
                }
                missing
            };

            // Group by install method for batch installation. Methods
            // that don't batch (binary, manual) keep one entry per package.
            // Each package is requested at its locked version when the
            // project's lock has one.
            let mut by_method: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            for (_tool_name, method) in &missing_tools {
                let package = lock.as_ref().map_or_else(
                    || method.package.to_string(),
                    |l| l.pinned(method.method, method.package),
                );
                by_method.entry(method.method).or_default().push(package);
            }

            // Display the install plan
            println!(
                "Missing {} tool(s), grouped by install method:",
                missing_tools.len()
            );
            println!();

            let method_order = &[
                "pip", "apt", "brew", "npm", "cargo", "gem", "snap", "binary", "manual",
            ];
            let ctx = crate::tools::InstallCtx {
                verbose,
                use_eatmydata: install_use_eatmydata,
                binary_pins: lock.as_ref().map(|l| l.binary.clone()).unwrap_or_default(),
            };
            let mut any_failed = false;
            for method in method_order {
                let Some(packages) = by_method.get(method) else {
                    continue;
                };
                let packages: Vec<&str> = packages.iter().map(String::as_str).collect();
                println!(
                    "  {} {}",
                    color::bold(&format!("[{method}]")),
                    packages.join(", ")
                );
                for argv in crate::tools::describe(method, &packages, &ctx.binary_pins) {
                    println!("       {}", color::dim(&argv.join(" ")));
                }
            }
            println!();

            // Non-interactive by default: CI and scripts are the common
            // callers and neither can answer a prompt. `-i` opts into the
            // confirmation, and declining it is a normal outcome, not an
            // error — the user saw the command list and said no.
            if install_interactive {
                print!("Proceed? [y/N] ");
                std::io::stdout()
                    .flush()
                    .context("Failed to flush stdout before install prompt")?;
                let mut answer = String::new();
                std::io::stdin()
                    .read_line(&mut answer)
                    .context("Failed to read install prompt answer")?;
                let answer = answer.trim().to_lowercase();
                if answer != "y" && answer != "yes" {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            for method in method_order {
                let Some(packages) = by_method.get(method) else {
                    continue;
                };
                let packages: Vec<&str> = packages.iter().map(String::as_str).collect();
                match crate::tools::run(method, &packages, &ctx) {
                    Ok(()) => println!(
                        "{} [{}] {}",
                        color::green("OK"),
                        method,
                        packages.join(", ")
                    ),
                    Err(e) => {
                        // `{:#}`, not `{}`: the alternate form walks the
                        // whole anyhow cause chain. Plain Display prints
                        // only the outermost context, which turns a real
                        // diagnosis ("... : http status: 403") back into a
                        // bare "Failed to query GitHub releases API".
                        println!(
                            "{} [{}] {}: {:#}",
                            color::red("FAILED"),
                            method,
                            packages.join(", "),
                            e
                        );
                        any_failed = true;
                    }
                }
            }

            if any_failed {
                return Err(crate::exit_code::RsconstructError::new(
                    crate::exit_code::RsconstructExitCode::ToolError,
                    "Some install commands failed",
                )
                .into());
            }
            println!("{}", color::green("All tools installed successfully."));
        }
        ToolAction::InstallDeps {
            interactive,
            no_eatmydata,
        } => {
            let builder = builder.ok_or_else(|| {
                anyhow::anyhow!("install-deps requires a project with rsconstruct.toml")
            })?;
            let config = &builder.config.dependencies;
            // Every package below is requested at its rsconstruct.lock
            // version; in CI one installed at another version is reinstalled.
            let wanted = project_wanted(builder, processors, is_enabled)?;
            let (lock, drift) = lock_for_install(ctx, &wanted, verbose)?;
            // eatmydata: off by default; turned on by the post-config
            // phase hook when CI=true. CLI --no-eatmydata overrides.
            // Wrap also requires eatmydata to be on PATH.
            let use_eatmydata =
                !no_eatmydata && config.eatmydata && which::which("eatmydata").is_ok();

            // The Python set comes from uv.lock by default (pyproject.toml
            // in pyproject mode); [dependencies].pip entries merge in front.
            // The uv installer (the default) takes the lock's pins from
            // `uv export`, whose environment markers let uv skip other
            // platforms' packages; the pip installer flattens the lock.
            let python_installer = config.python_installer;
            let pip_deps = match python_installer {
                crate::config::PythonInstaller::Uv => {
                    // Bootstrap uv with pip when it is not on PATH — CI
                    // runner images ship pip but not necessarily uv (the
                    // ubuntu-26.04 image does not), and requiring every
                    // consuming repo's workflow to install uv first would
                    // put installer knowledge back into the yml this
                    // command exists to keep generic. pip's scripts dir is
                    // already on PATH wherever pip itself runs from, so
                    // the uv binary is spawnable right after.
                    // Only when there is Python work to do — a repo with no
                    // pyproject.toml and no pip list needs no uv.
                    let has_python_deps =
                        std::path::Path::new("pyproject.toml").exists() || !config.pip.is_empty();
                    if has_python_deps && which::which("uv").is_err() {
                        println!("uv not found on PATH; bootstrapping it with pip");
                        crate::tools::run(
                            "pip",
                            &["uv"],
                            &crate::tools::InstallCtx {
                                verbose,
                                use_eatmydata: false,
                                binary_pins: BTreeMap::new(),
                            },
                        )
                        .context(
                            "failed to bootstrap uv with pip; install uv manually \
                                    or set `python_installer = \"pip\"` under [dependencies]",
                        )?;
                    }
                    config.effective_pip_uv(std::path::Path::new("."), || uv_export_reqs(ctx))?
                }
                crate::config::PythonInstaller::Pip => {
                    config.effective_pip(std::path::Path::new("."))?
                }
            };
            // uv.lock's pins already carry `==`, so only the bare
            // `[dependencies].pip` names pick up a version here.
            let pip_deps: Vec<String> = pip_deps.iter().map(|r| lock.pinned("pip", r)).collect();

            // The Node set comes from package.json (and, by default, the
            // closure package-lock.json pins); [dependencies].npm entries
            // are global extras installed alongside.
            let node_deps = config.node_deps(std::path::Path::new("."))?;

            if config.is_empty() && pip_deps.is_empty() && node_deps.is_empty() {
                println!(
                    "No dependencies declared in [dependencies], pyproject.toml or package.json."
                );
                return Ok(());
            }

            // Install order is FIXED and load-bearing: system → pip → npm → gem → cargo.
            // Language-level packages (pip, gem, npm) often build native
            // extensions at install time that link against system libraries
            // via pkg-config — system deps must be present first.
            // See docs/src/configuration.md "Install order".
            let system_method: &str = if which::which("apt-get").is_ok() {
                "apt"
            } else if which::which("dnf").is_ok() {
                "dnf"
            } else if which::which("pacman").is_ok() {
                "pacman"
            } else if which::which("brew").is_ok() {
                "brew"
            } else if !config.system.is_empty() {
                bail!(
                    "No supported package manager found (apt-get, dnf, pacman, brew); install these system packages manually: {}",
                    config.system.join(", ")
                );
            } else {
                "apt" // unused
            };

            let mut skipped: Vec<String> = Vec::new();
            let mut groups: Vec<(&str, Vec<String>)> = Vec::new();

            let system_missing: Vec<String> = config
                .system
                .iter()
                .filter(|pkg| {
                    let installed = is_system_package_installed(ctx, pkg);
                    if installed {
                        skipped.push(format!("[system] {pkg}"));
                    }
                    !installed
                })
                .cloned()
                .collect();
            if !system_missing.is_empty() {
                groups.push((system_method, system_missing));
            }

            match python_installer {
                crate::config::PythonInstaller::Uv => {
                    // No per-package probing: uv audits the whole set itself
                    // in one pass, faster and version-aware where `pip show`
                    // only proves some version of the name is present.
                    if !pip_deps.is_empty() {
                        groups.push(("uv", pip_deps));
                    }
                }
                crate::config::PythonInstaller::Pip => {
                    let pip_missing: Vec<String> = pip_deps
                        .iter()
                        .filter(|pkg| {
                            // `pip show` only proves the base distribution is present;
                            // it says nothing about an extras entry like `pkg[extra]`,
                            // whose extra dependencies may still be missing. Always
                            // hand extras entries to pip — the install is idempotent.
                            if pkg.contains('[') {
                                return true;
                            }
                            let name = crate::config::normalized_distribution_name(pkg);
                            let installed =
                                package_probe_succeeds(ctx, "pip", &["show", name.as_str()])
                                    && !drift.contains(&drift_key("pip", pkg));
                            if installed {
                                skipped.push(format!("[pip] {pkg}"));
                            }
                            !installed
                        })
                        .cloned()
                        .collect();
                    if !pip_missing.is_empty() {
                        groups.push(("pip", pip_missing));
                    }
                }
            }

            // The project's own Node.js set: package.json declares the
            // names, and in package-lock mode `npm ci` installs exactly the
            // pinned closure into ./node_modules (the PATH augmentation at
            // startup makes its .bin resolvable to the NEXT invocation).
            // Skipped only when every declared package is already there at
            // the locked version; `npm ci` wipes node_modules first, so a
            // needless run is not cheap.
            if !node_deps.is_empty() {
                let root = std::path::Path::new(".");
                let (method, up_to_date) = match config.npm_source {
                    crate::config::NpmSource::PackageLock => {
                        let pins =
                            crate::config::package_lock_pins(&root.join("package-lock.json"))?;
                        let up_to_date = node_deps.iter().all(|name| {
                            let installed =
                                crate::config::installed_node_package_version(root, name);
                            installed.is_some() && installed == pins.get(name).cloned()
                        });
                        ("npm-ci", up_to_date)
                    }
                    crate::config::NpmSource::PackageJson => {
                        let up_to_date = node_deps.iter().all(|name| {
                            crate::config::installed_node_package_version(root, name).is_some()
                        });
                        ("npm-install", up_to_date)
                    }
                };
                if up_to_date {
                    for name in &node_deps {
                        skipped.push(format!("[{method}] {name}"));
                    }
                } else {
                    groups.push((method, node_deps));
                }
            }

            // `[dependencies]` cargo, npm and gem entries, and every other
            // locked crate, npm package, gem and release binary, go into the
            // project's tool directory. That is synced last: crates and gems
            // may build native code against the system packages above.
            let project = project_tools(builder, &wanted, &lock)?;

            if !skipped.is_empty() {
                println!("{}:", color::dim("Already installed (skipping)"));
                for s in &skipped {
                    println!("  {} {}", color::green("✓"), s);
                }
                println!();
            }
            print_project_tools(&project);

            if groups.is_empty() && !project.plan.has_work() {
                // Nothing to install, but bin/ is rebuilt all the same.
                apply_project_tools(&project, &lock, verbose)?;
                println!("{}", color::green("All dependencies already installed."));
                return Ok(());
            }

            let ctx = crate::tools::InstallCtx {
                verbose,
                use_eatmydata,
                binary_pins: lock.binary.clone(),
            };
            // The project tools to install were listed above; this is the
            // rest, when there is any.
            if !groups.is_empty() {
                println!("{}:", color::bold("Dependencies to install"));
                for (method, packages) in &groups {
                    let pkgs_ref: Vec<&str> = packages.iter().map(String::as_str).collect();
                    let preview = crate::tools::describe(method, &pkgs_ref, &ctx.binary_pins);
                    let line = preview.first().map(|a| a.join(" ")).unwrap_or_default();
                    println!(
                        "  {} {}",
                        color::bold(&format!("[{method}]")),
                        color::dim(&line)
                    );
                }
                println!();
            }

            // Non-interactive by default; see the note in Install above.
            if interactive {
                use std::io::Write;
                print!("Proceed? [y/N] ");
                std::io::stdout()
                    .flush()
                    .context("Failed to flush stdout before dependency install prompt")?;
                let mut answer = String::new();
                std::io::stdin()
                    .read_line(&mut answer)
                    .context("Failed to read dependency install prompt answer")?;
                if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            let mut any_failed = false;
            for (method, packages) in &groups {
                let pkgs_ref: Vec<&str> = packages.iter().map(String::as_str).collect();
                match crate::tools::run(method, &pkgs_ref, &ctx) {
                    Ok(()) => {
                        println!("{} [{}] {}", color::green("✓"), method, packages.join(", "));
                    }
                    Err(e) => {
                        // `{:#}` walks the whole cause chain; see the note
                        // in Install above.
                        println!(
                            "{} [{}] {}: {:#}",
                            color::red("✗"),
                            method,
                            packages.join(", "),
                            e
                        );
                        any_failed = true;
                    }
                }
            }
            if let Err(e) = apply_project_tools(&project, &lock, verbose) {
                println!(
                    "{} [{}]: {:#}",
                    color::red("✗"),
                    crate::tool_store::REPO_TOOLS_DIR,
                    e
                );
                any_failed = true;
            }

            if any_failed {
                return Err(crate::exit_code::RsconstructError::new(
                    crate::exit_code::RsconstructExitCode::ToolError,
                    "Some dependency installs failed",
                )
                .into());
            }
            println!(
                "{}",
                color::green("All dependencies installed successfully.")
            );
        }
    }

    Ok(())
}

/// Sanitize a name for use as a DOT node identifier
fn sanitize_node_id(prefix: &str, name: &str) -> String {
    format!("{}_{}", prefix, name.replace(['.', '-', ' ', '+'], "_"))
}

fn tools_graph_dot(tool_map: &BTreeMap<String, Vec<String>>) -> String {
    let mut out =
        String::from("digraph tools {\n    rankdir=LR;\n    node [fontname=\"sans-serif\"];\n");

    // Collect unique processor names
    let mut processors: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for procs in tool_map.values() {
        for p in procs {
            processors.insert(p);
        }
    }

    // Tool nodes
    out.push_str("    // Tools\n");
    for tool in tool_map.keys() {
        let id = sanitize_node_id("tool", tool);
        let _ = writeln!(
            out,
            "    {id} [label=\"{tool}\" shape=box style=filled fillcolor=lightblue];"
        );
    }

    // Processor nodes
    out.push_str("    // Processors\n");
    for proc in &processors {
        let id = sanitize_node_id("proc", proc);
        let _ = writeln!(
            out,
            "    {id} [label=\"{proc}\" shape=box style=filled fillcolor=lightyellow];"
        );
    }

    // Edges
    out.push_str("    // Edges\n");
    for (tool, procs) in tool_map {
        let tool_id = sanitize_node_id("tool", tool);
        for proc in procs {
            let proc_id = sanitize_node_id("proc", proc);
            let _ = writeln!(out, "    {tool_id} -> {proc_id};");
        }
    }

    out.push_str("}\n");
    out
}

fn tools_graph_mermaid(tool_map: &BTreeMap<String, Vec<String>>) -> String {
    let mut out = String::from("graph LR\n");

    for (tool, procs) in tool_map {
        let tool_id = sanitize_node_id("tool", tool);
        for proc in procs {
            let proc_id = sanitize_node_id("proc", proc);
            let _ = writeln!(
                out,
                "    {tool_id}[\"{tool}\"]:::tool --> {proc_id}[\"{proc}\"]:::processor"
            );
        }
    }

    out.push_str("    classDef tool fill:#add8e6\n");
    out.push_str("    classDef processor fill:#ffffe0\n");
    out
}

fn tools_graph_text(tool_map: &BTreeMap<String, Vec<String>>) -> String {
    let mut lines = Vec::new();
    for (tool, procs) in tool_map {
        for proc in procs {
            lines.push(format!("{tool} \u{2192} {proc}"));
        }
    }
    lines.join("\n")
}

fn tools_graph_json(tool_map: &BTreeMap<String, Vec<String>>) -> Result<String> {
    let entries: Vec<crate::json_output::ToolListEntry> = tool_map
        .iter()
        .map(|(tool, procs)| {
            crate::json_output::ToolListEntry::new(
                tool,
                crate::tools::tool_info(tool),
                procs.clone(),
            )
        })
        .collect();
    Ok(serde_json::to_string_pretty(&entries)?)
}

fn tools_graph_html(tool_map: &BTreeMap<String, Vec<String>>) -> String {
    let mermaid_content = tools_graph_mermaid(tool_map);
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>RSConstruct Tools Graph</title>
    <script src="https://cdn.jsdelivr.net/npm/mermaid/dist/mermaid.min.js"></script>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            margin: 40px;
            background: #f5f5f5;
        }}
        h1 {{
            color: #333;
        }}
        .mermaid {{
            background: white;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }}
    </style>
</head>
<body>
    <h1>RSConstruct Tools Graph</h1>
    <div class="mermaid">
{mermaid_content}
    </div>
    <script>
        mermaid.initialize({{ startOnLoad: true, theme: 'default', maxTextSize: 500000 }});
    </script>
</body>
</html>
"#
    )
}

fn tools_graph_svg(
    ctx: &crate::build_context::BuildContext,
    tool_map: &BTreeMap<String, Vec<String>>,
) -> Result<String> {
    crate::processor::dot_to_svg(ctx, &tools_graph_dot(tool_map))
}

#[cfg(test)]
mod dpkg_status_tests {
    use super::dpkg_status_means_installed;

    #[test]
    fn installed_package_is_installed() {
        assert!(dpkg_status_means_installed("install ok installed"));
        assert!(dpkg_status_means_installed("install ok installed\n"));
    }

    /// The regression this guards: dpkg-query exits 0 for a package it merely
    /// knows about, so the exit code alone reported aspell-en as satisfied on
    /// a runner that did not have it.
    #[test]
    fn known_but_absent_package_is_not_installed() {
        assert!(!dpkg_status_means_installed("unknown ok not-installed"));
        assert!(!dpkg_status_means_installed("deinstall ok config-files"));
        assert!(!dpkg_status_means_installed("install ok half-configured"));
        assert!(!dpkg_status_means_installed(""));
    }
}
