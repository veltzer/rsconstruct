//! Per-project tool installs: `.rsconstruct/tools`, filled from a
//! machine-wide store by hard links.
//!
//! A tool installed globally is shared by every project on the machine, so
//! two projects that lock different releases of it cannot both be right.
//! The tools rsconstruct installs from a registry — crates, npm packages,
//! gems, and the release binaries of the `binary` method — therefore go into
//! the project instead, at exactly the version its rsconstruct.lock names,
//! the way uv puts a project's Python packages into its `.venv`.
//!
//! Each version is installed once, into the store
//! (`$RSCONSTRUCT_TOOL_STORE`, else `$XDG_CACHE_HOME/rsconstruct/tools`,
//! else `~/.cache/rsconstruct/tools`), and every project that locks it gets
//! a hard-linked copy of that install: no second download or compile, and no
//! second copy on disk. Where a hard link is impossible (the store on another
//! filesystem) the files are copied. Store files are made read-only, because
//! a hard link shares the file: a tool writing to its own install would
//! change every project's copy.
//!
//! The layout under the project root:
//!
//! ```text
//! .rsconstruct/tools/pkgs/<method>/<name>@<version>/   one install prefix per package
//! .rsconstruct/tools/bin/<exe>                         symlink into its package's bin/
//! ```
//!
//! A gem's executable is the exception: `bin/<exe>` is a small launcher
//! that runs it with `GEM_HOME` and `GEM_PATH` naming only its own package
//! (see [`gem_launcher`]).
//!
//! `bin/` is put first on PATH at startup (see [`augment_env`]), so every
//! tool rsconstruct runs is the project's. System packages (apt and the
//! `.deb` binary recipes) and Python packages are not handled here: the
//! first belong to the OS, the second to the project's own virtualenv.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The project's tool directory, relative to the project root.
pub const REPO_TOOLS_DIR: &str = ".rsconstruct/tools";
/// The name of the tool directory inside `.rsconstruct`.
const TOOLS_DIR_NAME: &str = "tools";
const PKGS_DIR: &str = "pkgs";
const BIN_DIR: &str = "bin";
/// Overrides where the store lives. Set in images that bake tools in, so a
/// job running there with another `$HOME` still finds them.
pub const STORE_ENV: &str = "RSCONSTRUCT_TOOL_STORE";
/// The prefix of a directory something is still being installed into.
const TMP_PREFIX: &str = ".tmp-";

/// One package as the project's tool directory holds it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Package {
    pub method: String,
    pub name: String,
    /// The release installed: the locked version, the version an explicit
    /// request names, or for a binary `sha256-<hex>` of its executable.
    pub version: String,
}

impl Package {
    pub fn new(method: &str, name: &str, version: &str) -> Self {
        Self {
            method: method.to_string(),
            name: name.to_string(),
            version: version.to_string(),
        }
    }

    /// The directory name of this package, under its method's directory. A
    /// scoped npm name's `/` becomes `+`, which no package name contains.
    fn key(&self) -> String {
        format!("{}@{}", self.name.replace('/', "+"), self.version)
    }
}

impl std::fmt::Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} {}", self.method, self.name, self.version)
    }
}

/// Split a package directory name back into its name and version: the
/// version follows the last `@` (a scoped npm name starts with one).
fn parse_key(method: &str, key: &str) -> Option<(String, String)> {
    let (name, version) = key.rsplit_once('@')?;
    if name.is_empty() || version.is_empty() {
        return None;
    }
    let name = if method == "npm" {
        name.replace('+', "/")
    } else {
        name.to_string()
    };
    Some((name, version.to_string()))
}

/// The machine-wide store every project's tools are linked from.
pub fn store_root() -> Result<PathBuf> {
    let non_empty = |v: &std::ffi::OsString| !v.is_empty();
    if let Some(dir) = std::env::var_os(STORE_ENV).filter(non_empty) {
        return Ok(PathBuf::from(dir));
    }
    std::env::var_os("XDG_CACHE_HOME")
        .filter(non_empty)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .map(|base| base.join("rsconstruct").join("tools"))
        .with_context(|| {
            format!("cannot locate the tool store: set {STORE_ENV}, XDG_CACHE_HOME or HOME")
        })
}

fn tools_dir(root: &Path) -> PathBuf {
    root.join(REPO_TOOLS_DIR)
}

