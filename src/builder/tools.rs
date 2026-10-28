use std::collections::BTreeMap;
// Aliased: `std::io::Write` is also in scope here (for stdout flushing), and
// both traits define `write!`/`writeln!` methods.
use super::{Builder, sorted_keys};
use crate::cli::{GraphFormat, InstallAction, ToolAction};
use crate::color;
use crate::json_output;
use crate::tables;
use crate::tool_lock;
use anyhow::{Context, Result};
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
        // Trusting the exit code made an install silently skip aspell-en on a
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
/// may be replaced: `.deb` binaries, only in CI — outside it they belong to
/// the machine, and replacing one would change it under every other
/// project. Those left alone are reported, never passed off as installed.
/// The project's tool directory is not judged here: syncing it with the
/// lock is what installing does.
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
    let mut drift = std::collections::BTreeSet::new();
    for m in tool_lock::mismatches(ctx, &synced.lock, wanted) {
        if m.method != "binary" || crate::tools::binary_is_repo_local(&m.name) {
            continue;
        }
        if in_ci {
            drift.insert((m.method, m.name));
        } else if m.installed.is_some() {
            crate::output::warn(&format!(
                "{m}: left alone (a system package, replaced only in CI)"
            ));
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

    /// The tool directory with `package` added to what it already holds and
    /// nothing else installed: `tool install project NAME`.
    fn with_only(self, method: &str, package: &str) -> Self {
        let packages: std::collections::BTreeSet<crate::tool_store::Package> = self
            .packages
            .into_iter()
            .filter(|p| (p.method == method && p.name == package) || self.plan.present.contains(p))
            .collect();
        let plan = crate::tool_store::plan(std::path::Path::new("."), &packages);
        Self { packages, plan }
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
        ToolAction::Install { target } => match target {
            InstallAction::Machine {
                name,
                all,
                interactive,
                no_eatmydata,
            } => {
                // eatmydata speeds up apt/dnf/pacman by no-op'ing fsync; the
                // trade-off is loss-on-power-cut. Off by default; the
                // post-config phase hook `eatmydata_ci_default` flips it on
                // when CI=true. Users who want it off in CI should unset CI;
                // users who want it on outside CI should run with CI=true.
                // The CLI flag --no-eatmydata always wins. `--all` is
                // config-independent by design, so there may be no Config for
                // the hook to have run against. Apply the same CI=true policy
                // directly, otherwise the one caller that most wants the
                // speedup (a CI runner with no rsconstruct.toml) would never
                // get it.
                let config_enables_eatmydata = builder
                    .is_some_and(|b| b.config.dependencies.eatmydata)
                    || (all && crate::config::running_in_ci());
                let flags = InstallFlags {
                    interactive,
                    use_eatmydata: !no_eatmydata
                        && config_enables_eatmydata
                        && which::which("eatmydata").is_ok(),
                    verbose,
                };
                install_machine(
                    ctx,
                    processors,
                    is_enabled,
                    builder,
                    name.as_deref(),
                    all,
                    &flags,
                )?;
            }
            InstallAction::Project { name, interactive } => {
                let builder = builder.ok_or_else(|| {
                    anyhow::anyhow!("tool install project requires a project with rsconstruct.toml")
                })?;
                let flags = InstallFlags {
                    interactive,
                    use_eatmydata: false,
                    verbose,
                };
                install_project(
                    ctx,
                    processors,
                    is_enabled,
                    builder,
                    name.as_deref(),
                    &flags,
                )?;
            }
        },
    }

    Ok(())
}

/// How an install runs, from the command line.
struct InstallFlags {
    /// Ask before installing (`-i`). Off by default: CI and scripts are the
    /// common callers and neither can answer a prompt.
    interactive: bool,
    /// Wrap the system package manager in `eatmydata`.
    use_eatmydata: bool,
    verbose: bool,
}

/// The order machine installs run in, by method. Fixed and load-bearing:
/// system packages go first, because language packages (pip, gem, npm)
/// often build native extensions that link against them.
const MACHINE_METHOD_ORDER: &[&str] = &[
    "apt", "dnf", "pacman", "brew", "snap", "pip", "npm", "cargo", "gem", "binary",
];

/// The install method for `[dependencies]` system packages: the first
/// system package manager found on this machine.
fn system_package_method() -> Option<&'static str> {
    [
        ("apt-get", "apt"),
        ("dnf", "dnf"),
        ("pacman", "pacman"),
        ("brew", "brew"),
    ]
    .into_iter()
    .find(|(program, _)| which::which(program).is_ok())
    .map(|(_, method)| method)
}

