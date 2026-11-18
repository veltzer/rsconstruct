//! Shared redb database utilities.

use anyhow::Result;
use redb::Database;
use std::path::Path;

/// Open a redb database, creating it when it does not exist. Nothing is
/// ever deleted or recreated here.
///
/// The error says which of three things happened, decided from redb's
/// error type rather than from the text of its message:
/// - another rsconstruct process holds the lock;
/// - the file could not be read or written (permissions, a full disk);
/// - anything else: the file itself is bad, and `rsconstruct cache clear`
///   is the remedy. An IO failure used to be reported this way too, sending
///   the user to delete a healthy cache.
pub fn open(db_path: &Path, label: &str) -> Result<Database> {
    Database::create(db_path).map_err(|e| match e {
        redb::DatabaseError::DatabaseAlreadyOpen => anyhow::anyhow!(
            "Another rsconstruct process is using {}: {}\n\
             Wait for it to finish, or check for stale processes.",
            db_path.display(),
            e
        ),
        redb::DatabaseError::Storage(redb::StorageError::Io(io)) => {
            anyhow::anyhow!("Failed to open {} at {}: {}", label, db_path.display(), io)
        }
        other => anyhow::anyhow!(
            "{} is corrupted: {}\n\
             Run `rsconstruct cache clear` to delete it and rebuild.\n\
             Cause: {}",
            label,
            db_path.display(),
            other
        ),
    })
}
