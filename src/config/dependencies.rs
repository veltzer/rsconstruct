//! `[dependencies]`: the packages a project declares by package manager, and
//! the readers for the manifests and lockfiles (`pyproject.toml`, `uv.lock`,
//! `package.json`, `package-lock.json`) that `install-deps` and `doctor`
//! take the rest of the package set from.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Where `install-deps` and `doctor` take the Python package set from.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PipSource {
    /// Install the exact pinned closure from `uv.lock` (plus any
    /// `[dependencies].pip` extras). A project that declares Python
    /// dependencies in pyproject.toml but has no `uv.lock` is an error:
    /// run `uv lock`, or set `pip_source = "pyproject"`.
    #[default]
    UvLock,
    /// Resolve `[dependencies].pip` plus pyproject.toml's declared
    /// dependency names at install time (the pre-lockfile behavior;
    /// versions float to whatever pip resolves that day).
    Pyproject,
}

/// Which tool `install-deps` hands the Python package set to.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PythonInstaller {
    /// Install with `uv pip install` (the default). uv applies uv's own
    /// resolution policy to the lock it produced, so a pin whose
    /// Requires-Python upper bound excludes the running interpreter (which
    /// uv deliberately ignores but pip enforces) still installs, and in
    /// uv-lock mode the pins come from `uv export`, whose environment
    /// markers make uv skip other platforms' packages instead of failing
    /// on them.
    #[default]
    Uv,
    /// Install with `pip install` (the pre-uv behavior). pip enforces
    /// Requires-Python bounds that uv ignored when locking, so a uv.lock
    /// pin can be uninstallable for pip even though uv itself would
    /// install it.
    Pip,
}

/// Where `install-deps` takes the project's Node.js package set from.
///
/// The Node analog of `PipSource`: package.json declares the names,
/// package-lock.json pins the closure, and `npm ci` installs exactly the
/// lock into the project's `node_modules/`. Every rsconstruct invocation
/// prepends that `node_modules/.bin` to its PATH at startup, so the
/// installed executables are what processors and tool probes see.
#[derive(Debug, Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NpmSource {
    /// Run `npm ci`: install the exact closure package-lock.json pins. A
    /// project whose package.json declares dependencies but has no
    /// package-lock.json is an error: run `npm install --package-lock-only`
    /// to create it, or set `npm_source = "package-json"`.
    #[default]
    PackageLock,
    /// Run `npm install`: resolve package.json's declared ranges at install
    /// time (versions float to whatever npm resolves that day).
    PackageJson,
}

/// Declared project dependencies by package manager.
/// Used by `rsconstruct doctor` to verify and `rsconstruct tool install-deps` to install.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct DependenciesConfig {
    /// Python packages (installed via pip)
    #[serde(default)]
    pub pip: Vec<String>,
    /// Source of the Python package set: `"uv-lock"` (default) installs the
    /// pinned closure from `uv.lock`; `"pyproject"` resolves the declared
    /// names at install time.
    #[serde(default)]
    pub pip_source: PipSource,
    /// Which tool installs the Python package set: `"uv"` (default) or
    /// `"pip"`. See `PythonInstaller` for why the two differ on the same
    /// lock.
    #[serde(default)]
    pub python_installer: PythonInstaller,
    /// Node.js packages installed globally with `npm install -g`, on top of
    /// whatever the project's own package.json declares. The manifest is
    /// the normal home for a Node dependency; this list is for a package
    /// that must be global.
    #[serde(default)]
    pub npm: Vec<String>,
    /// Source of the project's Node.js package set: `"package-lock"`
    /// (default) installs the closure package-lock.json pins with `npm ci`;
    /// `"package-json"` resolves package.json's ranges with `npm install`.
    #[serde(default)]
    pub npm_source: NpmSource,
    /// Ruby gems (installed via gem)
    #[serde(default)]
    pub gem: Vec<String>,
    /// Crates installed from crates.io with `cargo install --locked`, by
    /// crate name: the cargo subcommands and other Rust-built tools the
    /// project's build runs (`cargo-deny`, `cargo-nextest`, `mdbook`).
    /// Bare names only — the version floats to the latest release, and
    /// `--locked` builds each crate with its own Cargo.lock. Presence is
    /// judged by `cargo install --list`, so a crate whose binary has a
    /// different name (`ripgrep` → `rg`) is handled the same.
    #[serde(default)]
    pub cargo: Vec<String>,
    /// System packages (checked via `which`, not auto-installed)
    #[serde(default)]
    pub system: Vec<String>,
    /// Wrap apt/dnf/pacman invocations with `eatmydata` to skip fsync calls
    /// and speed up package installs.
    ///
    /// Default: `false`. The post-config phase hook
    /// `eatmydata_ci_default` flips this to `true` when `CI=true` is in
    /// the environment. To turn the wrap off in CI, unset `CI` (or set
    /// it to a non-`true` value); to turn it on outside CI, run with
    /// `CI=true rsconstruct tool install-deps`. The CLI flag
    /// `--no-eatmydata` always wins.
    ///
    /// This is not a normal user-facing config field — it's set by the
    /// runtime, not declared in `rsconstruct.toml`. (The deserializer
    /// still accepts it for round-trip compatibility, but setting it in
    /// the file is overridden by the phase hook.)
    #[serde(default)]
    pub eatmydata: bool,
}

