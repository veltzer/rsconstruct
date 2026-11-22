//! Shared redb database utilities.

use anyhow::Result;
use redb::Database;
use std::path::Path;

/// Size of one database file before and after [`compact_all`].
pub struct Compacted {
    pub path: std::path::PathBuf,
    pub before: u64,
    pub after: u64,
}

/// Compact every redb database in `state_dir` (`.rsconstruct/`): the cache
/// index, the deps, mtime and web caches, and any added later, found by
/// their `.redb` extension rather than by a list that could fall behind.
///
/// redb reuses freed pages but never returns them to the file system, so a
/// database keeps the size of its largest moment: after `cache trim`,
/// `cache remove-stale` or a project shrinking, the space stays taken until
/// the file is compacted. A missing directory has nothing to compact.
pub fn compact_all(state_dir: &Path) -> Result<Vec<Compacted>> {
    use anyhow::Context;
    if !state_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(state_dir)
        .with_context(|| format!("Failed to list {}", state_dir.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<_>>()
        .with_context(|| format!("Failed to list {}", state_dir.display()))?;
    paths.retain(|p| p.extension().is_some_and(|ext| ext == "redb") && p.is_file());
    paths.sort();
    let size_of = |path: &Path| -> Result<u64> {
        Ok(std::fs::metadata(path)
            .with_context(|| format!("Failed to stat {}", path.display()))?
            .len())
    };
    let mut results = Vec::with_capacity(paths.len());
    for path in paths {
        let before = size_of(&path)?;
        let mut db = open(&path, &path.display().to_string())?;
        db.compact()
            .with_context(|| format!("Failed to compact {}", path.display()))?;
        drop(db);
        let after = size_of(&path)?;
        results.push(Compacted {
            path,
            before,
            after,
        });
    }
    Ok(results)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use redb::TableDefinition;

    const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("t");

    /// Deleting rows leaves the file at its peak size; compacting gives the
    /// space back, keeps what is still there, and skips other files.
    #[test]
    fn compact_all_shrinks_a_database_that_lost_its_rows() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("cache.redb");
        let blob = vec![7u8; 4096];
        {
            let db = open(&path, "test").unwrap();
            let txn = db.begin_write().unwrap();
            {
                let mut table = txn.open_table(TABLE).unwrap();
                for i in 0..2000 {
                    table
                        .insert(format!("k{i}").as_str(), blob.as_slice())
                        .unwrap();
                }
            }
            txn.commit().unwrap();
            let txn = db.begin_write().unwrap();
            {
                let mut table = txn.open_table(TABLE).unwrap();
                for i in 1..2000 {
                    table.remove(format!("k{i}").as_str()).unwrap();
                }
            }
            txn.commit().unwrap();
        }
        std::fs::write(tmp.path().join("notes.txt"), "not a database").unwrap();

        let compacted = compact_all(tmp.path()).unwrap();

        assert_eq!(compacted.len(), 1, "only .redb files are compacted");
        let c = &compacted[0];
        assert_eq!(c.path, path);
        assert!(
            c.after * 4 < c.before,
            "compaction frees the deleted rows' space: {} -> {}",
            c.before,
            c.after
        );
        let db = open(&path, "test").unwrap();
        let txn = redb::ReadableDatabase::begin_read(&db).unwrap();
        let table = txn.open_table(TABLE).unwrap();
        assert_eq!(table.get("k0").unwrap().unwrap().value(), blob.as_slice());
        assert!(table.get("k1").unwrap().is_none());
    }

    #[test]
    fn compact_all_of_a_missing_directory_does_nothing() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(compact_all(&tmp.path().join("absent")).unwrap().is_empty());
    }
}
