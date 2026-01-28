use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use crate::cli::{GraphFormat, GraphViewer};
use crate::color;
use crate::config::Config;
use crate::executor::Executor;
use crate::graph::BuildGraph;
use crate::object_store::ObjectStore;
use crate::processors::{Linter, ProductDiscovery, SleepProcessor, TemplateProcessor};

pub struct Builder {
    project_root: PathBuf,
    object_store: ObjectStore,
    config: Config,
}

impl Builder {
    pub fn new() -> Result<Self> {
        let project_root = std::env::current_dir()?;
        let config = Config::load(&project_root)?;
        let object_store = ObjectStore::new(project_root.clone(), config.cache.restore_method)?;

        Ok(Self {
            project_root,
            object_store,
            config,
        })
    }

    /// Execute an incremental build using the dependency graph
    pub fn build(&mut self, force: bool, verbose: bool, jobs: Option<usize>, timings: bool, keep_going: bool) -> Result<()> {
        // Create processors
        let processors = self.create_processors();

        // Build the dependency graph
        let graph = self.build_graph_with_processors(&processors)?;

        // Create executor with parallelism from command line or config
        let parallel = jobs.unwrap_or(self.config.build.parallel);
        let executor = Executor::new(&processors, parallel);

        // Execute the build
        let stats = executor.execute(&graph, &mut self.object_store, force, verbose, timings, keep_going)?;

        // Save object store index
        self.object_store.save()?;

        // Print summary (in verbose mode or when timings requested)
        stats.print_summary(verbose, timings);

        // Return error if there were failures in keep-going mode
        if stats.failed_count > 0 {
            anyhow::bail!("Build completed with {} error(s)", stats.failed_count);
        }

        Ok(())
    }

    /// Show the status of each product in the build graph
    pub fn status(&self) -> Result<()> {
        let processors = self.create_processors();
        let graph = self.build_graph_with_processors(&processors)?;

        let products = graph.products();
        if products.is_empty() {
            println!("No products discovered.");
            return Ok(());
        }

        let mut up_to_date = 0usize;
        let mut stale = 0usize;
        let mut restorable = 0usize;

        for product in products {
            let cache_key = product.cache_key();
            let input_checksum = match ObjectStore::combined_input_checksum(&product.inputs) {
                Ok(cs) => cs,
                Err(_) => {
                    println!("  {} [{}] {}", color::yellow("STALE"), product.processor, product.display());
                    stale += 1;
                    continue;
                }
            };

            if !self.object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs) {
                println!("  {} [{}] {}", color::green("UP-TO-DATE"), product.processor, product.display());
                up_to_date += 1;
            } else if self.object_store.can_restore(&cache_key, &input_checksum, &product.outputs) {
                println!("  {} [{}] {}", color::cyan("RESTORABLE"), product.processor, product.display());
                restorable += 1;
            } else {
                println!("  {} [{}] {}", color::yellow("STALE"), product.processor, product.display());
                stale += 1;
            }
        }

        println!();
        println!("{}: {} up-to-date, {} stale, {} restorable",
            color::bold("Summary"), up_to_date, stale, restorable);

        Ok(())
    }

    /// Clean all build artifacts using the dependency graph
    pub fn clean(&mut self) -> Result<()> {
        println!("{}", color::bold("Cleaning build artifacts..."));

        // Create processors and build graph
        let processors = self.create_processors();
        let graph = self.build_graph_with_processors(&processors)?;

        // Use executor to clean
        let executor = Executor::new(&processors, 1);
        executor.clean(&graph)?;

        // Clear the object store cache
        self.object_store.clear()?;

        // Also clean the lint stub directory if it exists
        let lint_stub_dir = self.project_root.join("out/lint");
        if lint_stub_dir.exists() {
            fs::remove_dir_all(&lint_stub_dir)
                .context("Failed to remove lint stub directory")?;
            println!("Removed lint stub directory: {}", lint_stub_dir.display());
        }

        // Also clean the sleep stub directory if it exists
        let sleep_stub_dir = self.project_root.join("out/sleep");
        if sleep_stub_dir.exists() {
            fs::remove_dir_all(&sleep_stub_dir)
                .context("Failed to remove sleep stub directory")?;
            println!("Removed sleep stub directory: {}", sleep_stub_dir.display());
        }

        println!("{}", color::green("Clean completed!"));
        Ok(())
    }

    /// Create all available processors
    fn create_processors(&self) -> HashMap<String, Box<dyn ProductDiscovery>> {
        let mut processors: HashMap<String, Box<dyn ProductDiscovery>> = HashMap::new();

        // Template processor
        let templates_dir = self.project_root.join("templates");
        let output_dir = self.project_root.clone();
        if let Ok(template_proc) = TemplateProcessor::new(templates_dir, output_dir, self.config.template.clone()) {
            processors.insert("template".to_string(), Box::new(template_proc));
        }

        // Lint processor
        let linter = Linter::new(self.project_root.clone(), self.config.lint.clone());
        processors.insert("lint".to_string(), Box::new(linter));

        // Sleep processor (for testing parallelism)
        let sleep_proc = SleepProcessor::new(self.project_root.clone());
        processors.insert("sleep".to_string(), Box::new(sleep_proc));

        processors
    }

    /// Print the dependency graph in the specified format
    pub fn print_graph(&self, format: GraphFormat) -> Result<()> {
        let graph = self.build_graph()?;

        // Output in the requested format
        let output = match format {
            GraphFormat::Dot => graph.to_dot(),
            GraphFormat::Mermaid => graph.to_mermaid(),
            GraphFormat::Json => graph.to_json(),
            GraphFormat::Text => graph.to_text(),
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
            GraphViewer::Dot => {
                // Check if dot is available
                let dot_check = Command::new("dot").arg("-V").output();
                if dot_check.is_err() || !dot_check.unwrap().status.success() {
                    anyhow::bail!("Graphviz 'dot' command not found. Install Graphviz or use --view=mermaid");
                }

                let dot_path = temp_dir.join("rsb_graph.dot");
                let svg_path = temp_dir.join("rsb_graph.svg");

                // Write DOT file
                let dot_content = graph.to_dot();
                fs::write(&dot_path, dot_content)
                    .context("Failed to write DOT file")?;

                // Convert to SVG
                let output = Command::new("dot")
                    .arg("-Tsvg")
                    .arg(&dot_path)
                    .arg("-o")
                    .arg(&svg_path)
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

    /// Build the dependency graph using provided processors
    fn build_graph_with_processors(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>) -> Result<BuildGraph> {
        let mut graph = BuildGraph::new();

        for (name, processor) in processors {
            if self.config.processors.is_enabled(name) {
                processor.discover(&mut graph)?;
            }
        }

        graph.resolve_dependencies();
        Ok(graph)
    }

    /// Build the dependency graph (creates processors internally)
    fn build_graph(&self) -> Result<BuildGraph> {
        let processors = self.create_processors();
        self.build_graph_with_processors(&processors)
    }

    /// Open a file with the system default application
    fn open_file(&self, path: &std::path::Path) -> Result<()> {
        use std::process::Command;

        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";

        #[cfg(target_os = "macos")]
        let cmd = "open";

        #[cfg(target_os = "windows")]
        let cmd = "start";

        Command::new(cmd)
            .arg(path)
            .spawn()
            .context(format!("Failed to open file with {}", cmd))?;

        Ok(())
    }
}
