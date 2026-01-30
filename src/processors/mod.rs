mod cc;
mod cpplint;
mod pylint;
mod ruff;
mod sleep;
mod spellcheck;
mod template;

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use walkdir::WalkDir;

use crate::color;
use crate::ignore::IgnoreRules;

pub use crate::graph::{BuildGraph, Product};

/// Directories to exclude when scanning Python projects
pub const PYTHON_EXCLUDE_DIRS: &[&str] = &[
    "/.venv/", "/__pycache__/", "/.git/", "/out/",
    "/node_modules/", "/.tox/", "/build/", "/dist/", "/.eggs/",
];

/// Directories to exclude when scanning C/C++ projects
pub const CC_EXCLUDE_DIRS: &[&str] = &["/.git/", "/out/", "/build/", "/dist/"];

/// Directories to exclude when scanning for spellcheck
pub const SPELLCHECK_EXCLUDE_DIRS: &[&str] = &[
    "/.git/", "/out/", "/.rsb/", "/node_modules/", "/build/", "/dist/", "/target/",
];

/// Find files matching given extensions in a directory.
///
/// - `root`: directory to walk
/// - `extensions`: file extensions to match (e.g., &[".py", ".pyi"])
/// - `exclude_dirs`: directory path segments to skip (e.g., &["/.git/", "/out/"])
/// - `ignore_rules`: project ignore rules
/// - `recursive`: if false, only scan the top-level directory
///
/// Returns sorted list of matching paths.
pub fn find_files(
    root: &Path,
    extensions: &[&str],
    exclude_dirs: &[&str],
    ignore_rules: &Arc<IgnoreRules>,
    recursive: bool,
) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }

    let walker = if recursive {
        WalkDir::new(root)
    } else {
        WalkDir::new(root).max_depth(1)
    };

    let mut files: Vec<PathBuf> = walker
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            if !path.is_file() {
                return false;
            }

            // Check exclude dirs
            if !exclude_dirs.is_empty() {
                let path_str = path.to_string_lossy();
                if exclude_dirs.iter().any(|dir| path_str.contains(dir)) {
                    return false;
                }
            }

            // Check extension match
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !extensions.iter().any(|ext| name.ends_with(ext)) {
                return false;
            }

            // Check ignore rules
            !ignore_rules.is_ignored(path)
        })
        .map(|e| e.path().to_path_buf())
        .collect();

    files.sort();
    files
}
pub use cc::CcProcessor;
pub use cpplint::Cpplinter;
pub use pylint::PylintProcessor;
pub use ruff::RuffProcessor;
pub use sleep::SleepProcessor;
pub use spellcheck::SpellcheckProcessor;
pub use template::TemplateProcessor;

/// Trait for processors that can discover products for the build graph
/// Must be Sync + Send for parallel execution support
pub trait ProductDiscovery: Sync + Send {
    /// Discover all products this processor can produce
    fn discover(&self, graph: &mut BuildGraph) -> Result<()>;

    /// Execute a single product
    fn execute(&self, product: &Product) -> Result<()>;

    /// Clean outputs for a product
    fn clean(&self, product: &Product) -> Result<()>;

    /// Auto-detect whether this processor is relevant for the current project
    fn auto_detect(&self) -> bool;
}

/// Timing for a single product execution
#[derive(Debug, Clone)]
pub struct ProductTiming {
    pub display: String,
    pub processor: String,
    pub duration: Duration,
}

/// Statistics from processing a category of items
#[derive(Debug)]
pub struct ProcessStats {
    pub processed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub restored: usize,
    pub files_created: usize,
    pub files_restored: usize,
    pub duration: Duration,
    pub product_timings: Vec<ProductTiming>,
}

impl ProcessStats {
    pub fn new(_name: &str) -> Self {
        Self {
            processed: 0,
            failed: 0,
            skipped: 0,
            restored: 0,
            files_created: 0,
            files_restored: 0,
            duration: Duration::ZERO,
            product_timings: Vec::new(),
        }
    }

    pub fn total(&self) -> usize {
        self.processed + self.failed + self.skipped + self.restored
    }
}

/// Aggregated statistics from all processors
#[derive(Default)]
pub struct BuildStats {
    pub categories: Vec<ProcessStats>,
    pub total_duration: Duration,
    pub failed_count: usize,
    pub failed_messages: Vec<String>,
}

impl BuildStats {
    pub fn add(&mut self, stats: ProcessStats) {
        if stats.total() > 0 {
            self.categories.push(stats);
        }
    }

    pub fn total_processed(&self) -> usize {
        self.categories.iter().map(|s| s.processed).sum()
    }

    pub fn total_skipped(&self) -> usize {
        self.categories.iter().map(|s| s.skipped).sum()
    }

    pub fn total_restored(&self) -> usize {
        self.categories.iter().map(|s| s.restored).sum()
    }

    pub fn total_files_created(&self) -> usize {
        self.categories.iter().map(|s| s.files_created).sum()
    }

    pub fn total_files_restored(&self) -> usize {
        self.categories.iter().map(|s| s.files_restored).sum()
    }

    pub fn print_summary(&self, summary: bool, timings: bool) {
        if !summary && !timings {
            return;
        }

        if self.categories.is_empty() && self.failed_count == 0 {
            if summary {
                println!("{}", color::dim("Nothing to build."));
            }
            return;
        }

        if summary {
            let total_processed = self.total_processed();
            let total_restored = self.total_restored();
            let total_failed = self.failed_count;
            let total_skipped = self.total_skipped();
            let total_files_created = self.total_files_created();
            let total_files_restored = self.total_files_restored();

            let mut parts = Vec::new();
            if total_processed > 0 {
                if total_files_created > 0 {
                    parts.push(format!("{} processed ({} files created)", total_processed, total_files_created));
                } else {
                    parts.push(format!("{} processed", total_processed));
                }
            }
            if total_restored > 0 {
                if total_files_restored > 0 {
                    parts.push(format!("{} restored ({} files)", total_restored, total_files_restored));
                } else {
                    parts.push(format!("{} restored", total_restored));
                }
            }
            if total_failed > 0 {
                parts.push(format!("{} failed", total_failed));
            }
            if total_skipped > 0 {
                parts.push(format!("{} unchanged", total_skipped));
            }

            if parts.is_empty() {
                println!("{}", color::dim("Nothing to build."));
            } else {
                let line = format!("Build summary: {}", parts.join(", "));
                if total_failed > 0 {
                    println!("{}", color::red(&line));
                } else {
                    println!("{}", color::green(&line));
                }
            }
        }

        if self.failed_count > 0 {
            println!("{}", color::red(&format!("Build finished with {} error(s):", self.failed_count)));
            for msg in &self.failed_messages {
                println!("  {} {}", color::red("*"), msg);
            }
        }

        if timings {
            println!();
            println!("{}", color::bold("Timing:"));
            for cat in &self.categories {
                for pt in &cat.product_timings {
                    println!("  [{}] {} {}", pt.processor, pt.display,
                        color::dim(&format!("({:.3}s)", pt.duration.as_secs_f64())));
                }
            }
            println!("  {}", color::bold(&format!("Total: {:.3}s", self.total_duration.as_secs_f64())));
        }
    }
}
