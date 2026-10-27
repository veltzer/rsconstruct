use super::Builder;
use crate::cli::DisplayOptions;
use crate::color;
use crate::executor::{Executor, ExecutorOptions};
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

impl Builder {
    /// Clean build artifacts using the dependency graph.
    /// If `processor_filter` is `Some`, only clean outputs from the selected
    /// processors; the selectors are the ones `build -p` takes (names with or
    /// without the `processor.` prefix, instances, `@type`, `@tool`, `@short`).
    /// When `sweep_empty_dirs` is true (default), directories left empty after
    /// per-product cleanup are removed bottom-up.
    pub fn clean(
        &self,
        ctx: &crate::build_context::BuildContext,
        verbose: bool,
        processor_filter: Option<&[String]>,
        sweep_empty_dirs: bool,
    ) -> Result<()> {
        // Create processors and build graph (fast path: skip dependency scanning)
        let processors = self.create_processors()?;
        let allowed = super::build::resolve_processor_filter(processor_filter, None, &processors)?;
        if let Some(names) = &allowed {
            crate::output::info(&color::bold(&format!(
                "Cleaning outputs for: {}",
                names.join(", ")
            )));
        } else {
            crate::output::info(&color::bold("Cleaning build artifacts..."));
        }
        let mut graph = self.build_graph_for_clean_with_processors(ctx, &processors)?;

        // Filter the graph to only include products from the selected processors
        if let Some(names) = &allowed {
            let filter_set: HashSet<&str> = names.iter().map(std::string::String::as_str).collect();
            graph.retain_products(|p| filter_set.contains(p.processor.as_str()));
        }

        // Collect every directory whose contents may be affected by the clean:
        // the parent of each output file, and the output_dirs themselves
        // (whose parents become candidates once the dir is removed).
        let mut candidate_dirs: HashSet<PathBuf> = HashSet::new();
        for product in graph.products() {
            for output in &product.outputs {
                if let Some(parent) = output.parent()
                    && !parent.as_os_str().is_empty()
                {
                    candidate_dirs.insert(parent.to_path_buf());
                }
            }
            for dir in &product.output_dirs {
                if let Some(parent) = dir.parent()
                    && !parent.as_os_str().is_empty()
                {
                    candidate_dirs.insert(parent.to_path_buf());
                }
            }
        }

        // Use executor to clean (batch_size doesn't matter for clean)
        let policy = crate::executor::IncrementalPolicy;
        let executor = Executor::new(
            &processors,
            ctx,
            &policy,
            ExecutorOptions {
                parallel: 1,
                verbose: false,
                display_opts: DisplayOptions::minimal(),
                batch_size: None,
                explain: false,
                retry: 0,
                force: false,
                keep_going: false,
                timings: false,
                worker_stack_bytes: self.config.build.worker_stack_mib * 1024 * 1024,
            },
        );
        let stats = executor.clean(&graph, verbose)?;

        // Walk every candidate directory bottom-up: try fs::remove_dir (only
        // succeeds if empty), then climb to the parent and try again. Stops at
        // the project root. Try-and-ignore avoids the TOCTOU race where another
        // process populates the dir between an emptiness check and the removal.
        let dirs_removed = if sweep_empty_dirs {
            remove_empty_ancestors(&candidate_dirs, verbose)
        } else {
            0
        };

        // Print summary
        let total_files: usize = stats.values().sum();
        if total_files == 0 && dirs_removed == 0 {
            crate::output::info(&color::dim("Clean summary: nothing to clean"));
        } else {
            crate::output::info(&color::bold("Clean summary:"));
            let sorted_stats: std::collections::BTreeMap<_, _> = stats.iter().collect();
            for (proc, count) in &sorted_stats {
                crate::output::info(&format!("  {proc}: {count} file(s)"));
            }
            if dirs_removed > 0 {
                crate::output::info(&format!("  {dirs_removed} empty dir(s) removed"));
            }
            crate::output::info(&color::green(&format!(
                "Total: {total_files} file(s) removed",
            )));
        }
        Ok(())
    }

