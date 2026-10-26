//! `rsconstruct.lock`: the exact versions of the external tools a project
//! builds with, the way `Cargo.lock` and `uv.lock` record its libraries.
//!
//! `rsconstruct.toml` names tools by bare name — a processor's tools, the
//! `[dependencies]` lists — and a bare name alone would let every CI runner
//! install whatever release was newest that day. This file records which
//! release the project builds with. It is written by `rsconstruct tool
//! lock`, read by `tool install`, `tool install-deps` and `tool check`, and
//! checked by `build` in CI (for the processors that build; see
//! [`Coverage`]). rsconstruct itself is not locked: the fleet's
//! `scripts/ci-install-tools.sh` downloads the latest release (only the last
//! few releases stay published, so a pinned one would vanish from under CI,
//! and every release would have meant a fleet-wide lock bump). Locks written
//! before 2026-10-26 carry an `[rsconstruct]` table; it is accepted so they
//! keep parsing, ignored, and left out of the next rewrite.
//!
//! What is locked is what rsconstruct itself installs from a registry:
//! crates (`cargo`), `PyPI` packages (`pip`), global npm packages, gems, and
//! the release assets of the `binary` install method (by URL and SHA-256).
//! Packages a project lockfile already pins (uv.lock, package-lock.json)
//! belong to that file; system packages (apt and friends) come with the OS
//! image and are not locked.
//!
//! Outside CI a missing or incomplete lock is filled in (only the missing
//! entries are resolved; nothing already locked moves). In CI (`CI=true`)
//! it is an error: a lock written on a runner is never committed, so each
//! runner would resolve its own versions. Moving locked entries to newer
//! releases is `rsconstruct tool lock --upgrade` and nothing else.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::build_context::BuildContext;
use crate::processor::ProcessorMap;

pub const LOCK_FILE: &str = "rsconstruct.lock";
/// The file `rsconstruct tool lock` wrote before rsconstruct.lock existed.
/// It recorded machine-local tool paths, so it cannot be converted; finding
/// one is an error that says to delete it.
pub const LEGACY_LOCK_FILE: &str = ".tools.versions";
const LOCK_VERSION: u32 = 1;

const LOCK_HEADER: &str = "\
# rsconstruct.lock: the exact tool versions this project builds with.
# Written by `rsconstruct tool lock`; move to newer releases with
# `rsconstruct tool lock --upgrade`. Commit this file; do not edit it by hand.
";

/// The registry-backed install methods whose versions the lock records,
/// besides `binary`.
const VERSIONED_METHODS: [&str; 4] = ["cargo", "gem", "npm", "pip"];

/// The parsed contents of rsconstruct.lock.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolLock {
    pub version: u32,
    /// The `[rsconstruct]` table of a lock written before 2026-10-26, when
    /// `tool lock` recorded the release it ran as for CI to download. Read
    /// so a committed lock keeps parsing under `deny_unknown_fields`, never
    /// consulted, and never written: a rewrite drops it (see
    /// [`describe_changes`]).
    #[serde(default, skip_serializing)]
    pub rsconstruct: Option<LegacySelfPin>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cargo: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub gem: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub npm: BTreeMap<String, String>,
    /// Keyed by PEP 503-normalized distribution name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pip: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub binary: BTreeMap<String, BinaryPin>,
}

/// The body of the legacy `[rsconstruct]` table: the release `tool lock` ran
/// as. Only ever parsed, see [`ToolLock::rsconstruct`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacySelfPin {
    pub version: String,
}

/// One `binary` recipe, pinned: the exact asset to download, the hash it
/// must have, and how to recognize the installed result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinaryPin {
    pub url: String,
    /// SHA-256 of the downloaded file (the archive, for an archive).
    pub sha256: String,
    /// SHA-256 of the installed executable, for every recipe but a `.deb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_sha256: Option<String>,
    /// A `.deb`'s package name and version, as dpkg will report them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deb_package: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deb_version: Option<String>,
}

impl ToolLock {
    fn empty() -> Self {
        Self {
            version: LOCK_VERSION,
            ..Self::default()
        }
    }

    fn versions(&self, method: &str) -> &BTreeMap<String, String> {
        match method {
            "cargo" => &self.cargo,
            "gem" => &self.gem,
            "npm" => &self.npm,
            "pip" | "uv" => &self.pip,
            other => unreachable!("not a versioned lock method: {other}"),
        }
    }

    fn versions_mut(&mut self, method: &str) -> &mut BTreeMap<String, String> {
        match method {
            "cargo" => &mut self.cargo,
            "gem" => &mut self.gem,
            "npm" => &mut self.npm,
            "pip" | "uv" => &mut self.pip,
            other => unreachable!("not a versioned lock method: {other}"),
        }
    }

    /// `package` as an install request for `method`, pinned to its locked
    /// version: `name@1.2.3` for cargo and npm, `name==1.2.3` for pip/uv
    /// (extras kept), `name:1.2.3` for gem. Returned unchanged when the
    /// lock has no entry for it, or when the entry already carries its own
    /// version constraint, which then stands as written.
    pub fn pinned(&self, method: &str, package: &str) -> String {
        if !VERSIONED_METHODS.contains(&method) && method != "uv" {
            return package.to_string();
        }
        if !is_bare(method, package) {
            return package.to_string();
        }
        let Some(version) = self.versions(method).get(&lock_key(method, package)) else {
            return package.to_string();
        };
        match method {
            "pip" | "uv" => format!("{package}=={version}"),
            "gem" => format!("{package}:{version}"),
            _ => format!("{package}@{version}"),
        }
    }
}

