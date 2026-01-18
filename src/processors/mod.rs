mod linter;
mod template;

use anyhow::Result;

pub use crate::graph::{BuildGraph, Product};
pub use linter::Linter;
pub use template::TemplateProcessor;

/// Trait for processors that can discover products for the build graph
pub trait ProductDiscovery {
    /// Discover all products this processor can produce
    fn discover(&self, graph: &mut BuildGraph) -> Result<()>;

    /// Execute a single product
    fn execute(&self, product: &Product) -> Result<()>;

    /// Clean outputs for a product
    fn clean(&self, product: &Product) -> Result<()>;
}

/// Statistics from processing a category of items
pub struct ProcessStats {
    pub name: String,
    pub processed: usize,
    pub skipped: usize,
}

impl ProcessStats {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            processed: 0,
            skipped: 0,
        }
    }

    pub fn total(&self) -> usize {
        self.processed + self.skipped
    }
}

/// Aggregated statistics from all processors
#[derive(Default)]
pub struct BuildStats {
    pub categories: Vec<ProcessStats>,
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

    pub fn print_summary(&self, verbose: bool) {
        if !verbose {
            return;
        }

        if self.categories.is_empty() {
            println!("Nothing to build.");
            return;
        }

        let total_processed = self.total_processed();
        let total_skipped = self.total_skipped();

        if total_processed == 0 && total_skipped > 0 {
            println!("Build complete: everything up to date ({} files unchanged)", total_skipped);
        } else {
            let mut parts = Vec::new();
            for cat in &self.categories {
                if cat.processed > 0 {
                    parts.push(format!("{} {}", cat.processed, cat.name));
                }
            }
            println!("Build complete: {} processed", parts.join(", "));
        }
    }
}
