use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result, bail};
use crate::color;
use crate::deps_cache::DepsCache;
use super::{Builder, sorted_keys};

impl Builder {
    /// Handle `rsb deps` subcommands
    pub fn deps(&self, action: crate::cli::DepsAction) -> Result<()> {
        use crate::cli::DepsAction;

        match action {
            DepsAction::List => {
                // List all available dependency analyzers
                let analyzers = self.create_analyzers(false);
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

                    println!("{:<10} {} — {}", name, status, color::dim(analyzer.description()));
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
                    let deps_file = PathBuf::from(".rsb/deps.redb");
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
                let mut total_files = 0;
                let mut total_deps = 0;
                for name in sorted_keys(&stats) {
                    let (files, deps) = stats[name];
                    total_files += files;
                    total_deps += deps;
                    println!("{}: {} files, {} dependencies",
                        color::bold(name), files, deps);
                }
                println!();
                println!("{}: {} files, {} dependencies",
                    color::bold("Total"), total_files, total_deps);
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
