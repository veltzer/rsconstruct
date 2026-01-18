use anyhow::Result;
use std::path::Path;
use crate::checksum::ChecksumCache;

/// Result of processing a single item
pub enum ProcessResult {
    /// Item was processed successfully
    Processed,
    /// Item was skipped (unchanged)
    Skipped,
}

/// Trait for items that can be processed with checksum-based caching
pub trait Processable {
    /// Get the source path for checksum calculation
    fn source_path(&self) -> &Path;

    /// Get a unique cache key for this item
    fn cache_key(&self) -> String;

    /// Get a display name for logging
    fn display_name(&self) -> String;

    /// Process the item (called only when checksum indicates change)
    fn process(&self) -> Result<()>;
}

/// Generic processor that handles checksum caching and logging
pub struct Processor {
    name: String,
}

impl Processor {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }

    /// Process a single item with checksum caching
    fn process_item<T: Processable>(
        &self,
        item: &T,
        cache: &mut ChecksumCache,
        force: bool,
        verbose: bool,
    ) -> Result<ProcessResult> {
        let source_path = item.source_path();
        let cache_key = item.cache_key();
        let display_name = item.display_name();

        // Calculate current checksum
        let current_checksum = ChecksumCache::calculate_checksum(source_path)?;

        // Check if item has changed
        if !force && cache.get_by_key(&cache_key) == Some(&current_checksum) {
            if verbose {
                println!("[{}] Skipping (unchanged): {}", self.name, display_name);
            }
            return Ok(ProcessResult::Skipped);
        }

        // Process the item
        println!("[{}] Processing: {}", self.name, display_name);
        item.process()?;

        // Update cache on success
        cache.set_by_key(cache_key, current_checksum);

        Ok(ProcessResult::Processed)
    }

    /// Process multiple items and return stats
    pub fn process_all<T: Processable>(
        &self,
        items: &[T],
        cache: &mut ChecksumCache,
        force: bool,
        verbose: bool,
    ) -> Result<ProcessStats> {
        let mut stats = ProcessStats::new(&self.name);

        for item in items {
            match self.process_item(item, cache, force, verbose)? {
                ProcessResult::Processed => stats.processed += 1,
                ProcessResult::Skipped => stats.skipped += 1,
            }
        }

        Ok(stats)
    }
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
