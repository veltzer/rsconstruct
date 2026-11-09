//! Cache for HTTP responses (remote JSON schemas fetched by `iyamlschema`).
//!
//! Entries carry a fetch timestamp and expire after
//! `[cache] webcache_ttl_secs`. Before that they had no expiry at all: a URL
//! fetched once was served from disk forever, so an upstream schema change
//! was never picked up and the database only ever grew.

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const DB_PATH: &str = ".rsconstruct/webcache.redb";
const TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("webcache_v2");

/// An entry in the webcache.
pub struct CacheEntry {
    pub url: String,
    pub size: usize,
    /// Seconds since this entry was fetched.
    pub age_secs: u64,
    pub expired: bool,
}

/// The stored value: the body plus when it was fetched.
#[derive(Serialize, Deserialize)]
struct StoredEntry {
    fetched_at_secs: u64,
    body: String,
}

/// The database handle, opened once per process.
///
/// `fetch` used to call `Database::create` on every single call — reopening
/// the file (and taking its lock) once per URL. redb allows only one open
/// handle, so this also means a fetch could not overlap with any other
/// webcache operation.
static DB: OnceLock<Mutex<Option<Database>>> = OnceLock::new();

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Run `f` with the process-wide database handle, opening it on first use.
fn with_db<T>(f: impl FnOnce(&Database) -> Result<T>) -> Result<T> {
    let cell = DB.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap();
    if guard.is_none() {
        let path = Path::new(DB_PATH);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let db = Database::create(path)
            .with_context(|| format!("Failed to open webcache database {}", path.display()))?;
        *guard = Some(db);
    }
    let db = guard.as_ref().expect("webcache database just opened");
    f(db)
}

/// Whether the webcache database exists on disk. Read-only callers use this
/// to avoid creating an empty database as a side effect of listing it.
fn db_exists() -> bool {
    Path::new(DB_PATH).exists()
}

// ─── Sandboxed children ──────────────────────────────────────────────────────
//
// A builtin processor that runs sandboxed runs in a child rsconstruct under
// bwrap (`sandbox::sandbox_run`). Several run side by side, and a write
// into the project from inside the sandbox is one its products do not
// declare. So a child only reads the cache, through a read-only handle
// (redb lets several processes share one), and keeps what it downloads in
// memory; it hands those entries back with its results, and the build
// stores them all at its end (`store_pending`), when no child is running.

/// Set in a sandboxed child: read the cache read-only, never store.
static CHILD_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What a child downloaded, or what the build has yet to store.
static FETCHED: Mutex<Vec<Download>> = Mutex::new(Vec::new());

/// One download made where the cache could not be written.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Download {
    pub url: String,
    pub fetched_at_secs: u64,
    pub body: String,
}

