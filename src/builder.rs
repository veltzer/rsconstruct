use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use crate::checksum::ChecksumCache;
use crate::cli::{GraphFormat, GraphViewer};
use crate::config::Config;
use crate::executor::Executor;
use crate::graph::BuildGraph;
use crate::processors::{Linter, ProductDiscovery, SleepProcessor, TemplateProcessor};

const CACHE_FILE: &str = ".rsb_cache.json";

pub struct Builder {
    project_root: PathBuf,
    checksum_cache: ChecksumCache,
    cache_file_path: PathBuf,
    config: Config,
}

impl Builder {
    pub fn new() -> Result<Self> {
        let project_root = std::env::current_dir()?;
        let cache_file_path = project_root.join(CACHE_FILE);

        let checksum_cache = ChecksumCache::load_from_file(&cache_file_path)
            .unwrap_or_else(|_| ChecksumCache::new());

        let config = Config::load(&project_root)?;

        Ok(Self {
            project_root,
            checksum_cache,
            cache_file_path,
            config,
        })
    }

    /// Execute an incremental build using the dependency graph
    pub fn build(&mut self, force: bool, verbose: bool, jobs: Option<usize>) -> Result<()> {
        // Create processors
        let processors = self.create_processors();

        // Build the dependency graph
        let graph = self.build_graph_with_processors(&processors)?;

        // Create executor with parallelism from command line or config
        let parallel = jobs.unwrap_or(self.config.build.parallel);
        let executor = Executor::new(&processors, parallel);

        // Execute the build
        let stats = executor.execute(&graph, &mut self.checksum_cache, force, verbose)?;

        // Save checksum cache
        self.save_cache()?;

        // Print summary (only in verbose mode)
        stats.print_summary(verbose);

        Ok(())
    }

    /// Clean all build artifacts using the dependency graph
    pub fn clean(&mut self) -> Result<()> {
        println!("Cleaning build artifacts...");

        // Clear checksum cache
        self.checksum_cache.clear();

        // Remove cache file
        if self.cache_file_path.exists() {
            fs::remove_file(&self.cache_file_path)
                .context("Failed to remove cache file")?;
            println!("Removed cache file: {}", self.cache_file_path.display());
        }

        // Create processors and build graph
        let processors = self.create_processors();
        let graph = self.build_graph_with_processors(&processors)?;

        // Use executor to clean
        let executor = Executor::new(&processors, 1);
        executor.clean(&graph)?;

        // Also clean the lint stub directory if it exists
        let lint_stub_dir = self.project_root.join("out/lint");
        if lint_stub_dir.exists() {
            fs::remove_dir_all(&lint_stub_dir)
                .context("Failed to remove lint stub directory")?;
            println!("Removed lint stub directory: {}", lint_stub_dir.display());
        }

        println!("Clean completed!");
        Ok(())
    }

    /// Create all available processors
    fn create_processors(&self) -> HashMap<String, Box<dyn ProductDiscovery>> {
        let mut processors: HashMap<String, Box<dyn ProductDiscovery>> = HashMap::new();

        // Template processor
        let templates_dir = self.project_root.join("templates");
        let output_dir = self.project_root.clone();
        if let Ok(template_proc) = TemplateProcessor::new(templates_dir, output_dir) {
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

    fn save_cache(&self) -> Result<()> {
        self.checksum_cache.save_to_file(&self.cache_file_path)?;
        Ok(())
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
