use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use anyhow::{Context, Result, bail};
use crate::cli::DisplayOptions;
use crate::color;
use crate::executor::{Executor, ExecutorOptions};
use super::Builder;

impl Builder {
    /// Clean all build artifacts using the dependency graph
    pub fn clean(&self, verbose: bool) -> Result<()> {
        println!("{}", color::bold("Cleaning build artifacts..."));

        // Create processors and build graph (fast path: skip dependency scanning)
        let processors = self.create_processors()?;
        let graph = self.build_graph_for_clean_with_processors(&processors)?;

        // Use executor to clean (batch_size doesn't matter for clean)
        let executor = Executor::new(&processors, ExecutorOptions {
            parallel: 1,
            verbose: false,
            display_opts: DisplayOptions::minimal(),
            batch_size: None,
            explain: false,
            retry: 0,
        }, Arc::new(std::sync::atomic::AtomicBool::new(false)));
        let stats = executor.clean(&graph, verbose)?;

        // Remove empty subdirectories under out/
        // Use try-and-ignore pattern to avoid TOCTOU races (directory could be
        // populated between emptiness check and removal by another process).
        let mut dirs_removed = 0usize;
        let out_dir = std::path::PathBuf::from("out");
        if out_dir.is_dir() {
            for entry in fs::read_dir(&out_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir()
                    && let Ok(()) = fs::remove_dir(&path)
                {
                    dirs_removed += 1;
                    if verbose {
                        println!("Removed empty directory: {}", path.display());
                    }
                }
            }
            // Remove out/ itself if now empty
            if let Ok(()) = fs::remove_dir(&out_dir) {
                dirs_removed += 1;
                if verbose {
                    println!("Removed empty directory: {}", out_dir.display());
                }
            }
        }

        // Print summary
        let total_files: usize = stats.values().sum();
        if total_files == 0 && dirs_removed == 0 {
            println!("{}", color::dim("Clean summary: nothing to clean"));
        } else {
            println!("{}", color::bold("Clean summary:"));
            let sorted_stats: std::collections::BTreeMap<_, _> = stats.iter().collect();
            for (proc, count) in &sorted_stats {
                println!("  {}: {} file(s)", proc, count);
            }
            if dirs_removed > 0 {
                println!("  {} empty dir(s) removed", dirs_removed);
            }
            println!("{}", color::green(&format!(
                "Total: {} file(s) removed",
                total_files,
            )));
        }
        Ok(())
    }

    /// Remove all build outputs and cache directories (.rsconstruct/ and out/)
    pub fn distclean(&self) -> Result<()> {
        println!("{}", color::bold("Removing build directories..."));

        let rsconstruct_dir = std::path::Path::new(".rsconstruct");
        if rsconstruct_dir.exists() {
            fs::remove_dir_all(rsconstruct_dir)
                .context("Failed to remove .rsconstruct/ directory")?;
            println!("Removed {}", rsconstruct_dir.display());
        }

        let out_dir = std::path::Path::new("out");
        if out_dir.exists() {
            fs::remove_dir_all(out_dir)
                .context("Failed to remove out/ directory")?;
            println!("Removed {}", out_dir.display());
        }

        println!("{}", color::green("Distclean completed!"));
        Ok(())
    }

    /// Hard clean using `git clean -qffxd`. Requires a git repository.
    pub fn hardclean(&self) -> Result<()> {
        use std::process::Command;

        if !std::path::Path::new(".git").exists() {
            bail!("Not a git repository. Hardclean requires a git repository.");
        }

        println!("{}", color::bold("Running git clean -qffxd..."));

        let output = Command::new("git")
            .args(["clean", "-qffxd"])
            .output()
            .context("Failed to run git clean")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git clean failed:\n{}", stderr);
        }

