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
        tool_lock::verify_lock_file(&self.project_root, &tool_commands)
    }

    /// Handle `rsb tools` subcommands
    pub fn tools(&self, action: ToolsAction) -> Result<()> {
        let processors = self.create_processors()?;

        let show_all = matches!(&action, ToolsAction::List { all: true } | ToolsAction::Check { all: true });

        let mut tool_pairs: Vec<(String, String)> = Vec::new();
        for name in sorted_keys(&processors) {
            if !show_all && !self.config.processor.is_enabled(name) {
                continue;
            }
            for tool in processors[name].required_tools() {
                tool_pairs.push((tool, name.clone()));
            }
        }
        tool_pairs.sort();
        tool_pairs.dedup();

        match action {
            ToolsAction::List { .. } => {
                for (tool, processor) in &tool_pairs {
                    println!("{} ({})", tool, processor);
                }
            }
            ToolsAction::Check { .. } => {
                let mut any_missing = false;
                for (tool, processor) in &tool_pairs {
                    if let Ok(path) = which::which(tool) {
                        println!("{} ({}) {} {}", tool, processor, color::green("found"), color::dim(&path.display().to_string()));
                    } else {
                        println!("{} ({}) {}", tool, processor, color::red("missing"));
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
                    tool_lock::verify_lock_file(&self.project_root, &tool_commands)?;
                    println!("{}", color::green("Tool versions match lock file."));
                } else {
                    let lock = tool_lock::create_lock(&tool_commands)?;
                    for (name, info) in &lock.tools {
                        let first_line = info.version_output.lines().next().unwrap_or("");
                        println!("{} {} {}", name, color::green("locked"), color::dim(first_line));
                    }
                    tool_lock::write_lock_file(&self.project_root, &lock)?;
                    println!("Wrote {}", color::bold(".tools.versions"));
                }
            }
        }

        Ok(())
    }
}
