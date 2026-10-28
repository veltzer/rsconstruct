//! `[dependencies]`: the packages a project declares by package manager, and
//! the readers for the manifests and lockfiles (`pyproject.toml`, `uv.lock`,
//! `package.json`, `package-lock.json`) that `tool install project` and
//! `doctor` take the rest of the package set from.
//!
//! There is no Python list here. A project's Python packages, the Python
//! tools its processors run included, are declared in pyproject.toml,
//! pinned by uv.lock, and installed by `uv sync` into the repo's `.venv`:
//! one environment, owned by uv.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Where `tool install project` takes the project's Node.js package set
/// from.
///
/// package.json declares the names, package-lock.json pins the closure, and
/// `npm ci` installs exactly the lock into the project's `node_modules/`.
/// Every rsconstruct invocation prepends that `node_modules/.bin` to its
/// PATH at startup, so the installed executables are what processors and
/// tool probes see.
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
/// Used by `rsconstruct doctor` to verify and `rsconstruct tool install` to install.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct DependenciesConfig {
    /// Node.js command-line packages installed into the project's tool
    /// directory (`.rsconstruct/tools`, see `tool_store`), on top of
    /// whatever the project's own package.json declares. The manifest is
    /// the normal home for a Node dependency; this list is for a tool the
    /// project does not declare there.
    #[serde(default)]
    pub npm: Vec<String>,
    /// Source of the project's Node.js package set: `"package-lock"`
    /// (default) installs the closure package-lock.json pins with `npm ci`;
    /// `"package-json"` resolves package.json's ranges with `npm install`.
    #[serde(default)]
    pub npm_source: NpmSource,
    /// Ruby gems, installed into the project's tool directory.
    #[serde(default)]
    pub gem: Vec<String>,
    /// Crates from crates.io, by crate name: the cargo subcommands and
    /// other Rust-built tools the project's build runs (`cargo-deny`,
    /// `cargo-nextest`, `mdbook`). Installed into the project's tool
    /// directory with `cargo install --locked`, at the version
    /// rsconstruct.lock pins; `--locked` builds each crate with its own
    /// Cargo.lock. Presence is judged by that directory, by crate, so a
    /// crate whose binary has a different name (`ripgrep` → `rg`) is
    /// handled the same.
    #[serde(default)]
    pub cargo: Vec<String>,
    /// System packages, installed onto the machine by `tool install
    /// machine` with the system package manager.
    #[serde(default)]
    pub system: Vec<String>,
    /// Wrap apt/dnf/pacman invocations with `eatmydata` to skip fsync calls
    /// and speed up package installs.
    ///
    /// Default: `false`. The post-config phase hook
    /// `eatmydata_ci_default` flips this to `true` when `CI=true` is in
    /// the environment. To turn the wrap off in CI, unset `CI` (or set
    /// it to a non-`true` value); to turn it on outside CI, run with
    /// `CI=true rsconstruct tool install machine`. The CLI flag
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
        self.npm.is_empty()
            && self.gem.is_empty()
            && self.cargo.is_empty()
            && self.system.is_empty()
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

/// Whether the project rooted at `project_root` has a Python environment to
/// set up: a `uv.lock`, or a pyproject.toml that declares dependencies. A
/// pyproject that declares dependencies without a `uv.lock` beside it is an
/// error, the lockfile being what makes the install reproducible.
pub fn has_python_deps(project_root: &Path) -> Result<bool> {
    let lock = project_root.join("uv.lock");
    if lock.exists() {
        return Ok(true);
    }
    let pyproject = project_root.join("pyproject.toml");
    if pyproject_python_deps(&pyproject)?.is_empty() {
        return Ok(false);
    }
    anyhow::bail!(
        "{} declares Python dependencies but {} does not exist; run `uv lock` to create it",
        pyproject.display(),
        lock.display(),
    );
}

/// The project's pinned Python closure, as `name==version` requirements:
/// what `uv sync` installs into the repo's `.venv`. Empty when the project
/// has no Python environment (see [`has_python_deps`]).
pub fn python_deps(project_root: &Path) -> Result<Vec<String>> {
    if has_python_deps(project_root)? {
        uv_lock_pinned_deps(&project_root.join("uv.lock"))
    } else {
        Ok(Vec::new())
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
/// `{ virtual = "." }`) is skipped — it is not a dependency of itself. Every other entry must be a registry package; a source kind
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
                "{}: package {name} has a source kind rsconstruct does not read \
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
