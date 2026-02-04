//! Dependency analyzers for scanning source files and adding dependencies to the build graph.
//!
//! Analyzers are separate from processors - they run after product discovery to add
//! dependency information (like header files for C/C++ or imports for Python).

mod cpp;
mod python;

pub use cpp::CppDepAnalyzer;
pub use python::PythonDepAnalyzer;

use anyhow::Result;
use crate::deps_cache::DepsCache;
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
    /// Name of this analyzer (e.g., "cpp", "python")
    #[allow(dead_code)]
    fn name(&self) -> &str;

    /// Human-readable description of what this analyzer does
    #[allow(dead_code)]
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
    fn analyze(&self, graph: &mut BuildGraph, deps_cache: &mut DepsCache, file_index: &FileIndex) -> Result<()>;
}