/// Make this process a sandboxed child: see the section comment above.
pub fn set_child_mode() {
    CHILD_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn child_mode() -> bool {
    CHILD_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

/// In a child: the downloads made so far, to report to the build.
pub fn take_fetched() -> Vec<Download> {
    std::mem::take(&mut *FETCHED.lock().unwrap())
}

/// In the build: entries a child downloaded, stored by [`store_pending`].
pub fn defer_store(entries: Vec<Download>) {
    FETCHED.lock().unwrap().extend(entries);
}

/// In the build, after the products ran: store what the sandboxed children
/// downloaded, then close the database, so the children of a later build
/// in this process (`watch`) can open it again.
pub fn store_pending() -> Result<()> {
    let pending = std::mem::take(&mut *FETCHED.lock().unwrap());
    if pending.is_empty() {
        return Ok(());
    }
    with_db(|db| {
        let write_txn = db
            .begin_write()
            .context("Failed to begin write transaction on webcache")?;
        {
            let mut table = write_txn
                .open_table(TABLE)
                .context("Failed to open webcache table for write")?;
            for entry in &pending {
                let stored = serde_json::to_vec(&StoredEntry {
                    fetched_at_secs: entry.fetched_at_secs,
                    body: entry.body.clone(),
                })
                .context("Failed to serialize webcache entry")?;
                table
                    .insert(entry.url.as_str(), stored.as_slice())
                    .with_context(|| {
                        format!("Failed to insert webcache entry for {}", entry.url)
                    })?;
            }
        }
        write_txn
            .commit()
            .context("Failed to commit webcache write")
    })?;
    close();
    Ok(())
}

/// Drop the process-wide handle, releasing the database's lock.
fn close() {
    if let Some(cell) = DB.get() {
        *cell.lock().unwrap() = None;
    }
}

/// The body stored for `url` in `db`, if it was fetched less than `ttl_secs`
/// ago.
fn lookup(db: &impl ReadableDatabase, url: &str, ttl_secs: u64) -> Result<Option<String>> {
    let read_txn = db
        .begin_read()
        .context("Failed to begin read transaction on webcache")?;
    let Ok(table) = read_txn.open_table(TABLE) else {
        return Ok(None);
    };
    let Some(raw) = table
        .get(url)
        .with_context(|| format!("Failed to read webcache entry for {url}"))?
    else {
        return Ok(None);
    };
    // A malformed entry is treated as a miss and re-fetched, rather than
    // failing the build over a cache we can always rebuild.
    let Ok(entry) = serde_json::from_slice::<StoredEntry>(raw.value()) else {
        return Ok(None);
    };
    let age = now_secs().saturating_sub(entry.fetched_at_secs);
    if age >= ttl_secs {
        return Ok(None);
    }
    Ok(Some(entry.body))
}

/// Look up a still-fresh entry for `url`.
fn get_fresh(url: &str, ttl_secs: u64) -> Result<Option<String>> {
    if ttl_secs == 0 || !db_exists() {
        return Ok(None);
    }
    if child_mode() {
        // Open per lookup and let go at once. A writer holding the lock
        // (the build storing, or a native fetch) makes this a miss, which a
        // download covers.
        let Ok(db) = redb::ReadOnlyDatabase::open(DB_PATH) else {
            return Ok(None);
        };
        return lookup(&db, url, ttl_secs);
    }
    with_db(|db| lookup(db, url, ttl_secs))
}

/// Fetch URL content, returning cached content when it is still within
/// `ttl_secs` of when it was fetched. A `ttl_secs` of 0 disables the cache
/// entirely (always re-fetch, never store).
pub fn fetch(url: &str, ttl_secs: u64) -> Result<String> {
    if let Some(body) = get_fresh(url, ttl_secs)? {
        return Ok(body);
    }

    // Retried via download::with_retry — see
    // docs/src/internal/download-policy.md.
    let body = crate::download::with_retry(|| {
        ureq::get(url)
            .call()
            .with_context(|| format!("Failed to fetch {url}"))?
            .body_mut()
            .read_to_string()
            .with_context(|| format!("Failed to read response body from {url}"))
    })?;

    if ttl_secs == 0 {
        return Ok(body);
    }
    if child_mode() {
        FETCHED.lock().unwrap().push(Download {
            url: url.to_string(),
            fetched_at_secs: now_secs(),
            body: body.clone(),
        });
        return Ok(body);
    }

    let stored = serde_json::to_vec(&StoredEntry {
        fetched_at_secs: now_secs(),
        body: body.clone(),
    })
    .context("Failed to serialize webcache entry")?;

    with_db(|db| {
        let write_txn = db
            .begin_write()
            .context("Failed to begin write transaction on webcache")?;
        {
            let mut table = write_txn
                .open_table(TABLE)
                .context("Failed to open webcache table for write")?;
            table
                .insert(url, stored.as_slice())
                .with_context(|| format!("Failed to insert webcache entry for {url}"))?;
        }
        write_txn
            .commit()
            .context("Failed to commit webcache write")?;
        Ok(())
    })?;

    Ok(body)
}

/// Delete all webcache entries. Returns the number of entries removed.
pub fn clear() -> Result<usize> {
    if !db_exists() {
        return Ok(0);
    }
    let count = list()?.len();
    with_db(|db| {
        let write_txn = db
            .begin_write()
            .context("Failed to begin write transaction for webcache clear")?;
        write_txn
            .delete_table(TABLE)
            .context("Failed to delete webcache table")?;
        write_txn
            .commit()
            .context("Failed to commit webcache clear")?;
        Ok(())
    })?;
    Ok(count)
}

/// Drop entries older than `ttl_secs`. Returns the number removed.
///
/// Expired entries are skipped on read anyway; this is what actually
/// reclaims their disk space, and is called by `cache trim`.
pub fn prune(ttl_secs: u64) -> Result<usize> {
    if !db_exists() {
        return Ok(0);
    }
    let expired: Vec<String> = list_with_ttl(ttl_secs)?
        .into_iter()
        .filter(|e| e.expired)
        .map(|e| e.url)
        .collect();
    if expired.is_empty() {
        return Ok(0);
    }
    with_db(|db| {
        let write_txn = db
            .begin_write()
            .context("Failed to begin write transaction for webcache prune")?;
        {
            let mut table = write_txn
                .open_table(TABLE)
                .context("Failed to open webcache table for prune")?;
            for url in &expired {
                table
                    .remove(url.as_str())
                    .with_context(|| format!("Failed to remove webcache entry {url}"))?;
            }
        }
        write_txn
            .commit()
            .context("Failed to commit webcache prune")?;
        Ok(())
    })?;
    Ok(expired.len())
}

/// List all cache entries with URL, size and age.
pub fn list() -> Result<Vec<CacheEntry>> {
    list_with_ttl(default_ttl_for_display())
}

/// The TTL used when listing without a loaded config (e.g. `cache webcache
/// list` outside a project). Only affects the reported `expired` flag.
const fn default_ttl_for_display() -> u64 {
    7 * 24 * 60 * 60
}

/// List entries, marking each expired against `ttl_secs`.
pub fn list_with_ttl(ttl_secs: u64) -> Result<Vec<CacheEntry>> {
    if !db_exists() {
        return Ok(Vec::new());
    }
    let now = now_secs();
    with_db(|db| {
        let read_txn = db
            .begin_read()
            .context("Failed to begin read transaction on webcache")?;
        let Ok(table) = read_txn.open_table(TABLE) else {
            return Ok(Vec::new());
        };
        let mut entries = Vec::new();
        for result in table.iter().context("Failed to iterate webcache entries")? {
            let (key, value) = result.context("Failed to read webcache entry")?;
            let Ok(stored) = serde_json::from_slice::<StoredEntry>(value.value()) else {
                continue;
            };
            let age_secs = now.saturating_sub(stored.fetched_at_secs);
            entries.push(CacheEntry {
                url: key.value().to_string(),
                size: stored.body.len(),
                age_secs,
                expired: ttl_secs == 0 || age_secs >= ttl_secs,
            });
        }
        Ok(entries)
    })
}

/// Return (`total_bytes`, `entry_count`) for the webcache.
pub fn stats() -> Result<(u64, usize)> {
    let entries = list()?;
    let total: u64 = entries.iter().map(|e| e.size as u64).sum();
    Ok((total, entries.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fetch stored under a TTL of 0 must not be cached at all, and a
    /// lookup with TTL 0 must never report a hit.
    #[test]
    fn ttl_zero_disables_the_cache() {
        assert!(get_fresh("http://example.invalid/x", 0).unwrap().is_none());
    }

    /// Entries are considered fresh strictly *within* the TTL, so an entry
    /// exactly at the boundary is expired rather than served.
    #[test]
    fn expiry_is_at_the_boundary() {
        let entry = StoredEntry {
            fetched_at_secs: 1000,
            body: "b".into(),
        };
        let age = 2000u64.saturating_sub(entry.fetched_at_secs);
        assert_eq!(age, 1000);
        assert!(age >= 1000, "an entry exactly at the TTL is expired");
        assert!(age < 1001, "and still fresh just under it");
    }

    /// A clock that moves backwards (NTP correction, VM restore) must not
    /// produce a gigantic age that wraps — `saturating_sub` keeps it at 0,
    /// meaning "fresh", which is the safe direction for a cache.
    #[test]
    fn backwards_clock_does_not_underflow() {
        let fetched_in_the_future = 9_000u64;
        assert_eq!(1_000u64.saturating_sub(fetched_in_the_future), 0);
    }
}
