//! `rsconstruct tool cache`: look at and prune the two machine-wide tool
//! caches, the tool store ([`crate::tool_store::store_root`]) and the
//! download cache ([`crate::tool_store::download_cache_root`]).
//!
//! An entry is one store install (`<method>/<name>@<version>`), one
//! downloaded asset, or one pin record: what was learned from an asset
//! ([`PinRecord`], `<asset entry>.pin`). Its last use is its modification
//! time: a store entry's is refreshed whenever a project links it, a
//! download's or a record's whenever an install or `tool lock` is served
//! from it ([`mark_used`]). Deleting any entry is safe: a project holds
//! hard links to its tools, and whatever is missing is installed or
//! downloaded again when next needed.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// What a download of a release asset showed: the binary it is for and
/// the lock entry it makes (its hashes, or a `.deb`'s package and
/// version). Kept beside the asset in the download cache, so that `tool
/// lock` can record the release without the asset, and the asset can go
/// once the tool store holds what it unpacks to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinRecord {
    pub name: String,
    pub pin: crate::tool_lock::BinaryPin,
}

/// The suffix a pin record adds to its asset's entry name.
const PIN_SUFFIX: &str = ".pin";

/// The file the pin record of `url` is kept in, under the cache directory
/// `dir`: the asset's entry with [`PIN_SUFFIX`].
pub fn pin_record_path(dir: &Path, url: &str) -> PathBuf {
    let mut name = download_entry(dir, url).into_os_string();
    name.push(PIN_SUFFIX);
    PathBuf::from(name)
}

/// The pin record of `url`, if the cache has one.
pub fn read_pin_record(dir: &Path, url: &str) -> Result<Option<PinRecord>> {
    read_pin_record_at(&pin_record_path(dir, url))
}

fn read_pin_record_at(path: &Path) -> Result<Option<PinRecord>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("Failed to read {}", path.display())),
    };
    toml::from_str(&text)
        .map(Some)
        .with_context(|| format!("Failed to parse the pin record {}", path.display()))
}

