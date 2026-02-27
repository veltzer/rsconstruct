//! JSON output mode for machine-readable build events.
//!
//! When enabled, rsb outputs JSON Lines (one JSON object per line) instead of
//! human-readable text. This is useful for CI integration, build dashboards,
//! and IDE integration.

use serde::Serialize;

use crate::errors;
use std::io::{self, Write};
use std::time::Duration;

/// Check if JSON output mode is enabled.
pub fn is_json_mode() -> bool {
    crate::runtime_flags::json_mode()
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

    /// A product is about to be processed
    ProductStart {
        /// Product identifier
        product: String,
        /// Processor name
        processor: String,
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

/// Entry for `rsb processors list --json`.
#[derive(Debug, Serialize)]
pub struct ProcessorListEntry {
    pub name: String,
    pub processor_type: String,
    pub enabled: bool,
    pub detected: bool,
    pub hidden: bool,
    pub batch: bool,
    pub description: String,
}

/// Entry for `rsb tools list --json`.
#[derive(Debug, Serialize)]
pub struct ToolListEntry {
    pub tool: String,
    pub processors: Vec<String>,
}

/// Emit a JSON event to stdout.
pub fn emit(event: &BuildEvent) {
    if !is_json_mode() {
        return;
    }

    let json = serde_json::to_string(event).expect(errors::JSON_SERIALIZE);
    let mut stdout = io::stdout().lock();
    // Intentionally discard write errors: stdout may be a broken pipe (SIGPIPE)
    // when the consumer closes early, which is not an error we can recover from.
    let _ = writeln!(stdout, "{}", json);
}

/// Emit a build start event.
pub fn emit_build_start(total_products: usize) {
    emit(&BuildEvent::BuildStart { total_products });
}

/// Emit a product start event.
pub fn emit_product_start(product: &str, processor: &str) {
    emit(&BuildEvent::ProductStart {
        product: product.to_string(),
        processor: processor.to_string(),
    });
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

