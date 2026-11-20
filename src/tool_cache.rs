//! `rsconstruct tool cache`: look at and prune the two machine-wide tool
//! caches, the tool store ([`crate::tool_store::store_root`]) and the
//! download cache ([`crate::tool_store::download_cache_root`]).
//!
//! An entry is one store install (`<method>/<name>@<version>`) or one
//! downloaded asset. Its last use is its modification time: a store entry's
//! is refreshed whenever a project links it, a download's whenever an
//! install or `tool lock` is served from it ([`mark_used`]). Deleting any
//! entry is safe: a project holds hard links to its tools, and whatever is
//! missing is installed or downloaded again when next needed.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Which cache an entry is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Store,
    Downloads,
}

impl Kind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Store => "store",
            Self::Downloads => "downloads",
        }
    }
}

/// One entry of a cache.
#[derive(Debug, Clone)]
pub struct Entry {
    pub kind: Kind,
    /// The entry's path relative to its cache's root.
    pub name: String,
    pub path: PathBuf,
    /// Bytes its files take, counting each file once. A store file a
    /// project links to stays on disk after the store's copy is removed.
    pub bytes: u64,
    pub last_used: SystemTime,
}

/// The file a download of `url` is kept in, under the cache directory
/// `dir`: a short hash of the URL, which keys it, and the asset's own name,
/// which says what it is.
pub fn download_entry(dir: &Path, url: &str) -> PathBuf {
    let key = crate::checksum::bytes_checksum(url.as_bytes());
    let asset: String = url
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    dir.join(format!("{}-{asset}", &key[..16]))
}

/// Record that `path`, a cache entry, was just used, so that pruning by
/// age keeps it.
pub fn mark_used(path: &Path) -> Result<()> {
    let file = fs::File::open(path)
        .with_context(|| format!("Failed to open {} to mark it used", path.display()))?;
    file.set_modified(SystemTime::now())
        .with_context(|| format!("Failed to mark {} used", path.display()))
}

/// Every entry of both caches, store first, each sorted by name. A cache
/// that does not exist yet has no entries.
pub fn entries() -> Result<Vec<Entry>> {
    let mut out = store_entries(&crate::tool_store::store_root()?)?;
    out.extend(download_entries(&crate::tool_store::download_cache_root()?)?);
    Ok(out)
}

