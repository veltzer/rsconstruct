use indicatif::ProgressBar;

use crate::color;
use crate::json_output::{emit_product_complete, ProductStatus};

use super::{Executor, RestoreOutcome, SharedState};

impl<'a> Executor<'a> {
    /// Handle the "skip (unchanged)" case for a product.
    /// Logs, emits JSON event, increments stats. Does NOT advance the progress bar
    /// since skips are instant and the bar total excludes them.
    pub(super) fn handle_skip(
        &self,
        product: &crate::graph::Product,
        shared: &SharedState,
    ) {
        if self.verbose {
            println!("[{}] {} {}", product.processor,
                color::dim("Skipping (unchanged):"),
                self.product_display(product));
        }
        emit_product_complete(
            &self.product_display(product),
            &product.processor,
            ProductStatus::Skipped,
            None,
            None,
        );
        let mut stats = shared.stats.lock();
        let proc_stats = stats
            .entry(product.processor.clone())
            .or_default();
        proc_stats.skipped += 1;
        Self::inc_global(shared);
    }

    /// Handle cache restore for a product.
    /// Try to restore a product from cache.
    /// When `emit_fail_event` is true, emits a product_complete "failed" JSON event on error.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_restore(
        &self,
        product: &crate::graph::Product,
        id: usize,
        object_store: &crate::object_store::ObjectStore,
        cache_key: &str,
        input_checksum: &str,
        force: bool,
        keep_going: bool,
        emit_fail_event: bool,
        shared: &SharedState,
        pb_ref: &ProgressBar,
    ) -> RestoreOutcome {
        if force {
            return RestoreOutcome::NotRestorable;
        }
        let restore_result = object_store.restore_from_cache(cache_key, input_checksum, &product.outputs);
        match restore_result {
            Ok(true) => {
                if self.verbose {
                    println!("[{}] {} {}", product.processor,
                        color::cyan("Restored from cache:"),
                        self.product_display(product));
                }
                emit_product_complete(
                    &self.product_display(product),
                    &product.processor,
                    ProductStatus::Restored,
                    None,
                    None,
                );
                let mut stats = shared.stats.lock();
                let proc_stats = stats
                    .entry(product.processor.clone())
                    .or_default();
                proc_stats.restored += 1;
                proc_stats.files_restored += product.outputs.len();
                Self::inc_progress(pb_ref, shared);
                RestoreOutcome::Restored
            }
            Err(e) => {
                if emit_fail_event {
                    emit_product_complete(
                        &self.product_display(product),
                        &product.processor,
                        ProductStatus::Failed,
                        None,
                        Some(&e.to_string()),
                    );
                }
                if keep_going {
                    let msg = format!("[{}] {}: {}", product.processor, self.product_display(product), e);
                    println!("{}", color::red(&format!("Error: {}", msg)));
                    shared.failed_products.lock().insert(id);
                    shared.failed_messages.lock().push(msg);
                } else {
                    shared.failed_products.lock().insert(id);
                    shared.errors.lock().push(e);
                }
                Self::inc_progress(pb_ref, shared);
                RestoreOutcome::Failed
            }
            Ok(false) => RestoreOutcome::NotRestorable,
        }
    }

    /// Handle a product execution error.
    /// Emits a JSON event, records failure stats, and records keep-going / non-keep-going state.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_error(
        &self,
        product: &crate::graph::Product,
        id: usize,
        proc_name: &str,
        error: anyhow::Error,
        duration: Option<std::time::Duration>,
        keep_going: bool,
        shared: &SharedState,
    ) {
        emit_product_complete(
            &self.product_display(product),
            &product.processor,
            ProductStatus::Failed,
            duration,
            Some(&error.to_string()),
        );
        {
            let mut stats = shared.stats.lock();
            let proc_stats = stats
                .entry(proc_name.to_string())
                .or_default();
            proc_stats.failed += 1;
        }
        if keep_going {
            let msg = format!("[{}] {}: {}", proc_name, self.product_display(product), error);
            println!("{}", color::red(&format!("Error: {}", msg)));
            shared.failed_products.lock().insert(id);
            shared.failed_messages.lock().push(msg);
        } else {
            shared.failed_products.lock().insert(id);
            shared.failed_processors.lock().insert(proc_name.to_string());
            shared.errors.lock().push(error);
        }
    }

    /// Handle caching outputs and recording stats after successful execution.
    /// Returns `true` if caching succeeded, `false` if it failed (error is handled internally).
    /// On success, emits a product_complete "success" JSON event and increments processed/files_created.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_success(
        &self,
        product: &crate::graph::Product,
        id: usize,
        object_store: &crate::object_store::ObjectStore,
        cache_key: &str,
        input_checksum: &str,
        proc_name: &str,
        duration: Option<std::time::Duration>,
        keep_going: bool,
        shared: &SharedState,
        pb_ref: &ProgressBar,
    ) -> bool {
        match object_store.cache_outputs(cache_key, input_checksum, &product.outputs) {
            Ok(changed) => {
                if !changed {
                    shared.unchanged_products.lock().insert(id);
                }
            }
            Err(e) => {
                emit_product_complete(
                    &self.product_display(product),
                    &product.processor,
                    ProductStatus::Failed,
                    duration,
                    Some(&e.to_string()),
                );
                if keep_going {
                    let msg = format!("[{}] {}: {}", product.processor, self.product_display(product), e);
                    println!("{}", color::red(&format!("Error: {}", msg)));
                    shared.failed_products.lock().insert(id);
                    shared.failed_messages.lock().push(msg);
                } else {
                    shared.failed_products.lock().insert(id);
                    shared.errors.lock().push(e);
                }
                Self::inc_progress(pb_ref, shared);
                return false;
            }
        }
        emit_product_complete(
            &self.product_display(product),
            &product.processor,
            ProductStatus::Success,
            duration,
            None,
        );
        let mut stats = shared.stats.lock();
        let proc_stats = stats
            .entry(proc_name.to_string())
            .or_default();
        proc_stats.processed += 1;
        proc_stats.files_created += product.outputs.len();
        true
    }
}
