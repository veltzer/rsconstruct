//! Dependency cache for storing source file dependencies.
//!
//! Uses a redb key/value store to cache dependency information discovered
//! from source files. This avoids re-scanning files that haven't changed.
//!
//! Cache key: `"<analyzer>\0<source path>"` — analyzer name is part of the
//! key so two analyzers scanning the same file (e.g. a future `python` +
//! `mypy-imports` pair) never overwrite each other's entries. The NUL
//! separator is safe: neither analyzer inames nor paths can contain NUL.
//!
//! Cache value: (`source_checksum`, dependencies, `dependency_checksums`,
//! `absent`, `config_fingerprint`)
//!
//! The cache is invalidated when the source file or any listed dependency
//! changes. Checking the dependencies matters for analyzers whose list is
//! transitive (`icpp`, `cpp`): when `a.h` gains `#include "b.h"`, the source
//! that includes `a.h` is unchanged, but its dependency list is now short —
//! only `a.h`'s new checksum reveals that it must be rescanned.
//!
//! Two more things decide a dependency list without being file contents:
//! - which files *do not* exist. `#include "x.h"` resolved to `include/x.h`
//!   because `src/x.h` was absent; once `src/x.h` appears it shadows the
//!   old match. The analyzer reports such probe paths as `absent`, and an
//!   entry is dropped as soon as any of them exists.
//! - the analyzer's configuration (include paths, load paths, pkg-config
//!   output, ...). Every entry records the fingerprint it was scanned
//!   under; an entry from another configuration is a miss.

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::build_context::BuildContext;
use crate::checksum::{ChecksumPath, checksum_fast};

const RSBUILD_DIR: &str = ".rsconstruct";
const DEPS_DB_FILE: &str = "deps.redb";

const DEPS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("deps");

/// Cached dependency entry
#[derive(Debug, Serialize, Deserialize)]
struct DepsEntry {
    /// Checksum of the source file when dependencies were scanned
    source_checksum: String,
    /// List of dependency paths (relative to project root)
    dependencies: Vec<String>,
    /// Checksum of each entry of `dependencies`, in the same order, when
    /// they were scanned. An entry written before this field existed has
    /// none, fails the length check and is rescanned once.
    #[serde(default)]
    dependency_checksums: Vec<String>,
    /// Paths whose absence the dependency list relies on (see the module
    /// doc). The entry is invalid once any of them exists.
    #[serde(default)]
    absent: Vec<String>,
    /// `AnalyzerId::fingerprint` the entry was scanned under.
    #[serde(default)]
    config_fingerprint: String,
    /// Name of the analyzer that created this entry (e.g., "cpp", "python")
    #[serde(default)]
    analyzer: String,
}

/// The analyzer instance a cache entry belongs to, and the configuration it
/// scans under.
#[derive(Debug, Clone, Copy)]
pub struct AnalyzerId<'a> {
    /// Analyzer instance name — part of the entry's key.
    pub iname: &'a str,
    /// Hash of everything other than file contents that decides how the
    /// analyzer resolves dependencies: its config and any search paths it
    /// resolved at run time. Stored in the entry; a mismatch is a miss.
    pub fingerprint: &'a str,
}

/// Result of a `classify` call — the predict-pass analogue of `DepsCacheStats`.
/// `MtimeHit` + `ContentHit` = a hit that `get` would also report; Miss means
/// `get` would rescan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifyResult {
    /// Cache valid, mtime-cache shortcut applied (no I/O needed).
    MtimeHit,
    /// Cache valid, but mtime was stale so the file was read and re-hashed.
    ContentHit,
    /// Cache invalid or absent.
    Miss,
}