fn read_dir_sorted(dir: &Path) -> Result<Vec<fs::DirEntry>> {
    let mut entries = match fs::read_dir(dir) {
        Ok(entries) => entries
            .collect::<std::io::Result<Vec<_>>>()
            .with_context(|| format!("Failed to read directory {}", dir.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(e).with_context(|| format!("Failed to read directory {}", dir.display()));
        }
    };
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

fn store_entries(root: &Path) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for method in read_dir_sorted(root)? {
        let method_path = method.path();
        if !method_path.is_dir() {
            continue;
        }
        for pkg in read_dir_sorted(&method_path)? {
            out.push(entry(Kind::Store, root, pkg.path())?);
        }
    }
    Ok(out)
}

fn download_entries(root: &Path) -> Result<Vec<Entry>> {
    read_dir_sorted(root)?
        .into_iter()
        .map(|e| entry(Kind::Downloads, root, e.path()))
        .collect()
}

fn entry(kind: Kind, root: &Path, path: PathBuf) -> Result<Entry> {
    let meta = fs::symlink_metadata(&path)
        .with_context(|| format!("Failed to stat {}", path.display()))?;
    let last_used = meta
        .modified()
        .with_context(|| format!("Failed to read the modification time of {}", path.display()))?;
    let name = path
        .strip_prefix(root)
        .unwrap_or(&path)
        .to_string_lossy()
        .into_owned();
    Ok(Entry {
        kind,
        name,
        bytes: disk_bytes(&path)?,
        path,
        last_used,
    })
}

/// The bytes of the files under `path` (or of `path` itself).
fn disk_bytes(path: &Path) -> Result<u64> {
    let meta =
        fs::symlink_metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;
    if !meta.is_dir() {
        return Ok(meta.len());
    }
    let mut total = 0;
    for child in read_dir_sorted(path)? {
        total += disk_bytes(&child.path())?;
    }
    Ok(total)
}

/// Remove `entry` from its cache.
fn remove(entry: &Entry) -> Result<()> {
    let result = if entry.path.is_dir() {
        fs::remove_dir_all(&entry.path)
    } else {
        fs::remove_file(&entry.path)
    };
    match result {
        Ok(()) => Ok(()),
        // Removed by someone else meanwhile: just as gone.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("Failed to remove {}", entry.path.display())),
    }
}

/// The entries of `all` last used before `now - older_than`.
pub fn stale(all: Vec<Entry>, older_than: Duration, now: SystemTime) -> Vec<Entry> {
    let cutoff = now
        .checked_sub(older_than)
        .unwrap_or(SystemTime::UNIX_EPOCH);
    all.into_iter().filter(|e| e.last_used < cutoff).collect()
}

/// Remove `chosen` (unless `dry_run`) and return them.
pub fn remove_all(chosen: Vec<Entry>, dry_run: bool) -> Result<Vec<Entry>> {
    if !dry_run {
        for entry in &chosen {
            remove(entry)?;
        }
    }
    Ok(chosen)
}

/// `--older-than`: a number of days, written `60d`.
pub fn parse_age(text: &str) -> std::result::Result<Duration, String> {
    let days = text
        .strip_suffix('d')
        .and_then(|n| n.parse::<u64>().ok())
        .ok_or_else(|| format!("expected a number of days such as 60d, got `{text}`"))?;
    Ok(Duration::from_secs(days * 24 * 60 * 60))
}

/// How long ago `t` was, as the list shows it.
pub fn age(t: SystemTime, now: SystemTime) -> String {
    let days = now.duration_since(t).unwrap_or_default().as_secs() / (24 * 60 * 60);
    match days {
        0 => "today".to_string(),
        1 => "1 day ago".to_string(),
        n => format!("{n} days ago"),
    }
}

fn size(bytes: u64) -> String {
    humansize::format_size(bytes, humansize::BINARY)
}

fn entry_json(e: &Entry, now: SystemTime) -> serde_json::Value {
    serde_json::json!({
        "cache": e.kind.name(),
        "entry": e.name,
        "path": e.path,
        "bytes": e.bytes,
        "days_since_used": now.duration_since(e.last_used).unwrap_or_default().as_secs() / 86400,
    })
}

/// Report what a prune or clear removed (or, in a dry run, would remove).
fn report_removed(removed: &[Entry], dry_run: bool, now: SystemTime) {
    let bytes: u64 = removed.iter().map(|e| e.bytes).sum();
    if crate::json_output::is_json_mode() {
        let out = serde_json::json!({
            "dry_run": dry_run,
            "removed": removed.iter().map(|e| entry_json(e, now)).collect::<Vec<_>>(),
            "removed_bytes": bytes,
        });
        println!("{out}");
        return;
    }
    let verb = if dry_run { "Would remove" } else { "Removed" };
    for e in removed {
        println!(
            "{verb} {} {} ({}, last used {})",
            e.kind.name(),
            e.name,
            size(e.bytes),
            age(e.last_used, now)
        );
    }
    let what = if removed.len() == 1 {
        "entry"
    } else {
        "entries"
    };
    crate::output::info(&format!(
        "{verb} {} {what} ({}).",
        removed.len(),
        size(bytes)
    ));
}

/// `rsconstruct tool cache <action>`.
pub fn handle(action: crate::cli::ToolCacheAction) -> Result<()> {
    use crate::cli::ToolCacheAction;
    let now = SystemTime::now();
    match action {
        ToolCacheAction::List => {
            let all = entries()?;
            if crate::json_output::is_json_mode() {
                let out = serde_json::json!({
                    "store": crate::tool_store::store_root()?,
                    "downloads": crate::tool_store::download_cache_root()?,
                    "entries": all.iter().map(|e| entry_json(e, now)).collect::<Vec<_>>(),
                    "bytes": all.iter().map(|e| e.bytes).sum::<u64>(),
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
                return Ok(());
            }
            println!("store:     {}", crate::tool_store::store_root()?.display());
            println!(
                "downloads: {}",
                crate::tool_store::download_cache_root()?.display()
            );
            println!();
            let rows: Vec<Vec<String>> = all
                .iter()
                .map(|e| {
                    vec![
                        e.kind.name().to_string(),
                        e.name.clone(),
                        size(e.bytes),
                        age(e.last_used, now),
                    ]
                })
                .collect();
            let total = all.iter().map(|e| e.bytes).sum();
            let what = if all.len() == 1 { "entry" } else { "entries" };
            crate::tables::print_with_total(
                &crate::tables::defs::TOOL_CACHE,
                false,
                &rows,
                &[
                    "Total".to_string(),
                    format!("{} {what}", all.len()),
                    size(total),
                    String::new(),
                ],
            );
        }
        ToolCacheAction::Prune {
            older_than,
            dry_run,
        } => {
            let removed = remove_all(stale(entries()?, older_than, now), dry_run)?;
            report_removed(&removed, dry_run, now);
        }
        ToolCacheAction::Clear => {
            let removed = remove_all(entries()?, false)?;
            report_removed(&removed, false, now);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_age(path: &Path, days: u64) {
        let t = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
        fs::File::open(path).unwrap().set_modified(t).unwrap();
    }

    /// The entries of both layouts, their sizes, and pruning by last use:
    /// only what is older than the cutoff goes, and a dry run removes
    /// nothing.
    #[test]
    fn entries_are_listed_and_pruned_by_last_use() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("store");
        let old = store.join("binary/uv@sha256-aa");
        let new = store.join("cargo/cargo-deny@0.18.0");
        fs::create_dir_all(old.join("bin")).unwrap();
        fs::create_dir_all(new.join("bin")).unwrap();
        fs::write(old.join("bin/uv"), b"12345").unwrap();
        fs::write(new.join("bin/cargo-deny"), b"123").unwrap();
        set_age(&old, 90);
        let downloads = tmp.path().join("downloads");
        fs::create_dir_all(&downloads).unwrap();
        let asset = download_entry(&downloads, "https://example.com/r/v1/tool.tar.gz?x=1");
        fs::write(&asset, b"1234567").unwrap();
        set_age(&asset, 61);

        let mut all = store_entries(&store).unwrap();
        all.extend(download_entries(&downloads).unwrap());
        let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names[0], "binary/uv@sha256-aa");
        assert_eq!(names[1], "cargo/cargo-deny@0.18.0");
        assert!(names[2].ends_with("-tool.tar.gz"), "{names:?}");
        assert_eq!(
            all.iter().map(|e| e.bytes).collect::<Vec<_>>(),
            vec![5, 3, 7]
        );

        let chosen = stale(all.clone(), parse_age("60d").unwrap(), SystemTime::now());
        let kinds: Vec<Kind> = chosen.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![Kind::Store, Kind::Downloads]);

        remove_all(chosen.clone(), true).unwrap();
        assert!(old.exists() && asset.exists(), "a dry run removes nothing");
        remove_all(chosen, false).unwrap();
        assert!(!old.exists() && !asset.exists());
        assert!(new.exists(), "a recently used entry stays");

        // Marking an entry used makes it recent again.
        set_age(&new, 400);
        mark_used(&new).unwrap();
        let left = stale(
            store_entries(&store).unwrap(),
            parse_age("1d").unwrap(),
            SystemTime::now(),
        );
        assert!(left.is_empty());
    }

    #[test]
    fn ages_are_whole_days() {
        assert_eq!(parse_age("60d").unwrap(), Duration::from_hours(60 * 24));
        assert!(parse_age("60").is_err());
        assert!(parse_age("2w").is_err());
        let now = SystemTime::now();
        assert_eq!(age(now, now), "today");
        assert_eq!(age(now - Duration::from_hours(24), now), "1 day ago");
        assert_eq!(age(now - Duration::from_hours(5 * 24), now), "5 days ago");
    }

    /// The asset's name is kept, made safe for a file name; the key comes
    /// from the whole URL.
    #[test]
    fn download_entries_are_named_after_the_asset() {
        let dir = Path::new("/c");
        let a = download_entry(dir, "https://h/x/v1/zizmor-x86_64.tar.gz");
        let b = download_entry(dir, "https://h/x/v2/zizmor-x86_64.tar.gz");
        assert!(a.to_string_lossy().ends_with("-zizmor-x86_64.tar.gz"));
        assert_ne!(a, b);
        let odd = download_entry(dir, "https://h/a%40b c");
        assert!(
            odd.to_string_lossy().ends_with("-a_40b_c"),
            "{}",
            odd.display()
        );
    }
}