/// A tool error, reported with the tool-error exit code.
fn tool_error(message: String) -> anyhow::Error {
    crate::exit_code::RsconstructError::new(
        crate::exit_code::RsconstructExitCode::ToolError,
        message,
    )
    .into()
}

/// Ask before installing when `-i` was given. True to go ahead; declining is
/// a normal outcome, not an error — the user saw the plan and said no.
fn go_ahead(flags: &InstallFlags) -> Result<bool> {
    if flags.interactive && !confirm()? {
        println!("Aborted.");
        return Ok(false);
    }
    Ok(true)
}

/// Run each `(method, packages)` group with `tools::run`, reporting each.
/// Returns whether one failed; the rest still run.
fn run_install_groups(groups: &[(&str, Vec<String>)], ctx: &crate::tools::InstallCtx) -> bool {
    let mut any_failed = false;
    for (method, packages) in groups {
        let packages_ref: Vec<&str> = packages.iter().map(String::as_str).collect();
        match crate::tools::run(method, &packages_ref, ctx) {
            Ok(()) => println!("{} [{method}] {}", color::green("✓"), packages.join(", ")),
            Err(e) => {
                // `{:#}`, not `{}`: the alternate form walks the whole
                // anyhow cause chain. Plain Display prints only the
                // outermost context, which turns a real diagnosis ("... :
                // http status: 403") back into a bare "Failed to query
                // GitHub releases API".
                println!(
                    "{} [{method}] {}: {e:#}",
                    color::red("✗"),
                    packages.join(", ")
                );
                any_failed = true;
            }
        }
    }
    any_failed
}

/// Print each `(method, packages)` group with the commands it will run.
fn print_install_groups(groups: &[(&str, Vec<String>)], ctx: &crate::tools::InstallCtx) {
    for (method, packages) in groups {
        let packages_ref: Vec<&str> = packages.iter().map(String::as_str).collect();
        println!(
            "  {} {}",
            color::bold(&format!("[{method}]")),
            packages.join(", ")
        );
        for argv in crate::tools::describe(method, &packages_ref, &ctx.binary_pins) {
            println!("       {}", color::dim(&argv.join(" ")));
        }
    }
    println!();
}

