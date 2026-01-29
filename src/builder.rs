use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use crate::cli::{GraphFormat, GraphViewer};
use crate::color;
use crate::config::Config;
use crate::executor::Executor;
use crate::graph::BuildGraph;
use crate::ignore::IgnoreRules;
use crate::object_store::ObjectStore;
use crate::processors::{CcProcessor, Cpplinter, Pylinter, ProductDiscovery, SleepProcessor, SpellcheckProcessor, TemplateProcessor};

pub struct Builder {
    project_root: PathBuf,
    object_store: ObjectStore,
    config: Config,
    ignore_rules: Arc<IgnoreRules>,
}

impl Builder {
    pub fn new() -> Result<Self> {
        let project_root = std::env::current_dir()?;
        let config = Config::load(&project_root)?;
        let object_store = ObjectStore::new(project_root.clone(), config.cache.restore_method)?;
        let ignore_rules = Arc::new(IgnoreRules::load(&project_root)?);

        Ok(Self {
            project_root,
            object_store,
            config,
            ignore_rules,
        })
    }

    /// Execute an incremental build using the dependency graph
    pub fn build(&mut self, force: bool, verbose: bool, jobs: Option<usize>, timings: bool, keep_going: bool, processor_verbose: u8, interrupted: Arc<std::sync::atomic::AtomicBool>) -> Result<()> {
        // Create processors
        let processors = self.create_processors(processor_verbose);

        // Build the dependency graph
        let graph = self.build_graph_with_processors(&processors)?;

        // Create executor with parallelism from command line or config
        let parallel = jobs.unwrap_or(self.config.build.parallel);
        let executor = Executor::new(&processors, parallel, processor_verbose, Arc::clone(&interrupted));

        // Execute the build
        let result = executor.execute(&graph, &mut self.object_store, force, verbose, timings, keep_going);

        // Always save object store index, even after errors or interrupt
        self.object_store.save()?;

        // Exit after saving if interrupted
        if interrupted.load(std::sync::atomic::Ordering::SeqCst) {
            std::process::exit(130);
        }

        let stats = result?;

        // Print summary (in verbose mode or when timings requested)
        stats.print_summary(verbose, timings);

        // Return error if there were failures in keep-going mode
        if stats.failed_count > 0 {
            anyhow::bail!("Build completed with {} error(s)", stats.failed_count);
        }

        Ok(())
    }

    /// Show what would happen without executing anything
    pub fn dry_run(&self, force: bool) -> Result<()> {
        let processors = self.create_processors(0);
        let graph = self.build_graph_with_processors(&processors)?;

        let order = graph.topological_sort()?;
        if order.is_empty() {
            println!("No products discovered.");
            return Ok(());
        }

        let mut skip_count = 0usize;
        let mut restore_count = 0usize;
        let mut build_count = 0usize;

        for &id in &order {
            let product = graph.get_product(id).unwrap();
            let cache_key = product.cache_key();
            let input_checksum = match ObjectStore::combined_input_checksum(&product.inputs) {
                Ok(cs) => cs,
                Err(_) => {
                    println!("  {} [{}] {}", color::yellow("BUILD"), product.processor, product.display(0));
                    build_count += 1;
                    continue;
                }
            };

            if !force && !self.object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs) {
                println!("  {} [{}] {}", color::dim("SKIP"), product.processor, product.display(0));
                skip_count += 1;
            } else if !force && self.object_store.can_restore(&cache_key, &input_checksum, &product.outputs) {
                println!("  {} [{}] {}", color::cyan("RESTORE"), product.processor, product.display(0));
                restore_count += 1;
            } else {
                println!("  {} [{}] {}", color::yellow("BUILD"), product.processor, product.display(0));
                build_count += 1;
            }
        }

        println!();
        println!("{}: {} skip, {} restore, {} build",
            color::bold("Summary"), skip_count, restore_count, build_count);