/// Statistics about dependency cache usage.
/// `mtime_hits + content_hits == hits` always holds.
#[derive(Debug, Default, Clone)]
pub struct DepsCacheStats {
    /// Total number of cache hits (`mtime_hits` + `content_hits`).
    pub hits: usize,
    /// Hits where the mtime cache shortcut succeeded — no file I/O was done.
    pub mtime_hits: usize,
    /// Hits where the mtime was stale so the file had to be re-read and
    /// re-hashed, but the content checksum still matched the stored one
    /// (e.g. a touched-but-unchanged file).
    pub content_hits: usize,
    /// Number of cache misses.
    pub misses: usize,
}

/// Dependency cache using redb key/value store
pub struct DepsCache {
    db: Database,
    stats: DepsCacheStats,
    /// Entries `set` but not yet written (see `flush`), by key.
    pending: HashMap<String, DepsEntry>,
}

impl DepsCache {
    /// Open or create the dependency cache
    pub fn open() -> Result<Self> {
        Self::open_in(Path::new(""))
    }

    /// Open the cache with `.rsconstruct` rooted under `base`. Lets tests use
    /// a tempdir without mutating the process-global current directory (which
    /// races the parallel test runner).
    pub fn open_in(base: &Path) -> Result<Self> {
        let rsconstruct_dir = base.join(RSBUILD_DIR);
        let db_path = rsconstruct_dir.join(DEPS_DB_FILE);

        // Ensure .rsconstruct directory exists
        fs::create_dir_all(&rsconstruct_dir).context("Failed to create .rsconstruct directory")?;

        let db = crate::db::open_or_recreate(&db_path, "Dependency cache")?;

        Ok(Self {
            db,
            stats: DepsCacheStats::default(),
            pending: HashMap::new(),
        })
    }

    /// Get cached dependencies for a (analyzer, source) pair if the cache is
    /// valid. Returns None if the file has changed or isn't cached.
    /// Updates internal statistics (hits/misses). Every caller hits exactly
    /// one of the two counters — no silent path that leaves both unchanged,
    /// so `hits + misses` always equals the number of `get` calls.
    pub fn get(
        &mut self,
        ctx: &BuildContext,
        analyzer: AnalyzerId<'_>,
        source: &Path,
    ) -> Option<Vec<PathBuf>> {
        let Some((deps, checksum_path)) = self.lookup(ctx, analyzer, source) else {
            self.stats.misses += 1;
            return None;
        };
        self.stats.hits += 1;
        match checksum_path {
            ChecksumPath::MtimeShortcut => self.stats.mtime_hits += 1,
            ChecksumPath::FullRead => self.stats.content_hits += 1,
        }
        Some(deps)
    }

    /// Dry-run of `get`: predict whether this (analyzer, source) pair would
    /// hit the cache, and if so whether the mtime shortcut would apply.
    /// Used by the pre-scan classify pass to count expected hits vs rescans
    /// before the actual scan runs. Identical validity rules to `get`.
    /// Does not touch stats.
    pub fn classify(
        &self,
        ctx: &BuildContext,
        analyzer: AnalyzerId<'_>,
        source: &Path,
    ) -> ClassifyResult {
        match self.lookup(ctx, analyzer, source) {
            None => ClassifyResult::Miss,
            Some((_, ChecksumPath::MtimeShortcut)) => ClassifyResult::MtimeHit,
            Some((_, ChecksumPath::FullRead)) => ClassifyResult::ContentHit,
        }
    }

    /// The validity rules shared by `get` and `classify`: the cached list for
    /// `source` and how its checksum was confirmed, or None when the entry
    /// cannot be trusted.
    ///
    /// Any failure to reach the stored entry — DB not yet created, table
    /// missing, deserialization error, stat failure — is a miss: these all
    /// mean "we can't trust the cache for this file."
    fn lookup(
        &self,
        ctx: &BuildContext,
        analyzer: AnalyzerId<'_>,
        source: &Path,
    ) -> Option<(Vec<PathBuf>, ChecksumPath)> {
        let key = key_for(analyzer.iname, source);
        if let Some(entry) = self.pending.get(&key) {
            return Self::validate(ctx, analyzer, source, entry);
        }
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(DEPS_TABLE).ok()?;
        let data = table.get(key.as_str()).ok()??;
        let entry = serde_json::from_slice::<DepsEntry>(data.value()).ok()?;
        Self::validate(ctx, analyzer, source, &entry)
    }