fn package_dir(root: &Path, pkg: &Package) -> PathBuf {
    tools_dir(root)
        .join(PKGS_DIR)
        .join(&pkg.method)
        .join(pkg.key())
}

/// What the project's tool directory holds: every installed version, by
/// `(method, name)`. Normally one version each; a leftover from an
/// interrupted sync can add another.
pub fn installed_versions(root: &Path) -> BTreeMap<(String, String), Vec<String>> {
    let mut found: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let pkgs = tools_dir(root).join(PKGS_DIR);
    let Ok(methods) = fs::read_dir(&pkgs) else {
        return found;
    };
    for method in methods.flatten() {
        let method_name = method.file_name().to_string_lossy().into_owned();
        let Ok(entries) = fs::read_dir(method.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let key = entry.file_name().to_string_lossy().into_owned();
            if key.starts_with(TMP_PREFIX) || !entry.path().is_dir() {
                continue;
            }
            if let Some((name, version)) = parse_key(&method_name, &key) {
                found
                    .entry((method_name.clone(), name))
                    .or_default()
                    .push(version);
            }
        }
    }
    for versions in found.values_mut() {
        versions.sort();
    }
    found
}

/// The project's copy of tool `name`, when its tool directory has one.
/// Resolved against the working directory, which is the project root.
pub fn repo_tool(name: &str) -> Option<PathBuf> {
    let path = std::env::current_dir()
        .ok()?
        .join(REPO_TOOLS_DIR)
        .join(BIN_DIR)
        .join(name);
    path.exists().then_some(path)
}

/// What syncing the project's tool directory with a package set will do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Already in place.
    pub present: Vec<Package>,
    /// To be linked in, from the store when it has them, installed into the
    /// store first when not.
    pub missing: Vec<Package>,
    /// Package directories nothing wants any more, as `method name@version`.
    pub stale: Vec<String>,
}

impl Plan {
    /// Whether applying the plan changes the package directories.
    pub const fn has_work(&self) -> bool {
        !self.missing.is_empty() || !self.stale.is_empty()
    }
}

/// Compare the project's tool directory with `packages`.
pub fn plan(root: &Path, packages: &BTreeSet<Package>) -> Plan {
    let mut out = Plan::default();
    for pkg in packages {
        if package_dir(root, pkg).is_dir() {
            out.present.push(pkg.clone());
        } else {
            out.missing.push(pkg.clone());
        }
    }
    let wanted: BTreeSet<(String, String)> = packages
        .iter()
        .map(|p| (p.method.clone(), p.key()))
        .collect();
    let pkgs = tools_dir(root).join(PKGS_DIR);
    if let Ok(methods) = fs::read_dir(&pkgs) {
        for method in methods.flatten() {
            let method_name = method.file_name().to_string_lossy().into_owned();
            let Ok(entries) = fs::read_dir(method.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let key = entry.file_name().to_string_lossy().into_owned();
                if !wanted.contains(&(method_name.clone(), key.clone())) {
                    out.stale.push(format!("{method_name} {key}"));
                }
            }
        }
    }
    out.stale.sort();
    out
}

/// Installs one package into an empty directory, leaving its executables in
/// that directory's `bin/`.
pub type Installer<'a> = dyn Fn(&Package, &Path) -> Result<()> + 'a;

/// Make the project's tool directory hold exactly `packages`: link in what
/// is missing (installing it into `store` first when the store lacks it),
/// remove what nothing wants, and rebuild `bin/`. Calls `on_linked` after
/// each package is in place.
pub fn sync(
    root: &Path,
    store: &Path,
    packages: &BTreeSet<Package>,
    install: &Installer<'_>,
    on_linked: &mut dyn FnMut(&Package, bool),
) -> Result<()> {
    let todo = plan(root, packages);
    for pkg in &todo.missing {
        let (stored, fresh) = ensure_in_store(store, pkg, install)?;
        let dest = package_dir(root, pkg);
        let parent = dest
            .parent()
            .context("a package directory always has a parent")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
        let tmp = parent.join(format!("{TMP_PREFIX}{}-{}", pkg.key(), std::process::id()));
        remove_if_present(&tmp)?;
        link_tree(&stored, &tmp)?;
        fs::rename(&tmp, &dest).with_context(|| {
            format!(
                "Failed to move {} into place as {}",
                tmp.display(),
                dest.display()
            )
        })?;
        on_linked(pkg, fresh);
    }
    let pkgs = tools_dir(root).join(PKGS_DIR);
    for stale in &todo.stale {
        if let Some((method, key)) = stale.split_once(' ') {
            remove_if_present(&pkgs.join(method).join(key))?;
        }
    }
    rebuild_bin(root, packages)
}

