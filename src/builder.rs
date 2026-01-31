use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use crate::cli::{GraphFormat, GraphViewer};
use crate::color;
use crate::config::Config;
use crate::executor::Executor;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;
use crate::object_store::ObjectStore;
use crate::processors::{CcProcessor, CpplintProcessor, MakeProcessor, PylintProcessor, RuffProcessor, ProductDiscovery, SleepProcessor, SpellcheckProcessor, TemplateProcessor, log_command};

pub struct Builder {
    project_root: PathBuf,
    object_store: ObjectStore,
    config: Config,
    file_index: FileIndex,
}

impl Builder {
    pub fn new() -> Result<Self> {
        let project_root = std::env::current_dir()?;
        Config::require_config(&project_root)?;
        let config = Config::load(&project_root)?;
        let object_store = ObjectStore::new(project_root.clone(), config.cache.restore_method)?;
        let file_index = FileIndex::build(&project_root)?;

        Ok(Self {
            project_root,
            object_store,
            config,
            file_index,
        })
    }

    /// Get a reference to the file index
    pub fn file_index(&self) -> &FileIndex {
        &self.file_index
    }

    /// Execute an incremental build using the dependency graph
    pub fn build(&mut self, force: bool, verbose: u8, jobs: Option<usize>, timings: bool, keep_going: bool, interrupted: Arc<std::sync::atomic::AtomicBool>, summary: bool) -> Result<()> {
        // Create processors
        let processors = self.create_processors(verbose)?;

        // Build the dependency graph
        let graph = self.build_graph_with_processors(&processors)?;

        // Create executor with parallelism from command line or config
        let parallel = jobs.unwrap_or(self.config.build.parallel);
        let executor = Executor::new(&processors, parallel, verbose, Arc::clone(&interrupted));

        // Execute the build
        let result = executor.execute(&graph, &mut self.object_store, force, timings, keep_going);

        // Always save object store index, even after errors or interrupt
        self.object_store.save()?;

        // Exit after saving if interrupted
        if interrupted.load(std::sync::atomic::Ordering::SeqCst) {
            std::process::exit(130);
        }

        let stats = result?;

        // Print summary
        stats.print_summary(summary, timings);

        // Return error if there were failures in keep-going mode
        if stats.failed_count > 0 {
            anyhow::bail!("Build completed with {} error(s)", stats.failed_count);
        }

        Ok(())
    }