    /// The checks an entry — stored or pending — must pass to be trusted.
    fn validate(
        ctx: &BuildContext,
        analyzer: AnalyzerId<'_>,
        source: &Path,
        entry: &DepsEntry,
    ) -> Option<(Vec<PathBuf>, ChecksumPath)> {
        // Scanned under another configuration: its resolution may differ.
        if entry.config_fingerprint != analyzer.fingerprint {
            return None;
        }
        // Verify source file hasn't changed. `checksum_fast` consults the
        // persistent mtime cache so unchanged files skip the full read + hash.
        let (current_checksum, checksum_path) = checksum_fast(ctx, source).ok()?;
        if entry.source_checksum != current_checksum {
            return None;
        }
        // Verify every dependency still exists with the content it was
        // scanned with, and that nothing has appeared where resolution
        // relied on finding nothing (see the module doc for both).
        if !dependencies_unchanged(ctx, entry) || !absent_still_absent(entry) {
            return None;
        }
        let deps = entry.dependencies.iter().map(PathBuf::from).collect();
        Some((deps, checksum_path))
    }

    /// Compute the source checksum for use with [`Self::set`]. Call this
    /// BEFORE scanning the file: pairing a checksum taken after the scan with
    /// deps derived from the pre-scan content would let a mid-build edit
    /// poison the cache with a stale dependency list. Uses `checksum_fast` so
    /// the mtime cache is populated alongside — subsequent `get()` calls can
    /// then short-circuit on mtime.
    pub fn source_checksum(ctx: &BuildContext, source: &Path) -> Result<String> {
        let (checksum, _) = checksum_fast(ctx, source)?;
        Ok(checksum)
    }

    /// Store dependencies for a (analyzer, source) pair.
    /// `source_checksum` must come from [`Self::source_checksum`] taken
    /// before the scan that produced `dependencies`. The dependencies'
    /// checksums are taken here, after the scan, since the scan is what
    /// names them; a dependency edited between its scan and this call is
    /// recorded with its new content, and that one change goes unnoticed
    /// until the file changes again — the same window a build always has
    /// for an input edited while it runs.
    ///
    /// `absent` lists the paths the scan probed and found missing on the
    /// way to its result (see the module doc).
    ///
    /// The entry is staged, not written: lookups see it at once, and
    /// [`flush`](Self::flush) writes every staged entry in one transaction.
    /// A redb commit is durable — an fsync — so committing per entry cost
    /// one disk sync per scanned file on a cold cache.
    pub fn set(
        &mut self,
        ctx: &BuildContext,
        analyzer: AnalyzerId<'_>,
        source: &Path,
        source_checksum: String,
        dependencies: &[PathBuf],
        absent: &[PathBuf],
    ) -> Result<()> {
        let key = key_for(analyzer.iname, source);

        let dependency_checksums = dependencies
            .iter()
            .map(|dep| {
                checksum_fast(ctx, dep)
                    .map(|(checksum, _)| checksum)
                    .with_context(|| format!("Failed to checksum dependency {}", dep.display()))
            })
            .collect::<Result<Vec<String>>>()?;
        let entry = DepsEntry {
            source_checksum,
            dependencies: dependencies
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            dependency_checksums,
            absent: absent.iter().map(|p| p.display().to_string()).collect(),
            config_fingerprint: analyzer.fingerprint.to_string(),
            analyzer: analyzer.iname.to_string(),
        };
        self.pending.insert(key, entry);
        Ok(())
    }

