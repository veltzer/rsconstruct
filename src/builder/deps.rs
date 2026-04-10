use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result, bail};
use tabled::builder::Builder as TableBuilder;
use crate::color;
use crate::deps_cache::DepsCache;
use super::{Builder, sorted_keys};

/// List all available dependency analyzers (works without rsconstruct.toml).
pub fn list_analyzers() {
    use crate::analyzers::{CppDepAnalyzer, MarkdownDepAnalyzer, PythonDepAnalyzer, TeraDepAnalyzer, DepAnalyzer};
    let analyzers: Vec<(&str, Box<dyn DepAnalyzer>)> = vec![
        ("cpp", Box::new(CppDepAnalyzer::new(Default::default(), false))),
        ("markdown", Box::new(MarkdownDepAnalyzer::new())),
        ("python", Box::new(PythonDepAnalyzer::new())),
        ("tera", Box::new(TeraDepAnalyzer::new())),
    ];
    let mut builder = TableBuilder::new();
    builder.push_record(["Name", "Description"]);
    for (name, analyzer) in &analyzers {
        builder.push_record([name.to_string(), analyzer.description().to_string()]);
    }
    color::print_table(builder.build());
}

/// Print per-analyzer dependency stats with a total line.
fn print_deps_stats(stats: &std::collections::HashMap<String, (usize, usize)>) {
    let mut total_files = 0;
    let mut total_deps = 0;
    let mut builder = TableBuilder::new();
    builder.push_record(["Analyzer", "Files", "Dependencies"]);
    for name in sorted_keys(stats) {
        let (files, deps) = stats[name];
        total_files += files;
        total_deps += deps;
        builder.push_record([name.to_string(), files.to_string(), deps.to_string()]);
    }
    builder.push_record(["Total".to_string(), total_files.to_string(), total_deps.to_string()]);
    color::print_table(builder.build());
}

