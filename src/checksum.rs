use anyhow::{Context, Result};
use redb::ReadableDatabase;
use redb::ReadableTable;
use redb::TableDefinition;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Buffer size for streaming file reads in `file_checksum`. 64 KB is a
/// good trade-off: large enough to amortize syscall overhead, small
/// enough to fit in L1 cache and bound peak memory regardless of file
/// size.
const HASH_BUF_SIZE: usize = 64 * 1024;

use crate::build_context::BuildContext;

const MTIME_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("mtime_cache");

/// Cached stamp-to-checksum mapping for a single file. The checksum is
/// trusted while the file's whole stamp — mtime, ctime, size and inode
/// (see `platform::FileStamp`) — is unchanged. An entry from before the
/// stamp existed fails to deserialize and is re-hashed once.
#[derive(Clone, Serialize, Deserialize)]
struct MtimeEntry {
    stamp: crate::platform::FileStamp,
    checksum: String,
}

/// The session's view of the mtime cache, by path key.
///
/// `stored` is the database table, read into memory on the first lookup.
/// Looking each file up in redb cost a read transaction, a B-tree walk and
/// a lock on the one shared database handle per file — 75k of them for a
/// build that changes nothing — and the lock serialized every thread that
/// checksums. One read of the table up front makes each lookup a hash-map
/// probe under a read lock, which any number of threads take at once.
///
/// `pending` holds entries computed this session and not yet written.
/// Writing each entry as it was computed cost one durable redb commit (an
/// fdatasync) per checksummed file. Entries are staged here instead and
/// written by `flush_mtime_cache` at a few points in a run: after classify,
/// after execute, and when the command ends; the flush moves them into
/// `stored`. Lookups see a staged entry at once. An entry lost to a crash
/// only costs a re-hash on the next run.
///
/// Another process writing the database meanwhile cannot make `stored`
/// wrong, only stale: every entry is checked against the file's stamp, so a
/// stale one is a miss and a re-hash.
#[derive(Default)]
pub struct MtimeState {
    stored: Option<HashMap<String, MtimeEntry>>,
    pending: HashMap<String, MtimeEntry>,
}

impl MtimeState {
    fn get(&self, path_str: &str) -> Option<&MtimeEntry> {
        self.pending
            .get(path_str)
            .or_else(|| self.stored.as_ref()?.get(path_str))
    }
}

/// Read the whole mtime table into memory. A table that does not exist yet
/// is a fresh cache: empty. An entry that does not parse (written by
/// another rsconstruct version) is left out, so it is a miss: the file is
/// re-hashed and the entry rewritten. Any other storage error is reported
/// rather than turning the mtime cache off without a word.
fn load_mtime_table(ctx: &BuildContext) -> Result<HashMap<String, MtimeEntry>> {
    let db_guard = get_mtime_db(ctx)?;
    let db = crate::errors::ctx_opt(db_guard.as_ref(), "Mtime database not available")?;
    let read_txn = crate::errors::ctx(
        db.begin_read(),
        "Failed to begin read transaction for mtime cache",
    )?;
    let table = match read_txn.open_table(MTIME_TABLE) {
        Ok(table) => table,
        Err(redb::TableError::TableDoesNotExist(_)) => return Ok(HashMap::new()),
        Err(e) => {
            return Err(anyhow::Error::from(e).context("Failed to open the mtime cache table"));
        }
    };
    let mut stored = HashMap::new();
    for row in table.iter().context("Failed to iterate the mtime cache")? {
        let (key, value) = row.context("Failed to read an mtime cache entry")?;
        if let Ok(entry) = serde_json::from_slice::<MtimeEntry>(value.value()) {
            stored.insert(key.value().to_string(), entry);
        }
    }
    Ok(stored)
}

/// The cached entry for `path_str`, staged or stored, loading the table on
/// first use.
fn lookup_mtime_entry(ctx: &BuildContext, path_str: &str) -> Result<Option<MtimeEntry>> {
    {
        let state = ctx.mtime_state.read().unwrap();
        if state.stored.is_some() {
            return Ok(state.get(path_str).cloned());
        }
    }
    let mut state = ctx.mtime_state.write().unwrap();
    // Another thread may have loaded it while this one waited.
    if state.stored.is_none() {
        state.stored = Some(load_mtime_table(ctx)?);
    }
    Ok(state.get(path_str).cloned())
}

/// Seconds within which a file counts as just changed: no entry is stored
/// for it (see `fast_checksum`).
const RECENT_CHANGE_SECS: i64 = 2;

