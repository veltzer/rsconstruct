use crate::graph::Product;
use crate::object_store::{ExplainAction, ObjectStore};

/// What action should be taken for a product.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProductAction {
    /// Outputs are up-to-date — skip execution entirely.
    Skip,
    /// Outputs can be restored from the cache without re-executing.
    Restore,
    /// Product must be executed (new, changed, or forced).
    Build,
}

/// Decides whether each product should be skipped, restored, or built.
///
/// The default implementation ([`IncrementalPolicy`]) encodes the current
/// behavior: skip if outputs match, restore if cache has blobs, else build.
/// Future implementations could add time-based expiry, always-rebuild,
/// demand-driven filtering, or deterministic-verification modes.
pub(crate) trait BuildPolicy: Sync + Send {
    /// Classify a single product given the current cache state.
    ///
    /// `dep_changed` is true if any dependency of this product will be rebuilt
    /// or restored in this run (i.e. its outputs cannot be trusted even if the
    /// descriptor matches).
    fn classify(
        &self,
        product: &Product,
        object_store: &ObjectStore,
        input_checksum: &str,
        dep_changed: bool,
        force: bool,
    ) -> ProductAction;

    /// Return a human-readable explanation of what action would be taken and why.
    /// Used by `--explain`.
    fn explain(
        &self,
        product: &Product,
        object_store: &ObjectStore,
        input_checksum: &str,
        force: bool,
    ) -> ExplainAction;
}

/// The standard incremental build policy: skip unchanged, restore from cache
/// when possible, otherwise build.
pub(crate) struct IncrementalPolicy;

impl BuildPolicy for IncrementalPolicy {
    fn classify(
        &self,
        product: &Product,
        object_store: &ObjectStore,
        input_checksum: &str,
        dep_changed: bool,
        force: bool,
    ) -> ProductAction {
        let desc_key = product.descriptor_key(input_checksum);
        let needs_rebuild = object_store.needs_rebuild_descriptor(&desc_key, &product.outputs);
        let can_restore = object_store.can_restore_descriptor(&desc_key);

        if !force && !dep_changed && !needs_rebuild {
            ProductAction::Skip
        } else if !force && !dep_changed && can_restore {
            ProductAction::Restore
        } else {
            ProductAction::Build
        }
    }

    fn explain(
        &self,
        product: &Product,
        object_store: &ObjectStore,
        input_checksum: &str,
        force: bool,
    ) -> ExplainAction {
        let desc_key = product.descriptor_key(input_checksum);
        object_store.explain_descriptor(&desc_key, &product.outputs, force)
    }
}