    /// Write every entry staged by [`set`](Self::set) in one transaction.
    /// Staged entries are dropped on error as well as on success: the cache
    /// only saves rescans, so losing a batch costs one rescan per entry.
    pub fn flush(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let pending = std::mem::take(&mut self.pending);
        let write_txn = self
            .db
            .begin_write()
            .context("Failed to begin write transaction")?;
        {
            let mut table = write_txn
                .open_table(DEPS_TABLE)
                .context("Failed to open deps table")?;
            for (key, entry) in &pending {
                let data =
                    serde_json::to_vec(entry).context("Failed to serialize dependency entry")?;
                table
                    .insert(key.as_str(), data.as_slice())
                    .context("Failed to write to dependency cache")?;
            }
        }
        write_txn
            .commit()
            .context("Failed to commit dependency cache write")?;
        Ok(())
    }

    /// Count a scan that never consulted the cache (an analyzer that always
    /// rescans) as a miss, so `misses` is every source actually scanned.
    pub const fn count_uncached_scan(&mut self) {
        self.stats.misses += 1;
    }

    /// Get cache statistics (hits and misses)
    pub const fn stats(&self) -> &DepsCacheStats {
        &self.stats
    }

    /// Collect all entries from the database as (analyzer, `source_path`, `DepsEntry`) triples.
    /// Returns an empty Vec on any error (missing table, etc.).
    /// Entries with malformed keys (no NUL separator) are skipped — that would
    /// be a pre-key-format-change entry from an older build, effectively invalid.
    fn collect_entries(&self) -> Vec<(String, PathBuf, DepsEntry)> {
        let Ok(read_txn) = self.db.begin_read() else {
            return Vec::new();
        };
        let Ok(table) = read_txn.open_table(DEPS_TABLE) else {
            return Vec::new();
        };
        let Ok(iter) = table.iter() else {
            return Vec::new();
        };
        iter.filter_map(|item| {
            let (key, value) = item.ok()?;
            let (analyzer, source) = parse_key(key.value())?;
            let entry: DepsEntry = serde_json::from_slice(value.value()).ok()?;
            Some((analyzer, source, entry))
        })
        .collect()
    }

    /// Get all raw cached entries for a given source path, across every
    /// analyzer that has scanned it. Each returned tuple is (dependencies,
    /// `analyzer_name`). Returns an empty Vec if the source has no entries.
    /// Used by `analyzer show files <path>`, where the user gives a path and
    /// expects to see every analyzer's view of it.
    pub fn get_raw_for_path(&self, source: &Path) -> Vec<(Vec<PathBuf>, String)> {
        self.collect_entries()
            .into_iter()
            .filter(|(_a, s, _e)| s == source)
            .map(|(analyzer, _s, entry)| {
                let deps = entry.dependencies.iter().map(PathBuf::from).collect();
                (deps, analyzer)
            })
            .collect()
    }

    /// List all cached source files and their dependencies.
    /// Returns tuples of (`source_path`, dependencies, `analyzer_name`).
    pub fn list_all(&self) -> Vec<(PathBuf, Vec<PathBuf>, String)> {
        self.collect_entries()
            .into_iter()
            .map(|(analyzer, source, entry)| {
                let deps: Vec<PathBuf> = entry.dependencies.iter().map(PathBuf::from).collect();
                (source, deps, analyzer)
            })
            .collect()
    }

    /// Get statistics about cached dependencies by analyzer.
    /// Returns a map of `analyzer_name` -> (`file_count`, `total_dep_count`).
    pub fn stats_by_analyzer(&self) -> std::collections::HashMap<String, (usize, usize)> {
        let mut stats: std::collections::HashMap<String, (usize, usize)> =
            std::collections::HashMap::new();
        for (analyzer, _source, entry) in self.collect_entries() {
            let name = if analyzer.is_empty() {
                "unknown".to_string()
            } else {
                analyzer
            };
            let (files, deps) = stats.entry(name).or_insert((0, 0));
            *files += 1;
            *deps += entry.dependencies.len();
        }
        stats
    }