/// The key `package` is locked under: the PEP 503-normalized name for a
/// Python package (extras dropped), the name itself otherwise.
fn lock_key(method: &str, package: &str) -> String {
    match method {
        "pip" | "uv" => crate::config::normalized_distribution_name(package),
        _ => package.to_string(),
    }
}

/// Whether `package` is a bare name the lock can pin, rather than a request
/// that already says which version it wants (`ruff>=0.5`, `pkg@1.0`,
/// `rake:13.0`).
fn is_bare(method: &str, package: &str) -> bool {
    match method {
        "pip" | "uv" => !package.contains(['<', '>', '=', '!', '~', ';', '@', ' ']),
        // A scoped package starts with '@'; only a later '@' is a version.
        "npm" => package.rfind('@').is_none_or(|i| i == 0),
        "gem" => !package.contains(':'),
        "cargo" => !package.contains('@'),
        _ => false,
    }
}

/// Everything a project's lock must hold: the tools its configured
/// processors install from a registry and its `[dependencies]` packages,
/// by install method.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub cargo: BTreeSet<String>,
    pub gem: BTreeSet<String>,
    pub npm: BTreeSet<String>,
    /// PEP 503-normalized distribution names.
    pub pip: BTreeSet<String>,
    pub binary: BTreeSet<String>,
}

impl Wanted {
    fn names(&self, method: &str) -> &BTreeSet<String> {
        match method {
            "cargo" => &self.cargo,
            "gem" => &self.gem,
            "npm" => &self.npm,
            "pip" => &self.pip,
            "binary" => &self.binary,
            other => unreachable!("not a lock method: {other}"),
        }
    }

    /// Record `package` under `method` when the lock can pin it. Returns
    /// whether it was recorded.
    fn add(&mut self, method: &str, package: &str) -> bool {
        let set = match method {
            "cargo" => &mut self.cargo,
            "gem" => &mut self.gem,
            "npm" => &mut self.npm,
            "pip" | "uv" => &mut self.pip,
            "binary" => {
                if !crate::tools::binary_is_pinnable(package) {
                    return false;
                }
                self.binary.insert(package.to_string());
                return true;
            }
            _ => return false,
        };
        if !is_bare(method, package) {
            return false;
        }
        set.insert(lock_key(method, package));
        true
    }
}

/// Collect what the project's lock must hold.
///
/// Processor tools are taken by the install method `tool install` uses for
/// them (the registry's first). A processor tool that a project lockfile
/// already pins — a Python package in uv.lock, a Node package in
/// package-lock.json — is left to that file: install-deps installs it from
/// there, and a second pin here could only disagree with it.
/// `[dependencies]` entries are always included; install-deps installs them
/// with these pins.
pub fn collect_wanted(
    processors: &ProcessorMap,
    enabled: &dyn Fn(&str) -> bool,
    deps: &crate::config::DependenciesConfig,
    root: &Path,
) -> Result<Wanted> {
    let LockfileOwned {
        pip: uv_owned,
        npm: npm_owned,
    } = lockfile_owned(deps, root)?;

    let mut wanted = Wanted::default();
    let mut names: Vec<&String> = processors.keys().collect();
    names.sort();
    for name in names {
        if !enabled(name) {
            continue;
        }
        for tool in processors[name].required_tools() {
            // A repo-local script (`scripts/check.py`) is not installed.
            if tool.contains('/') {
                continue;
            }
            let Some(method) = crate::tools::tool_info(&tool)
                .and_then(crate::tools::ToolInfo::default_install_method)
            else {
                continue;
            };
            let key = lock_key(method.method, method.package);
            let owned = match method.method {
                "pip" => uv_owned.contains(&key),
                "npm" => npm_owned.contains(&key),
                _ => false,
            };
            if !owned {
                wanted.add(method.method, method.package);
            }
        }
    }
    for (method, list) in [
        ("cargo", &deps.cargo),
        ("gem", &deps.gem),
        ("npm", &deps.npm),
        ("pip", &deps.pip),
    ] {
        for package in list {
            wanted.add(method, package);
        }
    }
    Ok(wanted)
}

/// The packages a project lockfile pins, which rsconstruct.lock therefore
/// leaves alone.
#[derive(Debug, Default, PartialEq, Eq)]
struct LockfileOwned {
    /// PEP 503-normalized names pinned by uv.lock.
    pip: BTreeSet<String>,
    /// Names pinned by package-lock.json.
    npm: BTreeSet<String>,
}

/// What uv.lock and package-lock.json own, each only when install-deps
/// actually installs from it: a lockfile that is on disk but not the
/// configured source (`pip_source = "pyproject"`, `npm_source =
/// "package-json"`) pins nothing, so its packages must be locked here or
/// they float.
fn lockfile_owned(deps: &crate::config::DependenciesConfig, root: &Path) -> Result<LockfileOwned> {
    let mut owned = LockfileOwned::default();
    let uv_lock = root.join("uv.lock");
    if deps.pip_source == crate::config::PipSource::UvLock && uv_lock.exists() {
        owned.pip = crate::config::uv_lock_pinned_deps(&uv_lock)?
            .iter()
            .map(|req| crate::config::normalized_distribution_name(req))
            .collect();
    }
    let package_lock = root.join("package-lock.json");
    if deps.npm_source == crate::config::NpmSource::PackageLock && package_lock.exists() {
        owned.npm = crate::config::package_lock_pins(&package_lock)?
            .into_keys()
            .collect();
    }
    Ok(owned)
}

/// Fail when the pre-rsconstruct.lock file is still around.
fn reject_legacy_lock() -> Result<()> {
    if Path::new(LEGACY_LOCK_FILE).exists() {
        return Err(crate::exit_code::config_error(format!(
            "{LEGACY_LOCK_FILE} is no longer read: rsconstruct.lock replaced it, and its \
             machine-local tool paths cannot be converted. Delete {LEGACY_LOCK_FILE} and run \
             `rsconstruct tool lock`."
        )));
    }
    Ok(())
}

