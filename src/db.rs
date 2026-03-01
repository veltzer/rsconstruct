//! Shared redb database utilities.

use anyhow::{Context, Result};
use redb::Database;
use std::path::Path;

/// Open or create a redb database.
///
/// If the database file is corrupted, return an error telling the user
/// to run `rsb cache clear` to recreate it.
pub fn open_or_recreate(db_path: &Path, label: &str) -> Result<Database> {
    Database::create(db_path).with_context(|| {
        format!(
            "{} is corrupted: {}\nRun `rsb cache clear` to delete it and rebuild.",
            label,
            db_path.display()
        )
    })
}
