use std::collections::BTreeMap;
use std::io::Write;
use std::process::Command;
use anyhow::{bail, Context, Result};
use crate::cli::{GraphFormat, ToolsAction};
use crate::color;
use crate::json_output;
use crate::tool_lock;
use super::{Builder, sorted_keys};

/// Check if a system package is installed using the platform's package manager.
/// Tries dpkg-query (Debian/Ubuntu), rpm (Fedora/RHEL), pacman (Arch), and brew (macOS).
fn is_system_package_installed(pkg: &str) -> bool {
    if which::which("dpkg-query").is_ok() {
        return Command::new("dpkg-query")
            .args(["-W", "-f", "${Status}", pkg])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    }
    if which::which("rpm").is_ok() {
        return Command::new("rpm")
            .args(["-q", pkg])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    }
    if which::which("pacman").is_ok() {
        return Command::new("pacman")
            .args(["-Q", pkg])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    }
    if which::which("brew").is_ok() {
        return Command::new("brew")
            .args(["list", pkg])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    }
    // Fallback: check if the package name is available as a command
    which::which(pkg).is_ok()
}

/// Return the language runtime category for a tool.
/// Delegates to the central `TOOLS` registry in `processors/mod.rs`.
fn tool_runtime(tool: &str) -> &'static str {
    crate::processors::tool_runtime(tool).unwrap_or_else(|| {
        debug_assert!(false, "tool_runtime: unrecognized tool '{tool}'");
        "system"
    })
}

/// Handle `rsconstruct tools` subcommands without a project config.
/// Uses default processor configs so List, Stats, Install, and Graph work
/// even outside a project directory.
pub fn tools_no_config(action: ToolsAction, verbose: bool) -> Result<()> {
    let processors = super::create_all_default_processors();
    run_tools_command(&processors, &|_name| true, action, verbose, None)
}

impl Builder {
    /// Verify tool versions against .tools.versions lock file.
    /// Called at the start of build unless --ignore-tool-versions is passed.
    pub fn verify_tool_versions(&self) -> Result<()> {
        let processors = self.create_processors()?;
        let tool_commands = tool_lock::collect_tool_commands(
            &processors,
            &|_name| true,
        );
        if tool_commands.is_empty() {
            return Ok(());
        }
        tool_lock::verify_lock_file(&tool_commands)
    }

    /// Handle `rsconstruct tools` subcommands
    pub fn tools(&self, action: ToolsAction, verbose: bool) -> Result<()> {
        let processors = self.create_processors()?;
        run_tools_command(
            &processors,
            &|_name| true,
            action,
            verbose,
            Some(self),
        )
    }
}