/// The store's install of `pkg`, installing it first when the store lacks
/// it; the flag says whether it was just installed. The install goes into a
/// private directory that is renamed into place once complete, so a store
/// entry is always a finished install, and two processes racing for the
/// same package both end up using whichever finished first.
fn ensure_in_store(
    store: &Path,
    pkg: &Package,
    install: &Installer<'_>,
) -> Result<(PathBuf, bool)> {
    let dir = store.join(&pkg.method).join(pkg.key());
    if dir.is_dir() {
        return Ok((dir, false));
    }
    let parent = dir.parent().context("a store entry always has a parent")?;
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    let tmp = parent.join(format!("{TMP_PREFIX}{}-{}", pkg.key(), std::process::id()));
    remove_if_present(&tmp)?;
    fs::create_dir_all(&tmp).with_context(|| format!("Failed to create {}", tmp.display()))?;
    if let Err(e) = install(pkg, &tmp) {
        remove_if_present(&tmp).ok();
        return Err(e.context(format!("Failed to install {pkg} into the tool store")));
    }
    make_files_read_only(&tmp)?;
    match fs::rename(&tmp, &dir) {
        Ok(()) => Ok((dir, true)),
        Err(_) if dir.is_dir() => {
            remove_if_present(&tmp)?;
            Ok((dir, false))
        }
        Err(e) => Err(e).with_context(|| {
            format!(
                "Failed to move {} into place as {}",
                tmp.display(),
                dir.display()
            )
        }),
    }
}

/// Recreate the tree at `src` as `dst`: directories created, files hard
/// linked (copied when linking fails, e.g. across filesystems), symlinks
/// recreated with the same target.
fn link_tree(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir(dst).with_context(|| format!("Failed to create {}", dst.display()))?;
    let entries =
        fs::read_dir(src).with_context(|| format!("Failed to read directory {}", src.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("Failed to read directory {}", src.display()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let kind = entry
            .file_type()
            .with_context(|| format!("Failed to stat {}", from.display()))?;
        if kind.is_dir() {
            link_tree(&from, &to)?;
        } else if kind.is_symlink() {
            let target = fs::read_link(&from)
                .with_context(|| format!("Failed to read symlink {}", from.display()))?;
            crate::platform::symlink_file(&target, &to)
                .with_context(|| format!("Failed to create symlink {}", to.display()))?;
        } else if fs::hard_link(&from, &to).is_err() {
            fs::copy(&from, &to).with_context(|| {
                format!("Failed to copy {} to {}", from.display(), to.display())
            })?;
        }
    }
    Ok(())
}

/// Clear the write bits of every regular file under `dir`. Directories keep
/// theirs, so a project's copy can still be removed.
fn make_files_read_only(dir: &Path) -> Result<()> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("Failed to read directory {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("Failed to read directory {}", dir.display()))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .with_context(|| format!("Failed to stat {}", path.display()))?;
        if kind.is_dir() {
            make_files_read_only(&path)?;
        } else if kind.is_file() {
            let meta = fs::metadata(&path)
                .with_context(|| format!("Failed to stat {}", path.display()))?;
            let mode = crate::platform::get_mode(&meta) & !0o222;
            crate::platform::set_permissions_mode(&path, mode)
                .with_context(|| format!("Failed to make {} read-only", path.display()))?;
        }
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("Failed to remove {}", path.display())),
    }
}

