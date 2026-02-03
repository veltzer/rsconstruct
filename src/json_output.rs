//! JSON output mode for machine-readable build events.
//!
//! When enabled, rsb outputs JSON Lines (one JSON object per line) instead of
//! human-readable text. This is useful for CI integration, build dashboards,
//! and IDE integration.

use serde::Serialize;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Global flag: when true, output JSON instead of human-readable text.
static JSON_MODE: AtomicBool = AtomicBool::new(false);

/// Enable JSON output mode (called once from main).
pub fn set_json_mode(enabled: bool) {
    JSON_MODE.store(enabled, Ordering::Relaxed);
}

/// Check if JSON output mode is enabled.
pub fn is_json_mode() -> bool {
    JSON_MODE.load(Ordering::Relaxed)
}

/// Build event types for JSON output.
#[derive(Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum BuildEvent {
    /// Build is starting
    BuildStart {
        /// Total number of products to process
        total_products: usize,
    },

    /// A product is starting execution
    ProductStart {
        /// Product identifier (processor:file)
        product: String,
        /// Processor name
        processor: String,
        /// Input files
        inputs: Vec<String>,
        /// Output files
        outputs: Vec<String>,
    },

    /// A product completed successfully
    ProductComplete {
        /// Product identifier
        product: String,
        /// Processor name
        processor: String,
        /// "success", "failed", "skipped", or "restored"
        status: String,
        /// Duration in milliseconds (if executed)
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        /// Error message (if failed)
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },

    /// Build completed
    BuildSummary {
        /// Total products
        total: usize,
        /// Successfully processed
        success: usize,
        /// Failed
        failed: usize,
        /// Skipped (unchanged)
        skipped: usize,
        /// Restored from cache
        restored: usize,
        /// Total duration in milliseconds
        duration_ms: u64,
        /// List of error messages
        #[serde(skip_serializing_if = "Vec::is_empty")]
        errors: Vec<String>,
    },

    /// Status of a single product (for `rsb status --json`)
    #[allow(dead_code)]
    ProductStatus {
        /// Product identifier
        product: String,
        /// Processor name
        processor: String,
        /// "current", "stale", or "restorable"
        status: String,
        /// Input files
        inputs: Vec<String>,
        /// Output files
        outputs: Vec<String>,
    },

    /// Processor info (for `rsb processor list --json`)
    #[allow(dead_code)]
    ProcessorInfo {
        /// Processor name
        name: String,
        /// Description
        description: String,
        /// Whether enabled
        enabled: bool,
        /// Whether hidden
        hidden: bool,
        /// Required tools
        tools: Vec<String>,
    },

    /// Cache entry info (for `rsb cache list --json`)
    #[allow(dead_code)]
    CacheEntry {
        /// Cache key
        cache_key: String,
        /// Input checksum
        input_checksum: String,
        /// Output files and existence status
        outputs: Vec<CacheOutput>,
    },
}

/// Output file info for cache entries.
#[derive(Debug, Serialize)]
pub struct CacheOutput {
    pub path: String,
    pub exists: bool,
}

/// Emit a JSON event to stdout.
pub fn emit(event: &BuildEvent) {
    if !is_json_mode() {
        return;
    }

    let json = serde_json::to_string(event).expect("Failed to serialize JSON event");
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{}", json);
}

/// Emit a build start event.
pub fn emit_build_start(total_products: usize) {
    emit(&BuildEvent::BuildStart { total_products });
}

/// Emit a product start event.
pub fn emit_product_start(
    product: &str,
    processor: &str,
    inputs: &[PathBuf],
    outputs: &[PathBuf],
) {
    emit(&BuildEvent::ProductStart {
        product: product.to_string(),
        processor: processor.to_string(),
        inputs: inputs.iter().map(|p| p.display().to_string()).collect(),
        outputs: outputs.iter().map(|p| p.display().to_string()).collect(),
    });
}

/// Emit a product complete event.
pub fn emit_product_complete(
    product: &str,
    processor: &str,
    status: &str,
    duration: Option<Duration>,
    error: Option<&str>,
) {
    emit(&BuildEvent::ProductComplete {
        product: product.to_string(),
        processor: processor.to_string(),
        status: status.to_string(),
        duration_ms: duration.map(|d| d.as_millis() as u64),
        error: error.map(|s| s.to_string()),
    });
}

/// Emit a build summary event.
pub fn emit_build_summary(
    total: usize,
    success: usize,
    failed: usize,
    skipped: usize,
    restored: usize,
    duration: Duration,
    errors: &[String],
) {
    emit(&BuildEvent::BuildSummary {
        total,
        success,
        failed,
        skipped,
        restored,
        duration_ms: duration.as_millis() as u64,
        errors: errors.to_vec(),
    });
}

/// Emit a product status event.
#[allow(dead_code)]
pub fn emit_product_status(
    product: &str,
    processor: &str,
    status: &str,
    inputs: &[PathBuf],
    outputs: &[PathBuf],
) {
    emit(&BuildEvent::ProductStatus {
        product: product.to_string(),
        processor: processor.to_string(),
        status: status.to_string(),
        inputs: inputs.iter().map(|p| p.display().to_string()).collect(),
        outputs: outputs.iter().map(|p| p.display().to_string()).collect(),
    });
}

/// Emit a processor info event.
#[allow(dead_code)]
pub fn emit_processor_info(
    name: &str,
    description: &str,
    enabled: bool,
    hidden: bool,
    tools: &[String],
) {
    emit(&BuildEvent::ProcessorInfo {
        name: name.to_string(),
        description: description.to_string(),
        enabled,
        hidden,
        tools: tools.to_vec(),
    });
}

/// Emit a cache entry event.
#[allow(dead_code)]
pub fn emit_cache_entry(cache_key: &str, input_checksum: &str, outputs: Vec<CacheOutput>) {
    emit(&BuildEvent::CacheEntry {
        cache_key: cache_key.to_string(),
        input_checksum: input_checksum.to_string(),
        outputs,
    });
}