/// Keep `record` in the cache directory `dir`. Written beside its final
/// name and renamed into place, like an asset, so a reader sees the whole
/// record or none.
pub fn write_pin_record(dir: &Path, record: &PinRecord) -> Result<()> {
    let path = pin_record_path(dir, &record.pin.url);
    fs::create_dir_all(dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    let text = toml::to_string(record).context("Failed to serialize a pin record")?;
    let name = path
        .file_name()
        .context("a pin record always has a name")?
        .to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    fs::write(&tmp, text).with_context(|| format!("Failed to write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| {
        format!(
            "Failed to move {} into place as {}",
            tmp.display(),
            path.display()
        )
    })
}

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

/// What the projects under some directories use: the store entries their
/// tool directories link or their locks name, and the release assets their
/// locks name.
#[derive(Debug, Default)]
pub struct References {
    /// Store entry names (`<method>/<name>@<version>`).
    store: BTreeSet<String>,
    /// The URLs of the release assets the locks pin.
    urls: BTreeSet<String>,
}

/// The references of every project (a directory with an rsconstruct.lock)
/// under `roots`. Hidden directories, `node_modules` and `target` are not
/// searched: none holds a project of its own.
pub fn references(roots: &[PathBuf]) -> Result<References> {
    let mut refs = References::default();
    for root in roots {
        if !root.is_dir() {
            anyhow::bail!("{} is not a directory", root.display());
        }
        add_projects_under(&mut refs, root)?;
    }
    Ok(refs)
}

fn add_projects_under(refs: &mut References, dir: &Path) -> Result<()> {
    if dir.join(crate::tool_lock::LOCK_FILE).is_file() {
        add_project(refs, dir)?;
    }
    for child in read_dir_sorted(dir)? {
        let name = child.file_name();
        let name = name.to_string_lossy();
        let file_type = child
            .file_type()
            .with_context(|| format!("Failed to stat {}", child.path().display()))?;
        if file_type.is_dir()
            && !name.starts_with('.')
            && name != "node_modules"
            && name != "target"
        {
            add_projects_under(refs, &child.path())?;
        }
    }
    Ok(())
}

/// The store entry name of a package.
fn store_name(method: &str, name: &str, version: &str) -> String {
    let key = crate::tool_store::Package::new(method, name, version).key();
    format!("{method}/{key}")
}

fn add_project(refs: &mut References, project: &Path) -> Result<()> {
    for ((method, name), versions) in crate::tool_store::installed_versions(project) {
        for version in versions {
            refs.store.insert(store_name(&method, &name, &version));
        }
    }
    let lock = crate::tool_lock::read_lock_at(&project.join(crate::tool_lock::LOCK_FILE))?;
    for (method, versions) in [
        ("cargo", &lock.cargo),
        ("gem", &lock.gem),
        ("npm", &lock.npm),
    ] {
        for (name, version) in versions {
            refs.store.insert(store_name(method, name, version));
        }
    }
    for (name, pin) in &lock.binary {
        if let Some(version) = crate::tool_lock::binary_version(pin) {
            refs.store.insert(store_name("binary", name, &version));
        }
        refs.urls.insert(pin.url.clone());
    }
    Ok(())
}

/// What a prune removes: the entries that meet every criterion given.
pub struct Criteria {
    /// Last used longer ago than this.
    pub older_than: Option<Duration>,
    /// Used by none of the projects these references come from.
    pub unreferenced: Option<References>,
    /// A downloaded asset whose pin record says the tool store holds what
    /// it unpacks to. Its record stays, so `tool lock` still needs no
    /// download.
    pub installed_archives: bool,
}

/// The entries of `all` that `criteria` removes. `downloads` and `store`
/// are the roots of the two caches.
pub fn chosen(
    all: Vec<Entry>,
    criteria: &Criteria,
    now: SystemTime,
    downloads: &Path,
    store: &Path,
) -> Result<Vec<Entry>> {
    let cutoff = criteria
        .older_than
        .map(|age| now.checked_sub(age).unwrap_or(SystemTime::UNIX_EPOCH));
    let referenced_downloads: BTreeSet<String> = criteria
        .unreferenced
        .iter()
        .flat_map(|refs| refs.urls.iter())
        .flat_map(|url| {
            [
                download_entry(downloads, url),
                pin_record_path(downloads, url),
            ]
        })
        .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    let mut out = Vec::new();
    for entry in all {
        if cutoff.is_some_and(|cutoff| entry.last_used >= cutoff) {
            continue;
        }
        if let Some(refs) = &criteria.unreferenced {
            let used = match entry.kind {
                Kind::Store => refs.store.contains(&entry.name),
                Kind::Downloads => referenced_downloads.contains(&entry.name),
            };
            if used {
                continue;
            }
        }
        if criteria.installed_archives && !is_installed_archive(&entry, store)? {
            continue;
        }
        out.push(entry);
    }
    Ok(out)
}

/// Whether `entry` is a downloaded asset whose pin record names an
/// executable the tool store holds.
fn is_installed_archive(entry: &Entry, store: &Path) -> Result<bool> {
    if entry.kind != Kind::Downloads || entry.name.ends_with(PIN_SUFFIX) {
        return Ok(false);
    }
    let mut record_path = entry.path.clone().into_os_string();
    record_path.push(PIN_SUFFIX);
    let Some(record) = read_pin_record_at(Path::new(&record_path))? else {
        return Ok(false);
    };
    let Some(version) = crate::tool_lock::binary_version(&record.pin) else {
        return Ok(false);
    };
    Ok(store
        .join(store_name("binary", &record.name, &version))
        .is_dir())
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
            unreferenced_under,
            installed_archives,
            dry_run,
        } => {
            let criteria = Criteria {
                older_than,
                unreferenced: if unreferenced_under.is_empty() {
                    None
                } else {
                    Some(references(&unreferenced_under)?)
                },
                installed_archives,
            };
            let chosen = chosen(
                entries()?,
                &criteria,
                now,
                &crate::tool_store::download_cache_root()?,
                &crate::tool_store::store_root()?,
            )?;
            let removed = remove_all(chosen, dry_run)?;
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

        let chosen = by_age(all.clone(), "60d", &downloads, &store);
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
        let left = by_age(store_entries(&store).unwrap(), "1d", &downloads, &store);
        assert!(left.is_empty());
    }

    fn by_age(all: Vec<Entry>, age: &str, downloads: &Path, store: &Path) -> Vec<Entry> {
        let criteria = Criteria {
            older_than: Some(parse_age(age).unwrap()),
            unreferenced: None,
            installed_archives: false,
        };
        chosen(all, &criteria, SystemTime::now(), downloads, store).unwrap()
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    fn pin(url: &str, exe: &str) -> crate::tool_lock::BinaryPin {
        crate::tool_lock::BinaryPin {
            url: url.to_string(),
            sha256: "d".repeat(64),
            executable_sha256: Some(exe.to_string()),
            deb_package: None,
            deb_version: None,
        }
    }

    /// A record reads back as written, sits beside its asset's entry, and
    /// is absent until written.
    #[test]
    fn pin_records_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let url = "https://h/o/r/releases/download/v1/tool.tar.gz";
        assert!(read_pin_record(tmp.path(), url).unwrap().is_none());
        let record = PinRecord {
            name: "tool".to_string(),
            pin: pin(url, "ab"),
        };
        write_pin_record(tmp.path(), &record).unwrap();
        assert_eq!(read_pin_record(tmp.path(), url).unwrap(), Some(record));
        let path = pin_record_path(tmp.path(), url);
        assert_eq!(
            path.to_string_lossy(),
            format!("{}.pin", download_entry(tmp.path(), url).display())
        );
        let names: Vec<String> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 1, "no temporary file is left: {names:?}");
    }

    /// Pruning by reference keeps what a project's tool directory links or
    /// its lock names, under any directory searched, and removes the rest;
    /// hidden directories are not searched.
    #[test]
    fn unreferenced_entries_are_pruned() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("store");
        let downloads = tmp.path().join("downloads");
        for dir in [
            "binary/uv@sha256-aa",
            "binary/uv@sha256-old",
            "cargo/mdbook@0.5.0",
            "cargo/mdbook@0.4.0",
            "npm/@scope+pkg@1.0.0",
            "gem/mdl@0.13.0",
        ] {
            fs::create_dir_all(store.join(dir)).unwrap();
        }
        fs::create_dir_all(&downloads).unwrap();
        let used_url = "https://h/uv/releases/download/v2/uv.tar.gz";
        let old_url = "https://h/uv/releases/download/v1/uv.tar.gz";
        for url in [used_url, old_url] {
            fs::write(download_entry(&downloads, url), b"x").unwrap();
            write_pin_record(
                &downloads,
                &PinRecord {
                    name: "uv".to_string(),
                    pin: pin(url, "aa"),
                },
            )
            .unwrap();
        }
        // A project locking uv (binary) and mdbook 0.5.0, and linking an
        // npm package installed from its package-lock.json.
        let git = tmp.path().join("git");
        let project = git.join("a");
        fs::create_dir_all(project.join(".rsconstruct/tools/pkgs/npm/@scope+pkg@1.0.0")).unwrap();
        fs::write(
            project.join(crate::tool_lock::LOCK_FILE),
            format!(
                "version = 1\n[cargo]\nmdbook = \"0.5.0\"\n[binary.uv]\nurl = \"{used_url}\"\nsha256 = \"{}\"\nexecutable_sha256 = \"aa\"\n",
                "d".repeat(64)
            ),
        )
        .unwrap();
        // A project in a hidden directory does not count.
        let hidden = git.join(".hidden");
        fs::create_dir_all(&hidden).unwrap();
        fs::write(
            hidden.join(crate::tool_lock::LOCK_FILE),
            "version = 1\n[gem]\nmdl = \"0.13.0\"\n",
        )
        .unwrap();

        let mut all = store_entries(&store).unwrap();
        all.extend(download_entries(&downloads).unwrap());
        let criteria = Criteria {
            older_than: None,
            unreferenced: Some(references(&[git]).unwrap()),
            installed_archives: false,
        };
        let removed = chosen(all, &criteria, SystemTime::now(), &downloads, &store).unwrap();
        let old_asset = download_entry(&downloads, old_url);
        let old_asset = old_asset.file_name().unwrap().to_string_lossy();
        assert_eq!(
            names(&removed),
            vec![
                "binary/uv@sha256-old",
                "cargo/mdbook@0.4.0",
                "gem/mdl@0.13.0",
                &*old_asset,
                &format!("{old_asset}.pin"),
            ]
        );
        assert!(references(&[tmp.path().join("missing")]).is_err());
    }

    /// An archive goes when its record names an executable the store
    /// holds; its record, an archive without a record, and one whose tool
    /// the store lacks stay.
    #[test]
    fn installed_archives_are_pruned_and_their_records_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("store");
        let downloads = tmp.path().join("downloads");
        fs::create_dir_all(store.join("binary/uv@sha256-aa")).unwrap();
        fs::create_dir_all(&downloads).unwrap();
        let installed = "https://h/uv/releases/download/v2/uv.tar.gz";
        let missing = "https://h/uv/releases/download/v1/uv.tar.gz";
        let unrecorded = "https://h/x/releases/download/v1/x";
        for (url, exe) in [(installed, "aa"), (missing, "bb")] {
            write_pin_record(
                &downloads,
                &PinRecord {
                    name: "uv".to_string(),
                    pin: pin(url, exe),
                },
            )
            .unwrap();
        }
        for url in [installed, missing, unrecorded] {
            fs::write(download_entry(&downloads, url), b"x").unwrap();
        }
        let criteria = Criteria {
            older_than: None,
            unreferenced: None,
            installed_archives: true,
        };
        let all = download_entries(&downloads).unwrap();
        let removed = chosen(all, &criteria, SystemTime::now(), &downloads, &store).unwrap();
        let expected = download_entry(&downloads, installed);
        assert_eq!(
            names(&removed),
            vec![&*expected.file_name().unwrap().to_string_lossy()]
        );
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