/// Rebuild `bin/` from scratch: one entry per executable of every package —
/// a relative symlink into the package's own `bin/`, or for a gem a
/// [`gem_launcher`]. Two packages providing the same executable is an
/// error, since only one could win.
fn rebuild_bin(root: &Path, packages: &BTreeSet<Package>) -> Result<()> {
    let tools = tools_dir(root);
    let tmp = tools.join(format!("{TMP_PREFIX}bin-{}", std::process::id()));
    remove_if_present(&tmp)?;
    fs::create_dir_all(&tmp).with_context(|| format!("Failed to create {}", tmp.display()))?;
    let mut owner: BTreeMap<String, &Package> = BTreeMap::new();
    for pkg in packages {
        let pkg_bin = package_dir(root, pkg).join(BIN_DIR);
        let Ok(entries) = fs::read_dir(&pkg_bin) else {
            continue;
        };
        let mut exes: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        exes.sort();
        for exe in exes {
            if let Some(other) = owner.insert(exe.clone(), pkg) {
                remove_if_present(&tmp).ok();
                anyhow::bail!("both {other} and {pkg} provide the executable {exe}");
            }
            let package = Path::new("..")
                .join(PKGS_DIR)
                .join(&pkg.method)
                .join(pkg.key());
            let entry = tmp.join(&exe);
            if pkg.method == "gem" {
                fs::write(&entry, gem_launcher(&package, &exe))
                    .with_context(|| format!("Failed to write {}", entry.display()))?;
                crate::platform::set_permissions_mode(&entry, 0o755)
                    .with_context(|| format!("Failed to make {} executable", entry.display()))?;
            } else {
                crate::platform::symlink_file(&package.join(BIN_DIR).join(&exe), &entry)
                    .with_context(|| format!("Failed to link {exe} into {}", tmp.display()))?;
            }
        }
    }
    let bin = tools.join(BIN_DIR);
    remove_if_present(&bin)?;
    fs::rename(&tmp, &bin).with_context(|| {
        format!(
            "Failed to move {} into place as {}",
            tmp.display(),
            bin.display()
        )
    })
}

/// The `bin/` entry for executable `exe` of the gem package at `package`
/// (relative to `bin/`): a shell script that runs the package's binstub
/// with `GEM_HOME` and `GEM_PATH` naming the package alone — exactly the
/// environment it was installed under (see `tools::install_into_prefix`),
/// so the gem and its whole dependency closure resolve inside it.
///
/// Setting `GEM_PATH` for everything instead would not do: rubygems
/// activates the highest version of a gem found anywhere on the path, so a
/// newer copy installed elsewhere on the machine would beat the locked one.
/// The launcher scopes the variables to this one tool, the way rbenv and
/// Bundler binstubs do. The path is relative to the launcher, so the
/// project directory can move.
fn gem_launcher(package: &Path, exe: &str) -> String {
    format!(
        "#!/bin/sh\n\
         # Written by rsconstruct: run {exe} with only its own gems visible.\n\
         pkg=\"$(cd \"$(dirname \"$0\")/{package}\" && pwd)\" || exit 1\n\
         GEM_HOME=\"$pkg\" GEM_PATH=\"$pkg\" exec \"$pkg/bin/{exe}\" \"$@\"\n",
        package = package.display(),
    )
}

