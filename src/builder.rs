use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use crate::checksum::ChecksumCache;
use crate::cli::GraphFormat;
use crate::config::Config;
use crate::graph::BuildGraph;
use crate::processors::{BuildStats, Linter, ProcessStats, ProductDiscovery, TemplateProcessor};

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
    pub fn build(&mut self, force: bool, verbose: bool) -> Result<()> {
        // Create processors
        let processors = self.create_processors();

        // Build the dependency graph
        let mut graph = BuildGraph::new();

        for (name, processor) in &processors {
            if self.config.processors.is_enabled(name) {
                processor.discover(&mut graph)?;
            }
        }

        // Resolve dependencies between products
        graph.resolve_dependencies();

        // Get execution order
        let order = graph.topological_sort()?;

        // Execute products in order
        let mut stats_by_processor: HashMap<String, ProcessStats> = HashMap::new();

        for id in order {
            let product = graph.get_product(id).unwrap();

            // Check if this product needs rebuilding
            if !graph.needs_rebuild(product, &self.checksum_cache, force)? {
                if verbose {
                    println!("[{}] Skipping (unchanged): {}", product.processor, product.display());
                }
                let stats = stats_by_processor
                    .entry(product.processor.clone())
                    .or_insert_with(|| ProcessStats::new(&product.processor));
                stats.skipped += 1;
                continue;
            }

            // Find the processor and execute
            if let Some(processor) = processors.get(&product.processor) {
                println!("[{}] Processing: {}", product.processor, product.display());
                processor.execute(product)?;

                // Update cache
                graph.update_cache(product, &mut self.checksum_cache)?;

                let stats = stats_by_processor
                    .entry(product.processor.clone())
                    .or_insert_with(|| ProcessStats::new(&product.processor));
                stats.processed += 1;
            }
        }

        // Save checksum cache
        self.save_cache()?;

        // Build aggregated stats
        let mut stats = BuildStats::default();
        for (_, proc_stats) in stats_by_processor {
            stats.add(proc_stats);
        }

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

        // Create processors and discover products
        let processors = self.create_processors();
        let mut graph = BuildGraph::new();

        for (name, processor) in &processors {
            if self.config.processors.is_enabled(name) {
                processor.discover(&mut graph)?;
            }
        }

        // Clean all products
        for product in graph.products() {
            if let Some(processor) = processors.get(&product.processor) {
                processor.clean(product)?;
            }
        }

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

        processors
    }

    fn save_cache(&self) -> Result<()> {
        self.checksum_cache.save_to_file(&self.cache_file_path)?;
        Ok(())
    }

    /// Print the dependency graph in the specified format
    pub fn print_graph(&self, format: GraphFormat) -> Result<()> {
        // Create processors and discover products
        let processors = self.create_processors();
        let mut graph = BuildGraph::new();

        for (name, processor) in &processors {
            if self.config.processors.is_enabled(name) {
                processor.discover(&mut graph)?;
            }
        }

        // Resolve dependencies
        graph.resolve_dependencies();

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
}