    /// List cached source files and their dependencies filtered by analyzer names.
    /// Returns tuples of (`source_path`, dependencies, `analyzer_name`).
    pub fn list_by_analyzers(&self, analyzers: &[String]) -> Vec<(PathBuf, Vec<PathBuf>, String)> {
        self.collect_entries()
            .into_iter()
            .filter_map(|(analyzer, source, entry)| {
                if !analyzers.contains(&analyzer) {
                    return None;
                }
                let deps: Vec<PathBuf> = entry.dependencies.iter().map(PathBuf::from).collect();
                Some((source, deps, analyzer))
            })
            .collect()
    }

    /// Remove all cached entries created by a specific analyzer.
    /// Returns the number of entries removed.
    pub fn remove_by_analyzer(&self, analyzer: &str) -> Result<usize> {
        // Collect raw keys to remove by re-encoding (analyzer, source) → key.
        let keys_to_remove: Vec<String> = self
            .collect_entries()
            .into_iter()
            .filter_map(|(a, source, _entry)| {
                if a == analyzer {
                    Some(key_for(&a, &source))
                } else {
                    None
                }
            })
            .collect();

        let mut removed = 0;
        if !keys_to_remove.is_empty() {
            let write_txn = self
                .db
                .begin_write()
                .context("Failed to begin write transaction")?;
            {
                let mut table = write_txn
                    .open_table(DEPS_TABLE)
                    .context("Failed to open deps table")?;
                for key in &keys_to_remove {
                    if table.remove(key.as_str()).is_ok() {
                        removed += 1;
                    }
                }
            }
            write_txn
                .commit()
                .context("Failed to commit dependency cache removal")?;
        }

        Ok(removed)
    }
}

/// Whether every dependency of `entry` still exists with the checksum it
/// had when it was scanned. A missing or unreadable file, or a stored
/// checksum list that does not line up with the paths, counts as changed.
/// `checksum_fast` consults the mtime cache, so unchanged files cost a stat.
fn dependencies_unchanged(ctx: &BuildContext, entry: &DepsEntry) -> bool {
    entry.dependencies.len() == entry.dependency_checksums.len()
        && entry
            .dependencies
            .iter()
            .zip(&entry.dependency_checksums)
            .all(|(dep, stored)| {
                checksum_fast(ctx, Path::new(dep)).is_ok_and(|(current, _)| &current == stored)
            })
}

/// Whether every path the scan relied on being missing is still missing.
/// Anything at all appearing there — file, directory, symlink — may change
/// how the analyzer resolves, so any existence counts.
fn absent_still_absent(entry: &DepsEntry) -> bool {
    entry
        .absent
        .iter()
        .all(|path| fs::symlink_metadata(path).is_err())
}

/// Build the composite cache key for an (analyzer, source) pair. NUL is used
/// as the separator because neither analyzer inames nor filesystem paths can
/// contain NUL bytes, so there's no possible ambiguity.
fn key_for(analyzer: &str, path: &Path) -> String {
    let mut s = String::with_capacity(analyzer.len() + 1 + path.as_os_str().len());
    s.push_str(analyzer);
    s.push('\0');
    s.push_str(&path.display().to_string());
    s
}