    /// `clean all -p`: remove everything of the selected processors, output
    /// files and cache entries both, so the next build runs their products
    /// again instead of restoring them. `selectors` is the `build -p`
    /// language and must select at least one processor (without `-p`,
    /// `clean all` is `distclean`). With `dry_run`, print what would go and
    /// remove nothing.
    ///
    /// The cache entry removed is the one for each product's current inputs
    /// (the key the next build would look up); entries for earlier inputs are
    /// `cache remove-stale`'s business, and the blobs are `cache trim`'s. A
    /// remote cache, when pull is on, may still satisfy the next build.
    pub fn clean_all_of(
        &self,
        ctx: &crate::build_context::BuildContext,
        verbose: bool,
        selectors: &[String],
        dry_run: bool,
        sweep_empty_dirs: bool,
    ) -> Result<()> {
        let processors = self.create_processors()?;
        let allowed = super::build::resolve_processor_filter(Some(selectors), None, &processors)?
            .unwrap_or_default();
        if allowed.is_empty() {
            bail!("clean all -p selected no processor");
        }
        crate::output::info(&color::bold(&format!(
            "{} everything of: {}",
            if dry_run { "Would clean" } else { "Cleaning" },
            allowed.join(", ")
        )));

        // The full graph, dependencies included: the cache key of a product
        // mixes in its input checksums, and generated inputs are only known
        // once dependencies are resolved.
        let mut graph = self.build_graph_with_processors(ctx, &processors)?;
        let filter_set: HashSet<&str> = allowed.iter().map(String::as_str).collect();
        graph.retain_products(|p| filter_set.contains(p.processor.as_str()));

        let mut cache_keys: Vec<(String, String)> = Vec::new();
        for product in graph.products() {
            let input_checksum = crate::checksum::combined_input_checksum(ctx, &product.inputs)?;
            cache_keys.push((
                product.processor.clone(),
                product.descriptor_key(&input_checksum),
            ));
        }

        if dry_run {
            let mut files = 0usize;
            let mut dirs = 0usize;
            for product in graph.products() {
                for output in &product.outputs {
                    if output.exists() {
                        files += 1;
                        if verbose {
                            crate::output::info(&format!("  would remove {}", output.display()));
                        }
                    }
                }
                for dir in &product.output_dirs {
                    if dir.exists() {
                        dirs += 1;
                        if verbose {
                            crate::output::info(&format!("  would remove {}/", dir.display()));
                        }
                    }
                }
            }
            let entries = cache_keys
                .iter()
                .filter(|(_, key)| self.object_store.has_descriptor(key))
                .count();
            crate::output::info(&format!(
                "Would remove {files} output file(s), {dirs} output dir(s) and {entries} cache entr{} for {} product(s); nothing removed (--dry-run)",
                if entries == 1 { "y" } else { "ies" },
                graph.products().len()
            ));
            return Ok(());
        }

        let mut candidate_dirs: HashSet<PathBuf> = HashSet::new();
        for product in graph.products() {
            for output in &product.outputs {
                if let Some(parent) = output.parent()
                    && !parent.as_os_str().is_empty()
                {
                    candidate_dirs.insert(parent.to_path_buf());
                }
            }
            for dir in &product.output_dirs {
                if let Some(parent) = dir.parent()
                    && !parent.as_os_str().is_empty()
                {
                    candidate_dirs.insert(parent.to_path_buf());
                }
            }
        }
        let policy = crate::executor::IncrementalPolicy;
        let executor = Executor::new(
            &processors,
            ctx,
            &policy,
            ExecutorOptions {
                parallel: 1,
                verbose: false,
                display_opts: DisplayOptions::minimal(),
                batch_size: None,
                explain: false,
                retry: 0,
                force: false,
                keep_going: false,
                timings: false,
                worker_stack_bytes: self.config.build.worker_stack_mib * 1024 * 1024,
            },
        );
        let stats = executor.clean(&graph, verbose)?;

        let mut entries: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for (processor, key) in &cache_keys {
            if self.object_store.remove_descriptor(key)? {
                *entries.entry(processor.as_str()).or_default() += 1;
            }
        }

        let dirs_removed = if sweep_empty_dirs {
            remove_empty_ancestors(&candidate_dirs, verbose)
        } else {
            0
        };

        let total_files: usize = stats.values().sum();
        let total_entries: usize = entries.values().sum();
        if total_files == 0 && total_entries == 0 && dirs_removed == 0 {
            crate::output::info(&color::dim("Clean summary: nothing to clean"));
            return Ok(());
        }
        crate::output::info(&color::bold("Clean summary:"));
        for name in &allowed {
            let files = stats.get(name).copied().unwrap_or(0);
            let cached = entries.get(name.as_str()).copied().unwrap_or(0);
            crate::output::info(&format!(
                "  {}: {files} file(s), {cached} cache entr{}",
                crate::tables::short_processor_name(name),
                if cached == 1 { "y" } else { "ies" }
            ));
        }
        if dirs_removed > 0 {
            crate::output::info(&format!("  {dirs_removed} empty dir(s) removed"));
        }
        crate::output::info(&color::green(&format!(
            "Total: {total_files} file(s) and {total_entries} cache entr{} removed; the next build runs these products again",
            if total_entries == 1 { "y" } else { "ies" }
        )));
        Ok(())
    }