/// Core implementation for `tools` subcommands, shared by `Builder::tools()` and `tools_no_config()`.
/// `builder` is `Some` when running with a project config (needed for `open_file`, `Check`, `Lock`).
fn run_tools_command(
    processors: &crate::processors::ProcessorMap,
    is_enabled: &dyn Fn(&str) -> bool,
    action: ToolsAction,
    verbose: bool,
    builder: Option<&Builder>,
) -> Result<()> {
    let show_all = matches!(&action, ToolsAction::List { all: true, .. });
    let show_methods = matches!(&action, ToolsAction::List { methods: true, .. });
    let install_yes = matches!(&action, ToolsAction::Install { yes: true, .. });
    let install_no_eatmydata = matches!(&action, ToolsAction::Install { no_eatmydata: true, .. });
    // eatmydata is opt-in-by-availability: use it if installed and neither
    // the CLI flag nor the project config disables it. It dramatically
    // speeds up apt/dnf/pacman by no-op'ing fsync; the trade-off is
    // loss-on-power-cut, which we accept for transient install steps.
    //
    // Precedence: CLI --no-eatmydata > [dependencies].eatmydata config >
    // default (true).
    let config_allows_eatmydata = builder.is_none_or(|b| b.config.dependencies.eatmydata);
    let install_use_eatmydata = !install_no_eatmydata
        && config_allows_eatmydata
        && which::which("eatmydata").is_ok();

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
        ToolsAction::List { .. } => {
            if crate::json_output::is_json_mode() {
                let entries: Vec<json_output::ToolListEntry> = tool_map.iter()
                    .map(|(tool, procs)| {
                        let info = crate::processors::tool_info(tool);
                        let install_methods = info
                            .map(|i| i.install_methods.iter().map(|m| {
                                json_output::ToolInstallMethodEntry {
                                    method: m.method.to_string(),
                                    command: m.command(),
                                }
                            }).collect())
                            .unwrap_or_default();
                        json_output::ToolListEntry {
                            tool: tool.clone(),
                            installed: which::which(tool).is_ok(),
                            runtime: info.map(|i| i.runtime).unwrap_or("unknown").to_string(),
                            processors: procs.clone(),
                            install_methods,
                        }
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
                let info = crate::processors::tool_info(tool);
                let runtime = info.map(|i| i.runtime).unwrap_or("unknown");
                let install_str = if show_methods {
                    let methods: Vec<String> = info
                        .map(|i| i.install_methods.iter()
                            .map(|m| format!("{}: {}", m.method, m.command()))
                            .collect())
                        .unwrap_or_default();
                    if methods.is_empty() { "?".to_string() } else { methods.join(" | ") }
                } else {
                    info.and_then(|i| i.install_methods.first())
                        .map(super::super::processors::InstallMethod::command)
                        .unwrap_or_else(|| "?".to_string())
                };
                println!("{} [{}] [{}] ({}) — {}",
                    tool, installed, runtime, procs.join(", "), color::dim(&install_str));
            }
        }
        ToolsAction::Check => {
            let tool_commands = tool_lock::collect_tool_commands(processors, is_enabled);
            tool_lock::verify_lock_file(&tool_commands)?;
            let lock = tool_lock::read_lock_file()?;
            if crate::json_output::is_json_mode() {
                let entries: Vec<serde_json::Value> = lock
                    .as_ref()
                    .map(|l| l.tools.iter().map(|(name, info)| {
                        let version = tool_lock::extract_semver(&info.version_output).unwrap_or("?");
                        serde_json::json!({
                            "tool": name,
                            "status": "ok",
                            "version": version,
                        })
                    }).collect())
                    .unwrap_or_default();
                let out = serde_json::json!({
                    "match": true,
                    "tools": entries,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                if verbose
                    && let Some(lock) = &lock {
                        for (name, info) in &lock.tools {
                            let version = tool_lock::extract_semver(&info.version_output).unwrap_or("?");
                            println!("{} {} {}", name, color::green("ok"), color::dim(version));
                        }
                    }
                println!("{}", color::green("Tool versions match lock file."));
            }
        }
        ToolsAction::Lock => {
            let tool_commands = tool_lock::collect_tool_commands(processors, is_enabled);
            let lock = tool_lock::create_lock(&tool_commands)?;
            tool_lock::write_lock_file(&lock)?;
            if crate::json_output::is_json_mode() {
                let entries: Vec<serde_json::Value> = lock.tools.iter().map(|(name, info)| {
                    let version = tool_lock::extract_semver(&info.version_output).unwrap_or("?");
                    serde_json::json!({
                        "tool": name,
                        "status": "locked",
                        "version": version,
                    })
                }).collect();
                let out = serde_json::json!({
                    "lock_file": ".tools.versions",
                    "tools": entries,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                for (name, info) in &lock.tools {
                    let version = tool_lock::extract_semver(&info.version_output).unwrap_or("?");
                    println!("{} {} {}", name, color::green("locked"), color::dim(version));
                }
                println!("Wrote {}", color::bold(".tools.versions"));
            }
        }
        ToolsAction::Graph { format, view } => {
            if view {
                let html_content = tools_graph_html(&tool_map);
                let html_path = std::env::temp_dir().join("rsconstruct_tools_graph.html");
                std::fs::write(&html_path, &html_content)
                    .map_err(|e| anyhow::anyhow!("Failed to write HTML file: {e}"))?;
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
                    GraphFormat::Svg => tools_graph_svg(&tool_map)?,
                };
                println!("{output}");
            }
        }
        ToolsAction::Stats => {
            let mut tool_stats: Vec<json_output::ToolStat> = Vec::new();
            for (tool, procs) in &tool_map {
                let installed = which::which(tool).is_ok();
                let runtime = tool_runtime(tool).to_string();
                let install_command = crate::processors::tool_install_command(tool);
                tool_stats.push(json_output::ToolStat {
                    name: tool.clone(),
                    installed,
                    runtime,
                    processors: procs.clone(),
                    install_command,
                });
            }

            let mut runtime_map: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            for stat in &tool_stats {
                let entry = runtime_map.entry(tool_runtime(&stat.name)).or_insert((0, 0));
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
                let rows: Vec<Vec<String>> = tool_stats.iter().map(|stat| {
                    let status = if stat.installed {
                        color::green("\u{2713}")
                    } else {
                        color::red("\u{2717}")
                    };
                    let procs = stat.processors.join(", ");
                    let install = stat.install_command.as_deref().unwrap_or("").to_string();
                    vec![stat.name.clone(), status.to_string(), procs, install]
                }).collect();
                color::print_table(&["Tool", "Status", "Processors", "Install"], &rows);

                println!();
                println!("Runtime summary:");
                let runtime_display: &[(&str, &str)] = &[
                    ("python", "Python"),
                    ("node", "Node.js"),
                    ("ruby", "Ruby"),
                    ("rust", "Rust"),
                    ("perl", "Perl"),
                    ("system", "System"),
                ];
                let rt_rows: Vec<Vec<String>> = runtime_display.iter().filter_map(|(key, label)| {
                    runtime_stats.iter().find(|r| r.runtime == *key).map(|rs| {
                        let status_str = format!("{}/{}", rs.installed, rs.total);
                        let line = if rs.missing > 0 {
                            color::yellow(&status_str)
                        } else {
                            color::green(&status_str)
                        };
                        vec![label.to_string(), line.to_string()]
                    })
                }).collect();
                color::print_table(&["Runtime", "Installed"], &rt_rows);

                println!();
                let total_line = format!("Total: {installed_count}/{total_tools} tools installed");
                if missing_count > 0 {
                    println!("{}", color::yellow(&total_line));
                } else {
                    println!("{}", color::green(&total_line));
                }
            }
        }
        ToolsAction::Install { name, .. } => {
            // Collect missing tools with their install info
            let missing_tools: Vec<(&str, &crate::processors::InstallMethod)> = if let Some(ref name) = name {
                // Install a specific tool
                match crate::processors::tool_info(name) {
                    Some(info) => {
                        if let Some(method) = info.install_methods.first() {
                            vec![(info.name, method)]
                        } else {
                            eprintln!("{}: No install method for '{}'", color::red("Error"), name);
                            return Err(crate::exit_code::RsconstructError::new(
                                crate::exit_code::RsconstructExitCode::ToolError,
                                format!("No install method known for tool '{name}'"),
                            ).into());
                        }
                    }
                    None => {
                        eprintln!("{}: Unknown tool '{}'", color::red("Error"), name);
                        return Err(crate::exit_code::RsconstructError::new(
                            crate::exit_code::RsconstructExitCode::ToolError,
                            format!("Unknown tool '{name}'"),
                        ).into());
                    }
                }
            } else {
                // Build tool list from all enabled processors.
                // Every processor in the config gets its tools installed —
                // auto_detect is not checked because the tool itself may need
                // to be installed before detection can succeed.
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
                    if which::which(tool_name).is_ok() {
                        continue;
                    }
                    match crate::processors::tool_info(tool_name) {
                        Some(info) if !info.install_methods.is_empty() => {
                            missing.push((info.name, &info.install_methods[0]));
                        }
                        _ => {
                            eprintln!("{}: No install method for '{}'", color::red("Error"), tool_name);
                            any_unknown = true;
                        }
                    }
                }
                if any_unknown {
                    return Err(crate::exit_code::RsconstructError::new(
                        crate::exit_code::RsconstructExitCode::ToolError,
                        "Some tools have no known install procedure",
                    ).into());
                }
                if missing.is_empty() {
                    println!("{}", color::green("All tools are already installed."));
                    return Ok(());
                }
                missing
            };

            // Group by install method for batch installation
            let mut by_method: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            let mut ungroupable: Vec<(&str, String)> = Vec::new();
            for (tool_name, method) in &missing_tools {
                match method.method {
                    "pip" | "apt" | "npm" | "cargo" | "gem" | "snap" => {
                        by_method.entry(method.method).or_default().push(method.package);
                    }
                    _ => {
                        ungroupable.push((tool_name, method.command()));
                    }
                }
            }

            // Display the install plan
            println!("Missing {} tool(s), grouped by package manager:", missing_tools.len());
            println!();

            // Method display order and labels
            let method_order = &["pip", "apt", "npm", "cargo", "gem", "snap"];
            let mut plans: Vec<crate::processors::InstallPlan> = Vec::new();
            for method in method_order {
                if let Some(packages) = by_method.get(method) {
                    let mut plan = crate::processors::InstallMethod::batch_plan(method, packages);
                    if install_use_eatmydata {
                        plan = plan.wrap_with_eatmydata();
                    }
                    println!("  {} {}", color::bold(&format!("[{method}]")),
                        packages.iter().map(std::string::ToString::to_string).collect::<Vec<_>>().join(", "));
                    println!("       {}", color::dim(&plan.display()));
                    plans.push(plan);
                }
            }
            for (tool_name, cmd) in &ungroupable {
                println!("  {} {}", color::bold(&format!("[{tool_name}]")), color::dim(cmd));
                // Free-form registry entries (curl|tar pipelines, etc.) require a shell.
                // See docs/src/no-shell-policy.md.
                plans.push(crate::processors::InstallPlan::Shell(cmd.clone()));
            }
            println!();

            if !install_yes {
                print!("Proceed? [y/N] ");
                std::io::stdout().flush()?;
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                let answer = answer.trim().to_lowercase();
                if answer != "y" && answer != "yes" {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            // Execute batch commands. Argv plans are run directly (no shell);
            // Shell plans go through `sh -c` because they contain pipelines
            // baked into the static registry. See docs/src/no-shell-policy.md.
            let mut any_failed = false;
            for plan in &plans {
                println!("Running: {}", color::dim(&plan.display()));
                let status = match plan {
                    crate::processors::InstallPlan::Argv(argv) => {
                        Command::new(&argv[0]).args(&argv[1..]).status()?
                    }
                    crate::processors::InstallPlan::Shell(s) => {
                        Command::new("sh").arg("-c").arg(s).status()?
                    }
                };
                if status.success() {
                    println!("{}", color::green("OK"));
                } else {
                    println!("{} (exit code {})", color::red("FAILED"),
                        status.code().map_or("unknown".to_string(), |c| c.to_string()));
                    any_failed = true;
                }
            }

            if any_failed {
                return Err(crate::exit_code::RsconstructError::new(
                    crate::exit_code::RsconstructExitCode::ToolError,
                    "Some install commands failed",
                ).into());
            }
            println!("{}", color::green("All tools installed successfully."));
        }
        ToolsAction::InstallDeps { yes, no_eatmydata } => {
            let config = builder
                .map(|b| &b.config.dependencies)
                .ok_or_else(|| anyhow::anyhow!("install-deps requires a project with rsconstruct.toml"))?;
            // eatmydata speeds up apt/dnf/pacman by no-op'ing fsync.
            // Precedence: CLI --no-eatmydata > [dependencies].eatmydata
            // config > default (true). The wrap also requires eatmydata
            // to actually be installed on the system.
            let use_eatmydata = !no_eatmydata
                && config.eatmydata
                && which::which("eatmydata").is_ok();

            if config.is_empty() {
                println!("No dependencies declared in [dependencies].");
                return Ok(());
            }

            // Filter out already-installed packages, then build install commands.
            // argv form (not a shell string) so package specifiers like
            // "setuptools<82" reach the installer verbatim instead of being
            // interpreted as shell redirections.
            let mut commands: Vec<(String, Vec<String>)> = Vec::new(); // (description, argv)
            let mut skipped: Vec<String> = Vec::new();

            // Install order is FIXED and load-bearing: system → pip → npm → gem.
            // Do not reorder. Language-level packages (pip, gem, npm) often
            // build native extensions at install time that link against
            // system libraries via pkg-config. Example: `pip install manim`
            // pulls in manimpango, which needs libpango1.0-dev present on
            // the system before its wheel can build, otherwise the install
            // fails with "Package 'pangocairo' was not found".
            // See docs/src/configuration.md "Install order" for the rationale.
            if !config.system.is_empty() {
                let missing: Vec<&str> = config.system.iter()
                    .filter(|pkg| {
                        let installed = is_system_package_installed(pkg);
                        if installed { skipped.push(format!("[system] {pkg}")); }
                        !installed
                    })
                    .map(std::string::String::as_str)
                    .collect();
                if !missing.is_empty() {
                    let (mgr, mut argv, supports_eatmydata) = if which::which("apt-get").is_ok() {
                        ("apt", vec!["sudo".to_string(), "apt-get".to_string(), "install".to_string(), "-y".to_string()], true)
                    } else if which::which("dnf").is_ok() {
                        ("dnf", vec!["sudo".to_string(), "dnf".to_string(), "install".to_string(), "-y".to_string()], true)
                    } else if which::which("pacman").is_ok() {
                        ("pacman", vec!["sudo".to_string(), "pacman".to_string(), "-S".to_string(), "--noconfirm".to_string()], true)
                    } else if which::which("brew").is_ok() {
                        ("brew", vec!["brew".to_string(), "install".to_string()], false)
                    } else {
                        bail!(
                            "No supported package manager found (apt-get, dnf, pacman, brew); install these system packages manually: {}",
                            missing.join(", ")
                        );
                    };
                    // Insert eatmydata after sudo (so the LD_PRELOAD applies
                    // to the package manager, not to sudo). brew skipped:
                    // eatmydata is Linux-only and brew runs on macOS.
                    if use_eatmydata && supports_eatmydata {
                        argv.insert(1, "eatmydata".to_string());
                    }
                    argv.extend(missing.iter().map(std::string::ToString::to_string));
                    commands.push((
                        format!("[{}] {}", mgr, missing.join(", ")),
                        argv,
                    ));
                }
            }
            if !config.pip.is_empty() {
                let missing: Vec<&str> = config.pip.iter()
                    .filter(|pkg| {
                        // Strip version specifiers (e.g., "ruff>=0.4" -> "ruff")
                        let name = pkg.split(&['>', '<', '=', '!', '~'][..]).next().unwrap_or(pkg);
                        let installed = Command::new("pip")
                            .args(["show", name])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status()
                            .is_ok_and(|s| s.success());
                        if installed { skipped.push(format!("[pip] {pkg}")); }
                        !installed
                    })
                    .map(std::string::String::as_str)
                    .collect();
                if !missing.is_empty() {
                    let mut argv = vec!["pip".to_string(), "install".to_string()];
                    argv.extend(missing.iter().map(std::string::ToString::to_string));
                    commands.push((
                        format!("[pip] {}", missing.join(", ")),
                        argv,
                    ));
                }
            }
            if !config.npm.is_empty() {
                let missing: Vec<&str> = config.npm.iter()
                    .filter(|pkg| {
                        let installed = Command::new("npm")
                            .args(["ls", "-g", pkg])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status()
                            .is_ok_and(|s| s.success());
                        if installed { skipped.push(format!("[npm] {pkg}")); }
                        !installed
                    })
                    .map(std::string::String::as_str)
                    .collect();
                if !missing.is_empty() {
                    let mut argv = vec!["npm".to_string(), "install".to_string()];
                    argv.extend(missing.iter().map(std::string::ToString::to_string));
                    commands.push((
                        format!("[npm] {}", missing.join(", ")),
                        argv,
                    ));
                }
            }
            if !config.gem.is_empty() {
                let missing: Vec<&str> = config.gem.iter()
                    .filter(|pkg| {
                        let installed = Command::new("gem")
                            .args(["list", "-i", pkg])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status()
                            .is_ok_and(|s| s.success());
                        if installed { skipped.push(format!("[gem] {pkg}")); }
                        !installed
                    })
                    .map(std::string::String::as_str)
                    .collect();
                if !missing.is_empty() {
                    let mut argv = vec!["gem".to_string(), "install".to_string()];
                    argv.extend(missing.iter().map(std::string::ToString::to_string));
                    commands.push((
                        format!("[gem] {}", missing.join(", ")),
                        argv,
                    ));
                }
            }

            if !skipped.is_empty() {
                println!("{}:", color::dim("Already installed (skipping)"));
                for s in &skipped {
                    println!("  {} {}", color::green("✓"), s);
                }
                println!();
            }

            if commands.is_empty() {
                println!("{}", color::green("All dependencies already installed."));
                return Ok(());
            }

            println!("{}:", color::bold("Dependencies to install"));
            for (desc, argv) in &commands {
                println!("  {} {}", color::bold(desc), color::dim(&format!("({})", argv.join(" "))));
            }
            println!();

            if !yes {
                use std::io::Write;
                print!("Proceed? [y/N] ");
                std::io::stdout().flush()?;
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                if !matches!(answer.trim().to_lowercase().as_str(), "y" | "yes") {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            // Suppress per-command output by default to keep CI logs short.
            // Capture stdout+stderr and only emit them when the install fails.
            // Run via argv (no shell) so package specifiers like "setuptools<82"
            // are not interpreted as shell redirections.
            let mut any_failed = false;
            for (desc, argv) in &commands {
                let output = Command::new(&argv[0])
                    .args(&argv[1..])
                    .output()?;
                if output.status.success() {
                    println!("{} {}", color::green("✓"), desc);
                } else {
                    println!("{} {} (exit code {})", color::red("✗"), desc,
                        output.status.code().map_or("unknown".to_string(), |c| c.to_string()));
                    println!("  {}: {}", color::dim("command"), argv.join(" "));
                    if !output.stdout.is_empty() {
                        println!("{}", color::dim("--- stdout ---"));
                        std::io::Write::write_all(&mut std::io::stdout(), &output.stdout)?;
                    }
                    if !output.stderr.is_empty() {
                        println!("{}", color::dim("--- stderr ---"));
                        std::io::Write::write_all(&mut std::io::stderr(), &output.stderr)?;
                    }
                    any_failed = true;
                }
            }

            if any_failed {
                return Err(crate::exit_code::RsconstructError::new(
                    crate::exit_code::RsconstructExitCode::ToolError,
                    "Some dependency installs failed",
                ).into());
            }
            println!("{}", color::green("All dependencies installed successfully."));
        }
    }

    Ok(())
}

/// Sanitize a name for use as a DOT node identifier
fn sanitize_node_id(prefix: &str, name: &str) -> String {
    format!("{}_{}", prefix, name.replace(['.', '-', ' ', '+'], "_"))
}

fn tools_graph_dot(tool_map: &BTreeMap<String, Vec<String>>) -> String {
    let mut out = String::from("digraph tools {\n    rankdir=LR;\n    node [fontname=\"sans-serif\"];\n");

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
        out.push_str(&format!(
            "    {id} [label=\"{tool}\" shape=box style=filled fillcolor=lightblue];\n"
        ));
    }

    // Processor nodes
    out.push_str("    // Processors\n");
    for proc in &processors {
        let id = sanitize_node_id("proc", proc);
        out.push_str(&format!(
            "    {id} [label=\"{proc}\" shape=box style=filled fillcolor=lightyellow];\n"
        ));
    }

    // Edges
    out.push_str("    // Edges\n");
    for (tool, procs) in tool_map {
        let tool_id = sanitize_node_id("tool", tool);
        for proc in procs {
            let proc_id = sanitize_node_id("proc", proc);
            out.push_str(&format!("    {tool_id} -> {proc_id};\n"));
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
            out.push_str(&format!(
                "    {tool_id}[\"{tool}\"]:::tool --> {proc_id}[\"{proc}\"]:::processor\n"
            ));
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
            let info = crate::processors::tool_info(tool);
            let install_methods = info
                .map(|i| i.install_methods.iter().map(|m| {
                    crate::json_output::ToolInstallMethodEntry {
                        method: m.method.to_string(),
                        command: m.command(),
                    }
                }).collect())
                .unwrap_or_default();
            crate::json_output::ToolListEntry {
                tool: tool.clone(),
                installed: which::which(tool).is_ok(),
                runtime: info.map(|i| i.runtime).unwrap_or("unknown").to_string(),
                processors: procs.clone(),
                install_methods,
            }
        })
        .collect();
    Ok(serde_json::to_string_pretty(&entries)?)
}

fn tools_graph_html(tool_map: &BTreeMap<String, Vec<String>>) -> String {
    let mermaid_content = tools_graph_mermaid(tool_map);
    format!(r#"<!DOCTYPE html>
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
"#)
}

fn tools_graph_svg(tool_map: &BTreeMap<String, Vec<String>>) -> Result<String> {
    use std::process::Stdio;
    use crate::processors::{check_command_output, log_command};

    let dot_content = tools_graph_dot(tool_map);

    let mut cmd = Command::new("dot");
    cmd.arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    log_command(&cmd);
    let mut child = cmd
        .spawn()
        .map_err(|_| anyhow::anyhow!("Graphviz 'dot' command not found. Install Graphviz to use SVG format"))?;

    child.stdin.take()
        .context("stdin was not piped to dot command")?
        .write_all(dot_content.as_bytes())?;

    let output = child.wait_with_output()?;
    check_command_output(&output, "dot")?;

    Ok(String::from_utf8(output.stdout)?)
}