/// Open or get the mtime database from the `BuildContext`.
fn get_mtime_db(ctx: &BuildContext) -> Result<std::sync::MutexGuard<'_, Option<redb::Database>>> {
    let mut guard = ctx.mtime_db.lock().unwrap();
    if guard.is_none() {
        let dir = PathBuf::from(".rsconstruct");
        crate::errors::ctx(
            fs::create_dir_all(&dir),
            "Failed to create .rsconstruct directory",
        )?;
        let db = crate::db::open(&dir.join("mtime.redb"), "Mtime cache")?;
        *guard = Some(db);
    }
    Ok(guard)
}

/// Calculate SHA-256 checksum of a file's contents, using the `BuildContext`'s
/// in-memory cache. First call for a given path streams the file through
/// the hasher and caches the result. Subsequent calls return the cached
/// value.
///
/// Streaming (vs `fs::read` + hash) keeps peak memory bounded at
/// `HASH_BUF_SIZE` regardless of file size — a 100 MB binary doesn't
/// allocate a 100 MB Vec.
pub fn file_checksum(ctx: &BuildContext, path: &Path) -> Result<String> {
    if let Some(cached) = session_checksum(ctx, path) {
        return Ok(cached);
    }
    // Hashed outside the lock, so other threads keep reading meanwhile.
    let checksum = stream_file_checksum(path)?;
    remember_in_session(ctx, path, &checksum);
    Ok(checksum)
}

/// The checksum this session already took of `path`, if any.
///
/// Within one build run an input's checksum is taken once and then served
/// from here, without a `stat`: content is assumed stable for the run except
/// where the build itself writes, and every such write goes through
/// [`forget_in_session`]. A build of 18k products referencing 75k inputs
/// (51k distinct) used to stat each reference at classify and again in the
/// executor, about 224k `statx` calls for a build that changed nothing.
///
/// The one reader that must not trust it is the check, after a product ran,
/// that its inputs were not edited while the tool ran; it calls
/// [`fresh_combined_input_checksum`].
fn session_checksum(ctx: &BuildContext, path: &Path) -> Option<String> {
    ctx.checksum_cache.read().unwrap().get(path).cloned()
}

fn remember_in_session(ctx: &BuildContext, path: &Path, checksum: &str) {
    ctx.checksum_cache
        .write()
        .unwrap()
        .insert(path.to_path_buf(), checksum.to_string());
}

/// Evict the files a product writes from the in-session caches: its
/// declared `paths`, and every file under its declared output `dirs`, whose
/// contents are not listed one by one. The executor calls this whenever it
/// writes or restores a product's outputs, the writes that break the
/// in-session assumption that content is stable for the run. Without it, the
/// rewritten file's old checksum would be served to every later reader,
/// starting with the executor's check of the downstream products that
/// consume it.
pub fn forget_in_session(ctx: &BuildContext, paths: &[PathBuf], dirs: &[std::sync::Arc<PathBuf>]) {
    let under_a_dir = |path: &Path| dirs.iter().any(|d| path.starts_with(d.as_path()));
    {
        let mut cache = ctx.checksum_cache.write().unwrap();
        for path in paths {
            cache.remove(path);
        }
        if !dirs.is_empty() {
            cache.retain(|path, _| !under_a_dir(path));
        }
    }
    let mut memo = ctx.combined_memo.lock().unwrap();
    if paths.iter().any(|p| memo.covered.contains(p))
        || (!dirs.is_empty() && memo.covered.iter().any(|p| under_a_dir(p)))
    {
        *memo = CombinedMemo::default();
    }
}

/// Input lists at least this long have their combined checksum memoized for
/// the session. Below it, recomputing is as cheap as the memo bookkeeping.
const MEMO_MIN_INPUTS: usize = 32;

/// In-session memo of combined checksums for large input lists.
///
/// Many products can share one long input list: every page a zola site
/// renders depends on the whole content tree, ~900 files for 1,400 products.
/// Without the memo each product re-checks every file (a `stat` and an mtime
/// lookup apiece), twice per build — over a million of each for a build
/// that changes nothing. The memo is valid while none of the files it covers
/// is rewritten; `forget_in_session` (called on every write and restore)
/// drops it the moment one is, and `BuildContext::clear_session_caches`
/// drops it wholesale between watch-mode rebuilds, where the files that
/// changed are the user's sources rather than anything the build wrote.
#[derive(Default)]
pub struct CombinedMemo {
    by_inputs: HashMap<Vec<PathBuf>, String>,
    covered: std::collections::HashSet<PathBuf>,
}

impl CombinedMemo {
    fn get(&self, inputs: &[PathBuf]) -> Option<String> {
        self.by_inputs.get(inputs).cloned()
    }

    fn insert(&mut self, inputs: &[PathBuf], combined: String) {
        self.covered.extend(inputs.iter().cloned());
        self.by_inputs.insert(inputs.to_vec(), combined);
    }
}

