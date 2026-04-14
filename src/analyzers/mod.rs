//! Dependency analyzers for scanning source files and adding dependencies to the build graph.
//!
//! Analyzers are separate from processors - they run after product discovery to add
//! dependency information (like header files for C/C++ or imports for Python).

mod cpp;
mod icpp;
mod markdown;
mod python;
mod tera;


use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use crate::deps_cache::DepsCache;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;
use crate::processors::{format_command, run_command_capture};
use crate::progress;

/// Trait for dependency analyzers that scan source files and add dependencies to the graph.
///
/// Analyzers run after processors have discovered products. They scan source files
/// to find dependencies (like #include for C/C++ or import for Python) and add
/// them to the appropriate products in the graph.
///
/// Must be Sync + Send for potential parallel execution.
pub trait DepAnalyzer: Sync + Send {
    /// Human-readable description of what this analyzer does.
    fn description(&self) -> &str;

    /// Whether this analyzer is active. Default true; override to respect
    /// the `enabled` field on an analyzer's config struct.
    fn enabled(&self) -> bool { true }

    /// Auto-detect if this analyzer is relevant for the project.
    /// Called with the file index to check for relevant file types.
    fn auto_detect(&self, file_index: &FileIndex) -> bool;

    /// Analyze dependencies and add them to products in the graph.
    ///
    /// The analyzer should:
    /// 1. Find products it can analyze (based on file extensions, etc.)
    /// 2. For each product, scan the primary source file for dependencies
    /// 3. Use deps_cache to avoid re-scanning unchanged files
    /// 4. Add discovered dependencies to the product's inputs
    fn analyze(&self, graph: &mut BuildGraph, deps_cache: &mut DepsCache, file_index: &FileIndex, verbose: bool) -> Result<()>;
}

/// Query pkg-config for include paths from the given packages.
/// Uses `pkg-config --cflags-only-I` and strips the `-I` prefix.
/// Returns an empty list if `packages` is empty or the query fails.
///
/// - `tag`: prefix for log messages (e.g., "cpp" or "icpp")
/// - `packages`: pkg-config package names to query
/// - `verbose`: whether to emit diagnostic messages to stderr
pub fn query_pkg_config_include_paths(tag: &str, packages: &[String], verbose: bool) -> Vec<PathBuf> {
    if packages.is_empty() {
        return Vec::new();
    }

    let mut cmd = Command::new("pkg-config");
    cmd.arg("--cflags-only-I");
    cmd.args(packages);

    if verbose {
        eprintln!("[{}] Querying pkg-config: {}", tag, format_command(&cmd));
    }

    let output = match run_command_capture(&mut cmd) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[{}] Failed to query pkg-config: {}", tag, e);
            return Vec::new();
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("[{}] pkg-config failed: {}", tag, stderr.trim());
        return Vec::new();
    }

    let paths: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|flag| flag.strip_prefix("-I").map(PathBuf::from))
        .collect();

    if verbose && !paths.is_empty() {
        eprintln!("[{}] Found {} include paths from pkg-config", tag, paths.len());
    }

    paths
}

/// Run each command in `commands` via `sh -c` and collect its stdout (trimmed) as an include path.
/// Commands that fail, produce empty output, or yield non-directory paths are skipped with a warning.
///
/// - `tag`: prefix for log messages (e.g., "cpp" or "icpp")
/// - `commands`: shell command strings to run
/// - `verbose`: whether to emit diagnostic messages to stderr
pub fn run_include_path_commands(tag: &str, commands: &[String], verbose: bool) -> Vec<PathBuf> {
    if commands.is_empty() {
        return Vec::new();
    }

    let mut paths = Vec::new();
    for cmd_str in commands {
        if cmd_str.trim().is_empty() {
            continue;
        }

        // Run via shell to support shell syntax (command substitution, etc.)
        let mut cmd = Command::new("sh");
        cmd.arg("-c");
        cmd.arg(cmd_str);

        if verbose {
            eprintln!("[{}] Running include path command: sh -c '{}'", tag, cmd_str);
        }

        let output = match run_command_capture(&mut cmd) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("[{}] Failed to run '{}': {}", tag, cmd_str, e);
                continue;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("[{}] Command '{}' failed: {}", tag, cmd_str, stderr.trim());
            continue;
        }

        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if path_str.is_empty() {
            continue;
        }

        let path = PathBuf::from(&path_str);
        if path.is_dir() {
            if verbose {
                eprintln!("[{}] Added include path from command: {}", tag, path.display());
            }
            paths.push(path);
        } else if verbose {
            eprintln!("[{}] Command output is not a directory: {}", tag, path_str);
        }
    }

    if verbose && !paths.is_empty() {
        eprintln!("[{}] Found {} include paths from commands", tag, paths.len());
    }

    paths
}

/// Shared helper for analyzer `analyze()` implementations.
///
/// Iterates over products in the graph, filters them using `match_product`, checks the
/// dependency cache, scans dependencies using `scan_deps` on cache miss, caches results,
/// and adds discovered dependencies to product inputs. Shows a progress bar and cache stats.
///
/// - `match_product`: given a product, returns `Some(source_path)` if the product is relevant
/// - `scan_deps`: given a source path, returns the list of dependency paths
pub fn analyze_with_scanner<F, G>(
    graph: &mut BuildGraph,
    deps_cache: &mut DepsCache,
    analyzer_name: &str,
    match_product: F,
    scan_deps: G,
    verbose: bool,
) -> Result<()>
where
    F: Fn(&crate::graph::Product) -> Option<PathBuf>,
    G: Fn(&Path) -> Result<Vec<PathBuf>>,
{
    // Collect matching products: (product_id, source_path)
    let products: Vec<(usize, PathBuf)> = graph.products()
        .iter()
        .filter_map(|p| {
            match_product(p).map(|source| (p.id, source))
        })
        .collect();

    if products.is_empty() {
        return Ok(());
    }

    // Show progress bar (hidden in verbose or JSON mode, matching executor style)
    let pb = progress::create_bar(
        products.len() as u64,
        verbose || crate::json_output::is_json_mode(),
    );

    for (id, source) in &products {
        pb.set_message(format!("[{}] {}", analyzer_name, source.display()));

        // Try to get cached dependencies, otherwise scan
        let deps = if let Some(cached) = deps_cache.get(source) {
            cached
        } else {
            let scanned = scan_deps(source)?;
            if let Err(e) = deps_cache.set(source, &scanned, analyzer_name) {
                eprintln!("Warning: failed to cache dependencies for {}: {}", source.display(), e);
            }
            scanned
        };

        // Add dependencies to the product (filter out duplicates via HashSet)
        if !deps.is_empty()
            && let Some(product) = graph.get_product_mut(*id) {
                let existing: HashSet<&PathBuf> = product.inputs.iter().collect();
                let new_deps: Vec<PathBuf> = deps.into_iter()
                    .filter(|dep| !existing.contains(dep))
                    .collect();
                product.inputs.extend(new_deps);
            }

        pb.inc(1);
    }
    pb.finish_and_clear();

    // Show cache stats in verbose mode
    if verbose {
        let stats = deps_cache.stats();
        if stats.hits > 0 || stats.misses > 0 {
            eprintln!("[{}] Dependency cache: {} hits, {} recalculated",
                analyzer_name, stats.hits, stats.misses);
        }
    }

    Ok(())
}