/// Read rsconstruct.lock. Returns None if it doesn't exist.
pub fn read_lock_file() -> Result<Option<ToolLock>> {
    let path = Path::new(LOCK_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    parse_lock(&content).map(Some)
}

fn parse_lock(content: &str) -> Result<ToolLock> {
    let lock: ToolLock = toml::from_str(content)
        .map_err(|e| crate::exit_code::config_error(format!("Failed to parse {LOCK_FILE}: {e}")))?;
    if lock.version != LOCK_VERSION {
        return Err(crate::exit_code::config_error(format!(
            "{LOCK_FILE} has format version {}, but this rsconstruct reads version \
             {LOCK_VERSION}",
            lock.version
        )));
    }
    Ok(lock)
}

fn render_lock(lock: &ToolLock) -> Result<String> {
    let body = toml::to_string(lock).context("Failed to serialize rsconstruct.lock")?;
    Ok(format!("{LOCK_HEADER}\n{body}"))
}

/// Write rsconstruct.lock atomically (temp file + rename), so a crash or
/// concurrent reader never observes a truncated lock.
fn write_lock_file(lock: &ToolLock) -> Result<()> {
    let path = Path::new(LOCK_FILE);
    let text = render_lock(lock)?;
    let tmp = format!("{LOCK_FILE}.tmp-{}", std::process::id());
    fs::write(&tmp, text).with_context(|| format!("Failed to write {tmp}"))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("Failed to move {tmp} into place as {}", path.display()))?;
    Ok(())
}

/// Lock entries as `(method, name)` pairs.
type Entries = Vec<(String, String)>;

/// The entries `wanted` needs that `lock` lacks, and the ones `lock` holds
/// that nothing needs any more.
fn diff(lock: &ToolLock, wanted: &Wanted) -> (Entries, Entries) {
    let mut missing = Vec::new();
    let mut stale = Vec::new();
    for method in VERSIONED_METHODS {
        let locked = lock.versions(method);
        let names = wanted.names(method);
        missing.extend(
            names
                .iter()
                .filter(|n| !locked.contains_key(*n))
                .map(|n| (method.to_string(), n.clone())),
        );
        stale.extend(
            locked
                .keys()
                .filter(|n| !names.contains(*n))
                .map(|n| (method.to_string(), n.clone())),
        );
    }
    missing.extend(
        wanted
            .binary
            .iter()
            .filter(|n| !lock.binary.contains_key(*n))
            .map(|n| ("binary".to_string(), n.clone())),
    );
    stale.extend(
        lock.binary
            .keys()
            .filter(|n| !wanted.binary.contains(*n))
            .map(|n| ("binary".to_string(), n.clone())),
    );
    (missing, stale)
}

/// The latest release of `name` in `method`'s registry.
fn resolve_version(method: &str, name: &str) -> Result<String> {
    let (url, pointer) = match method {
        "cargo" => (
            format!("https://crates.io/api/v1/crates/{name}"),
            "/crate/max_stable_version",
        ),
        "gem" => (
            format!("https://rubygems.org/api/v1/versions/{name}/latest.json"),
            "/version",
        ),
        // A scoped name's '/' is part of the package, not a path step.
        "npm" => (
            format!(
                "https://registry.npmjs.org/{}/latest",
                name.replace('/', "%2F")
            ),
            "/version",
        ),
        "pip" => (
            format!("https://pypi.org/pypi/{name}/json"),
            "/info/version",
        ),
        other => unreachable!("not a versioned lock method: {other}"),
    };
    let json = crate::download::get_json(&url)?;
    json.pointer(pointer)
        .and_then(serde_json::Value::as_str)
        .filter(|v| !v.is_empty() && *v != "unknown")
        .map(str::to_string)
        .with_context(|| format!("{url} names no released version of {method} package {name}"))
}

/// Fill `lock`'s entry for `(method, name)` from the registry.
fn resolve_into(
    ctx: &BuildContext,
    lock: &mut ToolLock,
    method: &str,
    name: &str,
    verbose: bool,
) -> Result<()> {
    if verbose {
        println!("Resolving {method} {name}");
    }
    if method == "binary" {
        let pin = crate::tools::lock_binary(ctx, name, verbose)
            .with_context(|| format!("Failed to lock binary {name}"))?;
        lock.binary.insert(name.to_string(), pin);
    } else {
        let version = resolve_version(method, name)
            .with_context(|| format!("Failed to lock {method} package {name}"))?;
        lock.versions_mut(method).insert(name.to_string(), version);
    }
    Ok(())
}

/// How much of the lock a check against it judges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// The lock holds exactly what is wanted: nothing missing, nothing
    /// unused. `tool check` and the installs in CI, which see the whole
    /// project.
    Exact,
    /// The lock holds at least what is wanted; what else it holds is not
    /// judged. The `build` check, which sees only the processors with
    /// products in the graph (after `-p`/`-x`): an entry that selection does
    /// not need may still be needed by a processor outside it.
    AtLeast,
}

/// The committed lock, as CI reads it: it must exist and cover `wanted` to
/// the degree `coverage` asks.
fn read_locked(wanted: &Wanted, coverage: Coverage) -> Result<ToolLock> {
    reject_legacy_lock()?;
    let Some(lock) = read_lock_file()? else {
        return Err(lock_error(format!(
            "No {LOCK_FILE} found. Run `rsconstruct tool lock` outside CI and commit it."
        )));
    };
    let (missing, stale) = diff(&lock, wanted);
    let mut problems: Vec<String> = Vec::new();
    problems.extend(
        missing
            .iter()
            .map(|(m, n)| format!("{m} {n} — needed but not locked")),
    );
    if coverage == Coverage::Exact {
        problems.extend(
            stale
                .iter()
                .map(|(m, n)| format!("{m} {n} — locked but no longer needed")),
        );
    }
    if !problems.is_empty() {
        return Err(lock_error(format!(
            "{LOCK_FILE} does not match this project (run `rsconstruct tool lock` \
             outside CI and commit the result):\n{}",
            problems.join("\n")
        )));
    }
    Ok(lock)
}

