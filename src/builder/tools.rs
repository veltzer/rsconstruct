use std::collections::BTreeMap;
use std::io::Write;
use std::process::Command;
use anyhow::Result;
use crate::cli::{GraphFormat, ToolsAction};
use crate::color;
use crate::json_output;
use crate::tool_lock;
use super::{Builder, sorted_keys};

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
    let file_index = crate::file_index::FileIndex::build().ok();
    run_tools_command(&processors, &|_name| true, action, verbose, None, file_index.as_ref())
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
            Some(&self.file_index),
        )
    }
}

/// Core implementation for `tools` subcommands, shared by `Builder::tools()` and `tools_no_config()`.
/// `builder` is `Some` when running with a project config (needed for `open_file`, `Check`, `Lock`).
/// `file_index` is `Some` when running inside a project (needed for auto-detection filtering).
fn run_tools_command(
    processors: &crate::processors::ProcessorMap,
    is_enabled: &dyn Fn(&str) -> bool,
    action: ToolsAction,
    verbose: bool,
    builder: Option<&Builder>,
    file_index: Option<&crate::file_index::FileIndex>,
) -> Result<()> {
    let show_all = matches!(&action, ToolsAction::List { all: true, .. });
    let show_methods = matches!(&action, ToolsAction::List { methods: true, .. });
    let install_yes = matches!(&action, ToolsAction::Install { yes: true, .. });
    let install_all = matches!(&action, ToolsAction::Install { all: true, .. });

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
                        .map(|m| m.command())
                        .unwrap_or_else(|| "?".to_string())
                };
                println!("{} [{}] [{}] ({}) — {}",
                    tool, installed, runtime, procs.join(", "), color::dim(&install_str));
            }
        }
        ToolsAction::Check => {
            let tool_commands = tool_lock::collect_tool_commands(processors, is_enabled);
            tool_lock::verify_lock_file(&tool_commands)?;
            if verbose {
                let lock = tool_lock::read_lock_file()?;
                if let Some(lock) = lock {
                    for (name, info) in &lock.tools {
                        let version = tool_lock::extract_semver(&info.version_output).unwrap_or("?");
                        println!("{} {} {}", name, color::green("ok"), color::dim(version));
                    }
                }
            }
            println!("{}", color::green("Tool versions match lock file."));
        }
        ToolsAction::Lock => {
            let tool_commands = tool_lock::collect_tool_commands(processors, is_enabled);
            let lock = tool_lock::create_lock(&tool_commands)?;
            for (name, info) in &lock.tools {
                let version = tool_lock::extract_semver(&info.version_output).unwrap_or("?");
                println!("{} {} {}", name, color::green("locked"), color::dim(version));
            }
            tool_lock::write_lock_file(&lock)?;
            println!("Wrote {}", color::bold(".tools.versions"));
        }
        ToolsAction::Graph { format, view } => {
            if view {
                let html_content = tools_graph_html(&tool_map);
                let html_path = std::env::temp_dir().join("rsconstruct_tools_graph.html");
                std::fs::write(&html_path, &html_content)
                    .map_err(|e| anyhow::anyhow!("Failed to write HTML file: {}", e))?;
                if let Some(b) = builder {
                    b.open_file(&html_path)?;
                    println!("Opened tools graph in browser: {}", html_path.display());
                } else {
                    println!("Wrote tools graph to: {}", html_path.display());
                }
            } else {
                let output = match format {
                    GraphFormat::Dot => tools_graph_dot(&tool_map),
                    GraphFormat::Mermaid => tools_graph_mermaid(&tool_map),
                    GraphFormat::Text => tools_graph_text(&tool_map),
                    GraphFormat::Json => tools_graph_json(&tool_map)?,
                    GraphFormat::Svg => tools_graph_svg(&tool_map)?,
                };
                println!("{}", output);
            }
        }
        ToolsAction::Stats => {
            let mut tool_stats: Vec<json_output::ToolStat> = Vec::new();
            for (tool, procs) in &tool_map {
                let installed = which::which(tool).is_ok();
                let runtime = tool_runtime(tool).to_string();
                let install_command = crate::processors::tool_install_command(tool)
                    .map(|s| s.to_string());
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
                println!("Tools:");
                let max_name = tool_stats.iter().map(|t| t.name.len()).max().unwrap_or(0);
                for stat in &tool_stats {
                    let status = if stat.installed {
                        color::green("\u{2713}")
                    } else {
                        color::red("\u{2717}")
                    };
                    let procs = stat.processors.join(", ");
                    let install = stat.install_command.as_deref()
                        .map(|c| format!("  ({})", color::dim(c)))
                        .unwrap_or_default();
                    println!("  {:width$}  {}  {}{}", stat.name, status, procs, install, width = max_name);
                }

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
                for (key, label) in runtime_display {
                    if let Some(rs) = runtime_stats.iter().find(|r| r.runtime == *key) {
                        let line = format!("  {:10}{}/{} installed", label, rs.installed, rs.total);
                        if rs.missing > 0 {
                            println!("{}", color::yellow(&line));
                        } else {
                            println!("{}", color::green(&line));
                        }
                    }
                }

                println!();
                let total_line = format!("Total: {}/{} tools installed", installed_count, total_tools);
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
                                format!("No install method known for tool '{}'", name),
                            ).into());
                        }
                    }
                    None => {
                        eprintln!("{}: Unknown tool '{}'", color::red("Error"), name);
                        return Err(crate::exit_code::RsconstructError::new(
                            crate::exit_code::RsconstructExitCode::ToolError,
                            format!("Unknown tool '{}'", name),
                        ).into());
                    }
                }
            } else {
                // Build tool list: filter by detected processors unless --all
                let mut install_tools: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for name in sorted_keys(processors) {
                    if !is_enabled(name) {
                        continue;
                    }
                    // Skip undetected processors unless --all
                    if !install_all
                        && let Some(fi) = file_index
                        && !processors[name].auto_detect(fi)
                    {
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
            let mut commands: Vec<String> = Vec::new();
            for method in method_order {
                if let Some(packages) = by_method.get(method) {
                    let cmd = crate::processors::InstallMethod::batch_command(method, packages);
                    println!("  {} {}", color::bold(&format!("[{}]", method)),
                        packages.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", "));
                    println!("       {}", color::dim(&cmd));
                    commands.push(cmd);
                }
            }
            for (tool_name, cmd) in &ungroupable {
                println!("  {} {}", color::bold(&format!("[{}]", tool_name)), color::dim(cmd));
                commands.push(cmd.clone());
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

            // Execute batch commands
            let mut any_failed = false;
            for cmd in &commands {
                println!("Running: {}", color::dim(cmd));
                let status = Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .status()?;
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
            } else {
                println!("{}", color::green("All tools installed successfully."));
            }
        }
        ToolsAction::InstallDeps { yes } => {
            let config = builder
                .map(|b| &b.config.dependencies)
                .ok_or_else(|| anyhow::anyhow!("install-deps requires a project with rsconstruct.toml"))?;

            if config.is_empty() {
                println!("No dependencies declared in [dependencies].");
                return Ok(());
            }

            // Build install commands
            let mut commands: Vec<(String, String)> = Vec::new(); // (description, command)
            if !config.pip.is_empty() {
                let pkgs = config.pip.join(" ");
                commands.push((
                    format!("[pip] {}", config.pip.join(", ")),
                    format!("pip install {}", pkgs),
                ));
            }
            if !config.npm.is_empty() {
                let pkgs = config.npm.join(" ");
                commands.push((
                    format!("[npm] {}", config.npm.join(", ")),
                    format!("npm install {}", pkgs),
                ));
            }
            if !config.gem.is_empty() {
                let pkgs = config.gem.join(" ");
                commands.push((
                    format!("[gem] {}", config.gem.join(", ")),
                    format!("gem install {}", pkgs),
                ));
            }
            if !config.system.is_empty() {
                println!("{}: system packages must be installed manually: {}",
                    color::yellow("Note"),
                    config.system.join(", "));
            }

            if commands.is_empty() {
                println!("No installable dependencies (only system packages declared).");
                return Ok(());
            }

            println!("{}:", color::bold("Dependencies to install"));
            for (desc, cmd) in &commands {
                println!("  {} {}", color::bold(desc), color::dim(&format!("({})", cmd)));
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

            let mut any_failed = false;
            for (desc, cmd) in &commands {
                println!("Running: {} ({})", desc, color::dim(cmd));
                let status = Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .status()?;
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
                    "Some dependency installs failed",
                ).into());
            } else {
                println!("{}", color::green("All dependencies installed successfully."));
            }
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
            "    {} [label=\"{}\" shape=box style=filled fillcolor=lightblue];\n",
            id, tool
        ));
    }

    // Processor nodes
    out.push_str("    // Processors\n");
    for proc in &processors {
        let id = sanitize_node_id("proc", proc);
        out.push_str(&format!(
            "    {} [label=\"{}\" shape=box style=filled fillcolor=lightyellow];\n",
            id, proc
        ));
    }

    // Edges
    out.push_str("    // Edges\n");
    for (tool, procs) in tool_map {
        let tool_id = sanitize_node_id("tool", tool);
        for proc in procs {
            let proc_id = sanitize_node_id("proc", proc);
            out.push_str(&format!("    {} -> {};\n", tool_id, proc_id));
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
                "    {}[\"{}\"]:::tool --> {}[\"{}\"]:::processor\n",
                tool_id, tool, proc_id, proc
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
            lines.push(format!("{} \u{2192} {}", tool, proc));
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
    use crate::errors;
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

    child.stdin.take().expect(errors::STDIN_PIPED).write_all(dot_content.as_bytes())?;

    let output = child.wait_with_output()?;
    check_command_output(&output, "dot")?;

    Ok(String::from_utf8(output.stdout)?)
}