/// `tool install machine`: install onto the machine, each tool through its
/// machine install (a system package manager or a user-wide prefix; see
/// `tools::run`). With a `name`, that one tool; with `all`, every tool in the
/// registry, config or not; otherwise what this project needs on the
/// machine: its `[dependencies]` system packages and the tools of its enabled
/// processors whose preferred target is the machine, among them the
/// installers `tool install project` runs (uv for a Python project, npm for
/// a Node one). Nothing here goes into the project.
fn install_machine(
    ctx: &crate::build_context::BuildContext,
    processors: &crate::processor::ProcessorMap,
    is_enabled: &dyn Fn(&str) -> bool,
    builder: Option<&Builder>,
    name: Option<&str>,
    all: bool,
    flags: &InstallFlags,
) -> Result<()> {
    let root = std::path::Path::new(".");
    // In a project, each tool is requested at its rsconstruct.lock version
    // (a `.deb` at its locked asset). `--all` is the config-independent
    // registry sweep and has no lock to follow.
    let (lock, drift) = match builder {
        Some(b) if !all => {
            let wanted = project_wanted(b, processors, is_enabled)?;
            let (lock, drift) = lock_for_install(ctx, &wanted, flags.verbose)?;
            (Some(lock), drift)
        }
        _ => (None, std::collections::BTreeSet::new()),
    };

    let mut missing: Vec<&crate::tools::InstallMethod> = Vec::new();
    let mut system_packages: Vec<String> = Vec::new();
    if all {
        // Registry-wide, like `tool list`. Nothing is skipped: a tool with
        // no machine install is a hard error, not a warning to scroll past,
        // since --all claiming success while a tool is absent is exactly the
        // silent gap the strict-by-default rule exists to prevent.
        let mut no_machine: Vec<&str> = Vec::new();
        for info in crate::tools::all_tools() {
            if which::which(info.name).is_ok() {
                continue;
            }
            match &info.machine {
                Some(method) => missing.push(method),
                None => no_machine.push(info.name),
            }
        }
        if !no_machine.is_empty() {
            return Err(tool_error(format!(
                "{} tool(s) have no machine install: {}",
                no_machine.len(),
                no_machine.join(", ")
            )));
        }
    } else if let Some(name) = name {
        let info = crate::tools::tool_info(name)
            .ok_or_else(|| tool_error(format!("Unknown tool '{name}'")))?;
        let method = info.machine.as_ref().ok_or_else(|| {
            tool_error(format!(
                "'{name}' has no machine install; install it into the project with \
                 `rsconstruct tool install project {name}`"
            ))
        })?;
        missing.push(method);
    } else {
        let builder = builder.ok_or_else(|| {
            anyhow::anyhow!(
                "`tool install machine` needs a project with rsconstruct.toml, a tool name, or --all"
            )
        })?;
        let deps = &builder.config.dependencies;
        system_packages = deps
            .system
            .iter()
            .filter(|pkg| !is_system_package_installed(ctx, pkg))
            .cloned()
            .collect();
        // The tools of the enabled processors. A stanza with `enabled =
        // false` is skipped: it is kept precisely so its tool need not be
        // installed. auto_detect is not checked, because the tool itself may
        // need to be installed before detection can succeed.
        let mut tools: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for pname in sorted_keys(processors) {
            if is_enabled(pname) {
                tools.extend(processors[pname].required_tools());
            }
        }
        // The installers `tool install project` runs.
        if crate::config::has_python_deps(root)? {
            tools.insert("uv".to_string());
        }
        if !deps.node_deps(root)?.is_empty() {
            tools.insert("npm".to_string());
        }
        for tool in &tools {
            if !is_installable_tool_name(tool) {
                continue;
            }
            // A tool the registry does not know is a binary of one of the
            // project's `[dependencies]` packages (cargo-deny from the crate
            // of that name), which `tool install project` installs and
            // checks for.
            let Some(info) = crate::tools::tool_info(tool) else {
                continue;
            };
            if info.preferred_target() != Some(crate::tools::InstallTarget::Machine) {
                continue;
            }
            let Some(method) = &info.machine else {
                continue;
            };
            // Present on PATH counts as installed, unless it was found at
            // a version other than the locked one that may be replaced (see
            // `lock_for_install`).
            let drifted = drift.contains(&(method.method.to_string(), method.package.to_string()));
            if which::which(tool).is_err() || drifted {
                missing.push(method);
            }
        }
    }

    // A machine install never goes into the project's own venv, which is
    // uv's (see `tool install project`).
    if missing.iter().any(|m| m.method == "pip")
        && let Ok(pip) = which::which("pip")
        && let (Ok(pip), Ok(here)) = (pip.canonicalize(), root.canonicalize())
        && pip.starts_with(&here)
    {
        return Err(tool_error(format!(
            "pip on PATH ({}) belongs to this project's venv, which `uv sync` owns; \
             activate a venv outside the project for a machine install",
            pip.display()
        )));
    }

    // Grouped by method, each package at its locked version when the
    // project's lock has one, in the fixed machine order.
    let mut by_method: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    if !system_packages.is_empty() {
        let method = system_package_method().ok_or_else(|| {
            anyhow::anyhow!(
                "No supported package manager found (apt-get, dnf, pacman, brew); \
                 install these system packages manually: {}",
                system_packages.join(", ")
            )
        })?;
        by_method.insert(method, system_packages);
    }
    for method in missing {
        let package = lock.as_ref().map_or_else(
            || method.package.to_string(),
            |l| l.pinned(method.method, method.package),
        );
        let packages = by_method.entry(method.method).or_default();
        if !packages.contains(&package) {
            packages.push(package);
        }
    }
    if by_method.is_empty() {
        println!(
            "{}",
            color::green("Everything this needs on the machine is already installed.")
        );
        return Ok(());
    }
    let groups: Vec<(&str, Vec<String>)> = MACHINE_METHOD_ORDER
        .iter()
        .filter_map(|m| by_method.remove(m).map(|packages| (*m, packages)))
        .collect();

    let install_ctx = crate::tools::InstallCtx {
        verbose: flags.verbose,
        use_eatmydata: flags.use_eatmydata,
        binary_pins: lock.map(|l| l.binary).unwrap_or_default(),
    };
    println!("{}:", color::bold("To install onto the machine"));
    print_install_groups(&groups, &install_ctx);
    if !go_ahead(flags)? {
        return Ok(());
    }
    if run_install_groups(&groups, &install_ctx) {
        return Err(tool_error("Some machine installs failed".to_string()));
    }
    println!("{}", color::green("All machine installs succeeded."));
    Ok(())
}

