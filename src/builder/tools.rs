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
fn tool_runtime(tool: &str) -> &'static str {
    match tool {
        "ruff" | "pylint" | "mypy" | "pyrefly" | "yamllint" | "sphinx-build" | "pip"
        | "jsonlint" | "a2x" | "python3" => "python",
        "marp" | "mmdc" | "markdownlint" | "npm" | "node" | "node_modules/.bin/markdownlint" => "node",
        "bundle" | "mdl" | "ruby" | "gems/bin/mdl" => "ruby",
        "cargo" | "rustc" | "mdbook" | "rumdl" | "taplo" => "rust",
        "perl" | "markdown" => "perl",
        "gcc" | "g++" | "clang" | "clang++" | "clang-tidy" | "cppcheck" | "make"
        | "shellcheck" | "jq" | "aspell" | "pandoc" | "pdflatex" | "qpdf" | "drawio"
        | "libreoffice" | "flock" | "pdfunite" | "dot" => "system",
        tool => {
            debug_assert!(false, "tool_runtime: unrecognized tool '{tool}'");
            "system"
        }
    }
}

impl Builder {
    /// Verify tool versions against .tools.versions lock file.
    /// Called at the start of build unless --ignore-tool-versions is passed.
    pub fn verify_tool_versions(&self) -> Result<()> {
        let processors = self.create_processors()?;
        let config = &self.config;
        let tool_commands = tool_lock::collect_tool_commands(
            &processors,
            &|name| config.processor.is_enabled(name),
        );
        if tool_commands.is_empty() {
            return Ok(());
        }
        tool_lock::verify_lock_file(&tool_commands)
    }

    /// Handle `rsbuild tools` subcommands
    pub fn tools(&self, action: ToolsAction, verbose: bool) -> Result<()> {
        let processors = self.create_processors()?;

        let show_all = matches!(&action, ToolsAction::List { all: true });
        let install_yes = matches!(&action, ToolsAction::Install { yes: true, .. });

        let mut tool_map: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
        for name in sorted_keys(&processors) {
            if !show_all && !self.config.processor.is_enabled(name) {
                continue;
            }
            for tool in processors[name].required_tools() {
                let procs = tool_map.entry(tool).or_default();
                if !procs.contains(name) {
                    procs.push(name.clone());
                }
            }
        }

        // Build install map from the central tool_install_command registry
        let install_map: std::collections::BTreeMap<String, Option<String>> = tool_map.keys()
            .map(|tool| {
                let cmd = crate::processors::tool_install_command(tool)
                    .map(|s| s.to_string());
                (tool.clone(), cmd)
            })
            .collect();

        match action {
            ToolsAction::List { .. } => {
                if crate::json_output::is_json_mode() {
                    let entries: Vec<crate::json_output::ToolListEntry> = tool_map.iter()
                        .map(|(tool, procs)| crate::json_output::ToolListEntry {
                            tool: tool.clone(),
                            processors: procs.clone(),
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&entries)?);
                    return Ok(());
                }

                for (tool, procs) in &tool_map {
                    println!("{} ({})", tool, procs.join(", "));
                }
            }
            ToolsAction::Check => {
                let config = &self.config;
                let tool_commands = tool_lock::collect_tool_commands(
                    &processors,
                    &|name| config.processor.is_enabled(name),
                );
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
                let config = &self.config;
                let tool_commands = tool_lock::collect_tool_commands(
                    &processors,
                    &|name| config.processor.is_enabled(name),
                );
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
                    let html_path = std::env::temp_dir().join("rsb_tools_graph.html");
                    std::fs::write(&html_path, html_content)
                        .map_err(|e| anyhow::anyhow!("Failed to write HTML file: {}", e))?;
                    self.open_file(&html_path)?;
                    println!("Opened tools graph in browser: {}", html_path.display());
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
                // Collect per-tool info
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

                // Build runtime summary
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
                    // Human-readable output
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

                    // Runtime summary
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
                let to_install: Vec<(String, String)> = if let Some(ref name) = name {
                    match install_map.get(name).and_then(|c| c.as_ref()) {
                        Some(cmd) => vec![(name.clone(), cmd.clone())],
                        None => {
                            eprintln!("{}: Installation procedure still not setup for '{}'", color::red("Error"), name);
                            return Err(crate::exit_code::RsbError::new(
                                crate::exit_code::RsbExitCode::ToolError,
                                format!("No install command known for tool '{}'", name),
                            ).into());
                        }
                    }
                } else {
                    let mut missing = Vec::new();
                    let mut any_unknown = false;
                    for tool in tool_map.keys() {
                        if which::which(tool).is_err() {
                            match install_map.get(tool).and_then(|c| c.as_ref()) {
                                Some(cmd) => missing.push((tool.clone(), cmd.clone())),
                                None => {
                                    eprintln!("{}: Installation procedure still not setup for '{}'", color::red("Error"), tool);
                                    any_unknown = true;
                                }
                            }
                        }
                    }
                    if any_unknown {
                        return Err(crate::exit_code::RsbError::new(
                            crate::exit_code::RsbExitCode::ToolError,
                            "Some tools have no known install procedure",
                        ).into());
                    }
                    if missing.is_empty() {
                        println!("{}", color::green("All tools are already installed."));
                        return Ok(());
                    }
                    missing
                };

                println!("The following tools will be installed:");
                for (tool, cmd) in &to_install {
                    println!("  {} \u{2014} {}", color::bold(tool), color::dim(cmd));
                }

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

                let mut any_failed = false;
                for (tool, cmd) in &to_install {
                    println!("Installing {} \u{2014} running: {}", color::bold(tool), color::dim(cmd));
                    let status = Command::new("sh")
                        .arg("-c")
                        .arg(cmd)
                        .status()?;
                    if status.success() {
                        println!("{} {}", tool, color::green("installed"));
                    } else {
                        println!("{} {} (exit code {})", tool, color::red("failed"),
                            status.code().map_or("unknown".to_string(), |c| c.to_string()));
                        any_failed = true;
                    }
                }
                if any_failed {
                    return Err(crate::exit_code::RsbError::new(
                        crate::exit_code::RsbExitCode::ToolError,
                        "Some tools failed to install",
                    ).into());
                }
            }
        }

        Ok(())
    }
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
        .map(|(tool, procs)| crate::json_output::ToolListEntry {
            tool: tool.clone(),
            processors: procs.clone(),
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
    <title>RSBuild Tools Graph</title>
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
    <h1>RSBuild Tools Graph</h1>
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
