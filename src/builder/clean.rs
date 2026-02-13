use std::fs;
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
        let mut dirs_removed = 0usize;
        let out_dir = std::path::PathBuf::from("out");
        if out_dir.is_dir() {
            for entry in fs::read_dir(&out_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() && fs::read_dir(&path)?.next().is_none() {
                    fs::remove_dir(&path)
                        .with_context(|| format!("Failed to remove directory {}", path.display()))?;
                    dirs_removed += 1;
                    if verbose {
                        println!("Removed empty directory: {}", path.display());
                    }
                }
            }
        }

        // Print summary
        let total_files: usize = stats.values().sum();
        if total_files == 0 && dirs_removed == 0 {
            println!("{}", color::dim("Clean summary: nothing to clean"));
        } else {
            let mut parts: Vec<String> = stats.iter()
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .map(|(proc, count)| format!("{}: {}", proc, count))
                .collect();
            if dirs_removed > 0 {
                parts.push(format!("{} empty dir(s)", dirs_removed));
            }
            let detail = if parts.is_empty() {
                String::new()
            } else {
                format!(" ({})", parts.join(", "))
            };
            println!("{}", color::green(&format!(
                "Clean summary: {} file(s) removed{}",
                total_files, detail,
            )));
        }
        Ok(())
    }

    /// Remove all build outputs and cache directories (.rsb/ and out/)
    pub fn distclean(&self) -> Result<()> {
        println!("{}", color::bold("Removing build directories..."));

        let rsb_dir = std::path::Path::new(".rsb");
        if rsb_dir.exists() {
            fs::remove_dir_all(rsb_dir)
                .context("Failed to remove .rsb/ directory")?;
            println!("Removed {}", rsb_dir.display());
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
}
