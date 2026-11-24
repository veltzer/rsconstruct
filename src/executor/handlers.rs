use crate::color;
use crate::json_output::{ProductStatus, emit_product_complete};
use crate::stats::ProcessStats;

use super::{Executor, HandlerContext, RestoreOutcome, SharedState};

impl Executor<'_> {
    /// Lock shared stats and apply an update function to the processor's stats entry.
    fn update_stats(shared: &SharedState, proc_name: &str, f: impl FnOnce(&mut ProcessStats)) {
        let mut stats = shared.stats.lock();
        f(stats.entry(proc_name.to_string()).or_default());
    }

    /// Everything a product's failure does, and the only place that does
    /// it: whatever went wrong (its inputs could not be read, a restore
    /// failed, its tool failed, its outputs could not be cached) the product
    /// is failed the same way.
    ///
    /// - A `product_complete` "failed" JSON event is emitted.
    /// - The processor's `failed` count goes up, so the per-processor table
    ///   adds up to the summary's total.
    /// - The product is marked failed, which is what blocks its dependents.
    /// - Its outputs are forgotten: the tool may have partly written them,
    ///   and no later read may serve a pre-write checksum.
    /// - The message, `[processor] product: error` in both modes (see
    ///   [`failure_message`](Self::failure_message)), is reported exactly
    ///   once: keeping going, it is printed now and kept for the summary;
    ///   failing fast, it is kept for the build's error, which reports every
    ///   failure of the run (see `collect_build_stats`).
    ///
    /// `stops_processor` is true when the processor's own run failed: failing
    /// fast, its remaining products are then not started.
    pub(super) fn fail_product(
        &self,
        shared: &SharedState,
        id: usize,
        product: &crate::graph::Product,
        error: &anyhow::Error,
        duration: Option<std::time::Duration>,
        stops_processor: bool,
    ) {
        emit_product_complete(
            &self.product_display(product),
            &product.processor,
            ProductStatus::Failed,
            duration,
            Some(&format!("{error:#}")),
        );
        Self::update_stats(shared, &product.processor, |s| s.failed += 1);
        shared.failed_products.lock().insert(id);
        crate::checksum::forget_in_session(self.build_ctx, &product.outputs, &product.output_dirs);

        let msg = self.failure_message(product, error);
        if self.run.keep_going {
            // stderr: errors must survive --json, and must not corrupt the
            // JSON event stream on stdout.
            crate::output::error(&msg);
            shared.failed_messages.lock().push(msg);
        } else {
            if stops_processor {
                shared
                    .failed_processors
                    .lock()
                    .insert(product.processor.clone());
            }
            shared.errors.lock().push(anyhow::anyhow!(msg));
        }
    }

    /// The one wording of a product failure: `[processor] product: error`,
    /// with the full anyhow chain (`{:#}`) so tera rendering details and IO
    /// causes are kept. Used by every path that records a failure, so a
    /// user can always tell which processor and product an error belongs to
    /// and the text is the same with and without `--keep-going`.
    pub(super) fn failure_message(
        &self,
        product: &crate::graph::Product,
        error: &anyhow::Error,
    ) -> String {
        format!(
            "[{}] {}: {error:#}",
            product.processor,
            self.product_display(product)
        )
    }

    /// Handle the "skip (unchanged)" case for a product.
    /// Logs, emits JSON event, increments stats. Does NOT advance the progress bar
    /// since skips are instant and the bar total excludes them.
    pub(super) fn handle_skip(&self, product: &crate::graph::Product, shared: &SharedState) {
        crate::output::detail(
            self.verbose,
            &format!(
                "[{}] {} {}",
                product.processor,
                color::dim("Skipping (unchanged):"),
                self.product_display(product)
            ),
        );
        emit_product_complete(
            &self.product_display(product),
            &product.processor,
            ProductStatus::Skipped,
            None,
            None,
        );
        Self::update_stats(shared, &product.processor, |s| s.skipped += 1);
        Self::inc_global(shared);
    }

    /// Handle cache restore for a product.
    /// Try to restore a product from cache.
    pub(super) fn handle_restore(
        &self,
        ctx: &HandlerContext,
        object_store: &crate::object_store::ObjectStore,
    ) -> RestoreOutcome {
        let desc_key = ctx.product.descriptor_key(ctx.input_checksum);
        let restore_result = object_store
            .restore_from_descriptor(self.build_ctx, &desc_key, &ctx.product.outputs)
            .and_then(|restored| {
                // A restored tree is read-only hardlinks (in hardlink mode):
                // record which descriptor they came from so the next rebuild
                // can unlink them before the tool writes.
                if restored && ctx.product.has_output_dirs() {
                    object_store.record_last_tree(&ctx.product.owner_key(), &desc_key)?;
                }
                Ok(restored)
            });
        match restore_result {
            Ok(true) => {
                // The restore rewrote the outputs on disk; evict any
                // pre-restore checksums from the in-session cache.
                crate::checksum::forget_in_session(
                    self.build_ctx,
                    &ctx.product.outputs,
                    &ctx.product.output_dirs,
                );
                ctx.shared.changed.lock().push(ctx.id);
                crate::output::detail(
                    self.verbose,
                    &format!(
                        "[{}] {} {}",
                        ctx.product.processor,
                        color::cyan("Restored from cache:"),
                        self.product_display(ctx.product)
                    ),
                );
                emit_product_complete(
                    &self.product_display(ctx.product),
                    &ctx.product.processor,
                    ProductStatus::Restored,
                    None,
                    None,
                );
                let output_count = ctx.product.outputs.len();
                Self::update_stats(ctx.shared, &ctx.product.processor, |s| {
                    s.restored += 1;
                    s.files_restored += output_count;
                });
                Self::inc_progress(ctx.pb, ctx.shared);
                RestoreOutcome::Restored
            }
            Err(e) => {
                self.fail_product(ctx.shared, ctx.id, ctx.product, &e, None, false);
                Self::inc_progress(ctx.pb, ctx.shared);
                RestoreOutcome::Failed
            }
            Ok(false) => RestoreOutcome::NotRestorable,
        }
    }

    /// Handle a product execution error: the processor's own run failed.
    pub(super) fn handle_error(
        &self,
        ctx: &HandlerContext,
        error: &anyhow::Error,
        duration: Option<std::time::Duration>,
    ) {
        self.fail_product(ctx.shared, ctx.id, ctx.product, error, duration, true);
    }

    /// Handle caching outputs and recording stats after successful execution.
    /// Returns `true` if caching succeeded, `false` if it failed (error is handled internally).
    /// On success, emits a `product_complete` "success" JSON event and increments `processed/files_created`.
    pub(super) fn handle_success(
        &self,
        ctx: &HandlerContext,
        object_store: &crate::object_store::ObjectStore,
        graph: &parking_lot::RwLock<crate::graph::BuildGraph>,
        duration: Option<std::time::Duration>,
    ) -> bool {
        // The product just (re)wrote its outputs: evict them from the
        // in-session checksum cache so every later read — the recompute
        // below when outputs double as inputs, and downstream products
        // consuming them — hashes the new bytes instead of returning the
        // pre-write value: the session's checksums are served without a
        // stat, so this eviction is their only invalidation.
        crate::checksum::forget_in_session(
            self.build_ctx,
            &ctx.product.outputs,
            &ctx.product.output_dirs,
        );
        ctx.shared.changed.lock().push(ctx.id);

        // The outputs are cached under the checksum taken at dispatch
        // (`ctx.input_checksum`): the inputs the tool was started on. Hash
        // them again now — if they differ, an input was edited while the
        // tool ran, and the outputs match neither version. Caching them
        // under the new content would make the next build skip with stale
        // outputs; caching under the old would serve them if that content
        // ever came back. So they are not cached at all, and the next build
        // runs the product again against whatever the input is then.
        let post_input_checksum = match crate::checksum::fresh_combined_input_checksum(
            self.build_ctx,
            &ctx.product.inputs,
        ) {
            Ok(cs) => cs,
            Err(e) => {
                let e = e.context("Failed to compute post-execution input checksum");
                self.fail_product(ctx.shared, ctx.id, ctx.product, &e, duration, false);
                return false;
            }
        };
        if post_input_checksum != ctx.input_checksum {
            crate::output::warn(&format!(
                "[{}] {}: inputs changed while it ran; outputs not cached, it will run again next build",
                ctx.product.processor,
                self.product_display(ctx.product),
            ));
            self.report_success(ctx, duration);
            return true;
        }
        let desc_key = ctx.product.descriptor_key(ctx.input_checksum);
        let cache_result = if ctx.product.outputs.is_empty() && !ctx.product.has_output_dirs() {
            // Checker: no outputs, just mark as passed
            object_store
                .store_marker(self.build_ctx, &desc_key)
                .map(|()| false)
        } else if ctx.product.outputs.len() == 1 && !ctx.product.has_output_dirs() {
            // Generator: single output file → blob
            object_store.store_blob_descriptor(self.build_ctx, &desc_key, &ctx.product.outputs[0])
        } else {
            // Explicit with output_dirs, or multi-output: always tree.
            // When walking output_dirs, skip paths declared as outputs of OTHER products —
            // they're owned by some other processor that contributes to the shared directory.
            // The graph is read per path, briefly: the coordinator may need
            // the write lock to re-analyze while a large tree is walked.
            let is_foreign = |path: &std::path::Path| -> bool {
                matches!(graph.read().path_owner(path), Some(owner) if owner != ctx.id)
            };
            object_store
                .store_tree_descriptor(
                    self.build_ctx,
                    &desc_key,
                    &ctx.product.output_dirs,
                    &ctx.product.outputs,
                    &is_foreign,
                )
                .and_then(|changed| {
                    // The tree now on disk is this one: the next rebuild
                    // unlinks it by this pointer (see remove_stale_outputs).
                    if ctx.product.has_output_dirs() {
                        object_store.record_last_tree(&ctx.product.owner_key(), &desc_key)?;
                    }
                    Ok(changed)
                })
        };
        match cache_result {
            Ok(_changed) => {}
            Err(e) => {
                self.fail_product(ctx.shared, ctx.id, ctx.product, &e, duration, false);
                return false;
            }
        }
        self.report_success(ctx, duration);
        true
    }

    /// Emit the success event and count the product as processed.
    fn report_success(&self, ctx: &HandlerContext, duration: Option<std::time::Duration>) {
        emit_product_complete(
            &self.product_display(ctx.product),
            &ctx.product.processor,
            ProductStatus::Success,
            duration,
            None,
        );
        let output_count = ctx.product.outputs.len();
        Self::update_stats(ctx.shared, ctx.proc_name, |s| {
            s.processed += 1;
            s.files_created += output_count;
        });
    }
}