/// `tool install project`: install into the project, from its lockfiles:
/// `uv sync` builds the repo's `.venv` from uv.lock (the code's Python
/// dependencies and the Python tools its processors run), `npm ci` fills
/// `node_modules` from package-lock.json, and the locked crates, npm
/// packages, gems and release binaries are linked into `.rsconstruct/tools`
/// from the tool store. With a `name`, only that tool's project install.
/// Nothing here touches the machine: a missing uv or npm is an error that
/// names `tool install machine`.
fn install_project(
    ctx: &crate::build_context::BuildContext,
    processors: &crate::processor::ProcessorMap,
    is_enabled: &dyn Fn(&str) -> bool,
    builder: &Builder,
    name: Option<&str>,
    flags: &InstallFlags,
) -> Result<()> {
    let root = std::path::Path::new(".");
    let deps = &builder.config.dependencies;
    let wanted = project_wanted(builder, processors, is_enabled)?;
    let (lock, _) = lock_for_install(ctx, &wanted, flags.verbose)?;
    let mut tool_dir = Some(project_tools(builder, &wanted, &lock)?);
    // The project's own package sets, each installed by its package manager
    // from its lockfile; the names are for display only.
    let mut groups: Vec<(&str, Vec<String>)> = Vec::new();
    let mut up_to_date: Vec<String> = Vec::new();

    if let Some(name) = name {
        let info = crate::tools::tool_info(name)
            .ok_or_else(|| tool_error(format!("Unknown tool '{name}'")))?;
        let method = info.project.as_ref().ok_or_else(|| {
            tool_error(format!(
                "'{name}' has no project install; install it onto the machine with \
                 `rsconstruct tool install machine {name}`"
            ))
        })?;
        if method.method == "pip" {
            let wanted_name = crate::config::normalized_distribution_name(method.package);
            let pinned = crate::config::python_deps(root)?
                .iter()
                .any(|req| crate::config::normalized_distribution_name(req) == wanted_name);
            if !pinned {
                return Err(crate::exit_code::config_error(format!(
                    "{} is not pinned by uv.lock. The repo's .venv is uv's, so add it to \
                     pyproject.toml's dev dependency group (`uv add --dev {}`)",
                    method.package, method.package
                )));
            }
            groups.push(("uv-sync", vec![method.package.to_string()]));
            tool_dir = None;
        } else {
            let dir = tool_dir.take().expect("set above");
            if !dir.covers(method.method, method.package) {
                return Err(tool_error(format!(
                    "'{name}' is not one of this project's tools: enable a processor that \
                     runs it, or add {} to [dependencies] {}",
                    method.package, method.method
                )));
            }
            tool_dir = Some(dir.with_only(method.method, method.package));
        }
    } else {
        // uv audits the whole set itself in one pass, so it always runs.
        if crate::config::has_python_deps(root)? {
            groups.push(("uv-sync", vec!["uv.lock".to_string()]));
        }
        // package.json declares the names, and in package-lock mode `npm
        // ci` installs exactly the pinned closure into ./node_modules.
        // Skipped only when every declared package is already there at the
        // locked version: `npm ci` wipes node_modules first, so a needless
        // run is not cheap.
        let node_deps = deps.node_deps(root)?;
        if !node_deps.is_empty() {
            let (method, current) = match deps.npm_source {
                crate::config::NpmSource::PackageLock => {
                    let pins = crate::config::package_lock_pins(&root.join("package-lock.json"))?;
                    let current = node_deps.iter().all(|name| {
                        let installed = crate::config::installed_node_package_version(root, name);
                        installed.is_some() && installed == pins.get(name).cloned()
                    });
                    ("npm-ci", current)
                }
                crate::config::NpmSource::PackageJson => {
                    let current = node_deps.iter().all(|name| {
                        crate::config::installed_node_package_version(root, name).is_some()
                    });
                    ("npm-install", current)
                }
            };
            if current {
                up_to_date.extend(node_deps.iter().map(|n| format!("[{method}] {n}")));
            } else {
                groups.push((method, node_deps));
            }
        }
    }

    // uv and npm are machine installs; this command does not make them.
    for (method, _) in &groups {
        let program = if *method == "uv-sync" { "uv" } else { "npm" };
        if which::which(program).is_err() {
            return Err(tool_error(format!(
                "{program} is not installed; install it onto the machine with \
                 `rsconstruct tool install machine {program}`"
            )));
        }
    }

    if !up_to_date.is_empty() {
        println!("{}:", color::dim("Already installed (skipping)"));
        for s in &up_to_date {
            println!("  {} {s}", color::green("✓"));
        }
        println!();
    }
    let install_ctx = crate::tools::InstallCtx {
        verbose: flags.verbose,
        use_eatmydata: false,
        binary_pins: lock.binary.clone(),
    };
    if !groups.is_empty() {
        println!("{}:", color::bold("Project environments"));
        print_install_groups(&groups, &install_ctx);
    }
    if let Some(dir) = &tool_dir {
        print_project_tools(dir);
    }
    let has_work = !groups.is_empty() || tool_dir.as_ref().is_some_and(|d| d.plan.has_work());
    if has_work && !go_ahead(flags)? {
        return Ok(());
    }
    let groups_failed = run_install_groups(&groups, &install_ctx);
    // The tool directory goes last: crates and gems may build native code
    // against what the environments above provide. With nothing to
    // install, its bin/ is still rebuilt.
    let tool_dir_failed = match &tool_dir {
        Some(dir) => match apply_project_tools(dir, &lock, flags.verbose) {
            Ok(()) => false,
            Err(e) => {
                println!(
                    "{} [{}]: {e:#}",
                    color::red("✗"),
                    crate::tool_store::REPO_TOOLS_DIR
                );
                true
            }
        },
        None => false,
    };
    if groups_failed || tool_dir_failed {
        return Err(tool_error("Some project installs failed".to_string()));
    }
    // A processor tool the registry does not know can only come from the
    // project's `[dependencies]` (a crate's binary), so after the installs
    // above it must be on PATH; otherwise nothing will ever install it.
    if name.is_none() {
        let mut unprovided: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for pname in sorted_keys(processors) {
            if !is_enabled(pname) {
                continue;
            }
            for tool in processors[pname].required_tools() {
                if is_installable_tool_name(&tool)
                    && crate::tools::tool_info(&tool).is_none()
                    && which::which(&tool).is_err()
                {
                    unprovided.insert(tool);
                }
            }
        }
        if !unprovided.is_empty() {
            let names: Vec<&str> = unprovided.iter().map(String::as_str).collect();
            return Err(tool_error(format!(
                "No install method known for: {} (not in the tool registry and not \
                 provided by [dependencies])",
                names.join(", ")
            )));
        }
    }
    if has_work {
        println!("{}", color::green("All project installs succeeded."));
    } else {
        println!(
            "{}",
            color::green("Everything this project installs is already installed.")
        );
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