/// How a command treats a lock that is missing or does not cover the
/// project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    /// The lock must exist and cover the project exactly; anything else is
    /// an error. CI.
    Locked,
    /// Resolve and add what is missing, drop what nothing needs; never move
    /// an entry that is already locked. Outside CI.
    Fill,
    /// Resolve every entry afresh. `tool lock --upgrade`; the named form
    /// is [`upgrade_named`].
    Upgrade,
}

impl LockMode {
    /// The mode for installs and builds: Locked in CI, Fill elsewhere.
    pub fn for_environment() -> Self {
        if crate::config::running_in_ci() {
            Self::Locked
        } else {
            Self::Fill
        }
    }
}

/// The lock after `sync`, and what syncing changed in it.
pub struct Synced {
    pub lock: ToolLock,
    /// No lock existed before; this one was just written.
    pub created: bool,
    /// One `+ method name version`, `- method name` or
    /// `~ method name old -> new` line per change.
    pub changes: Vec<String>,
}

impl Synced {
    /// Print what changed, unless nothing did. A lock that was just created
    /// is always announced, even one with no entries (a project whose tools
    /// are all apt packages): the file is new and must be committed.
    pub fn report(&self) {
        if self.changes.is_empty() && !self.created {
            return;
        }
        for line in &self.changes {
            println!("  {line}");
        }
        if self.created {
            println!("Created {LOCK_FILE}; commit it.");
        } else {
            println!("Updated {LOCK_FILE}; commit it.");
        }
    }
}

fn entry_label(lock: &ToolLock, method: &str, name: &str) -> String {
    if method == "binary" {
        lock.binary
            .get(name)
            .map_or_else(String::new, |p| p.url.clone())
    } else {
        lock.versions(method).get(name).cloned().unwrap_or_default()
    }
}

/// Bring rsconstruct.lock in line with `wanted` according to `mode`, and
/// return it.
pub fn sync(ctx: &BuildContext, wanted: &Wanted, mode: LockMode, verbose: bool) -> Result<Synced> {
    if mode == LockMode::Locked {
        let lock = read_locked(wanted, Coverage::Exact)?;
        return Ok(Synced {
            lock,
            created: false,
            changes: Vec::new(),
        });
    }

    reject_legacy_lock()?;
    let existing = read_lock_file()?;
    let upgrade_all = mode == LockMode::Upgrade;
    rewrite(ctx, wanted, existing, upgrade_all, &[], verbose)
}

/// `tool lock --upgrade <name>...`: everything `tool lock` does, plus
/// re-resolve the entries `names` select to their latest release. A name
/// selects that tool under every install method the project locks it for.
/// A name the project does not lock is an error, not a no-op: a typo must
/// not look like "already the latest" (and `rsconstruct` is such a name —
/// rsconstruct itself is not locked).
pub fn upgrade_named(
    ctx: &BuildContext,
    wanted: &Wanted,
    names: &[String],
    verbose: bool,
) -> Result<Synced> {
    reject_legacy_lock()?;
    let targets = select_entries(wanted, names)?;
    let existing = read_lock_file()?;
    rewrite(ctx, wanted, existing, false, &targets, verbose)
}

/// The `(method, name)` entries `names` select from `wanted`.
fn select_entries(wanted: &Wanted, names: &[String]) -> Result<Entries> {
    let mut targets = Vec::new();
    let mut unknown = Vec::new();
    for name in names {
        let before = targets.len();
        for method in VERSIONED_METHODS.iter().copied().chain(["binary"]) {
            let key = lock_key(method, name);
            if wanted.names(method).contains(&key) {
                targets.push((method.to_string(), key));
            }
        }
        if targets.len() == before {
            unknown.push(name.as_str());
        }
    }
    if !unknown.is_empty() {
        return Err(crate::exit_code::config_error(format!(
            "not a tool this project locks: {} (see the entries in {LOCK_FILE})",
            unknown.join(", ")
        )));
    }
    Ok(targets)
}

/// Write the lock for `wanted`, starting from `existing`: drop unused
/// entries, resolve missing ones, and re-resolve `reresolve` (or, with
/// `fresh`, every entry). A legacy `[rsconstruct]` table is dropped.
fn rewrite(
    ctx: &BuildContext,
    wanted: &Wanted,
    existing: Option<ToolLock>,
    fresh: bool,
    reresolve: &[(String, String)],
    verbose: bool,
) -> Result<Synced> {
    let created = existing.is_none();
    let before = existing.unwrap_or_else(ToolLock::empty);
    let mut lock = if fresh {
        ToolLock::empty()
    } else {
        before.clone()
    };
    for (method, name) in reresolve {
        if method == "binary" {
            lock.binary.remove(name);
        } else {
            lock.versions_mut(method).remove(name);
        }
    }
    let (missing, stale) = diff(&lock, wanted);
    for (method, name) in &stale {
        if method == "binary" {
            lock.binary.remove(name);
        } else {
            lock.versions_mut(method).remove(name);
        }
    }
    for (method, name) in &missing {
        resolve_into(ctx, &mut lock, method, name, verbose)?;
    }
    lock.rsconstruct = None;

    let changes = describe_changes(&before, &lock);
    if created || lock != before {
        write_lock_file(&lock)?;
    }
    Ok(Synced {
        lock,
        created,
        changes,
    })
}