impl Builder {
    /// Handle `rsconstruct deps` subcommands
    pub fn deps(&self, action: crate::cli::DepsAction) -> Result<()> {
        use crate::cli::DepsAction;

        match action {
            DepsAction::List => unreachable!("handled in main.rs"),
            DepsAction::Used => {
                let analyzers = self.create_analyzers(false);
                let mut builder = TableBuilder::new();
                builder.push_record(["Name", "Status", "Description"]);
                for name in sorted_keys(&analyzers) {
                    let analyzer = &analyzers[name];
                    let enabled = self.config.analyzer.is_enabled(name);
                    let detected = analyzer.auto_detect(&self.file_index);

                    let status = match (enabled, detected) {
                        (true, true) => color::green("enabled, detected"),
                        (true, false) => color::yellow("enabled, not detected"),
                        (false, true) => color::yellow("disabled, detected"),
                        (false, false) => color::dim("disabled"),
                    };

                    builder.push_record([name.to_string(), status.to_string(), analyzer.description().to_string()]);
                }
                color::print_table(builder.build());
            }
            DepsAction::Build => {
                let processors = self.create_processors()?;
                let mut graph = crate::graph::BuildGraph::new();

                // Phase 1: Discover products (fixed-point loop for cross-processor deps)
                let active: Vec<String> = sorted_keys(&processors).into_iter()
                    .filter(|name| self.is_processor_active(name, processors[*name].as_ref()))
                    .cloned()
                    .collect();
                self.discover_products(&mut graph, &processors, &active, false)?;

                let product_count = graph.products().len();
                if product_count == 0 {
                    println!("No products discovered.");
                    return Ok(());
                }

                // Phase 2: Run dependency analyzers
                self.run_analyzers(&mut graph, true)?;

                // Show summary from cache
                let deps_cache = DepsCache::open()?;
                let stats = deps_cache.stats_by_analyzer();
                if !stats.is_empty() {
                    print_deps_stats(&stats);
                }
            }
            DepsAction::Config { name } => {
                if let Some(name) = name {
                    match name.as_str() {
                        "cpp" => {
                            let toml = toml::to_string_pretty(&self.config.analyzer.cpp)
                                .context("Failed to serialize cpp analyzer config")?;
                            println!("[analyzer.cpp]");
                            print!("{}", toml);
                        }
                        "python" => {
                            let toml = toml::to_string_pretty(&self.config.analyzer.python)
                                .context("Failed to serialize python analyzer config")?;
                            println!("[analyzer.python]");
                            print!("{}", toml);
                        }
                        _ => bail!("Unknown analyzer '{}'. Available: cpp, python", name),
                    }
                } else {
                    // Show all analyzer configs
                    println!("[analyzer]");
                    println!("auto_detect = {}", self.config.analyzer.auto_detect);
                    println!("enabled = {:?}", self.config.analyzer.enabled);
                    println!();
                    let toml = toml::to_string_pretty(&self.config.analyzer.cpp)
                        .context("Failed to serialize cpp analyzer config")?;
                    println!("[analyzer.cpp]");
                    print!("{}", toml);
                    println!();
                    let toml = toml::to_string_pretty(&self.config.analyzer.python)
                        .context("Failed to serialize python analyzer config")?;
                    println!("[analyzer.python]");
                    print!("{}", toml);
                }
            }
            DepsAction::Clean { analyzer } => {
                if let Some(analyzer_name) = analyzer {
                    // Clear only entries from specific analyzer
                    let deps_cache = DepsCache::open()?;
                    let removed = deps_cache.remove_by_analyzer(&analyzer_name)?;
                    if removed > 0 {
                        println!("Removed {} entries from '{}' analyzer.", removed, analyzer_name);
                    } else {
                        println!("No entries found for '{}' analyzer.", analyzer_name);
                    }
                } else {
                    // Clear the entire dependency cache
                    let deps_file = PathBuf::from(".rsconstruct/deps.redb");
                    if deps_file.exists() {
                        fs::remove_file(&deps_file)
                            .context("Failed to remove dependency cache")?;
                        println!("Dependency cache cleared.");
                    } else {
                        println!("Dependency cache is already empty.");
                    }
                }
            }
            DepsAction::Stats => {
                // Show statistics by analyzer
                let deps_cache = DepsCache::open()?;
                let stats = deps_cache.stats_by_analyzer();
                if stats.is_empty() {
                    println!("Dependency cache is empty. Run a build first.");
                    return Ok(());
                }
                print_deps_stats(&stats);
            }
            DepsAction::Show { filter } => {
                use crate::cli::DepsShowFilter;
                let deps_cache = DepsCache::open()?;

                match filter {
                    DepsShowFilter::All => {
                        // List all entries
                        let mut entries: Vec<_> = deps_cache.list_all();
                        if entries.is_empty() {
                            println!("Dependency cache is empty. Run a build first.");
                            return Ok(());
                        }
                        // Sort by source path for consistent output
                        entries.sort_by(|a, b| a.0.cmp(&b.0));
                        for (source, deps, analyzer) in entries {
                            Self::print_deps(&source, &deps, &analyzer);
                        }
                    }
                    DepsShowFilter::Files { files } => {
                        // Query specific files
                        let mut found_any = false;
                        for file_arg in &files {
                            let file_path = PathBuf::from(file_arg);
                            if let Some((deps, analyzer)) = deps_cache.get_raw(&file_path) {
                                found_any = true;
                                Self::print_deps(&file_path, &deps, &analyzer);
                            } else {
                                eprintln!("{}: '{}' not in dependency cache", color::yellow("Warning"), file_arg);
                            }
                        }
                        if !found_any {
                            bail!("No cached dependencies found for the specified files");
                        }
                    }
                    DepsShowFilter::Analyzers { analyzers } => {
                        // Filter by analyzer names
                        let mut entries: Vec<_> = deps_cache.list_by_analyzers(&analyzers);
                        if entries.is_empty() {
                            println!("No cached dependencies found for analyzers: {}", analyzers.join(", "));
                            return Ok(());
                        }
                        // Sort by source path for consistent output
                        entries.sort_by(|a, b| a.0.cmp(&b.0));
                        for (source, deps, analyzer) in entries {
                            Self::print_deps(&source, &deps, &analyzer);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Print dependencies for a source file
    fn print_deps(source: &std::path::Path, deps: &[PathBuf], analyzer: &str) {
        let analyzer_tag = if analyzer.is_empty() {
            String::new()
        } else {
            format!(" {}", color::dim(&format!("[{}]", analyzer)))
        };
        if deps.is_empty() {
            println!("{}:{} {}", source.display(), analyzer_tag, color::dim("(no dependencies)"));
        } else {
            println!("{}:{}", source.display(), analyzer_tag);
            for dep in deps {
                println!("  {}", dep.display());
            }
        }
    }
}