        println!("{}", color::green("Hardclean completed!"));
        Ok(())
    }

    /// Remove files not tracked by git and not known as RSConstruct build outputs.
    /// Dry-run by default (lists files); use `force` to actually delete.
    pub fn clean_unknown(&self, force: bool, verbose: bool) -> Result<()> {
        use ignore::WalkBuilder;
        use std::process::Command;

        if !std::path::Path::new(".git").exists() {
            bail!("Not a git repository. clean unknown requires a git repository.");
        }

        // Build graph to discover RSConstruct outputs
        let processors = self.create_processors()?;
        let graph = self.build_graph_for_clean_with_processors(&processors)?;

        // Collect RSConstruct-known output files
        let mut rsconstruct_outputs: HashSet<PathBuf> = HashSet::new();
        let mut rsconstruct_output_dirs: Vec<PathBuf> = Vec::new();
        for product in graph.products() {
            for output in &product.outputs {
                rsconstruct_outputs.insert(output.clone());
            }
            if let Some(ref dir) = product.output_dir {
                rsconstruct_output_dirs.push(dir.as_ref().clone());
            }
        }

        // Get git-tracked files
        let output = Command::new("git")
            .args(["ls-files", "--cached"])
            .output()
            .context("Failed to run git ls-files")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git ls-files failed:\n{}", stderr);
        }
        let git_tracked: HashSet<PathBuf> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(PathBuf::from)
            .collect();

        // Walk the entire project tree. We disable .gitignore handling because
        // unknown files often live in gitignored directories (e.g. out/).
        // We do our own filtering against git-tracked and RSConstruct output sets.
        let mut unknown_files: Vec<PathBuf> = Vec::new();
        let walker = WalkBuilder::new(".")
            .hidden(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .build();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            // Skip directories — we only care about files
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let path = entry.path().strip_prefix("./").unwrap_or(entry.path());
            let path = PathBuf::from(path);

            // Skip .git/ and .rsconstruct/
            if path.starts_with(".git") || path.starts_with(".rsconstruct") {
                continue;
            }

            // Skip git-tracked files
            if git_tracked.contains(&path) {
                continue;
            }

            // Skip RSConstruct output files
            if rsconstruct_outputs.contains(&path) {
                continue;
            }

            // Skip files inside RSConstruct output directories
            if rsconstruct_output_dirs.iter().any(|dir| path.starts_with(dir)) {
                continue;
            }

            unknown_files.push(path);
        }

        unknown_files.sort();

        if unknown_files.is_empty() {
            println!("{}", color::green("No unknown files found."));
            return Ok(());
        }

        if force {
            println!("{}", color::bold("Removing unknown files..."));
            let mut removed = 0usize;
            for path in &unknown_files {
                if let Err(e) = fs::remove_file(path) {
                    eprintln!("  Failed to remove {}: {}", path.display(), e);
                } else {
                    if verbose {
                        println!("  {}", path.display());
                    }
                    removed += 1;
                }
            }

            // Clean up empty parent directories (bottom-up).
            // Walk the full parent chain of each removed file so that
            // nested empty directories are cleaned up too.
            let mut dirs_to_check: HashSet<PathBuf> = HashSet::new();
            for path in &unknown_files {
                let mut current = path.parent();
                while let Some(dir) = current {
                    if dir == std::path::Path::new("") {
                        break;
                    }
                    dirs_to_check.insert(dir.to_path_buf());
                    current = dir.parent();
                }
            }
            let mut sorted_dirs: Vec<PathBuf> = dirs_to_check.into_iter().collect();
            // Sort by depth descending so we remove leaf dirs first
            sorted_dirs.sort_by_key(|b| std::cmp::Reverse(b.components().count()));
            let mut dirs_removed = 0usize;
            for dir in &sorted_dirs {
                // Try to remove — only succeeds if empty
                if fs::remove_dir(dir).is_ok() {
                    dirs_removed += 1;
                }
            }

            let mut summary = format!("Removed {} unknown file(s)", removed);
            if dirs_removed > 0 {
                summary.push_str(&format!(", {} empty dir(s)", dirs_removed));
            }
            println!("{}", color::green(&summary));
        } else {
            println!("{}", color::bold(&format!("Found {} unknown file(s):", unknown_files.len())));
            for path in &unknown_files {
                println!("  {}", path.display());
            }
            println!();
            println!("{}", color::dim("This is a dry run. Run without --dry-run to delete them."));
        }

        Ok(())
    }
}