/// One line per entry that differs between `before` and `after`.
fn describe_changes(before: &ToolLock, after: &ToolLock) -> Vec<String> {
    let mut changes = Vec::new();
    // Dropping the legacy table is a change the user must see: without this
    // line a rewrite that did nothing else would be reported as "up to
    // date" while the file on disk changed.
    if let (Some(old), None) = (&before.rsconstruct, &after.rsconstruct) {
        changes.push(format!(
            "- rsconstruct {} (rsconstruct itself is no longer locked; CI downloads the latest release)",
            old.version
        ));
    }
    let methods = VERSIONED_METHODS.iter().copied().chain(["binary"]);
    for method in methods {
        let keys = |l: &ToolLock| -> BTreeSet<String> {
            if method == "binary" {
                l.binary.keys().cloned().collect()
            } else {
                l.versions(method).keys().cloned().collect()
            }
        };
        let all: BTreeSet<String> = keys(before).union(&keys(after)).cloned().collect();
        for name in all {
            let old = entry_label(before, method, &name);
            let new = entry_label(after, method, &name);
            match (old.is_empty(), new.is_empty()) {
                (true, false) => changes.push(format!("+ {method} {name} {new}")),
                (false, true) => changes.push(format!("- {method} {name}")),
                (false, false) if old != new => {
                    changes.push(format!("~ {method} {name} {old} -> {new}"));
                }
                _ => {}
            }
        }
    }
    changes
}

fn lock_error(msg: String) -> anyhow::Error {
    crate::exit_code::RsconstructError::new(crate::exit_code::RsconstructExitCode::ToolError, msg)
        .into()
}

/// A locked tool whose installed copy is not the locked one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub method: String,
    pub name: String,
    pub locked: String,
    /// What is installed instead, or None when nothing is.
    pub installed: Option<String>,
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.installed {
            Some(found) => write!(
                f,
                "{} {} — locked {}, installed {found}",
                self.method, self.name, self.locked
            ),
            None => write!(
                f,
                "{} {} — locked {}, not installed",
                self.method, self.name, self.locked
            ),
        }
    }
}

/// The Python packages installed in the environment `python3` on PATH
/// belongs to (the one install-deps and `tool install` install into),
/// with their versions, among `names`. Asks the interpreter itself, not
/// `pip show`: a uv-managed venv (the default installer's) has no pip.
/// Shared with `doctor`.
pub fn installed_pip_versions(ctx: &BuildContext, names: &[&str]) -> BTreeMap<String, String> {
    const SCRIPT: &str = "\
import sys, importlib.metadata as m
for n in sys.argv[1:]:
    try:
        print(n, m.version(n))
    except m.PackageNotFoundError:
        pass
";
    let mut cmd = std::process::Command::new("python3");
    cmd.arg("-c").arg(SCRIPT).args(names);
    let Ok(out) = crate::processor::run_command_capture(ctx, &cmd) else {
        return BTreeMap::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(n, v)| (n.to_string(), v.trim().to_string()))
        .collect()
}