/// Remove the `.rsconstruct` state directory `dir`, except for its `tools/`:
/// what `cache clear` and `clean all` do. The tools are not a cache of
/// anything the build produced, and a build right after a clear needs them.
/// Returns whether anything was removed.
pub fn clear_state_dir(dir: &Path) -> Result<bool> {
    if !dir.exists() {
        return Ok(false);
    }
    if !dir.join(TOOLS_DIR_NAME).exists() {
        fs::remove_dir_all(dir).with_context(|| format!("Failed to remove {}", dir.display()))?;
        return Ok(true);
    }
    let mut removed = false;
    let entries =
        fs::read_dir(dir).with_context(|| format!("Failed to read directory {}", dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("Failed to read directory {}", dir.display()))?;
        if entry.file_name() == TOOLS_DIR_NAME {
            continue;
        }
        let path = entry.path();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        let result = if is_dir {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.with_context(|| format!("Failed to remove {}", path.display()))?;
        removed = true;
    }
    Ok(removed)
}

/// Make the project's tools resolvable for this process and every child:
/// put `.rsconstruct/tools/bin` first on `PATH`. Only in a project directory
/// (one with rsconstruct.toml). The bin dir goes on `PATH` even before it
/// exists, so a tool installed by this very invocation is found by what it
/// runs next.
///
/// Called once at the top of `main()`, before any thread exists — the
/// safety condition of `platform::set_env`.
pub fn augment_env() {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    if !cwd.join(crate::config::CONFIG_FILE).exists() {
        return;
    }
    let bin = cwd.join(REPO_TOOLS_DIR).join(BIN_DIR);
    let path = std::env::var_os("PATH").unwrap_or_default();
    if let Some(new_path) = crate::tools::path_with_prepended_dir(&path, &bin) {
        crate::platform::set_env("PATH", &new_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An installer that writes one executable per package into `bin/`,
    /// named after the package (a scoped npm name without its scope, as npm
    /// does), and counts its calls.
    fn fake_installer(
        calls: &std::cell::Cell<usize>,
    ) -> impl Fn(&Package, &Path) -> Result<()> + '_ {
        move |pkg: &Package, prefix: &Path| {
            calls.set(calls.get() + 1);
            let exe = pkg.name.rsplit('/').next().unwrap_or(&pkg.name);
            fs::create_dir_all(prefix.join("bin"))?;
            fs::write(
                prefix.join("bin").join(exe),
                format!("#!/bin/sh\necho {}\n", pkg.version),
            )?;
            Ok(())
        }
    }

    fn set(packages: &[Package]) -> BTreeSet<Package> {
        packages.iter().cloned().collect()
    }

    #[test]
    fn keys_round_trip_including_scoped_npm_names() {
        for (method, name) in [
            ("cargo", "cargo-deny"),
            ("npm", "@scope/tool"),
            ("gem", "mdl"),
        ] {
            let pkg = Package::new(method, name, "1.2.3");
            assert_eq!(
                parse_key(method, &pkg.key()),
                Some((name.to_string(), "1.2.3".to_string()))
            );
        }
        assert_eq!(parse_key("cargo", "noversion"), None);
    }

    /// The first sync installs into the store and links into the project;
    /// a second project gets the same files by hard link, with no second
    /// install; a version change replaces the old package and its bin link.
    #[test]
    fn sync_installs_once_links_everywhere_and_replaces_versions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = tmp.path().join("store");
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let calls = std::cell::Cell::new(0);
        let install = fake_installer(&calls);
        let v1 = set(&[Package::new("cargo", "rssite", "0.1.14")]);

        let mut fresh = Vec::new();
        sync(&a, &store, &v1, &install, &mut |p, f| {
            fresh.push((p.version.clone(), f));
        })
        .unwrap();
        sync(&b, &store, &v1, &install, &mut |p, f| {
            fresh.push((p.version.clone(), f));
        })
        .unwrap();
        assert_eq!(calls.get(), 1, "the second project must link, not install");
        assert_eq!(
            fresh,
            vec![("0.1.14".to_string(), true), ("0.1.14".to_string(), false)]
        );

        let exe = |root: &Path| root.join(REPO_TOOLS_DIR).join("bin").join("rssite");
        let ino = |p: &Path| std::os::unix::fs::MetadataExt::ino(&fs::metadata(p).unwrap());
        assert_eq!(ino(&exe(&a)), ino(&exe(&b)), "not hard links of one file");
        assert_eq!(
            fs::read_to_string(exe(&a)).unwrap(),
            "#!/bin/sh\necho 0.1.14\n"
        );
        let mode = crate::platform::get_mode(&fs::metadata(exe(&a)).unwrap());
        assert_eq!(mode & 0o222, 0, "store files must be read-only");

        // A synced project has nothing left to do.
        let again = plan(&a, &v1);
        assert!(!again.has_work(), "{again:?}");

        let v2 = set(&[Package::new("cargo", "rssite", "0.1.15")]);
        assert_eq!(plan(&a, &v2).stale, vec!["cargo rssite@0.1.14"]);
        sync(&a, &store, &v2, &install, &mut |_, _| {}).unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(
            fs::read_to_string(exe(&a)).unwrap(),
            "#!/bin/sh\necho 0.1.15\n"
        );
        let installed = installed_versions(&a);
        assert_eq!(
            installed.get(&("cargo".to_string(), "rssite".to_string())),
            Some(&vec!["0.1.15".to_string()])
        );
        // The other project still has its own version.
        assert_eq!(
            fs::read_to_string(exe(&b)).unwrap(),
            "#!/bin/sh\necho 0.1.14\n"
        );
    }

    /// A tool leaving the package set leaves the project, bin link included.
    #[test]
    fn sync_prunes_packages_nothing_wants() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = tmp.path().join("store");
        let root = tmp.path().join("p");
        fs::create_dir_all(&root).unwrap();
        let calls = std::cell::Cell::new(0);
        let install = fake_installer(&calls);
        let both = set(&[
            Package::new("cargo", "mdbook", "1.0.0"),
            Package::new("npm", "@scope/tool", "2.0.0"),
        ]);
        sync(&root, &store, &both, &install, &mut |_, _| {}).unwrap();
        let bin = root.join(REPO_TOOLS_DIR).join("bin");
        assert!(bin.join("mdbook").exists());

        let one = set(&[Package::new("cargo", "mdbook", "1.0.0")]);
        sync(&root, &store, &one, &install, &mut |_, _| {}).unwrap();
        assert!(bin.join("mdbook").exists());
        assert!(
            !root
                .join(REPO_TOOLS_DIR)
                .join("pkgs/npm/@scope+tool@2.0.0")
                .exists()
        );
        assert_eq!(installed_versions(&root).len(), 1);
    }

    /// A failed install leaves nothing behind in the store: the next sync
    /// tries again rather than linking a half-finished install.
    #[test]
    fn failed_installs_leave_no_store_entry() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = tmp.path().join("store");
        let root = tmp.path().join("p");
        fs::create_dir_all(&root).unwrap();
        let pkgs = set(&[Package::new("cargo", "broken", "1.0.0")]);
        let failing = |_: &Package, prefix: &Path| -> Result<()> {
            fs::write(prefix.join("partial"), "x")?;
            anyhow::bail!("compile error")
        };
        let err = sync(&root, &store, &pkgs, &failing, &mut |_, _| {}).unwrap_err();
        assert!(format!("{err:#}").contains("compile error"), "{err:#}");
        let leftovers: Vec<_> = fs::read_dir(store.join("cargo")).unwrap().collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn two_packages_providing_one_executable_is_an_error() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = tmp.path().join("store");
        let root = tmp.path().join("p");
        fs::create_dir_all(&root).unwrap();
        let same_exe = |_: &Package, prefix: &Path| -> Result<()> {
            fs::create_dir_all(prefix.join("bin"))?;
            fs::write(prefix.join("bin").join("tool"), "")?;
            Ok(())
        };
        let pkgs = set(&[
            Package::new("cargo", "one", "1.0.0"),
            Package::new("npm", "two", "1.0.0"),
        ]);
        let err = sync(&root, &store, &pkgs, &same_exe, &mut |_, _| {}).unwrap_err();
        assert!(
            err.to_string().contains("provide the executable tool"),
            "{err}"
        );
    }

    /// Clearing the state directory keeps the tools and nothing else.
    #[test]
    fn clear_state_dir_keeps_tools() {
        let tmp = tempfile::TempDir::new().unwrap();
        let state = tmp.path().join(".rsconstruct");
        fs::create_dir_all(state.join("tools/bin")).unwrap();
        fs::create_dir_all(state.join("objects")).unwrap();
        fs::write(state.join("mtime.redb"), "").unwrap();
        assert!(clear_state_dir(&state).unwrap());
        let left: Vec<String> = fs::read_dir(&state)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["tools"]);

        let bare = tmp.path().join("bare");
        fs::create_dir_all(bare.join("objects")).unwrap();
        assert!(clear_state_dir(&bare).unwrap());
        assert!(!bare.exists());
        assert!(!clear_state_dir(&bare).unwrap());
    }

    /// A gem's bin entry is a launcher, not a symlink, and it scopes
    /// `GEM_HOME`/`GEM_PATH` to that package alone; it runs from any
    /// working directory.
    #[test]
    fn gem_executables_get_a_launcher_scoped_to_their_package() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = tmp.path().join("store");
        let root = tmp.path().join("p");
        fs::create_dir_all(&root).unwrap();
        // The stand-in binstub reports the gem environment it was run with.
        let install = |_: &Package, prefix: &Path| -> Result<()> {
            fs::create_dir_all(prefix.join("bin"))?;
            let stub = prefix.join("bin").join("mdl");
            fs::write(&stub, "#!/bin/sh\necho \"$GEM_HOME|$GEM_PATH|$1\"\n")?;
            crate::platform::set_permissions_mode(&stub, 0o755)?;
            Ok(())
        };
        let pkgs = set(&[Package::new("gem", "mdl", "0.12.0")]);
        sync(&root, &store, &pkgs, &install, &mut |_, _| {}).unwrap();

        let launcher = root.join(REPO_TOOLS_DIR).join("bin/mdl");
        let meta = fs::symlink_metadata(&launcher).unwrap();
        assert!(meta.is_file(), "a gem's bin entry must be a launcher");
        let out = std::process::Command::new(&launcher)
            .arg("x.md")
            .current_dir(tmp.path())
            .env("GEM_PATH", "/somewhere/else")
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        let pkg = root
            .join(REPO_TOOLS_DIR)
            .join("pkgs/gem/mdl@0.12.0")
            .canonicalize()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("{0}|{0}|x.md\n", pkg.display())
        );
    }
}