impl DependenciesConfig {
    pub const fn is_empty(&self) -> bool {
        self.pip.is_empty()
            && self.npm.is_empty()
            && self.gem.is_empty()
            && self.cargo.is_empty()
            && self.system.is_empty()
    }

    /// The full pip requirement list for the project rooted at
    /// `project_root`, according to `pip_source`.
    ///
    /// In `uv-lock` mode (the default) the list is the `[dependencies].pip`
    /// entries followed by the exact pinned closure from `uv.lock` (see
    /// `uv_lock_pinned_deps`). A project that declares Python dependencies
    /// in pyproject.toml but has no `uv.lock` is an error — the lockfile is
    /// what makes CI installs reproducible.
    ///
    /// In `pyproject` mode the list is the `[dependencies].pip` entries
    /// followed by the dependency names pyproject.toml declares (see
    /// `pyproject_python_deps`), resolved by pip at install time.
    ///
    /// Both modes deduplicate by distribution name plus extras — the first
    /// occurrence wins, so a `[dependencies].pip` entry overrides a
    /// pyproject or lock one. Extras are part of the key because
    /// `pkg[extra]` pulls in dependencies that plain `pkg` does not, so the
    /// two are different install requests.
    pub fn effective_pip(&self, project_root: &Path) -> Result<Vec<String>> {
        let from_project = self.project_python_reqs(project_root, uv_lock_pinned_deps)?;
        Ok(self.merge_with_pip(from_project))
    }

    /// Like `effective_pip`, but for the uv installer: in uv-lock mode the
    /// pinned closure comes from `uv export` (run by `export`, since
    /// subprocess execution belongs to the caller's context) rather than
    /// from flattening the lock. The export carries environment markers, so
    /// uv skips another platform's pin (`pywin32 ; sys_platform == 'win32'`
    /// on Linux) where the flattened list would try to install it and fail.
    pub fn effective_pip_uv(
        &self,
        project_root: &Path,
        export: impl FnOnce() -> Result<Vec<String>>,
    ) -> Result<Vec<String>> {
        let from_project = self.project_python_reqs(project_root, |_lock| export())?;
        Ok(self.merge_with_pip(from_project))
    }

    /// The project-declared part of the Python set: `pinned` on the lock in
    /// uv-lock mode, pyproject's declared names in pyproject mode. Owns the
    /// "declared dependencies but no lock" error both modes' callers rely on.
    fn project_python_reqs(
        &self,
        project_root: &Path,
        pinned: impl FnOnce(&Path) -> Result<Vec<String>>,
    ) -> Result<Vec<String>> {
        let pyproject = project_root.join("pyproject.toml");
        match self.pip_source {
            PipSource::UvLock => {
                let lock = project_root.join("uv.lock");
                if lock.exists() {
                    pinned(&lock)
                } else if pyproject_python_deps(&pyproject)?.is_empty() {
                    Ok(Vec::new())
                } else {
                    anyhow::bail!(
                        "{} declares Python dependencies but {} does not exist; \
                         run `uv lock` to create it, or set `pip_source = \"pyproject\"` \
                         under [dependencies] to resolve the declared names at install time",
                        pyproject.display(),
                        lock.display(),
                    );
                }
            }
            PipSource::Pyproject => pyproject_python_deps(&pyproject),
        }
    }