/// Globally installed npm packages and their versions. `-g`, because
/// `[dependencies].npm` entries are installed with `npm install -g`.
/// Shared with `doctor`.
pub fn installed_npm_globals(ctx: &BuildContext) -> BTreeMap<String, String> {
    let mut cmd = std::process::Command::new("npm");
    cmd.args(["ls", "-g", "--depth=0", "--json"]);
    // npm ls exits non-zero on any problem in the global tree while still
    // printing it, so the exit status is not consulted.
    let Ok(out) = crate::processor::run_command_capture(ctx, &cmd) else {
        return BTreeMap::new();
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else {
        return BTreeMap::new();
    };
    json.get("dependencies")
        .and_then(serde_json::Value::as_object)
        .map(|deps| {
            deps.iter()
                .filter_map(|(name, entry)| {
                    entry
                        .get("version")
                        .and_then(serde_json::Value::as_str)
                        .map(|v| (name.clone(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The installed versions of gem `name` (several can coexist).
fn installed_gem_versions(ctx: &BuildContext, name: &str) -> Vec<String> {
    let mut cmd = std::process::Command::new("gem");
    cmd.args(["list", "--local", "--exact", name]);
    let Ok(out) = crate::processor::run_command_capture(ctx, &cmd) else {
        return Vec::new();
    };
    parse_gem_list(&String::from_utf8_lossy(&out.stdout), name)
}

/// Parse `gem list` output (`name (1.2.0, default: 1.1.0)`) into the
/// versions installed of `name`. A native gem lists its platform after the
/// version (`ffi (1.17.4 x86_64-linux-gnu, 1.17.3)`); only the version is
/// kept, since that is what the lock records.
fn parse_gem_list(listing: &str, name: &str) -> Vec<String> {
    listing
        .lines()
        .find_map(|line| line.strip_prefix(name)?.trim().strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
        .map(|versions| {
            versions
                .split(',')
                .filter_map(|v| {
                    v.trim()
                        .trim_start_matches("default:")
                        .split_whitespace()
                        .next()
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// What is installed for a pinned binary: the executable's SHA-256, or a
/// `.deb`'s installed package version.
fn installed_binary_identity(ctx: &BuildContext, name: &str, pin: &BinaryPin) -> Option<String> {
    if let Some(package) = &pin.deb_package {
        let mut cmd = std::process::Command::new("dpkg-query");
        cmd.args(["-W", "-f", "${Version}", package]);
        let out = crate::processor::run_command_capture(ctx, &cmd).ok()?;
        let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return (out.status.success() && !version.is_empty()).then_some(version);
    }
    let path = which::which(crate::tools::binary_dest(name)?).ok()?;
    crate::tools::file_sha256(&path).ok()
}

/// The locked tools among `wanted` whose installed copy is not the locked
/// one.
pub fn mismatches(ctx: &BuildContext, lock: &ToolLock, wanted: &Wanted) -> Vec<Mismatch> {
    let mut found = Vec::new();
    let mut push = |method: &str, name: &str, locked: &str, installed: Option<String>| {
        found.push(Mismatch {
            method: method.to_string(),
            name: name.to_string(),
            locked: locked.to_string(),
            installed,
        });
    };

    if !wanted.cargo.is_empty() {
        let installed = crate::tools::installed_cargo_crates(ctx).unwrap_or_default();
        for name in &wanted.cargo {
            if let Some(locked) = lock.cargo.get(name) {
                let have = installed.get(name);
                if have != Some(locked) {
                    push("cargo", name, locked, have.cloned());
                }
            }
        }
    }
    if !wanted.pip.is_empty() {
        let names: Vec<&str> = wanted.pip.iter().map(String::as_str).collect();
        let installed = installed_pip_versions(ctx, &names);
        for name in &wanted.pip {
            if let Some(locked) = lock.pip.get(name) {
                let have = installed.get(name);
                if have != Some(locked) {
                    push("pip", name, locked, have.cloned());
                }
            }
        }
    }
    if !wanted.npm.is_empty() {
        let installed = installed_npm_globals(ctx);
        for name in &wanted.npm {
            if let Some(locked) = lock.npm.get(name) {
                let have = installed.get(name);
                if have != Some(locked) {
                    push("npm", name, locked, have.cloned());
                }
            }
        }
    }
    for name in &wanted.gem {
        if let Some(locked) = lock.gem.get(name) {
            let have = installed_gem_versions(ctx, name);
            if !have.contains(locked) {
                push(
                    "gem",
                    name,
                    locked,
                    (!have.is_empty()).then(|| have.join(", ")),
                );
            }
        }
    }
    for name in &wanted.binary {
        if let Some(pin) = lock.binary.get(name) {
            let (locked, have) = match (&pin.deb_version, &pin.executable_sha256) {
                (Some(version), _) => (version.clone(), installed_binary_identity(ctx, name, pin)),
                (None, Some(sha)) => (
                    format!("sha256 {sha}"),
                    installed_binary_identity(ctx, name, pin).map(|s| format!("sha256 {s}")),
                ),
                (None, None) => continue,
            };
            if have.as_ref() != Some(&locked) {
                push("binary", name, &locked, have);
            }
        }
    }
    found
}

/// Check that rsconstruct.lock covers `wanted` (exactly, or at least — see
/// [`Coverage`]) and that every wanted tool is installed at its locked
/// version. `tool check` asks for the whole project exactly; the `build`
/// check in CI and with `--verify-tool-versions` asks for the processors
/// that build.
pub fn verify(ctx: &BuildContext, wanted: &Wanted, coverage: Coverage) -> Result<ToolLock> {
    let lock = read_locked(wanted, coverage)?;
    let found = mismatches(ctx, &lock, wanted);
    if !found.is_empty() {
        let lines: Vec<String> = found.iter().map(ToString::to_string).collect();
        return Err(lock_error(format!(
            "Installed tools do not match {LOCK_FILE} (run `rsconstruct tool install-deps` \
             and `rsconstruct tool install`):\n{}",
            lines.join("\n")
        )));
    }
    Ok(lock)
}

/// Identify a single tool for cache-keying purposes, by the resolved
/// binary's `(path, size, mtime)` — no `--version` subprocess and,
/// critically, no read of the file's bytes.
///
/// Content-hashing the binary was the obvious choice and measured terribly:
/// tool binaries are big (pandoc is ~200 MB here) and there are dozens of
/// them, which made this 45% of a cold build. `(path, size, mtime)` is the
/// same identity assumption the mtime cache already makes for every input
/// file in the project, so keying tools this way is not a weaker standard
/// than the rest of the build — and it is strictly stronger than a
/// `--version` string, which misses a rebuilt binary reporting an unchanged
/// version. Reinstalling a tool changes its mtime, which is the case that
/// has to invalidate.
///
/// Returns None for a tool that is not on PATH: a missing tool is the tool
/// preflight's problem, not the cache key's, and failing here would break
/// builds whose products don't need that processor at all.
fn tool_identity(tool: &str) -> Option<String> {
    let path = which::which(tool).ok()?;
    let meta = match fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            // A tool that IS on PATH but can't be stat'd (dangling symlink,
            // unreadable parent) must not silently drop out of the cache key
            // — that reopens the "tool upgrade leaves stale PASSes valid"
            // hole this mechanism exists to close. Surface it.
            crate::output::warn(&format!(
                "Cannot stat tool '{tool}' at {}: {e} — excluding it from cache keys",
                path.display()
            ));
            return None;
        }
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some(format!("{tool}@{}:{}:{mtime}", path.display(), meta.len()))
}

/// Compute a tool version hash per processor.
///
/// Returns a map from processor name to a hash of its tools' identities, for
/// mixing into every product that processor builds — so upgrading a tool
/// invalidates what that tool produced. Set `[build] hash_tool_versions =
/// false` to opt out.
pub fn processor_tool_hashes(
    processors: &ProcessorMap,
    enabled: &dyn Fn(&str) -> bool,
) -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();

    let mut names: Vec<&String> = processors.keys().collect();
    names.sort();

    // Collect the distinct tool set first. One tool is typically required by
    // several processors; resolving and hashing it once per processor would
    // multiply the work for nothing.
    let mut wanted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for name in &names {
        if !enabled(name) {
            continue;
        }
        let tools = processors[*name].required_tools();
        if tools.is_empty() {
            continue;
        }
        wanted.insert((*name).clone(), tools);
    }
    let distinct: BTreeSet<String> = wanted.values().flatten().cloned().collect();

    // Resolve each distinct tool once. This is a `which` plus a stat per
    // tool, so it stays serial deliberately: an earlier attempt to thread
    // this measured *slower* on a cold page cache (~2.4s vs ~1.5s), because
    // the threads contended on one large sequential read. Making the identity
    // cheap beat making it concurrent — see `tool_identity`.
    let identity_cache: BTreeMap<String, Option<String>> = distinct
        .iter()
        .map(|t| (t.clone(), tool_identity(t)))
        .collect();

    for name in names {
        let Some(tools) = wanted.get(name) else {
            continue;
        };

        let mut version_parts: Vec<String> = Vec::new();
        for tool in tools {
            if let Some(Some(identity)) = identity_cache.get(tool) {
                version_parts.push(identity.clone());
            }
        }

        if !version_parts.is_empty() {
            version_parts.sort();
            // Length-prefixed parts, not a newline join: identities embed
            // filesystem paths, which can contain newlines.
            let parts: Vec<&str> = version_parts.iter().map(String::as_str).collect();
            result.insert(name.clone(), crate::checksum::hash_parts(&parts));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_lock() -> ToolLock {
        let mut lock = ToolLock::empty();
        lock.cargo
            .insert("cargo-deny".to_string(), "0.18.3".to_string());
        lock.pip.insert("ruff".to_string(), "0.14.0".to_string());
        lock.pip
            .insert("manim-voiceover".to_string(), "0.3.7".to_string());
        lock.npm
            .insert("@scope/tool".to_string(), "2.0.0".to_string());
        lock.gem.insert("mdl".to_string(), "0.13.0".to_string());
        lock.binary.insert(
            "rumdl".to_string(),
            BinaryPin {
                url: "https://example.com/rumdl.tar.gz".to_string(),
                sha256: "aa".to_string(),
                executable_sha256: Some("bb".to_string()),
                deb_package: None,
                deb_version: None,
            },
        );
        lock
    }

    #[test]
    fn lock_round_trips_through_toml_with_header() {
        let lock = sample_lock();
        let text = render_lock(&lock).unwrap();
        assert!(text.starts_with("# rsconstruct.lock"), "{text}");
        assert_eq!(parse_lock(&text).unwrap(), lock);
    }

    /// A lock written before 2026-10-26 carries an `[rsconstruct]` table.
    /// It must still parse (the fleet's committed locks have it), and a
    /// rewrite leaves it out and says so.
    #[test]
    fn legacy_rsconstruct_table_parses_and_is_dropped_on_rewrite() {
        let old = "version = 1\n\n[rsconstruct]\nversion = \"0.9.140\"\n\n[cargo]\ncargo-deny = \"0.20.2\"\n";
        let before = parse_lock(old).unwrap();
        assert_eq!(
            before.rsconstruct.as_ref().map(|p| p.version.as_str()),
            Some("0.9.140")
        );

        let mut after = before.clone();
        after.rsconstruct = None;
        let text = render_lock(&after).unwrap();
        assert!(!text.contains("[rsconstruct]"), "{text}");
        assert!(text.contains("[cargo]\ncargo-deny = \"0.20.2\""), "{text}");
        // Rendering never writes the table, whatever the struct holds.
        assert!(!render_lock(&before).unwrap().contains("[rsconstruct]"));

        let changes = describe_changes(&before, &after);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert!(
            changes[0].starts_with("- rsconstruct 0.9.140"),
            "{changes:?}"
        );
    }

    #[test]
    fn empty_sections_are_left_out() {
        let text = render_lock(&ToolLock::empty()).unwrap();
        assert!(!text.contains("[cargo]"), "{text}");
        assert!(!text.contains("[binary"), "{text}");
    }

    #[test]
    fn other_format_versions_and_unknown_keys_are_errors() {
        assert!(parse_lock("version = 2\n").is_err());
        assert!(parse_lock("version = 1\n[surprise]\n").is_err());
        assert!(parse_lock("version = 1\n").is_ok());
    }

    #[test]
    fn pinned_formats_each_package_manager() {
        let lock = sample_lock();
        assert_eq!(lock.pinned("cargo", "cargo-deny"), "cargo-deny@0.18.3");
        assert_eq!(lock.pinned("pip", "ruff"), "ruff==0.14.0");
        assert_eq!(lock.pinned("uv", "Ruff"), "Ruff==0.14.0");
        assert_eq!(
            lock.pinned("pip", "manim_voiceover[gtts]"),
            "manim_voiceover[gtts]==0.3.7"
        );
        assert_eq!(lock.pinned("npm", "@scope/tool"), "@scope/tool@2.0.0");
        assert_eq!(lock.pinned("gem", "mdl"), "mdl:0.13.0");
    }

    /// Unlocked names, explicit constraints and unversioned methods pass
    /// through untouched.
    #[test]
    fn pinned_leaves_other_requests_alone() {
        let lock = sample_lock();
        assert_eq!(lock.pinned("pip", "black"), "black");
        assert_eq!(lock.pinned("pip", "ruff>=0.5"), "ruff>=0.5");
        assert_eq!(lock.pinned("pip", "ruff==0.1.0"), "ruff==0.1.0");
        assert_eq!(lock.pinned("cargo", "cargo-deny@0.1.0"), "cargo-deny@0.1.0");
        assert_eq!(lock.pinned("apt", "ruff"), "ruff");
    }

    #[test]
    fn wanted_skips_constrained_entries_and_normalizes_pip_names() {
        let mut wanted = Wanted::default();
        assert!(wanted.add("pip", "Types_PyYAML"));
        assert!(!wanted.add("pip", "ruff>=0.5"));
        assert!(wanted.add("npm", "@scope/tool"));
        assert!(!wanted.add("npm", "@scope/tool@1.0.0"));
        assert!(!wanted.add("apt", "aspell"));
        assert!(!wanted.add("binary", "google-chrome-stable"));
        assert!(wanted.add("binary", "rumdl"));
        assert_eq!(wanted.pip.iter().collect::<Vec<_>>(), vec!["types-pyyaml"]);
        assert_eq!(wanted.npm.len(), 1);
        assert_eq!(wanted.binary.len(), 1);
    }

    #[test]
    fn diff_reports_missing_and_stale_entries() {
        let lock = sample_lock();
        let mut wanted = Wanted::default();
        wanted.add("cargo", "cargo-deny");
        wanted.add("cargo", "cargo-nextest");
        wanted.add("binary", "rumdl");
        let (missing, stale) = diff(&lock, &wanted);
        assert_eq!(
            missing,
            vec![("cargo".to_string(), "cargo-nextest".to_string())]
        );
        let stale: Vec<String> = stale.iter().map(|(m, n)| format!("{m} {n}")).collect();
        assert_eq!(
            stale,
            vec![
                "gem mdl",
                "npm @scope/tool",
                "pip manim-voiceover",
                "pip ruff"
            ]
        );
    }

    /// A name selects its tool under every method the project locks it for
    /// (Python names compared normalized); an unknown name is an error, and
    /// `rsconstruct` is one (rsconstruct itself is not locked).
    #[test]
    fn select_entries_matches_names_across_methods() {
        let mut wanted = Wanted::default();
        wanted.add("cargo", "oxlint");
        wanted.add("binary", "oxlint");
        wanted.add("pip", "types-PyYAML");
        let names = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();

        let targets = select_entries(&wanted, &names(&["oxlint", "Types_PyYAML"])).unwrap();
        let targets: Vec<String> = targets.iter().map(|(m, n)| format!("{m} {n}")).collect();
        assert_eq!(
            targets,
            vec!["cargo oxlint", "binary oxlint", "pip types-pyyaml"]
        );

        let err = select_entries(&wanted, &names(&["rsconstruct"])).unwrap_err();
        assert!(err.to_string().contains("rsconstruct"), "{err}");

        let err = select_entries(&wanted, &names(&["oxlint", "nosuch"])).unwrap_err();
        assert!(err.to_string().contains("nosuch"), "{err}");
    }

    #[test]
    fn describe_changes_lists_additions_removals_and_moves() {
        let before = sample_lock();
        let mut after = before.clone();
        after
            .cargo
            .insert("cargo-deny".to_string(), "0.19.0".to_string());
        after.gem.remove("mdl");
        after
            .cargo
            .insert("mdbook".to_string(), "0.4.40".to_string());
        assert_eq!(
            describe_changes(&before, &after),
            vec![
                "~ cargo cargo-deny 0.18.3 -> 0.19.0",
                "+ cargo mdbook 0.4.40",
                "- gem mdl",
            ]
        );
    }

    #[test]
    fn gem_list_parses_plain_and_default_versions() {
        assert_eq!(
            parse_gem_list("mdl (0.13.0, default: 0.12.0)\n", "mdl"),
            vec!["0.13.0", "0.12.0"]
        );
        assert_eq!(parse_gem_list("", "mdl"), Vec::<String>::new());
        assert_eq!(parse_gem_list("mdlx (1.0)\n", "mdl"), Vec::<String>::new());
    }

    /// A native gem's platform follows the version inside the parentheses.
    /// Keeping it made the drift check report `1.17.4 x86_64-linux-gnu`
    /// against a `1.17.4` pin as a mismatch.
    #[test]
    fn gem_list_drops_platform_suffixes() {
        assert_eq!(
            parse_gem_list("ffi (1.17.4 x86_64-linux-gnu, 1.17.3)\n", "ffi"),
            vec!["1.17.4", "1.17.3"]
        );
        assert_eq!(
            parse_gem_list("ffi (default: 1.17.4 x86_64-linux-gnu)\n", "ffi"),
            vec!["1.17.4"]
        );
    }

    /// A lockfile owns its packages only when install-deps installs from
    /// it. With `npm_source = "package-json"`, a package-lock.json on disk
    /// pins nothing, so the packages it names must still be locked here —
    /// before this, `tool lock` dropped them and they floated.
    #[test]
    fn lockfiles_own_packages_only_when_they_are_the_source() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(
            tmp.path().join("package-lock.json"),
            r#"{"lockfileVersion": 3, "packages": {"node_modules/markdownlint-cli": {"version": "0.45.0"}}}"#,
        )
        .unwrap();
        fs::write(
            tmp.path().join("uv.lock"),
            "version = 1\n\n[[package]]\nname = \"ruff\"\nversion = \"0.14.0\"\nsource = { registry = \"https://pypi.org/simple\" }\n",
        )
        .unwrap();

        let defaults = crate::config::DependenciesConfig::default();
        let owned = lockfile_owned(&defaults, tmp.path()).unwrap();
        assert_eq!(
            owned.npm.iter().collect::<Vec<_>>(),
            vec!["markdownlint-cli"]
        );
        assert_eq!(owned.pip.iter().collect::<Vec<_>>(), vec!["ruff"]);

        let floating = crate::config::DependenciesConfig {
            npm_source: crate::config::NpmSource::PackageJson,
            pip_source: crate::config::PipSource::Pyproject,
            ..Default::default()
        };
        assert_eq!(
            lockfile_owned(&floating, tmp.path()).unwrap(),
            LockfileOwned::default()
        );
    }
}
