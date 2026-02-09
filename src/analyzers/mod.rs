//! Dependency analyzers for scanning source files and adding dependencies to the build graph.
//!
//! Analyzers are separate from processors - they run after product discovery to add
//! dependency information (like header files for C/C++ or imports for Python).

mod cpp;
mod python;

pub use cpp::CppDepAnalyzer;
pub use python::PythonDepAnalyzer;

use anyhow::Result;
use std::path::{Path, PathBuf};
use crate::deps_cache::DepsCache;
use crate::errors;
use crate::file_index::FileIndex;
use crate::graph::BuildGraph;

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
    let pb = if verbose || crate::json_output::is_json_mode() {
        indicatif::ProgressBar::hidden()
    } else {
        let pb = indicatif::ProgressBar::new(products.len() as u64);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40} {pos}/{len} {msg}")
                .expect(errors::INVALID_PROGRESS_TEMPLATE)
                .progress_chars("=> "),
        );
        pb
    };

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

        // Add dependencies to the product
        if !deps.is_empty()
            && let Some(product) = graph.get_product_mut(*id) {
                for dep in deps {
                    if !product.inputs.contains(&dep) {
                        product.inputs.push(dep);
                    }
                }
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