/// Split a composite key back into (analyzer, source path). Returns None if
/// the key predates the composite format (no NUL separator) or is otherwise
/// malformed — such entries are treated as stale and ignored.
fn parse_key(key: &str) -> Option<(String, PathBuf)> {
    let (analyzer, path) = key.split_once('\0')?;
    Some((analyzer.to_string(), PathBuf::from(path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_and_parse_roundtrip() {
        let key = key_for("python", Path::new("src/foo/bar.py"));
        assert_eq!(key, "python\0src/foo/bar.py");
        let (analyzer, path) = parse_key(&key).expect("must parse");
        assert_eq!(analyzer, "python");
        assert_eq!(path, PathBuf::from("src/foo/bar.py"));
    }

    #[test]
    fn keys_differ_by_analyzer() {
        // The whole point of the composite key: two analyzers scanning the
        // same file must produce distinct cache entries.
        let k1 = key_for("python", Path::new("foo.py"));
        let k2 = key_for("mypy", Path::new("foo.py"));
        assert_ne!(k1, k2);
    }

    #[test]
    fn keys_differ_by_instance_name() {
        // Multi-instance analyzers (e.g. cpp.kernel vs cpp.userspace) must
        // also produce distinct keys.
        let k1 = key_for("cpp.kernel", Path::new("foo.c"));
        let k2 = key_for("cpp.userspace", Path::new("foo.c"));
        assert_ne!(k1, k2);
    }

    #[test]
    fn parse_rejects_key_without_separator() {
        // Pre-composite-format entries (just a bare path) must not parse —
        // they're treated as stale and dropped from listings.
        assert!(parse_key("just/a/path.py").is_none());
    }

    #[test]
    fn parse_handles_path_with_colons() {
        // The separator is NUL specifically because paths can contain every
        // other punctuation character. A path with colons must parse cleanly.
        let key = key_for("cpp", Path::new("src/a:b.c"));
        let (analyzer, path) = parse_key(&key).unwrap();
        assert_eq!(analyzer, "cpp");
        assert_eq!(path, PathBuf::from("src/a:b.c"));
    }

    /// A cached list must be dropped when a dependency changes, even though
    /// the source itself did not: that is how a header gaining a new
    /// `#include` gets the including source rescanned.
    #[test]
    fn get_misses_when_a_dependency_changes() {
        // The mtime cache lives in the working directory's .rsconstruct/,
        // shared with (and locked by) any rsconstruct process running there;
        // checksum by content so the test touches only its tempdir.
        let fresh_ctx = || {
            let ctx = crate::build_context::BuildContext::new();
            ctx.set_mtime_check(false);
            ctx
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = fresh_ctx();
        let mut cache = DepsCache::open_in(tmp.path()).expect("open fresh cache");
        let source = tmp.path().join("main.c");
        let header = tmp.path().join("a.h");
        fs::write(&source, "#include \"a.h\"\n").unwrap();
        fs::write(&header, "#define V 1\n").unwrap();

        let checksum = DepsCache::source_checksum(&ctx, &source).unwrap();
        cache
            .set(
                &ctx,
                ICPP,
                &source,
                checksum,
                std::slice::from_ref(&header),
                &[],
            )
            .unwrap();
        assert_eq!(cache.get(&ctx, ICPP, &source), Some(vec![header.clone()]));

        // A fresh context drops the in-session checksum cache, as a new
        // build would.
        fs::write(&header, "#include \"b.h\"\n").unwrap();
        let ctx = fresh_ctx();
        assert_eq!(cache.get(&ctx, ICPP, &source), None);
    }

    const ICPP: AnalyzerId<'static> = AnalyzerId {
        iname: "icpp",
        fingerprint: "config-a",
    };

    /// See `get_misses_when_a_dependency_changes` for why the mtime cache
    /// is off.
    fn content_only_ctx() -> crate::build_context::BuildContext {
        let ctx = crate::build_context::BuildContext::new();
        ctx.set_mtime_check(false);
        ctx
    }

    /// `set` stages; lookups see the staged entry at once, and only `flush`
    /// writes it — one transaction for the whole batch.
    #[test]
    fn set_is_visible_at_once_and_persisted_by_flush() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = content_only_ctx();
        let a = tmp.path().join("a.c");
        let b = tmp.path().join("b.c");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();

        {
            let mut cache = DepsCache::open_in(tmp.path()).expect("open fresh cache");
            let checksum = DepsCache::source_checksum(&ctx, &a).unwrap();
            cache.set(&ctx, ICPP, &a, checksum, &[], &[]).unwrap();
            assert_eq!(cache.get(&ctx, ICPP, &a), Some(Vec::new()), "staged entry");
            cache.flush().unwrap();
            // Staged after the flush and never flushed: lost with the handle.
            let checksum = DepsCache::source_checksum(&ctx, &b).unwrap();
            cache.set(&ctx, ICPP, &b, checksum, &[], &[]).unwrap();
        }

        let mut reopened = DepsCache::open_in(tmp.path()).expect("reopen cache");
        assert_eq!(reopened.get(&ctx, ICPP, &a), Some(Vec::new()), "flushed");
        assert_eq!(reopened.get(&ctx, ICPP, &b), None, "never flushed");
    }

    /// An entry scanned under one analyzer configuration says nothing about
    /// another: changing `include_paths` can resolve the same `#include` to
    /// a different file without any file changing.
    #[test]
    fn get_misses_when_the_config_fingerprint_changes() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = content_only_ctx();
        let mut cache = DepsCache::open_in(tmp.path()).expect("open fresh cache");
        let source = tmp.path().join("main.c");
        fs::write(&source, "int main;\n").unwrap();

        let checksum = DepsCache::source_checksum(&ctx, &source).unwrap();
        cache.set(&ctx, ICPP, &source, checksum, &[], &[]).unwrap();
        assert_eq!(cache.get(&ctx, ICPP, &source), Some(Vec::new()));

        let other = AnalyzerId {
            iname: "icpp",
            fingerprint: "config-b",
        };
        assert_eq!(cache.get(&ctx, other, &source), None);
        assert_eq!(cache.classify(&ctx, other, &source), ClassifyResult::Miss);
    }

    /// A probe path that was missing when the source was scanned must still
    /// be missing for the entry to hold: a header appearing earlier on the
    /// search path shadows the one the list names.
    #[test]
    fn get_misses_when_an_absent_path_appears() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = content_only_ctx();
        let mut cache = DepsCache::open_in(tmp.path()).expect("open fresh cache");
        let source = tmp.path().join("main.c");
        let found = tmp.path().join("include_x.h");
        let shadow = tmp.path().join("x.h");
        fs::write(&source, "#include \"x.h\"\n").unwrap();
        fs::write(&found, "\n").unwrap();

        let checksum = DepsCache::source_checksum(&ctx, &source).unwrap();
        cache
            .set(
                &ctx,
                ICPP,
                &source,
                checksum,
                std::slice::from_ref(&found),
                std::slice::from_ref(&shadow),
            )
            .unwrap();
        assert_eq!(cache.get(&ctx, ICPP, &source), Some(vec![found]));

        fs::write(&shadow, "\n").unwrap();
        assert_eq!(cache.get(&ctx, ICPP, &source), None);
        assert_eq!(cache.classify(&ctx, ICPP, &source), ClassifyResult::Miss);
    }

    /// Regression guard: every `get` call must increment exactly one counter.
    /// The earlier implementation used `.ok()?` on `begin_read` and
    /// `open_table`, which silently returned None without counting — so on a
    /// fresh DB the first call was statistically invisible and the predict
    /// pass's numbers wouldn't match the summary's. Calling `get` against a
    /// nonexistent source file (in a fresh tempdir, no cache yet) must count
    /// as a miss.
    #[test]
    fn get_counts_miss_even_when_db_is_fresh() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Root the cache in the tempdir directly — mutating the process-wide
        // current dir would race other tests in the parallel runner.
        let ctx = crate::build_context::BuildContext::new();
        let mut cache = DepsCache::open_in(tmp.path()).expect("open fresh cache");
        let nonexistent = tmp.path().join("does_not_exist.py");
        let python = AnalyzerId {
            iname: "python",
            fingerprint: "",
        };
        let result = cache.get(&ctx, python, &nonexistent);

        assert!(result.is_none(), "missing entry must return None");
        let stats = cache.stats();
        assert_eq!(
            stats.hits + stats.misses,
            1,
            "exactly one of hits/misses must advance per get call (hits={}, misses={})",
            stats.hits,
            stats.misses
        );
        assert_eq!(stats.misses, 1, "missing entry counts as a miss");
    }
}
