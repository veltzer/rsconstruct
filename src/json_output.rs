//! JSON output mode for machine-readable build events.
//!
//! When enabled, rsb outputs JSON Lines (one JSON object per line) instead of
//! human-readable text. This is useful for CI integration, build dashboards,
//! and IDE integration.

use serde::Serialize;
use std::io::{self, Write};
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

/// Status of a completed product in a build.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductStatus {
    Success,
    Failed,
    Skipped,
    Restored,
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

    /// A product completed successfully
    ProductComplete {
        /// Product identifier
        product: String,
        /// Processor name
        processor: String,
        /// Build status
        status: ProductStatus,
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

}

/// Processor file entry for `rsb processors files --json`.
#[derive(Debug, Serialize)]
pub struct ProcessorFileEntry {
    pub processor: String,
    pub processor_type: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
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

/// Emit a product complete event.
pub fn emit_product_complete(
    product: &str,
    processor: &str,
    status: ProductStatus,
    duration: Option<Duration>,
    error: Option<&str>,
) {
    emit(&BuildEvent::ProductComplete {
        product: product.to_string(),
        processor: processor.to_string(),
        status,
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

