use crate::color;
use crate::json_output::{emit_product_complete, ProductStatus};

use super::{Executor, HandlerContext, RestoreOutcome, SharedState};

impl<'a> Executor<'a> {
    /// Record a product failure into shared state.
    ///
    /// In keep-going mode: prints the error, records the message for the summary.
    /// In fail-fast mode: stores the error for later propagation.
    /// If `mark_processor_failed` is true, also records the processor name
    /// so its products are skipped in subsequent levels.
    fn record_failure(
        &self,
        ctx: &HandlerContext,
        error: anyhow::Error,
        mark_processor_failed: bool,
    ) {
        if ctx.keep_going {
            let msg = format!("[{}] {}: {}", ctx.proc_name, self.product_display(ctx.product), error);
            println!("{}", color::red(&format!("Error: {}", msg)));
            ctx.shared.failed_products.lock().insert(ctx.id);
            ctx.shared.failed_messages.lock().push(msg);
        } else {
            ctx.shared.failed_products.lock().insert(ctx.id);
            if mark_processor_failed {
                ctx.shared.failed_processors.lock().insert(ctx.proc_name.to_string());
            }
            ctx.shared.errors.lock().push(error);
        }
    }

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
    pub(super) fn handle_restore(
        &self,
        ctx: &HandlerContext,
        object_store: &crate::object_store::ObjectStore,
        force: bool,
        emit_fail_event: bool,
    ) -> RestoreOutcome {
        if force {
            return RestoreOutcome::NotRestorable;
        }
        let restore_result = object_store.restore_from_cache(&ctx.cache_key, ctx.input_checksum, &ctx.product.outputs);
        match restore_result {
            Ok(true) => {
                if self.verbose {
                    println!("[{}] {} {}", ctx.product.processor,
                        color::cyan("Restored from cache:"),
                        self.product_display(ctx.product));
                }
                emit_product_complete(
                    &self.product_display(ctx.product),
                    &ctx.product.processor,
                    ProductStatus::Restored,
                    None,
                    None,
                );
                let mut stats = ctx.shared.stats.lock();
                let proc_stats = stats
                    .entry(ctx.product.processor.clone())
                    .or_default();
                proc_stats.restored += 1;
                proc_stats.files_restored += ctx.product.outputs.len();
                Self::inc_progress(ctx.pb, ctx.shared);
                RestoreOutcome::Restored
            }
            Err(e) => {
                if emit_fail_event {
                    emit_product_complete(
                        &self.product_display(ctx.product),
                        &ctx.product.processor,
                        ProductStatus::Failed,
                        None,
                        Some(&e.to_string()),
                    );
                }
                self.record_failure(ctx, e, false);
                Self::inc_progress(ctx.pb, ctx.shared);
                RestoreOutcome::Failed
            }
            Ok(false) => RestoreOutcome::NotRestorable,
        }
    }

    /// Handle a product execution error.
    /// Emits a JSON event, records failure stats, and records keep-going / non-keep-going state.
    pub(super) fn handle_error(
        &self,
        ctx: &HandlerContext,
        error: anyhow::Error,
        duration: Option<std::time::Duration>,
    ) {
        emit_product_complete(
            &self.product_display(ctx.product),
            &ctx.product.processor,
            ProductStatus::Failed,
            duration,
            Some(&error.to_string()),
        );
        {
            let mut stats = ctx.shared.stats.lock();
            let proc_stats = stats
                .entry(ctx.proc_name.to_string())
                .or_default();
            proc_stats.failed += 1;
        }
        self.record_failure(ctx, error, true);
    }

    /// Handle caching outputs and recording stats after successful execution.
    /// Returns `true` if caching succeeded, `false` if it failed (error is handled internally).
    /// On success, emits a product_complete "success" JSON event and increments processed/files_created.
    pub(super) fn handle_success(
        &self,
        ctx: &HandlerContext,
        object_store: &crate::object_store::ObjectStore,
        duration: Option<std::time::Duration>,
    ) -> bool {
        match object_store.cache_outputs(&ctx.cache_key, ctx.input_checksum, &ctx.product.outputs) {
            Ok(changed) => {
                if !changed {
                    ctx.shared.unchanged_products.lock().insert(ctx.id);
                }
            }
            Err(e) => {
                emit_product_complete(
                    &self.product_display(ctx.product),
                    &ctx.product.processor,
                    ProductStatus::Failed,
                    duration,
                    Some(&e.to_string()),
                );
                self.record_failure(ctx, e, false);
                Self::inc_progress(ctx.pb, ctx.shared);
                return false;
            }
        }
        emit_product_complete(
            &self.product_display(ctx.product),
            &ctx.product.processor,
            ProductStatus::Success,
            duration,
            None,
        );
        let mut stats = ctx.shared.stats.lock();
        let proc_stats = stats
            .entry(ctx.proc_name.to_string())
            .or_default();
        proc_stats.processed += 1;
        proc_stats.files_created += ctx.product.outputs.len();
        true
    }
}