/// Stream a file through SHA-256 with a fixed-size buffer. The buffer is
/// heap-allocated to keep stack usage trivial — a single allocation per
/// hashed file is negligible next to the I/O.
fn stream_file_checksum(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("Failed to open file for checksum: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_BUF_SIZE];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("Failed to read file for checksum: {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// A dirty mtime-cache entry waiting to be flushed: (path key, entry).
type DirtyMtimeEntry = (String, MtimeEntry);

/// Get checksum using mtime pre-check to avoid re-reading unchanged files.
/// Returns the checksum, which path was taken, and optionally a dirty mtime
/// entry to flush (absent for cache hits and for recently-modified files,
/// which are deliberately not cached — see below).
fn fast_checksum(
    ctx: &BuildContext,
    path: &Path,
) -> Result<(String, ChecksumPath, Option<DirtyMtimeEntry>)> {
    let metadata =
        fs::metadata(path).with_context(|| format!("Failed to stat file: {}", path.display()))?;
    fast_checksum_with_metadata(ctx, path, &metadata)
}

/// [`fast_checksum`] for a caller that has already stat'ed `path`, so the
/// file is not stat'ed a second time.
fn fast_checksum_with_metadata(
    ctx: &BuildContext,
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<(String, ChecksumPath, Option<DirtyMtimeEntry>)> {
    let stamp = crate::platform::file_stamp(metadata);

    let path_str = path.display().to_string();

    // The staged entries, then the stored table (see `MtimeState`).
    let cached = lookup_mtime_entry(ctx, &path_str)?;

    if let Some(ref entry) = cached
        && entry.stamp == stamp
    {
        remember_in_session(ctx, path, &entry.checksum);
        return Ok((entry.checksum.clone(), ChecksumPath::MtimeShortcut, None));
    }

    // mtime changed or no cache entry — recompute checksum from disk.
    //
    // The caller stat'ed the file, so it wants what is on disk now, not a
    // checksum this session took earlier: hash the bytes directly, then
    // refresh the in-session cache.
    let checksum = stream_file_checksum(path)?;
    remember_in_session(ctx, path, &checksum);
    // Don't persist an entry for a file changed moments ago: on coarse-
    // timestamp filesystems a write landing in the same tick after we
    // hashed would leave a permanently valid-looking (stamp, checksum) pair
    // for stale content. The next run simply re-hashes such files. Judged
    // by ctime as well as mtime: `cp -p` leaves an old mtime on a file it
    // just wrote, and only ctime shows the write was just now.
    let now_secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(i64::MAX, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    if stamp.changed_within(now_secs, RECENT_CHANGE_SECS) {
        return Ok((checksum, ChecksumPath::FullRead, None));
    }

    let new_entry = MtimeEntry {
        stamp,
        checksum: checksum.clone(),
    };

    Ok((
        checksum,
        ChecksumPath::FullRead,
        Some((path_str, new_entry)),
    ))
}

/// Stage dirty mtime entries for the next `flush_mtime_cache`.
fn stage_mtime_entries(ctx: &BuildContext, dirty: Vec<DirtyMtimeEntry>) {
    if dirty.is_empty() {
        return;
    }
    ctx.mtime_state.write().unwrap().pending.extend(dirty);
}

/// Write every staged mtime entry in a single write transaction, then move
/// them into the session's copy of the table.
///
/// The staged entries stay visible to lookups until the commit is done: they
/// are copied out under the lock, not taken, and removed only afterwards
/// (unless a newer entry for the same path was staged meanwhile).
pub fn flush_mtime_cache(ctx: &BuildContext) -> Result<()> {
    let dirty: Vec<DirtyMtimeEntry> = ctx
        .mtime_state
        .read()
        .unwrap()
        .pending
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if dirty.is_empty() {
        return Ok(());
    }
    let db_guard = get_mtime_db(ctx)?;
    let db = crate::errors::ctx_opt(db_guard.as_ref(), "Mtime database not available")?;
    let write_txn = crate::errors::ctx(
        db.begin_write(),
        "Failed to begin write transaction for mtime cache",
    )?;
    {
        let mut table = write_txn
            .open_table(MTIME_TABLE)
            .context("Failed to open mtime cache table")?;
        for (path_str, entry) in &dirty {
            let value = serde_json::to_vec(entry).context("Failed to serialize mtime entry")?;
            table
                .insert(path_str.as_str(), value.as_slice())
                .context("Failed to insert mtime entry")?;
        }
    }
    crate::errors::ctx(write_txn.commit(), "Failed to commit mtime cache entries")?;
    drop(db_guard);
    let mut state = ctx.mtime_state.write().unwrap();
    for (path_str, entry) in dirty {
        if state
            .pending
            .get(&path_str)
            .is_some_and(|p| p.stamp == entry.stamp && p.checksum == entry.checksum)
        {
            state.pending.remove(&path_str);
        }
        if let Some(stored) = state.stored.as_mut() {
            stored.insert(path_str, entry);
        }
    }
    Ok(())
}

/// Hash a list of individual checksums into a single combined SHA-256 checksum.
/// Each element is length-prefixed: a plain `:` join would be ambiguous
/// because `MISSING:{path}` entries embed arbitrary paths that may themselves
/// contain `:`.
fn hash_checksums(checksums: &[String]) -> String {
    let parts: Vec<&str> = checksums.iter().map(String::as_str).collect();
    hash_parts(&parts)
}

/// Hash a sequence of string parts with length prefixes, so no part can
/// impersonate a boundary between parts — the same injection-proofing as
/// `hash_checksums`, exposed for the other key-composition sites
/// (`CacheKey::digest`/`descriptor_key`, tool identity hashing). Those sites
/// used to join user-influenced values (instance names, file paths) with
/// bare separators, letting crafted values realign key boundaries.
pub fn hash_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for p in parts {
        hasher.update((p.len() as u64).to_le_bytes());
        hasher.update(p.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Outcome of a `checksum_fast` call, surfacing whether the mtime cache
/// succeeded in avoiding a file read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumPath {
    MtimeShortcut,
    FullRead,
}

/// Drop mtime-cache entries whose file no longer exists. Returns the number
/// removed.
///
/// The cache is keyed by path and nothing ever removed an entry, so deleting
/// or renaming a file left its row behind forever — the database grew
/// monotonically with the history of every path the project ever had.
/// Called by `rsconstruct cache trim`.
pub fn prune_mtime_cache(ctx: &BuildContext) -> Result<usize> {
    if !PathBuf::from(".rsconstruct").join("mtime.redb").exists() {
        return Ok(0);
    }

    let stale: Vec<String> = {
        let db_guard = get_mtime_db(ctx)?;
        let Some(db) = db_guard.as_ref() else {
            return Ok(0);
        };
        let read_txn = crate::errors::ctx(
            db.begin_read(),
            "Failed to begin read transaction for mtime prune",
        )?;
        let Ok(table) = read_txn.open_table(MTIME_TABLE) else {
            return Ok(0);
        };
        let mut stale = Vec::new();
        for entry in table.iter().context("Failed to iterate mtime cache")? {
            let (key, _) = entry.context("Failed to read mtime cache entry")?;
            let path_str = key.value();
            if !Path::new(path_str).exists() {
                stale.push(path_str.to_string());
            }
        }
        stale
    };

    if stale.is_empty() {
        return Ok(0);
    }

    let db_guard = get_mtime_db(ctx)?;
    let db = crate::errors::ctx_opt(db_guard.as_ref(), "Mtime database not available")?;
    let write_txn = crate::errors::ctx(
        db.begin_write(),
        "Failed to begin write transaction for mtime prune",
    )?;
    {
        let mut table = write_txn
            .open_table(MTIME_TABLE)
            .context("Failed to open mtime cache table for prune")?;
        for path_str in &stale {
            table
                .remove(path_str.as_str())
                .with_context(|| format!("Failed to remove mtime entry for {path_str}"))?;
        }
    }
    crate::errors::ctx(write_txn.commit(), "Failed to commit mtime cache prune")?;
    drop(db_guard);
    if let Some(stored) = ctx.mtime_state.write().unwrap().stored.as_mut() {
        for path_str in &stale {
            stored.remove(path_str);
        }
    }
    Ok(stale.len())
}

/// Compute an input file's checksum: the one this session already took (see
/// [`session_checksum`]), else through the persistent mtime cache.
pub fn checksum_fast(ctx: &BuildContext, path: &Path) -> Result<(String, ChecksumPath)> {
    if !ctx.mtime_check_enabled() {
        return Ok((file_checksum(ctx, path)?, ChecksumPath::FullRead));
    }
    if let Some(checksum) = session_checksum(ctx, path) {
        return Ok((checksum, ChecksumPath::MtimeShortcut));
    }
    let (checksum, path_taken, dirty) = fast_checksum(ctx, path)?;
    stage_mtime_entries(ctx, dirty.into_iter().collect());
    Ok((checksum, path_taken))
}

/// Checksum an **output** file, using the persistent mtime cache but never
/// the in-session cache.
///
/// The in-session cache (`file_checksum`) assumes content is stable for the
/// whole run — true for inputs, false for outputs, which are rewritten by
/// the very build doing the checking. Output verification therefore takes
/// the mtime shortcut when available and otherwise re-hashes from disk.
pub fn checksum_output(ctx: &BuildContext, path: &Path) -> Result<(String, ChecksumPath)> {
    if !ctx.mtime_check_enabled() {
        return Ok((stream_file_checksum(path)?, ChecksumPath::FullRead));
    }
    let (checksum, path_taken, dirty) = fast_checksum(ctx, path)?;
    stage_mtime_entries(ctx, dirty.into_iter().collect());
    Ok((checksum, path_taken))
}

/// Get the combined input checksum for a list of input files, using mtime
/// pre-check to avoid re-reading unchanged files across builds.
pub fn combined_input_checksum(ctx: &BuildContext, inputs: &[PathBuf]) -> Result<String> {
    let memoize = inputs.len() >= MEMO_MIN_INPUTS;
    if memoize && let Some(combined) = ctx.combined_memo.lock().unwrap().get(inputs) {
        return Ok(combined);
    }
    let combined = compute_combined_input_checksum(ctx, inputs, Session::Trust)?;
    if memoize {
        ctx.combined_memo
            .lock()
            .unwrap()
            .insert(inputs, combined.clone());
    }
    Ok(combined)
}

/// [`combined_input_checksum`] of the inputs as they are on disk now,
/// ignoring every checksum this session took earlier (the memo and
/// [`session_checksum`]). For the check, after a product ran, that none of
/// its inputs was edited while its tool ran: an edit by anyone but the build
/// itself never reaches [`forget_in_session`], so only a fresh look sees it.
/// The fresh values replace the session's.
pub fn fresh_combined_input_checksum(ctx: &BuildContext, inputs: &[PathBuf]) -> Result<String> {
    compute_combined_input_checksum(ctx, inputs, Session::Ignore)
}

/// Whether a combined checksum may use the checksums this session already
/// took.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Session {
    Trust,
    Ignore,
}

fn compute_combined_input_checksum(
    ctx: &BuildContext,
    inputs: &[PathBuf],
    session: Session,
) -> Result<String> {
    let mut checksums = Vec::with_capacity(inputs.len());
    let mut dirty_entries = Vec::new();

    for input in inputs {
        match input_checksum(ctx, input, session)? {
            InputChecksum::Missing => checksums.push(format!("MISSING:{}", input.display())),
            InputChecksum::Present(checksum, dirty) => {
                checksums.push(checksum);
                dirty_entries.extend(dirty);
            }
        }
    }

    stage_mtime_entries(ctx, dirty_entries);

    Ok(hash_checksums(&checksums))
}

/// One input's contribution to a combined checksum.
enum InputChecksum {
    /// The file cannot be stat'ed. Not remembered for the session: a missing
    /// input is usually an upstream output not written yet.
    Missing,
    /// Its checksum, with an mtime-cache entry to stage when one was minted.
    Present(String, Option<DirtyMtimeEntry>),
}

fn input_checksum(ctx: &BuildContext, input: &Path, session: Session) -> Result<InputChecksum> {
    if session == Session::Trust
        && let Some(checksum) = session_checksum(ctx, input)
    {
        return Ok(InputChecksum::Present(checksum, None));
    }
    // One stat answers both "does it exist" and "what is its stamp". Any
    // stat failure counts as missing, as `Path::exists` did here.
    let Ok(metadata) = fs::metadata(input) else {
        return Ok(InputChecksum::Missing);
    };
    if ctx.mtime_check_enabled() {
        let (checksum, _, dirty) = fast_checksum_with_metadata(ctx, input, &metadata)?;
        return Ok(InputChecksum::Present(checksum, dirty));
    }
    let checksum = stream_file_checksum(input)?;
    remember_in_session(ctx, input, &checksum);
    Ok(InputChecksum::Present(checksum, None))
}

/// Take the session checksum of every file in `inputs`, `parallel` threads
/// at a time, before classify asks for them product by product.
///
/// Classify runs one dependency level at a time, and a level can be small
/// or share most of its inputs; filling the session first makes the stat,
/// mtime lookup or hash of each distinct file one evenly spread job, and
/// leaves classify only hash-map reads.
///
/// A file that cannot be read is left out rather than reported: whoever
/// asks for it next reads it again and reports the error where it means
/// something (classify counts the product as to be built, the executor fails
/// it). Missing files are left out too, as in [`InputChecksum::Missing`].
pub fn fill_session(ctx: &BuildContext, inputs: &[&Path], parallel: usize) {
    let staged: Vec<Option<DirtyMtimeEntry>> =
        crate::executor::map_in_parallel(inputs, parallel, |input| {
            match input_checksum(ctx, input, Session::Trust) {
                Ok(InputChecksum::Present(_, dirty)) => dirty,
                Ok(InputChecksum::Missing) | Err(_) => None,
            }
        });
    stage_mtime_entries(ctx, staged.into_iter().flatten().collect());
}

/// Calculate SHA-256 checksum of a byte slice. Not cached — pure function.
pub fn bytes_checksum(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `cp -p` does: same length, new bytes, mtime put back. The mtime
    /// cache used to compare mtime alone and keep serving the old checksum;
    /// the stamp moves because ctime does.
    #[test]
    fn stamp_changes_when_content_is_rewritten_with_mtime_restored() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("f.txt");
        fs::write(&path, "aaaa").unwrap();
        let old_mtime = SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old_mtime)
            .unwrap();
        let before = crate::platform::file_stamp(&fs::metadata(&path).unwrap());

        // Clear the ctime tick of the first write on coarse filesystems.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&path, "bbbb").unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old_mtime)
            .unwrap();
        let after = crate::platform::file_stamp(&fs::metadata(&path).unwrap());

        assert_eq!(before.mtime_secs, after.mtime_secs, "mtime was restored");
        assert_eq!(before.size, after.size, "same length");
        assert_ne!(before, after, "ctime still records the rewrite");
    }

    /// A staged entry serves lookups before anything is written, and only
    /// `flush_mtime_cache` puts it in the database, in one commit.
    #[test]
    fn staged_entries_are_visible_at_once_and_written_by_flush() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = BuildContext::new();
        // A database of our own: the default one lives under the CWD.
        *ctx.mtime_db.lock().unwrap() =
            Some(redb::Database::create(tmp.path().join("mtime.redb")).unwrap());
        let path = tmp.path().join("f.txt");
        fs::write(&path, "content").unwrap();
        let key = path.display().to_string();
        let entry = MtimeEntry {
            stamp: crate::platform::file_stamp(&fs::metadata(&path).unwrap()),
            checksum: "staged".to_string(),
        };
        stage_mtime_entries(&ctx, vec![(key.clone(), entry)]);

        assert_eq!(
            checksum_fast(&ctx, &path).unwrap(),
            ("staged".to_string(), ChecksumPath::MtimeShortcut),
            "the staged entry is used before the flush"
        );
        let stored = |ctx: &BuildContext| -> Option<String> {
            let guard = ctx.mtime_db.lock().unwrap();
            let txn = guard.as_ref().unwrap().begin_read().unwrap();
            let table = txn.open_table(MTIME_TABLE).ok()?;
            let data = table.get(key.as_str()).unwrap()?;
            Some(
                serde_json::from_slice::<MtimeEntry>(data.value())
                    .unwrap()
                    .checksum,
            )
        };
        assert_eq!(stored(&ctx), None, "nothing is written before the flush");

        flush_mtime_cache(&ctx).unwrap();
        assert_eq!(stored(&ctx), Some("staged".to_string()));
        let state = ctx.mtime_state.read().unwrap();
        assert!(state.pending.is_empty());
        assert_eq!(
            state.get(&key).map(|e| e.checksum.as_str()),
            Some("staged"),
            "the flushed entry moved into the session's copy of the table"
        );
    }

    /// The table is read once and then served from memory, and an entry
    /// another rsconstruct version wrote (one that does not parse) is a miss
    /// rather than an error.
    #[test]
    fn stored_table_is_loaded_once_and_skips_unparsable_entries() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = BuildContext::new();
        let db = redb::Database::create(tmp.path().join("mtime.redb")).unwrap();
        let path = tmp.path().join("f.txt");
        fs::write(&path, "content").unwrap();
        let key = path.display().to_string();
        let entry = MtimeEntry {
            stamp: crate::platform::file_stamp(&fs::metadata(&path).unwrap()),
            checksum: "from-db".to_string(),
        };
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(MTIME_TABLE).unwrap();
            let value = serde_json::to_vec(&entry).unwrap();
            table.insert(key.as_str(), value.as_slice()).unwrap();
            table.insert("garbage", b"not json".as_slice()).unwrap();
        }
        txn.commit().unwrap();
        *ctx.mtime_db.lock().unwrap() = Some(db);

        assert_eq!(
            checksum_fast(&ctx, &path).unwrap(),
            ("from-db".to_string(), ChecksumPath::MtimeShortcut)
        );
        let state = ctx.mtime_state.read().unwrap();
        let stored = state.stored.as_ref().expect("loaded by the first lookup");
        assert!(stored.contains_key(&key));
        assert!(!stored.contains_key("garbage"));
    }

    /// A file whose ctime is recent counts as just changed even when its
    /// mtime is old, so no entry is stored for it.
    #[test]
    fn changed_within_uses_the_later_of_mtime_and_ctime() {
        let stamp = crate::platform::FileStamp {
            mtime_secs: 1_000,
            mtime_nanos: 0,
            ctime_secs: 5_000,
            ctime_nanos: 0,
            size: 4,
            inode: 1,
        };
        assert!(stamp.changed_within(5_001, RECENT_CHANGE_SECS));
        assert!(!stamp.changed_within(5_010, RECENT_CHANGE_SECS));
    }

    /// Length-prefixing exists precisely so element boundaries can't be
    /// forged: ["a:b", "c"] and ["a", "b:c"] concatenate identically under
    /// a plain `:` join.
    #[test]
    fn hash_checksums_is_injection_proof() {
        let joined_left = hash_checksums(&["a:b".to_string(), "c".to_string()]);
        let joined_right = hash_checksums(&["a".to_string(), "b:c".to_string()]);
        assert_ne!(joined_left, joined_right);

        assert_ne!(
            hash_checksums(&[]),
            hash_checksums(&[String::new()]),
            "no elements and one empty element must differ"
        );
        assert_eq!(
            hash_checksums(&["x".to_string()]),
            hash_checksums(&["x".to_string()]),
            "must be deterministic"
        );
    }

    /// The streaming hasher must agree with the one-shot hasher at every
    /// buffer boundary — off-by-one in the read loop shows up exactly there.
    #[test]
    fn stream_checksum_matches_bytes_checksum_at_buffer_boundaries() {
        let tmp = tempfile::TempDir::new().unwrap();
        for size in [0, HASH_BUF_SIZE - 1, HASH_BUF_SIZE, HASH_BUF_SIZE + 1] {
            let data = vec![0xABu8; size];
            let path = tmp.path().join(format!("f{size}"));
            fs::write(&path, &data).unwrap();
            assert_eq!(
                stream_file_checksum(&path).unwrap(),
                bytes_checksum(&data),
                "stream and one-shot checksums diverge at {size} bytes"
            );
        }
    }

    /// Missing inputs are part of the combined key (a vanished input must
    /// change it), and the combination is positional, not a set.
    #[test]
    fn combined_input_checksum_missing_and_order_semantics() {
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        let tmp = tempfile::TempDir::new().unwrap();

        let empty = tmp.path().join("empty.txt");
        fs::write(&empty, b"").unwrap();
        let missing = tmp.path().join("missing.txt");

        assert_ne!(
            combined_input_checksum(&ctx, std::slice::from_ref(&empty)).unwrap(),
            combined_input_checksum(&ctx, std::slice::from_ref(&missing)).unwrap(),
            "a missing input must not hash like an empty one"
        );

        let a = tmp.path().join("a.txt");
        let b = tmp.path().join("b.txt");
        fs::write(&a, b"aaa").unwrap();
        fs::write(&b, b"bbb").unwrap();
        assert_ne!(
            combined_input_checksum(&ctx, &[a.clone(), b.clone()]).unwrap(),
            combined_input_checksum(&ctx, &[b, a]).unwrap(),
            "input order is part of the key"
        );
    }

    /// The in-session cache serves repeat reads within one build run; the
    /// executor evicts a product's outputs via `forget_in_session` whenever
    /// it writes or restores them. Eviction is what keeps `file_checksum`
    /// correct when mtime checking is off — without it, a mid-build rewrite
    /// would be invisible for the rest of the run.
    #[test]
    fn file_checksum_cache_evicts_on_forget() {
        let ctx = BuildContext::new();
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("f.txt");

        fs::write(&path, b"one").unwrap();
        let first = file_checksum(&ctx, &path).unwrap();
        fs::write(&path, b"two").unwrap();
        assert_eq!(
            file_checksum(&ctx, &path).unwrap(),
            first,
            "un-evicted reads serve the in-session cached checksum"
        );

        forget_in_session(&ctx, std::slice::from_ref(&path), &[]);
        let second = file_checksum(&ctx, &path).unwrap();
        assert_ne!(second, first, "eviction must expose the new content");

        let fresh_ctx = BuildContext::new();
        assert_eq!(
            file_checksum(&fresh_ctx, &path).unwrap(),
            second,
            "post-eviction value must match a fresh context's view"
        );
    }

    /// Watch mode reuses one context across rebuilds. The combined-input
    /// memo is consulted before any stat, and `forget_in_session` only ever
    /// evicts product outputs — so an edit to a *source* file in a large
    /// input list was invisible to every later rebuild. Clearing the
    /// session caches between rebuilds is what makes the edit count.
    #[test]
    fn clear_session_caches_drops_memo_and_checksum_cache() {
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        let tmp = tempfile::TempDir::new().unwrap();

        let inputs: Vec<PathBuf> = (0..MEMO_MIN_INPUTS)
            .map(|i| {
                let p = tmp.path().join(format!("in{i}.txt"));
                fs::write(&p, format!("v1-{i}")).unwrap();
                p
            })
            .collect();
        let first = combined_input_checksum(&ctx, &inputs).unwrap();
        assert!(
            ctx.combined_memo.lock().unwrap().get(&inputs).is_some(),
            "a list of MEMO_MIN_INPUTS files is memoized"
        );

        // A source edit: nothing calls forget_in_session for it.
        fs::write(&inputs[3], "v2-3").unwrap();
        assert_eq!(
            combined_input_checksum(&ctx, &inputs).unwrap(),
            first,
            "without a clear the memo hides the edit (the bug being fixed)"
        );

        ctx.clear_session_caches();
        assert!(
            ctx.combined_memo.lock().unwrap().get(&inputs).is_none(),
            "the memo is empty after a clear"
        );
        assert!(
            ctx.checksum_cache.read().unwrap().is_empty(),
            "the in-session checksum cache is empty after a clear"
        );
        let second = combined_input_checksum(&ctx, &inputs).unwrap();
        assert_ne!(second, first, "after a clear the edit is seen");
        // No mtime cache on the fresh context either: with it on, this would
        // open the project's .rsconstruct/mtime.redb (relative to the test's
        // cwd), which a concurrent build may hold.
        let fresh_ctx = BuildContext::new();
        fresh_ctx.set_mtime_check(false);
        assert_eq!(
            combined_input_checksum(&fresh_ctx, &inputs).unwrap(),
            second,
            "and agrees with a fresh context"
        );
    }

    /// A context whose checksums never touch the project's mtime database
    /// (see the note in the test above).
    fn session_only_ctx() -> BuildContext {
        let ctx = BuildContext::new();
        ctx.set_mtime_check(false);
        ctx
    }

    /// Within a run an input is looked at once: a later edit that nothing
    /// told the session about is not seen by `combined_input_checksum`, and
    /// is seen by `fresh_combined_input_checksum`, which also refreshes the
    /// session's value.
    #[test]
    fn fresh_checksum_sees_an_edit_the_session_does_not() {
        let ctx = session_only_ctx();
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("in.txt");
        fs::write(&path, "one").unwrap();
        let inputs = std::slice::from_ref(&path);

        let first = combined_input_checksum(&ctx, inputs).unwrap();
        fs::write(&path, "two").unwrap();
        assert_eq!(combined_input_checksum(&ctx, inputs).unwrap(), first);

        let fresh = fresh_combined_input_checksum(&ctx, inputs).unwrap();
        assert_ne!(fresh, first, "a fresh look reads the edited file");
        assert_eq!(
            combined_input_checksum(&ctx, inputs).unwrap(),
            fresh,
            "and the session now serves the fresh value"
        );
    }

    /// An output directory's files are not listed one by one, so evicting a
    /// product's outputs must cover every file under its output directories,
    /// in the session checksums and in the memo.
    #[test]
    fn forgetting_an_output_dir_evicts_the_files_under_it() {
        let ctx = session_only_ctx();
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("site");
        fs::create_dir(&dir).unwrap();
        let inside: Vec<PathBuf> = (0..MEMO_MIN_INPUTS)
            .map(|i| {
                let p = dir.join(format!("page{i}.html"));
                fs::write(&p, format!("v1-{i}")).unwrap();
                p
            })
            .collect();
        let outside = tmp.path().join("other.txt");
        fs::write(&outside, "x").unwrap();

        let first = combined_input_checksum(&ctx, &inside).unwrap();
        file_checksum(&ctx, &outside).unwrap();
        fs::write(&inside[0], "v2-0").unwrap();

        forget_in_session(&ctx, &[], &[std::sync::Arc::new(dir)]);
        assert!(ctx.combined_memo.lock().unwrap().get(&inside).is_none());
        assert!(session_checksum(&ctx, &inside[0]).is_none());
        assert!(
            session_checksum(&ctx, &outside).is_some(),
            "files outside the directory stay"
        );
        assert_ne!(combined_input_checksum(&ctx, &inside).unwrap(), first);
    }

    /// `fill_session` takes every present input's checksum up front, on
    /// several threads, and leaves missing files for the per-product walk.
    #[test]
    fn fill_session_takes_present_inputs_and_skips_missing_ones() {
        let ctx = session_only_ctx();
        let tmp = tempfile::TempDir::new().unwrap();
        let present: Vec<PathBuf> = (0..300)
            .map(|i| {
                let p = tmp.path().join(format!("f{i}.txt"));
                fs::write(&p, format!("content {i}")).unwrap();
                p
            })
            .collect();
        let missing = tmp.path().join("not-yet-written.txt");
        let mut wanted: Vec<&Path> = present.iter().map(PathBuf::as_path).collect();
        wanted.push(&missing);

        fill_session(&ctx, &wanted, 4);

        for path in &present {
            assert_eq!(
                session_checksum(&ctx, path),
                Some(stream_file_checksum(path).unwrap())
            );
        }
        assert!(session_checksum(&ctx, &missing).is_none());
        assert_eq!(
            combined_input_checksum(&ctx, std::slice::from_ref(&missing)).unwrap(),
            fresh_combined_input_checksum(&session_only_ctx(), std::slice::from_ref(&missing))
                .unwrap(),
            "a missing input still counts as missing"
        );
    }
}
