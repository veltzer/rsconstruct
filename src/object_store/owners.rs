use anyhow::{Context, Result};
use redb::ReadableDatabase;

use super::{LAST_TREE_TABLE, ObjectStore};

impl ObjectStore {
    /// Remember that the outputs `owner_key` (see `Product::owner_key`)
    /// currently has on disk are the ones described by `descriptor_key`.
    ///
    /// Descriptor keys mix in the input checksum, so once an input changes
    /// the key of the previous build can no longer be derived — yet its
    /// outputs are still on disk, read-only when they were hardlink-restored,
    /// and the tool about to rebuild cannot overwrite them. This pointer is
    /// what `remove_stale_outputs` follows to unlink them first.
    pub fn record_last_tree(&self, owner_key: &str, descriptor_key: &str) -> Result<()> {
        if self.last_tree_key(owner_key).as_deref() == Some(descriptor_key) {
            return Ok(());
        }
        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn
                .open_table(LAST_TREE_TABLE)
                .context("Failed to open last-tree table")?;
            table
                .insert(owner_key, descriptor_key)
                .context("Failed to record last tree")?;
        }
        write_txn.commit().context("Failed to commit last tree")?;
        Ok(())
    }

    /// The descriptor key recorded by the most recent `record_last_tree`
    /// for this owner, if any.
    pub fn last_tree_key(&self, owner_key: &str) -> Option<String> {
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(LAST_TREE_TABLE).ok()?;
        let value = table.get(owner_key).ok()??;
        Some(value.value().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_tree_pointer_round_trips_and_overwrites() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ObjectStore::new_in(tmp.path());

        assert_eq!(store.last_tree_key("owner"), None, "nothing recorded yet");

        store.record_last_tree("owner", "key-1").unwrap();
        assert_eq!(store.last_tree_key("owner").as_deref(), Some("key-1"));

        // Re-recording the same key is a no-op; a new key replaces it.
        store.record_last_tree("owner", "key-1").unwrap();
        store.record_last_tree("owner", "key-2").unwrap();
        assert_eq!(store.last_tree_key("owner").as_deref(), Some("key-2"));

        // Owners are independent.
        assert_eq!(store.last_tree_key("other"), None);
    }
}