        Ok(())
    }

    /// Show the status of each product in the build graph
    pub fn status(&self) -> Result<()> {
        let processors = self.create_processors(0);
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
                    println!("  {} [{}] {}", color::yellow("STALE"), product.processor, product.display(0));
                    stale += 1;
                    continue;
                }
            };

            if !self.object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs) {
                println!("  {} [{}] {}", color::green("UP-TO-DATE"), product.processor, product.display(0));
                up_to_date += 1;
            } else if self.object_store.can_restore(&cache_key, &input_checksum, &product.outputs) {
                println!("  {} [{}] {}", color::cyan("RESTORABLE"), product.processor, product.display(0));
                restorable += 1;
            } else {
                println!("  {} [{}] {}", color::yellow("STALE"), product.processor, product.display(0));
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
        let processors = self.create_processors(0);
        let graph = self.build_graph_with_processors(&processors)?;

        // Use executor to clean
        let executor = Executor::new(&processors, 1, 0, Arc::new(std::sync::atomic::AtomicBool::new(false)));
        executor.clean(&graph)?;

        // Also clean the pylint stub directory if it exists
        let pylint_stub_dir = self.project_root.join("out/pylint");
        if pylint_stub_dir.exists() {
            fs::remove_dir_all(&pylint_stub_dir)
                .context("Failed to remove pylint stub directory")?;
            println!("Removed pylint stub directory: {}", pylint_stub_dir.display());
        }

        // Also clean the cpplint stub directory if it exists
        let cpplint_stub_dir = self.project_root.join("out/cpplint");
        if cpplint_stub_dir.exists() {
            fs::remove_dir_all(&cpplint_stub_dir)
                .context("Failed to remove cpplint stub directory")?;
            println!("Removed cpplint stub directory: {}", cpplint_stub_dir.display());
        }

        // Also clean the sleep stub directory if it exists
        let sleep_stub_dir = self.project_root.join("out/sleep");
        if sleep_stub_dir.exists() {
            fs::remove_dir_all(&sleep_stub_dir)
                .context("Failed to remove sleep stub directory")?;
            println!("Removed sleep stub directory: {}", sleep_stub_dir.display());
        }

        // Also clean the cc output directory if it exists
        let cc_output_dir = self.project_root.join("out/cc");
        if cc_output_dir.exists() {
            fs::remove_dir_all(&cc_output_dir)
                .context("Failed to remove cc output directory")?;
            println!("Removed cc output directory: {}", cc_output_dir.display());
        }

        // Also clean the spellcheck stub directory if it exists
        let spellcheck_stub_dir = self.project_root.join("out/spellcheck");
        if spellcheck_stub_dir.exists() {
            fs::remove_dir_all(&spellcheck_stub_dir)
                .context("Failed to remove spellcheck stub directory")?;
            println!("Removed spellcheck stub directory: {}", spellcheck_stub_dir.display());
        }

        println!("{}", color::green("Clean completed!"));
        Ok(())
    }

    /// Remove all build outputs and cache directories (.rsb/ and out/)
    pub fn distclean(&self) -> Result<()> {
        println!("{}", color::bold("Removing build directories..."));

        let rsb_dir = self.project_root.join(".rsb");
        if rsb_dir.exists() {
            fs::remove_dir_all(&rsb_dir)
                .context("Failed to remove .rsb/ directory")?;
            println!("Removed {}", rsb_dir.display());
        }

        let out_dir = self.project_root.join("out");
        if out_dir.exists() {
            fs::remove_dir_all(&out_dir)
                .context("Failed to remove out/ directory")?;
            println!("Removed {}", out_dir.display());
        }

        println!("{}", color::green("Distclean completed!"));
        Ok(())
    }

    /// Create all available processors
    fn create_processors(&self, processor_verbose: u8) -> HashMap<String, Box<dyn ProductDiscovery>> {
        let mut processors: HashMap<String, Box<dyn ProductDiscovery>> = HashMap::new();

        // Template processor
        let templates_dir = self.project_root.join("templates");
        let output_dir = self.project_root.clone();
        if let Ok(template_proc) = TemplateProcessor::new(templates_dir, output_dir, self.config.processor.template.clone(), Arc::clone(&self.ignore_rules)) {
            processors.insert("template".to_string(), Box::new(template_proc));
        }

        // Python lint processor
        let pylinter = Pylinter::new(self.project_root.clone(), self.config.processor.pylint.clone(), Arc::clone(&self.ignore_rules));
        processors.insert("pylint".to_string(), Box::new(pylinter));

        // Sleep processor (for testing parallelism)
        let sleep_proc = SleepProcessor::new(self.project_root.clone(), Arc::clone(&self.ignore_rules));
        processors.insert("sleep".to_string(), Box::new(sleep_proc));

        // C/C++ compiler processor
        let cc_proc = CcProcessor::new(self.project_root.clone(), self.config.processor.cc.clone(), Arc::clone(&self.ignore_rules), processor_verbose);
        processors.insert("cc".to_string(), Box::new(cc_proc));

        // C/C++ lint processor
        let cpplinter = Cpplinter::new(self.project_root.clone(), self.config.processor.cpplint.clone(), self.config.processor.cc.clone(), Arc::clone(&self.ignore_rules));
        processors.insert("cpplint".to_string(), Box::new(cpplinter));

        // Spellcheck processor
        let spellcheck_proc = SpellcheckProcessor::new(self.project_root.clone(), self.config.processor.spellcheck.clone(), Arc::clone(&self.ignore_rules));
        processors.insert("spellcheck".to_string(), Box::new(spellcheck_proc));

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

        let mut names: Vec<&String> = processors.keys().collect();
        names.sort();
        for name in names {
            if self.config.processor.is_enabled(name) {
                processors[name].discover(&mut graph)?;
            }
        }

        graph.resolve_dependencies();
        Ok(graph)
    }

    /// Build the dependency graph (creates processors internally)
    fn build_graph(&self) -> Result<BuildGraph> {
        let processors = self.create_processors(0);
        self.build_graph_with_processors(&processors)
    }

    /// Open a file with the configured viewer or the system default application
    fn open_file(&self, path: &std::path::Path) -> Result<()> {
        use std::process::Command;

        let cmd = if let Some(ref viewer) = self.config.graph.viewer {
            viewer.as_str()
        } else {
            #[cfg(target_os = "linux")]
            { "xdg-open" }

            #[cfg(target_os = "macos")]
            { "open" }

            #[cfg(target_os = "windows")]
            { "start" }
        };

        Command::new(cmd)
            .arg(path)
            .spawn()
            .context(format!("Failed to open file with {}", cmd))?;

        Ok(())
    }
}