    /// `[dependencies].pip` entries followed by `from_project`, deduplicated
    /// by distribution name plus extras — first occurrence wins, so a
    /// `[dependencies].pip` entry overrides a pyproject or lock one.
    fn merge_with_pip(&self, from_project: Vec<String>) -> Vec<String> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merged: Vec<String> = Vec::new();
        for req in self.pip.iter().cloned().chain(from_project) {
            let key = format!(
                "{}[{}]",
                normalized_distribution_name(&req),
                requirement_extras(&req)
            );
            if seen.insert(key) {
                merged.push(req);
            }
        }
        merged
    }

    /// The Node.js package names the project rooted at `project_root`
    /// declares in its package.json, according to `npm_source`, or an empty
    /// list when there is no package.json. Owns the "declared dependencies
    /// but no lock" error: in package-lock mode a manifest that declares
    /// packages without a package-lock.json beside it is an error, the same
    /// way pyproject.toml without uv.lock is.
    pub fn node_deps(&self, project_root: &Path) -> Result<Vec<String>> {
        let manifest = project_root.join("package.json");
        let names = package_json_deps(&manifest)?;
        if names.is_empty() {
            return Ok(names);
        }
        let lock = project_root.join("package-lock.json");
        if self.npm_source == NpmSource::PackageLock && !lock.exists() {
            anyhow::bail!(
                "{} declares Node.js dependencies but {} does not exist; \
                 run `npm install --package-lock-only` to create it, or set \
                 `npm_source = \"package-json\"` under [dependencies] to resolve \
                 the declared ranges at install time",
                manifest.display(),
                lock.display(),
            );
        }
        Ok(names)
    }
}

/// Node.js package names a `package.json` declares, or an empty list when
/// the file does not exist. Collects `dependencies`, `devDependencies` and
/// `optionalDependencies` — the three sections `npm ci` installs — in
/// that order.
pub fn package_json_deps(package_json: &Path) -> Result<Vec<String>> {
    if !package_json.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(package_json)
        .with_context(|| format!("Failed to read {}", package_json.display()))?;
    let root: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        crate::exit_code::config_error(format!("Failed to parse {}: {e}", package_json.display()))
    })?;
    let mut names: Vec<String> = Vec::new();
    for section in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(map) = root.get(section).and_then(serde_json::Value::as_object) {
            for name in map.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
    }
    Ok(names)
}

/// The versions a `package-lock.json` pins for the packages installed
/// directly under the project's `node_modules/`, by package name. Nested
/// installs (`node_modules/a/node_modules/b`) are another package's
/// private copy and are not top-level pins. Understands lockfile versions
/// 2 and 3, which both carry the `packages` map; a version 1 lock (npm 6)
/// has no such map and yields an empty result, so every declared package
/// then reads as not installed and `npm ci` runs.
pub fn package_lock_pins(lock: &Path) -> Result<std::collections::BTreeMap<String, String>> {
    let content =
        fs::read_to_string(lock).with_context(|| format!("Failed to read {}", lock.display()))?;
    let root: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        crate::exit_code::config_error(format!("Failed to parse {}: {e}", lock.display()))
    })?;
    let mut pins = std::collections::BTreeMap::new();
    let Some(packages) = root.get("packages").and_then(serde_json::Value::as_object) else {
        return Ok(pins);
    };
    for (path, entry) in packages {
        let Some(name) = path.strip_prefix("node_modules/") else {
            continue;
        };
        if name.contains("/node_modules/") {
            continue;
        }
        if let Some(version) = entry.get("version").and_then(serde_json::Value::as_str) {
            pins.insert(name.to_string(), version.to_string());
        }
    }
    Ok(pins)
}

/// The version of the package installed at `node_modules/<name>`, read
/// from its own package.json, or `None` when it is not installed.
pub fn installed_node_package_version(project_root: &Path, name: &str) -> Option<String> {
    let manifest = project_root
        .join("node_modules")
        .join(name)
        .join("package.json");
    let content = fs::read_to_string(manifest).ok()?;
    let root: serde_json::Value = serde_json::from_str(&content).ok()?;
    root.get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Parse the stdout of `uv export --format requirements.txt` into requirement
/// strings, one per line, environment markers included. Comment lines, blank
/// lines, and option lines (`-e .`, `--index-url ...`) are not requirements
/// and are dropped.
pub fn parse_uv_export(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('-'))
        .map(str::to_string)
        .collect()
}

