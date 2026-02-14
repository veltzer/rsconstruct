use std::fs;
use anyhow::{Context, Result};
use crate::cli::{GraphFormat, GraphViewer};
use crate::processors::log_command;
use super::Builder;

impl Builder {
    /// Print the dependency graph in the specified format
    pub fn print_graph(&self, format: GraphFormat) -> Result<()> {
        let graph = self.build_graph()?;

        // Output in the requested format
        let output = match format {
            GraphFormat::Dot => graph.to_dot(),
            GraphFormat::Mermaid => graph.to_mermaid(),
            GraphFormat::Json => graph.to_json(),
            GraphFormat::Text => graph.to_text(),
            GraphFormat::Svg => graph.to_svg()?,
        };

        println!("{}", output);
        Ok(())
    }

    /// View the dependency graph in a viewer
    pub fn view_graph(&self, viewer: GraphViewer) -> Result<()> {
        use std::process::Command;

        let graph = self.build_graph()?;

        // Create temp file
        let temp_dir = std::env::temp_dir();

        match viewer {
            GraphViewer::Mermaid => {
                let html_path = temp_dir.join("rsb_graph.html");
                let html_content = graph.to_html();
                fs::write(&html_path, html_content)
                    .context("Failed to write HTML file")?;

                // Open in browser
                self.open_file(&html_path)?;
                println!("Opened graph in browser: {}", html_path.display());
            }
            GraphViewer::Svg => {
                // Check if dot is available
                let mut dot_check_cmd = Command::new("dot");
                dot_check_cmd.arg("-V");
                log_command(&dot_check_cmd);
                let dot_check = dot_check_cmd.output();
                if dot_check.map_or(true, |o| !o.status.success()) {
                    anyhow::bail!("Graphviz 'dot' command not found. Install Graphviz or use --view=mermaid");
                }

                let dot_path = temp_dir.join("rsb_graph.dot");
                let svg_path = temp_dir.join("rsb_graph.svg");

                // Write DOT file
                let dot_content = graph.to_dot();
                fs::write(&dot_path, dot_content)
                    .context("Failed to write DOT file")?;

                // Convert to SVG
                let mut dot_cmd = Command::new("dot");
                dot_cmd.arg("-Tsvg").arg(&dot_path).arg("-o").arg(&svg_path);
                log_command(&dot_cmd);
                let output = dot_cmd
                    .output()
                    .context("Failed to run dot command")?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!("dot command failed: {}", stderr);
                }

                // Open SVG
                self.open_file(&svg_path)?;
                println!("Opened graph: {}", svg_path.display());
            }
        }

        Ok(())
    }

    /// Open a file with the configured viewer or the system default application
    pub(super) fn open_file(&self, path: &std::path::Path) -> Result<()> {
        use std::process::Command;

        let cmd = if let Some(ref viewer) = self.config.graph.viewer {
            viewer.as_str()
        } else {
            #[cfg(target_os = "linux")]
            { "xdg-open" }

            #[cfg(target_os = "macos")]
            { "open" }
        };

        let mut open_cmd = Command::new(cmd);
        open_cmd.arg(path);
        log_command(&open_cmd);
        open_cmd
            .spawn()
            .with_context(|| format!("Failed to open file with {}", cmd))?;

        Ok(())
    }
}