    /// `clean all` without `-p`: remove the build output and cache
    /// directories (.rsconstruct/ and out/) outright — all of .rsconstruct/
    /// but the project's installed tools in .rsconstruct/tools/, which are
    /// not build state. With `dry_run`, name them and remove nothing.
    pub fn distclean(&self, dry_run: bool) -> Result<()> {
        crate::output::info(&color::bold(if dry_run {
            "Would remove build directories..."
        } else {
            "Removing build directories..."
        }));

        // The configured output root, not a hardcoded "out".
        let out_dir = std::path::PathBuf::from(&self.config.build.output_dir);
        for dir in [std::path::PathBuf::from(".rsconstruct"), out_dir] {
            if !dir.exists() {
                continue;
            }
            if dry_run {
                crate::output::info(&format!("Would remove {}/", dir.display()));
                continue;
            }
            if dir == std::path::Path::new(".rsconstruct") {
                crate::tool_store::clear_state_dir(&dir)?;
            } else {
                fs::remove_dir_all(&dir)
                    .with_context(|| format!("Failed to remove {}/ directory", dir.display()))?;
            }
            crate::output::info(&format!("Removed {}", dir.display()));
        }

        if dry_run {
            crate::output::info(&color::dim("Nothing removed (--dry-run)"));
        } else {
            crate::output::info(&color::green("Distclean completed!"));
        }
        Ok(())
    }

    /// Hard clean using `git clean -qffxd`. Requires a git repository.
    pub fn hardclean(&self, ctx: &crate::build_context::BuildContext) -> Result<()> {
        use std::process::Command;

        if !std::path::Path::new(".git").exists() {
            bail!("Not a git repository. Hardclean requires a git repository.");
        }

        crate::output::info(&color::bold("Running git clean -qffxd..."));

        let mut cmd = Command::new("git");
        cmd.args(["clean", "-qffxd"]);
        let output =
            crate::processor::run_command_capture(ctx, &cmd).context("Failed to run git clean")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git clean failed:\n{stderr}");
        }

