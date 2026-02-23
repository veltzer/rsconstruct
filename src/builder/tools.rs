use std::io::Write;
use std::process::Command;
use anyhow::Result;
use crate::cli::ToolsAction;
use crate::color;
use crate::tool_lock;
use super::{Builder, sorted_keys};

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

    /// Handle `rsb tools` subcommands
    pub fn tools(&self, action: ToolsAction) -> Result<()> {
        let processors = self.create_processors()?;

        let show_all = matches!(&action, ToolsAction::List { all: true } | ToolsAction::Check { all: true });
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
            ToolsAction::Check { .. } => {
                let mut any_missing = false;
                for (tool, procs) in &tool_map {
                    let procs_str = procs.join(", ");
                    if let Ok(path) = which::which(tool) {
                        let path_str = path.display().to_string();
                        println!("{} ({}) {} {}", tool, procs_str, color::green("found"), color::dim(&path_str));
                    } else {
                        let hint = install_map.get(tool).and_then(|c| c.as_deref())
                            .map(|h| format!(" \u{2014} install with: {}", color::dim(h)))
                            .unwrap_or_default();
                        println!("{} ({}) {}{}", tool, procs_str, color::red("missing"), hint);
                        any_missing = true;
                    }
                }
                if any_missing {
                    return Err(crate::exit_code::RsbError::new(
                        crate::exit_code::RsbExitCode::ToolError,
                        "Some required tools are missing",
                    ).into());
                }
            }
            ToolsAction::Lock { check } => {
                let config = &self.config;
                let tool_commands = tool_lock::collect_tool_commands(
                    &processors,
                    &|name| config.processor.is_enabled(name),
                );

                if check {
                    tool_lock::verify_lock_file(&tool_commands)?;
                    println!("{}", color::green("Tool versions match lock file."));
                } else {
                    let lock = tool_lock::create_lock(&tool_commands)?;
                    for (name, info) in &lock.tools {
                        let first_line = info.version_output.lines().next().unwrap_or("");
                        println!("{} {} {}", name, color::green("locked"), color::dim(first_line));
                    }
                    tool_lock::write_lock_file(&lock)?;
                    println!("Wrote {}", color::bold(".tools.versions"));
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