/// Python dependencies declared in a `pyproject.toml`, or an empty list when
/// the file does not exist. Collects, in order: `[project].dependencies`,
/// every list in `[project.optional-dependencies]`, and every PEP 735
/// `[dependency-groups]` list. Group entries that are tables
/// (`{include-group = "..."}`) are skipped — the included group's own
/// entries are already collected because every group is read.
pub fn pyproject_python_deps(pyproject: &Path) -> Result<Vec<String>> {
    if !pyproject.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(pyproject)
        .with_context(|| format!("Failed to read {}", pyproject.display()))?;
    let root: toml::Value = toml::from_str(&content).map_err(|e| {
        crate::exit_code::config_error(format!("Failed to parse {}: {e}", pyproject.display()))
    })?;

    let string_items = |v: &toml::Value| -> Vec<String> {
        v.as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };

    let mut deps: Vec<String> = Vec::new();
    if let Some(project) = root.get("project") {
        if let Some(list) = project.get("dependencies") {
            deps.extend(string_items(list));
        }
        if let Some(extras) = project
            .get("optional-dependencies")
            .and_then(toml::Value::as_table)
        {
            for list in extras.values() {
                deps.extend(string_items(list));
            }
        }
    }
    if let Some(groups) = root
        .get("dependency-groups")
        .and_then(toml::Value::as_table)
    {
        for list in groups.values() {
            deps.extend(string_items(list));
        }
    }
    Ok(deps)
}

/// The pinned Python package closure of a `uv.lock` file, as
/// `name==version` requirement strings in the lock's order.
///
/// The project itself (`source = { editable = "." }` or
/// `{ virtual = "." }`) is skipped — it is not something install-deps
/// installs. Every other entry must be a registry package; a source kind
/// this function does not understand (git, path, url) is an error rather
/// than a silently dropped dependency.
pub fn uv_lock_pinned_deps(lock: &Path) -> Result<Vec<String>> {
    let content =
        fs::read_to_string(lock).with_context(|| format!("Failed to read {}", lock.display()))?;
    let root: toml::Value = toml::from_str(&content).map_err(|e| {
        crate::exit_code::config_error(format!("Failed to parse {}: {e}", lock.display()))
    })?;
    let packages = root
        .get("package")
        .and_then(toml::Value::as_array)
        .map_or(&[] as &[toml::Value], Vec::as_slice);
    let mut pins: Vec<String> = Vec::new();
    for pkg in packages {
        let name = pkg
            .get("name")
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{}: package entry without a name", lock.display()))?;
        let source = pkg.get("source").and_then(toml::Value::as_table);
        let is_project =
            source.is_some_and(|s| s.contains_key("editable") || s.contains_key("virtual"));
        if is_project {
            continue;
        }
        let is_registry = source.is_some_and(|s| s.contains_key("registry"));
        if !is_registry {
            anyhow::bail!(
                "{}: package {name} has a source kind install-deps does not support \
                 (only registry packages and the project itself are understood)",
                lock.display(),
            );
        }
        let version = pkg
            .get("version")
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{}: package {name} has no version", lock.display()))?;
        pins.push(format!("{name}=={version}"));
    }
    Ok(pins)
}

/// The normalized extras of a requirement string (`"gtts"` for
/// `"manim-voiceover[gtts]"`), sorted and comma-joined so equivalent extras
/// lists compare equal; empty when the requirement names no extras.
pub fn requirement_extras(requirement: &str) -> String {
    let Some(open) = requirement.find('[') else {
        return String::new();
    };
    let Some(close) = requirement[open..].find(']') else {
        return String::new();
    };
    let mut extras: Vec<String> = requirement[open + 1..open + close]
        .split(',')
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .collect();
    extras.sort();
    extras.join(",")
}

/// The PEP 503-normalized distribution name of a requirement string: the
/// name is everything before the first extras bracket, version specifier,
/// environment marker, or whitespace; runs of `-`, `_`, and `.` compare
/// equal and case is ignored.
pub fn normalized_distribution_name(requirement: &str) -> String {
    let name = requirement
        .split(['[', '<', '>', '=', '!', '~', ';', ' ', '\t'])
        .next()
        .unwrap_or(requirement);
    let mut normalized = String::with_capacity(name.len());
    let mut prev_sep = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            if !prev_sep {
                normalized.push('-');
            }
            prev_sep = true;
        } else {
            normalized.extend(c.to_lowercase());
            prev_sep = false;
        }
    }
    normalized
}