        println!("{}", color::green("Hardclean completed!"));
        Ok(())
    }

    /// Remove files not tracked by git and not known as `RSConstruct` build outputs.
    /// Dry-run by default (lists files); use `force` to actually delete.
    pub fn clean_unknown(
        &self,
        ctx: &crate::build_context::BuildContext,
        force: bool,
        verbose: bool,
        respect_gitignore: bool,
    ) -> Result<()> {
        use ignore::WalkBuilder;
        use std::process::Command;

        if !std::path::Path::new(".git").exists() {
            bail!("Not a git repository. clean unknown requires a git repository.");
        }

        // Build graph to discover RSConstruct outputs
        let processors = self.create_processors()?;
        let graph = self.build_graph_for_clean_with_processors(ctx, &processors)?;

        // Collect RSConstruct-known output files
        let mut rsconstruct_outputs: HashSet<PathBuf> = HashSet::new();
        let mut rsconstruct_output_dirs: Vec<PathBuf> = Vec::new();
        for product in graph.products() {
            for output in &product.outputs {
                rsconstruct_outputs.insert(output.clone());
            }
            for dir in &product.output_dirs {
                rsconstruct_output_dirs.push(dir.as_ref().clone());
            }
        }

        // Get git-tracked files
        let mut cmd = Command::new("git");
        cmd.args(["ls-files", "--cached"]);
        let output = crate::processor::run_command_capture(ctx, &cmd)
            .context("Failed to run git ls-files")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git ls-files failed:\n{stderr}");
        }
        let git_tracked: HashSet<PathBuf> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(PathBuf::from)
            .collect();

        // Walk the project tree, respecting .gitignore so that intentionally
        // ignored files (IDE configs, virtualenvs, *.pyc, etc.) are not flagged
        // as unknown. RSConstruct outputs in gitignored directories (e.g. out/)
        // are already excluded via the rsconstruct_outputs/rsconstruct_output_dirs
        // checks, so respecting .gitignore doesn't hide anything we care about.
        let mut unknown_files: Vec<PathBuf> = Vec::new();
        let walker = WalkBuilder::new(".")
            .hidden(false)
            .git_ignore(respect_gitignore)
            .git_global(respect_gitignore)
            .git_exclude(respect_gitignore)
            .build();

        for entry in walker {
            let Ok(entry) = entry else { continue };
            // Skip directories — we only care about files
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let path = entry
                .path()
                .strip_prefix("./")
                .unwrap_or_else(|_| entry.path());
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
            if rsconstruct_output_dirs
                .iter()
                .any(|dir| path.starts_with(dir))
            {
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

            let mut summary = format!("Removed {removed} unknown file(s)");
            if dirs_removed > 0 {
                let _ = write!(summary, ", {dirs_removed} empty dir(s)");
            }
            println!("{}", color::green(&summary));
        } else {
            println!(
                "{}",
                color::bold(&format!("Found {} unknown file(s):", unknown_files.len()))
            );
            for path in &unknown_files {
                println!("  {}", path.display());
            }
            println!();
            println!(
                "{}",
                color::dim("This is a dry run. Run without --dry-run to delete them.")
            );
        }

        Ok(())
    }
}

/// Remove every empty directory rooted at any of the given candidates, walking
/// upward toward the project root. `fs::remove_dir` only succeeds on empty
/// directories, so non-empty ones (and their ancestors) are left intact.
/// Returns the number of directories removed.
fn remove_empty_ancestors(candidates: &HashSet<PathBuf>, verbose: bool) -> usize {
    let mut all: HashSet<PathBuf> = HashSet::new();
    for dir in candidates {
        let mut current: Option<&std::path::Path> = Some(dir.as_path());
        while let Some(p) = current {
            if p.as_os_str().is_empty() || p == std::path::Path::new(".") {
                break;
            }
            all.insert(p.to_path_buf());
            current = p.parent();
        }
    }

    // Deepest first so children are tried before their parents.
    let mut sorted: Vec<PathBuf> = all.into_iter().collect();
    sorted.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    let mut removed = 0usize;
    for dir in &sorted {
        if fs::remove_dir(dir).is_ok() {
            removed += 1;
            if verbose {
                println!("Removed empty directory: {}", dir.display());
            }
        }
    }
    removed
}