    /// Show what would happen without executing anything
    pub fn dry_run(&self, force: bool) -> Result<()> {
        let processors = self.create_processors(0)?;
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
                    println!("{} [{}] {}", color::yellow("BUILD"), product.processor, product.display(0));
                    build_count += 1;
                    continue;
                }
            };

            if !force && !self.object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs) {
                println!("{} [{}] {}", color::dim("SKIP"), product.processor, product.display(0));
                skip_count += 1;
            } else if !force && self.object_store.can_restore(&cache_key, &input_checksum, &product.outputs) {
                println!("{} [{}] {}", color::cyan("RESTORE"), product.processor, product.display(0));
                restore_count += 1;
            } else {
                println!("{} [{}] {}", color::yellow("BUILD"), product.processor, product.display(0));
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
        let processors = self.create_processors(0)?;
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
                    println!("{} [{}] {}", color::yellow("STALE"), product.processor, product.display(0));
                    stale += 1;
                    continue;
                }
            };

            if !self.object_store.needs_rebuild(&cache_key, &input_checksum, &product.outputs) {
                println!("{} [{}] {}", color::green("UP-TO-DATE"), product.processor, product.display(0));
                up_to_date += 1;
            } else if self.object_store.can_restore(&cache_key, &input_checksum, &product.outputs) {
                println!("{} [{}] {}", color::cyan("RESTORABLE"), product.processor, product.display(0));
                restorable += 1;
            } else {
                println!("{} [{}] {}", color::yellow("STALE"), product.processor, product.display(0));
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
        let processors = self.create_processors(0)?;
        let graph = self.build_graph_with_processors(&processors)?;

        // Use executor to clean
        let executor = Executor::new(&processors, 1, 0, Arc::new(std::sync::atomic::AtomicBool::new(false)));
        executor.clean(&graph)?;

        // Remove empty subdirectories under out/
        let out_dir = self.project_root.join("out");
        if out_dir.is_dir() {
            for entry in fs::read_dir(&out_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() && fs::read_dir(&path)?.next().is_none() {
                    fs::remove_dir(&path)
                        .context(format!("Failed to remove directory {}", path.display()))?;
                    println!("Removed empty directory: {}", path.display());
                }
            }
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

    /// Hard clean using `git clean -qffxd`. Requires a git repository.
    pub fn hardclean(&self) -> Result<()> {
        let git_dir = self.project_root.join(".git");
        if !git_dir.exists() {
            bail!("Not a git repository. Hardclean requires a git repository.");
        }

        println!("{}", color::bold("Running git clean -qffxd..."));

        let output = Command::new("git")
            .args(["clean", "-qffxd"])
            .current_dir(&self.project_root)
            .output()
            .context("Failed to run git clean")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git clean failed:\n{}", stderr);
        }

        println!("{}", color::green("Hardclean completed!"));
        Ok(())
    }

    /// Create all available processors
    pub fn create_processors(&self, verbose: u8) -> Result<HashMap<String, Box<dyn ProductDiscovery>>> {
        let mut processors: HashMap<String, Box<dyn ProductDiscovery>> = HashMap::new();

        // Template processor
        if let Ok(template_proc) = TemplateProcessor::new(self.project_root.clone(), self.config.processor.template.clone()) {
            processors.insert("template".to_string(), Box::new(template_proc));
        }

        // Ruff processor
        let ruff_proc = RuffProcessor::new(self.project_root.clone(), self.config.processor.ruff.clone());
        processors.insert("ruff".to_string(), Box::new(ruff_proc));

        // Pylint processor
        let pylint_proc = PylintProcessor::new(self.project_root.clone(), self.config.processor.pylint.clone());
        processors.insert("pylint".to_string(), Box::new(pylint_proc));

        // Sleep processor (for testing parallelism)
        let sleep_proc = SleepProcessor::new(self.project_root.clone(), self.config.processor.sleep.clone());
        processors.insert("sleep".to_string(), Box::new(sleep_proc));

        // C/C++ compiler processor (single-file compilation)
        let cc_proc = CcProcessor::new(self.project_root.clone(), self.config.processor.cc_single_file.clone(), verbose);
        processors.insert("cc_single_file".to_string(), Box::new(cc_proc));

        // C/C++ lint processor
        let cpplinter = CpplintProcessor::new(self.project_root.clone(), self.config.processor.cpplint.clone());
        processors.insert("cpplint".to_string(), Box::new(cpplinter));

        // Spellcheck processor
        match SpellcheckProcessor::new(self.project_root.clone(), self.config.processor.spellcheck.clone()) {
            Ok(spellcheck_proc) => {
                processors.insert("spellcheck".to_string(), Box::new(spellcheck_proc));
            }
            Err(e) => {
                if self.config.processor.is_enabled("spellcheck") {
                    return Err(e);
                }
            }
        }

        // Make processor
        let make_proc = MakeProcessor::new(self.project_root.clone(), self.config.processor.make.clone());
        processors.insert("make".to_string(), Box::new(make_proc));

        Ok(processors)
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
                let mut dot_check_cmd = Command::new("dot");
                dot_check_cmd.arg("-V");
                log_command(&dot_check_cmd);
                let dot_check = dot_check_cmd.output();
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

    /// Build the dependency graph using provided processors
    fn build_graph_with_processors(&self, processors: &HashMap<String, Box<dyn ProductDiscovery>>) -> Result<BuildGraph> {
        let mut graph = BuildGraph::new();

        let mut names: Vec<&String> = processors.keys().collect();
        names.sort();
        for name in names {
            let in_enabled_list = self.config.processor.is_enabled(name);
            if !in_enabled_list {
                continue;
            }
            let should_run = !self.config.processor.auto_detect || processors[name].auto_detect(&self.file_index);
            if should_run {
                processors[name].discover(&mut graph, &self.file_index)?;
            }
        }

        graph.resolve_dependencies();
        Ok(graph)
    }

    /// Build the dependency graph, optionally filtering to a single processor.
    /// If `include_all` is true, skip enabled/auto-detect checks.
    pub fn build_graph_filtered(
        &self,
        filter_name: Option<&str>,
        include_all: bool,
    ) -> Result<BuildGraph> {
        let processors = self.create_processors(0)?;
        let mut graph = BuildGraph::new();

        let mut names: Vec<&String> = processors.keys().collect();
        names.sort();
        for name in names {
            if let Some(filter) = filter_name {
                if name.as_str() != filter {
                    continue;
                }
            }
            if !include_all {
                let in_enabled_list = self.config.processor.is_enabled(name);
                if !in_enabled_list {
                    continue;
                }
                let should_run = !self.config.processor.auto_detect || processors[name].auto_detect(&self.file_index);
                if !should_run {
                    continue;
                }
            }
            processors[name].discover(&mut graph, &self.file_index)?;
        }

        graph.resolve_dependencies();
        Ok(graph)
    }

    /// Build the dependency graph (creates processors internally)
    fn build_graph(&self) -> Result<BuildGraph> {
        let processors = self.create_processors(0)?;
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

        let mut open_cmd = Command::new(cmd);
        open_cmd.arg(path);
        log_command(&open_cmd);
        open_cmd
            .spawn()
            .context(format!("Failed to open file with {}", cmd))?;

        Ok(())
    }
}
