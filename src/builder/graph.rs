use std::collections::BTreeMap;
use std::fs;
use anyhow::{Context, Result};
use crate::cli::{GraphAction, GraphFormat, GraphViewer};
use crate::color;
use crate::json_output;
use crate::processors::log_command;
use super::Builder;

impl Builder {
    /// Dispatch graph subcommands
    pub fn graph(&self, action: GraphAction) -> Result<()> {
        match action {
            GraphAction::Show { format } => self.print_graph(format),
            GraphAction::View { viewer } => self.view_graph(viewer),
            GraphAction::Stats => self.graph_stats(),
        }
    }

    /// Print the dependency graph in the specified format
    fn print_graph(&self, format: GraphFormat) -> Result<()> {
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
    fn view_graph(&self, viewer: GraphViewer) -> Result<()> {
        use std::process::Command;

        let graph = self.build_graph()?;

        // Create temp file
        let temp_dir = std::env::temp_dir();

        match viewer {
            GraphViewer::Mermaid => {
                let html_path = temp_dir.join("rsconstruct_graph.html");
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

                let dot_path = temp_dir.join("rsconstruct_graph.dot");
                let svg_path = temp_dir.join("rsconstruct_graph.svg");

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

    /// Show graph statistics (products, processors, dependencies)
    fn graph_stats(&self) -> Result<()> {
        let graph = self.build_graph()?;
        let products = graph.products();

        // Aggregate per-processor stats
        let mut per_processor: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
        let mut total_edges = 0usize;

        for product in products {
            let entry = per_processor.entry(product.processor.as_str()).or_insert((0, 0, 0));
            entry.0 += 1; // product count
            entry.1 += product.inputs.len();
            entry.2 += product.outputs.len();
            total_edges += graph.get_dependencies(product.id).len();
        }

        if json_output::is_json_mode() {
            let stats: Vec<serde_json::Value> = per_processor.iter().map(|(proc, (count, inputs, outputs))| {
                serde_json::json!({
                    "processor": proc,
                    "products": count,
                    "inputs": inputs,
                    "outputs": outputs,
                })
            }).collect();
            let json = serde_json::json!({
                "processors": stats,
                "total_products": products.len(),
                "total_edges": total_edges,
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        } else {
            for (proc, (count, inputs, outputs)) in &per_processor {
                println!("{}: {} products, {} inputs, {} outputs",
                    color::bold(proc), count, inputs, outputs);
            }
            println!();
            println!("{}: {} products, {} dependency edges",
                color::bold("Total"), products.len(), total_edges);
        }

        Ok(())
    }

    /// Open a file with the configured viewer or the system default application
    pub(super) fn open_file(&self, path: &std::path::Path) -> Result<()> {
        use std::process::Command;

        let cmd = if let Some(ref viewer) = self.config.graph.viewer {
            viewer.as_str()
        } else {
            "xdg-open"
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
