//! External tool registry and installation engine.
//!
//! Lives at the crate root because nothing here depends on the `Processor`
//! trait, and every file under `src/processor/` must be a real processor.
//! This is the data and the mechanics of "what tools exist and how do we
//! install them", which `builder/tools.rs` orchestrates on top of.

/// A single way to install a tool.
pub struct InstallMethod {
    /// Package manager or method name (e.g., "pip", "apt", "npm", "snap", "cargo", "binary")
    pub method: &'static str,
    /// Package name for the package manager (e.g., "taplo-cli" for cargo, "texlive-latex-base" for apt)
    pub package: &'static str,
}

/// Where an install puts a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallTarget {
    /// Inside the project: its tool directory (`.rsconstruct/tools`), or
    /// the venv inside the repo.
    Project,
    /// Onto the machine: a system package manager, a user-wide prefix
    /// (`~/.cargo/bin`, `npm -g`, `gem --user-install`), or a venv outside
    /// the repo.
    Machine,
}

impl InstallTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Machine => "machine",
        }
    }
}

/// Context for executing an install: verbosity and eatmydata preference.
///
/// `sudo` is decided at execution time via `crate::platform::needs_sudo()`;
/// callers don't need to pass it in.
pub struct InstallCtx {
    /// When true, print each subprocess argv before running it.
    pub verbose: bool,
    /// When true, prepend `eatmydata` to system-package-manager calls
    /// (apt/dnf/pacman) for faster installs. No-op when eatmydata isn't
    /// on PATH.
    pub use_eatmydata: bool,
    /// The project's rsconstruct.lock entries for `binary` installs, by
    /// recipe name: a pinned recipe downloads the locked URL and must match
    /// the locked SHA-256. Empty outside a project.
    pub binary_pins: std::collections::BTreeMap<String, crate::tool_lock::BinaryPin>,
}

impl InstallMethod {
    pub const fn new(method: &'static str, package: &'static str) -> Self {
        Self { method, package }
    }

    /// The targets this method can install to. A system package manager
    /// only reaches the machine; a language package manager or a release
    /// binary reaches either, depending on the prefix (or venv) it is
    /// pointed at. A `.deb` binary is installed by apt, so it is machine
    /// only; a binary kept as a whole release tree needs a prefix of its
    /// own, so it is project only. Empty for `manual`, which installs
    /// nothing, and for anything unknown.
    pub fn targets(&self) -> &'static [InstallTarget] {
        const MACHINE: &[InstallTarget] = &[InstallTarget::Machine];
        const BOTH: &[InstallTarget] = &[InstallTarget::Project, InstallTarget::Machine];
        match self.method {
            "apt" | "dnf" | "pacman" | "brew" | "snap" => MACHINE,
            "pip" | "npm" | "cargo" | "gem" => BOTH,
            "uv-sync" | "npm-ci" | "npm-install" => &[InstallTarget::Project],
            "binary" => match binary_recipe(self.package) {
                Some(BinaryRecipe {
                    archive: ArchiveKind::Deb { .. },
                    ..
                }) => MACHINE,
                Some(BinaryRecipe {
                    archive: ArchiveKind::ZipTree { .. },
                    ..
                }) => &[InstallTarget::Project],
                Some(_) => BOTH,
                None => &[],
            },
            _ => &[],
        }
    }

    /// Human-readable preview of what installing this entry's package
    /// would do — first line of the first describe step. Lossy for
    /// multi-step installs (binary fetches), but adequate for `tool list`,
    /// which is registry-wide and so has no project lock to pin against.
    pub fn command(&self) -> String {
        let steps = describe(
            self.method,
            &[self.package],
            &std::collections::BTreeMap::new(),
        );
        steps.first().map_or_else(
            || format!("({}) {}", self.method, self.package),
            |argv| join_argv(argv),
        )
    }
}

impl ToolInfo {
    /// The tool's preferred target: `prefer` when it is set, otherwise the
    /// project when the tool has a project install, since a project runs
    /// its own tools and the machine is the last resort. None only for a
    /// tool with neither slot, which `every_slot_holds_a_method_for_its_target`
    /// rules out.
    pub fn preferred_target(&self) -> Option<InstallTarget> {
        self.prefer.or_else(|| {
            if self.project.is_some() {
                Some(InstallTarget::Project)
            } else if self.machine.is_some() {
                Some(InstallTarget::Machine)
            } else {
                None
            }
        })
    }

    /// The tool's install method for `target`, if it has one.
    pub const fn slot(&self, target: InstallTarget) -> Option<&InstallMethod> {
        match target {
            InstallTarget::Project => self.project.as_ref(),
            InstallTarget::Machine => self.machine.as_ref(),
        }
    }

    /// The method `tool install` uses for this tool: the one in the slot of
    /// its preferred target.
    pub fn preferred_method(&self) -> Option<&InstallMethod> {
        self.slot(self.preferred_target()?)
    }

    /// Every method the tool lists, each once, in the order project,
    /// machine, alternatives, with the targets it is listed for: a method
    /// that fills both slots appears once, for both.
    pub fn methods(&self) -> Vec<(&InstallMethod, Vec<InstallTarget>)> {
        let mut out: Vec<(&InstallMethod, Vec<InstallTarget>)> = Vec::new();
        for target in [InstallTarget::Project, InstallTarget::Machine] {
            if let Some(m) = self.slot(target) {
                match out
                    .iter_mut()
                    .find(|(seen, _)| seen.method == m.method && seen.package == m.package)
                {
                    Some((_, targets)) => targets.push(target),
                    None => out.push((m, vec![target])),
                }
            }
        }
        out.extend(self.alternatives.iter().map(|m| (m, Vec::new())));
        out
    }
}

/// Display the steps that `run(method, packages, ctx)` would execute,
/// without executing them. Used for logs and previews. `binary_pins` is
/// the lock's `[binary]` table (see [`InstallCtx::binary_pins`]): a pinned
/// recipe is described at its locked URL, exactly as `run` installs it.
pub fn describe(
    method: &str,
    packages: &[&str],
    binary_pins: &std::collections::BTreeMap<String, crate::tool_lock::BinaryPin>,
) -> Vec<Vec<String>> {
    let sudo = sudo_argv();
    let strs = |parts: &[&str]| -> Vec<String> { parts.iter().map(|s| (*s).to_string()).collect() };
    let prefix = |head: &[&str], tail: &[&str]| -> Vec<String> {
        sudo.iter()
            .chain(head.iter())
            .chain(tail.iter())
            .map(|s| (*s).to_string())
            .collect()
    };
    match method {
        "apt" => {
            // Run `apt-get update` first so the package index isn't stale.
            // CI runners ship with indexes a few days old; without this, an
            // `install` for a security-updated package 404s on the mirror.
            let update = prefix(&["apt-get", "update"], &[]);
            let mut install = prefix(&["apt-get", "install", "-y"], &[]);
            install.extend(packages.iter().map(|s| (*s).to_string()));
            vec![update, install]
        }
        "dnf" => {
            let mut argv = prefix(&["dnf", "install", "-y"], &[]);
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            vec![argv]
        }
        "pacman" => {
            let mut argv = prefix(&["pacman", "-S", "--noconfirm"], &[]);
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            vec![argv]
        }
        "brew" => {
            let mut argv = strs(&["brew", "install"]);
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            vec![argv]
        }
        "snap" => {
            let mut argv = prefix(&["snap", "install"], &[]);
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            vec![argv]
        }
        "pip" => vec![{
            let mut a = strs(&["pip", "install"]);
            a.extend(packages.iter().map(|s| (*s).to_string()));
            a
        }],
        "npm" => vec![{
            let mut a = strs(&["npm", "install", "-g"]);
            a.extend(packages.iter().map(|s| (*s).to_string()));
            a
        }],
        // The project's own package sets: these commands take no package
        // names, they read the manifest (and lockfile) in the working
        // directory. `packages` is the declared list, kept for the
        // skip/preview display only.
        "uv-sync" => vec![uv_sync_argv()],
        "npm-ci" => vec![strs(&["npm", "ci"])],
        "npm-install" => vec![strs(&["npm", "install"])],
        "cargo" => vec![{
            let mut a = cargo_install_argv();
            a.extend(packages.iter().map(|s| (*s).to_string()));
            a
        }],
        "gem" => vec![{
            let mut a = gem_install_argv();
            a.extend(packages.iter().map(|s| (*s).to_string()));
            a
        }],
        "binary" => packages
            .iter()
            .flat_map(|p| describe_binary(p, binary_pins))
            .collect(),
        "manual" => packages
            .iter()
            .map(|p| vec!["# manual:".to_string(), (*p).to_string()])
            .collect(),
        _ => packages
            .iter()
            .map(|p| vec![format!("# unknown method '{}':", method), (*p).to_string()])
            .collect(),
    }
}

/// Execute the install for `method` over `packages` (batched when the
/// method supports it). Each subprocess runs argv-style — never via a
/// shell. Returns Err on the first failure.
pub fn run(method: &str, packages: &[&str], ctx: &InstallCtx) -> anyhow::Result<()> {
    use anyhow::Context as _;
    use std::process::Command;
    if packages.is_empty() {
        return Ok(());
    }
    let exec = |argv: &[String]| -> anyhow::Result<()> {
        if ctx.verbose {
            println!("Running: {}", join_argv(argv));
        }
        // Deliberately NOT routed through run_command: installers must
        // inherit the terminal, both so `sudo` can prompt for a password and
        // so apt/dnf can render progress. Capturing would hang on the
        // password prompt. This is one of the two documented exceptions to
        // the "everything goes through the central runner" rule (see
        // docs/src/internal/architecture.md).
        let status = Command::new(&argv[0])
            .args(&argv[1..])
            .status()
            .with_context(|| format!("failed to spawn: {}", join_argv(argv)))?;
        if !status.success() {
            anyhow::bail!(
                "{} exited with code {}",
                join_argv(argv),
                status
                    .code()
                    .map_or_else(|| "unknown".to_string(), |c| c.to_string())
            );
        }
        Ok(())
    };
    let sudo = sudo_argv();
    let eatmydata: &[&str] = if ctx.use_eatmydata && which::which("eatmydata").is_ok() {
        &["eatmydata"]
    } else {
        &[]
    };
    let pkgmgr_argv = |head: &[&str]| -> Vec<String> {
        sudo.iter()
            .chain(eatmydata.iter())
            .chain(head.iter())
            .map(|s| (*s).to_string())
            .collect()
    };
    match method {
        "apt" => {
            // Refresh the package index before installing — CI runners ship
            // with stale indexes that 404 on security-updated packages.
            let update = pkgmgr_argv(&["apt-get", "update"]);
            exec(&update)?;
            let mut argv = pkgmgr_argv(&["apt-get", "install", "-y"]);
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            exec(&argv)
        }
        "dnf" => {
            let mut argv = pkgmgr_argv(&["dnf", "install", "-y"]);
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            exec(&argv)
        }
        "pacman" => {
            let mut argv = pkgmgr_argv(&["pacman", "-S", "--noconfirm"]);
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            exec(&argv)
        }
        "brew" => {
            // brew is macOS-only; no sudo, no eatmydata.
            let mut argv = vec!["brew".to_string(), "install".to_string()];
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            exec(&argv)
        }
        "snap" => {
            // snap doesn't need eatmydata.
            let mut argv: Vec<String> = sudo
                .iter()
                .chain(["snap", "install"].iter())
                .map(|s| (*s).to_string())
                .collect();
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            exec(&argv)
        }
        "pip" => {
            let mut argv = vec!["pip".to_string(), "install".to_string()];
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            exec(&argv)
        }
        "npm" => {
            let mut argv = vec!["npm".to_string(), "install".to_string(), "-g".to_string()];
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            exec(&argv)
        }
        // See `describe`: these read the project's manifest and lock, so
        // the declared names are not passed (to `npm install` they would
        // mean "add these to package.json").
        "uv-sync" => exec(&uv_sync_argv()),
        "npm-ci" => exec(&["npm".to_string(), "ci".to_string()]),
        "npm-install" => exec(&["npm".to_string(), "install".to_string()]),
        "cargo" => {
            let mut argv = cargo_install_argv();
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            exec(&argv)
        }
        "gem" => {
            let mut argv = gem_install_argv();
            argv.extend(packages.iter().map(|s| (*s).to_string()));
            exec(&argv)
            // The bin dir a `--user-install` just created becomes visible to
            // the NEXT rsconstruct invocation, via the PATH augmentation at
            // startup. Nothing in this process probes for the tool after
            // installing it, and re-augmenting here would mutate the
            // environment with tokio threads alive (see platform::set_env).
        }
        "binary" => {
            for pkg in packages {
                run_binary(pkg, ctx)?;
            }
            Ok(())
        }
        "manual" => anyhow::bail!(
            "method '{}' is manual-only — install these packages by hand: {}",
            method,
            packages.join(", ")
        ),
        other => anyhow::bail!("unknown install method '{other}'"),
    }
}

/// Install `pkg` into the empty directory `prefix`, the way a project's
/// tool directory holds it (see `tool_store`): its executables end up in
/// `prefix/bin`, and nothing outside `prefix` is written — so no sudo, and
/// no global state for another project to disagree with.
pub fn install_into_prefix(
    pkg: &crate::tool_store::Package,
    prefix: &std::path::Path,
    ctx: &InstallCtx,
) -> anyhow::Result<()> {
    let root = prefix.to_string_lossy();
    let (name, version) = (pkg.name.as_str(), pkg.version.as_str());
    let request = format!("{name}@{version}");
    match pkg.method.as_str() {
        // --locked for the reason given at `cargo_install_argv`; no --force,
        // the root is empty.
        "cargo" => exec_inherited_env(
            &["cargo", "install", "--locked", "--root", &root, &request],
            &[],
            ctx.verbose,
        ),
        // --global with --prefix: the global layout (lib/node_modules plus
        // bin/ shims) under a prefix of our choosing.
        "npm" => exec_inherited_env(
            &["npm", "install", "--global", "--prefix", &root, &request],
            &[],
            ctx.verbose,
        ),
        // GEM_HOME/GEM_PATH name only the prefix, so every dependency is
        // installed into it rather than satisfied by whatever gems this
        // machine happens to have; the result runs anywhere GEM_PATH
        // includes it (see `tool_store::augment_env`).
        "gem" => {
            let bin = prefix.join("bin");
            exec_inherited_env(
                &[
                    "gem",
                    "install",
                    "--install-dir",
                    &root,
                    "--bindir",
                    &bin.to_string_lossy(),
                    "--no-document",
                    "--version",
                    version,
                    name,
                ],
                &[("GEM_HOME", &root), ("GEM_PATH", &root)],
                ctx.verbose,
            )
        }
        "binary" => install_binary_into_prefix(name, version, prefix, ctx),
        other => anyhow::bail!("install method '{other}' does not install into a project"),
    }
}

/// The `binary` case of [`install_into_prefix`]: download the locked asset,
/// check both hashes the lock records, and put the executable in
/// `prefix/bin`. `version` is `sha256-<executable hash>`, as
/// `tool_store::Package` records a binary.
fn install_binary_into_prefix(
    name: &str,
    version: &str,
    prefix: &std::path::Path,
    ctx: &InstallCtx,
) -> anyhow::Result<()> {
    use anyhow::Context as _;
    let recipe = binary_recipe(name)
        .ok_or_else(|| anyhow::anyhow!("no binary install recipe for '{name}'"))?;
    let pin = ctx
        .binary_pins
        .get(name)
        .with_context(|| format!("binary {name} has no rsconstruct.lock entry"))?;
    let work = prefix.join(".work");
    std::fs::create_dir_all(&work)
        .with_context(|| format!("Failed to create {}", work.display()))?;
    let fetched = fetch_binary(
        &recipe,
        &pin.url,
        Some(&pin.sha256),
        &work.to_string_lossy(),
        ctx.verbose,
    )?;
    let payload = std::path::Path::new(&fetched.payload);
    let sha = file_sha256(payload)?;
    if version != format!("sha256-{sha}") {
        anyhow::bail!(
            "the executable unpacked from {} has sha256 {sha}, but rsconstruct.lock wants {version}",
            pin.url
        );
    }
    if matches!(recipe.archive, ArchiveKind::ZipTree { .. }) {
        // The unpacked tree is the prefix: its top-level entries move up.
        let tree = std::path::PathBuf::from(tree_dir(&work.to_string_lossy(), recipe.dest));
        let entries = std::fs::read_dir(&tree)
            .with_context(|| format!("Failed to read directory {}", tree.display()))?;
        for entry in entries {
            let entry =
                entry.with_context(|| format!("Failed to read directory {}", tree.display()))?;
            let dest = prefix.join(entry.file_name());
            std::fs::rename(entry.path(), &dest).with_context(|| {
                format!(
                    "Failed to move {} into place as {}",
                    entry.path().display(),
                    dest.display()
                )
            })?;
        }
    } else {
        let bin = prefix.join("bin");
        std::fs::create_dir_all(&bin)
            .with_context(|| format!("Failed to create {}", bin.display()))?;
        let dest = bin.join(recipe.dest);
        std::fs::rename(payload, &dest).with_context(|| {
            format!(
                "Failed to move {} into place as {}",
                payload.display(),
                dest.display()
            )
        })?;
    }
    std::fs::remove_dir_all(&work).with_context(|| format!("Failed to remove {}", work.display()))
}

fn sudo_argv() -> &'static [&'static str] {
    if crate::platform::needs_sudo() {
        &["sudo"]
    } else {
        &[]
    }
}

/// The argv prefix for installing crates. `--locked` builds each crate with
/// the Cargo.lock it was published with instead of re-resolving its
/// dependencies on the installing machine, so the same crate version yields
/// the same binary everywhere. `--force` makes cargo replace a binary of the
/// same name that it did not install itself — without it cargo refuses with
/// "binary `x` already exists in destination", which is exactly what a CI
/// cache holding a prebuilt nextest tarball produced once the crate list
/// took over. It never causes extra work: only tools absent from PATH reach
/// this command, so the build was going to happen anyway. This is the
/// machine install of `tool install machine`; a project's crates
/// go into its tool directory instead (see [`install_into_prefix`]). Used by
/// both `describe` and `run` so the printed plan matches what executes.
fn cargo_install_argv() -> Vec<String> {
    vec![
        "cargo".to_string(),
        "install".to_string(),
        "--locked".to_string(),
        "--force".to_string(),
    ]
}

/// The argv prefix for installing gems. Appends `--user-install` when a
/// plain `gem install` would die on an unwritable default gem dir — the
/// apt-ruby case, where gems go to root-owned `/var/lib/gems` and rsconstruct
/// deliberately doesn't sudo-wrap language package managers. Used by both
/// `describe` and `run` so the printed plan matches what executes.
fn gem_install_argv() -> Vec<String> {
    let mut argv = vec!["gem".to_string(), "install".to_string()];
    if gem_needs_user_install() {
        argv.push("--user-install".to_string());
    }
    argv
}

/// The project install of the repo's Python packages: `uv sync` builds the
/// repo's `.venv` from uv.lock, the code's dependencies and the Python
/// tools its processors run together. `--locked` fails when uv.lock is out
/// of date with pyproject.toml instead of quietly re-resolving it.
/// `--no-install-project` leaves the project itself out: building it is
/// the build's job, not the install's, and a package whose build needs
/// something CI does not have (a build hook demanding an uncommitted
/// `client_secret.json`) failed the install outright when it was in. Used
/// by both `describe` and `run` so the printed plan matches what executes.
fn uv_sync_argv() -> Vec<String> {
    ["uv", "sync", "--locked", "--no-install-project"]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

/// Whether the default gem dir is unwritable for the current user (never
/// true for root). Asks `gem env gemdir` once per process; a missing or
/// broken `gem` reports false and lets the install fail with gem's own
/// message.
fn gem_needs_user_install() -> bool {
    static NEEDS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *NEEDS.get_or_init(|| {
        if crate::platform::is_root() {
            return false;
        }
        let Ok(out) = std::process::Command::new("gem")
            .args(["env", "gemdir"])
            .output()
        else {
            return false;
        };
        if !out.status.success() {
            return false;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let gemdir = std::path::Path::new(stdout.trim());
        if gemdir.as_os_str().is_empty() {
            return false;
        }
        !nearest_existing_ancestor_is_writable(gemdir)
    })
}

/// `access(W_OK)` on the path, or — when it doesn't exist yet — on its
/// nearest existing ancestor, which is what decides whether the path could
/// be created.
fn nearest_existing_ancestor_is_writable(path: &std::path::Path) -> bool {
    let mut p = path;
    loop {
        if p.exists() {
            return crate::platform::path_is_writable(p);
        }
        match p.parent() {
            Some(parent) => p = parent,
            None => return false,
        }
    }
}

/// Make the project's own Node.js executables resolvable: prepend
/// `./node_modules/.bin`, when it exists, to this process's PATH, so
/// `which`-based tool probes and every spawned child (tools, processor
/// scripts) see the packages `tool install project` put there from
/// package.json.
/// Prepended, not appended: the version the project's lock pins must win
/// over a global install of the same tool, which is the whole point of
/// installing from the lock. This is what `npm run` does for scripts.
///
/// Called once at the top of `main()`, before any thread exists — the
/// safety condition of `platform::set_env`.
pub fn augment_path_with_node_modules_bin() {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    let bin = cwd.join("node_modules").join(".bin");
    if !bin.is_dir() {
        return;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    if let Some(new_path) = path_with_prepended_dir(&path, &bin) {
        crate::platform::set_env("PATH", &new_path);
    }
}

/// Make the repo's Python environment the one everything runs in: prepend
/// `./.venv/bin` to this process's PATH. That is where `tool install
/// project` (`uv sync`) puts the code's dependencies and the Python tools
/// its processors run, and with it first, `python3`, `ruff` and `pytest`
/// resolve to the locked copies with no venv activated, in a shell or on a
/// CI runner alike.
///
/// Only in a project directory (one with rsconstruct.toml), and like the
/// tool directory's `bin/`, even before `.venv` exists, so the environment
/// `uv sync` creates in this very invocation is found by what it runs next.
///
/// Called once at the top of `main()`, before any thread exists — the
/// safety condition of `platform::set_env`.
pub fn augment_path_with_repo_venv() {
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    if !cwd.join(crate::config::CONFIG_FILE).exists() {
        return;
    }
    let bin = cwd.join(".venv").join("bin");
    let path = std::env::var_os("PATH").unwrap_or_default();
    if let Some(new_path) = path_with_prepended_dir(&path, &bin) {
        crate::platform::set_env("PATH", &new_path);
    }
}

/// `path` with `dir` moved to the front, or `None` when it is already
/// first. An occurrence further back is dropped rather than duplicated.
pub fn path_with_prepended_dir(
    path: &std::ffi::OsStr,
    dir: &std::path::Path,
) -> Option<std::ffi::OsString> {
    let existing: Vec<std::path::PathBuf> = std::env::split_paths(path).collect();
    if existing.first().is_some_and(|first| first == dir) {
        return None;
    }
    let rest = existing.into_iter().filter(|p| p != dir);
    // join_paths only errors on an entry containing the separator, which
    // can't happen: every entry came from split_paths or is a real dir.
    std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(rest)).ok()
}

/// Make user-installed gem executables resolvable: append the user gem bin
/// dirs that exist to this process's PATH, so `which`-based tool probes and
/// every spawned child (tools, processor scripts) see them. `gem install
/// --user-install` puts binaries in a dir that is on nobody's PATH by
/// default, which would otherwise leave a just-installed tool reported as
/// missing.
///
/// Called once at the top of `main()`, before any thread exists — the
/// safety condition of `platform::set_env`.
pub fn augment_path_with_user_gem_bins() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let gem_home = std::env::var_os("GEM_HOME");
    let dirs = user_gem_bin_dirs(std::path::Path::new(&home), gem_home.as_deref());
    if dirs.is_empty() {
        return;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    if let Some(new_path) = path_with_appended_dirs(&path, &dirs) {
        crate::platform::set_env("PATH", &new_path);
    }
}

/// Existing user-level gem executable dirs: `$GEM_HOME/bin` plus the
/// per-ruby-version `bin` dirs of both user-install layouts —
/// `~/.gem/ruby/<version>/bin` (upstream rubygems) and
/// `~/.local/share/gem/ruby/<version>/bin` (the Debian/Ubuntu XDG patch).
/// Only dirs that exist are returned; globbing the layouts costs two
/// readdirs and avoids spawning ruby at every startup.
fn user_gem_bin_dirs(
    home: &std::path::Path,
    gem_home: Option<&std::ffi::OsStr>,
) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(gem_home) = gem_home {
        let bin = std::path::Path::new(gem_home).join("bin");
        if bin.is_dir() {
            dirs.push(bin);
        }
    }
    for base in [home.join(".gem/ruby"), home.join(".local/share/gem/ruby")] {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        let mut versions: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path().join("bin"))
            .filter(|p| p.is_dir())
            .collect();
        versions.sort();
        dirs.append(&mut versions);
    }
    dirs
}

/// `path` with the dirs not already on it appended, or None when every dir
/// is already present. Append rather than prepend: a system-installed tool
/// keeps winning over a user gem of the same name.
fn path_with_appended_dirs(
    path: &std::ffi::OsStr,
    dirs: &[std::path::PathBuf],
) -> Option<std::ffi::OsString> {
    let existing: Vec<std::path::PathBuf> = std::env::split_paths(path).collect();
    let missing: Vec<std::path::PathBuf> = dirs
        .iter()
        .filter(|d| !existing.contains(d))
        .cloned()
        .collect();
    if missing.is_empty() {
        return None;
    }
    // join_paths only errors on an entry containing the separator, which
    // can't happen: every entry came from split_paths or is a real dir.
    std::env::join_paths(existing.into_iter().chain(missing)).ok()
}

fn join_argv(argv: &[String]) -> String {
    argv.join(" ")
}

/// The directory `run_binary` downloads and unpacks into.
const INSTALL_WORK_DIR: &str = "/tmp";

/// Description of the steps that `run_binary` would execute, for the
/// install plan `tool install` prints and the `-i` prompt shows. Built
/// from the same [`FetchPlan`] `fetch_binary` executes, at the same URL
/// `run_binary` downloads (the locked one when `binary_pins` has an entry),
/// so the preview cannot drift from the install. The one thing it cannot
/// show is a GitHub-release `.deb`'s asset URL, which is resolved from the
/// releases API at install time: a preview must not make that call.
fn describe_binary(
    pkg: &str,
    binary_pins: &std::collections::BTreeMap<String, crate::tool_lock::BinaryPin>,
) -> Vec<Vec<String>> {
    let Some(recipe) = binary_recipe(pkg) else {
        return vec![vec![format!("# unknown binary recipe '{pkg}'")]];
    };
    let mut steps: Vec<Vec<String>> = Vec::new();
    let pin = binary_pins.get(pkg);
    let url = match (pin, &recipe.archive) {
        (Some(pin), _) => pin.url.clone(),
        (
            None,
            ArchiveKind::Deb {
                source:
                    DebSource::GithubRelease {
                        repo,
                        asset_pattern,
                    },
            },
        ) => {
            steps.push(vec![format!(
                "# resolve latest '{asset_pattern}' .deb asset from {repo}"
            )]);
            "<resolved-asset-url>".to_string()
        }
        (
            None,
            ArchiveKind::Deb {
                source: DebSource::Url(url),
            },
        ) => (*url).to_string(),
        (None, _) => recipe.url.to_string(),
    };
    let plan = fetch_plan(&recipe, &url, INSTALL_WORK_DIR);
    steps.push(plan.curl);
    if let Some(pin) = pin {
        steps.push(vec![format!(
            "# sha256 of {} must be {} (rsconstruct.lock)",
            plan.download, pin.sha256
        )]);
    }
    steps.extend(plan.unpack.iter().map(Step::argv));
    if matches!(recipe.archive, ArchiveKind::ZipTree { .. }) {
        steps.push(vec![format!(
            "# {} becomes the package's prefix in {}",
            tree_dir(INSTALL_WORK_DIR, recipe.dest),
            crate::tool_store::REPO_TOOLS_DIR
        )]);
    } else {
        steps.push(install_argv(&recipe, &plan.payload));
    }
    steps
}

/// One step of turning a downloaded file into an installable one.
/// `fetch_binary` runs these and `describe_binary` prints them, from the
/// same [`FetchPlan`], so what is shown is what happens.
enum Step {
    /// Spawn argv with the terminal inherited.
    Exec(Vec<String>),
    Rename {
        from: String,
        to: String,
    },
    Remove(String),
    /// `chmod 0755`.
    MakeExecutable(String),
}

impl Step {
    /// The step as the command it amounts to, for previews.
    fn argv(&self) -> Vec<String> {
        match self {
            Self::Exec(argv) => argv.clone(),
            Self::Rename { from, to } => vec!["mv".to_string(), from.clone(), to.clone()],
            Self::Remove(path) => vec!["rm".to_string(), path.clone()],
            Self::MakeExecutable(path) => {
                vec!["chmod".to_string(), "+x".to_string(), path.clone()]
            }
        }
    }

    fn run(&self, verbose: bool) -> anyhow::Result<()> {
        use anyhow::Context as _;
        match self {
            Self::Exec(argv) => exec_inherited(
                &argv.iter().map(String::as_str).collect::<Vec<_>>(),
                verbose,
            ),
            Self::Rename { from, to } => {
                std::fs::rename(from, to).with_context(|| format!("rename {from} -> {to}"))
            }
            Self::Remove(path) => {
                std::fs::remove_file(path).with_context(|| format!("remove {path}"))
            }
            Self::MakeExecutable(path) => {
                crate::platform::set_permissions_mode(std::path::Path::new(path), 0o755)
                    .with_context(|| format!("chmod +x {path}"))
            }
        }
    }
}

/// How a recipe's payload gets from its URL to an installable file under
/// a work directory.
struct FetchPlan {
    /// Where curl puts the download: the archive, the binary itself, or
    /// the `.deb`.
    download: String,
    /// The curl argv that fetches `download`.
    curl: Vec<String>,
    /// What turns `download` into `payload`, in order, after the download's
    /// hash has been checked. Empty for a `.deb`, which apt installs as is.
    unpack: Vec<Step>,
    /// The executable ready to move into place, or the `.deb` for apt.
    payload: String,
}

/// The plan for `recipe`, downloading `url` into `base`.
fn fetch_plan(recipe: &BinaryRecipe, url: &str, base: &str) -> FetchPlan {
    let dest = recipe.dest;
    let final_tmp = format!("{base}/{dest}");
    let exec = |parts: &[&str]| Step::Exec(parts.iter().map(|s| (*s).to_string()).collect());
    let (download, unpack) = match recipe.archive {
        // A .deb is installed by apt as is: apt pulls the dependency tree
        // a bare binary would be missing, and the payload is not a single
        // executable.
        ArchiveKind::Deb { .. } => {
            let download = format!("{base}/{dest}.deb");
            return FetchPlan {
                curl: crate::download::curl_argv(url, &download),
                payload: download.clone(),
                download,
                unpack: Vec::new(),
            };
        }
        ArchiveKind::TarGz { inner } | ArchiveKind::Zip { inner } => {
            let download = format!("{base}/{dest}.dl");
            let extract = match recipe.archive {
                ArchiveKind::TarGz { .. } => exec(&["tar", "-xzf", &download, "-C", base, inner]),
                _ => exec(&["unzip", "-o", &download, inner, "-d", base]),
            };
            let mut unpack = vec![extract];
            // The member lands at <base>/<inner>, which need not be <dest>
            // (oxlint's is named after the target triple).
            let inner_path = format!("{base}/{inner}");
            if inner_path != final_tmp {
                unpack.push(Step::Rename {
                    from: inner_path,
                    to: final_tmp.clone(),
                });
            }
            unpack.push(Step::Remove(download.clone()));
            (download, unpack)
        }
        ArchiveKind::Gunzip => {
            // gunzip insists on a .gz name and strips it: the download is
            // renamed to <dest>.dl.gz, so gunzip leaves <dest>.dl, which
            // then moves to <dest>.
            let download = format!("{base}/{dest}.dl");
            let gz = format!("{download}.gz");
            let unpack = vec![
                Step::Rename {
                    from: download.clone(),
                    to: gz.clone(),
                },
                exec(&["gunzip", "-f", &gz]),
                Step::Rename {
                    from: download.clone(),
                    to: final_tmp.clone(),
                },
            ];
            (download, unpack)
        }
        ArchiveKind::Raw => {
            let download = format!("{base}/{dest}.dl");
            let unpack = vec![Step::Rename {
                from: download.clone(),
                to: final_tmp.clone(),
            }];
            (download, unpack)
        }
        // The whole archive is unpacked; the executable stays in its tree.
        ArchiveKind::ZipTree { exe } => {
            let download = format!("{base}/{dest}.dl");
            let tree = tree_dir(base, dest);
            let payload = format!("{tree}/{exe}");
            return FetchPlan {
                curl: crate::download::curl_argv(url, &download),
                unpack: vec![
                    exec(&["unzip", "-q", "-o", &download, "-d", &tree]),
                    Step::Remove(download.clone()),
                    Step::MakeExecutable(payload.clone()),
                ],
                download,
                payload,
            };
        }
    };
    let mut unpack = unpack;
    unpack.push(Step::MakeExecutable(final_tmp.clone()));
    FetchPlan {
        curl: crate::download::curl_argv(url, &download),
        download,
        unpack,
        payload: final_tmp,
    }
}

/// Where a [`ArchiveKind::ZipTree`] recipe unpacks its archive under `base`.
fn tree_dir(base: &str, dest: &str) -> String {
    format!("{base}/{dest}.tree")
}

/// The command that installs a fetched payload: apt for a `.deb`, a move
/// into `/usr/local/bin` for an executable. Shared by `run_binary` and
/// `describe_binary`. Not for a [`ArchiveKind::ZipTree`], which only a
/// project's tool directory installs.
fn install_argv(recipe: &BinaryRecipe, payload: &str) -> Vec<String> {
    let mut argv: Vec<String> = sudo_argv().iter().map(|s| (*s).to_string()).collect();
    if matches!(recipe.archive, ArchiveKind::Deb { .. }) {
        argv.extend(["apt-get", "install", "-y", payload].map(str::to_string));
    } else {
        argv.extend([
            "mv".to_string(),
            payload.to_string(),
            format!("/usr/local/bin/{}", recipe.dest),
        ]);
    }
    argv
}

/// The GitHub API's description of `repo`'s latest release.
fn latest_github_release(repo: &str) -> anyhow::Result<serde_json::Value> {
    crate::download::get_json(&format!(
        "https://api.github.com/repos/{repo}/releases/latest"
    ))
}

/// The `releases/latest/download/` infix of a GitHub URL that always serves
/// the newest release's asset of that name.
const GITHUB_LATEST_DOWNLOAD: &str = "/releases/latest/download/";

/// Rewrite a `https://github.com/<owner>/<repo>/releases/latest/download/<asset>`
/// URL to the same asset of release `tag`, or None when `url` is not one.
fn pin_github_latest_url(url: &str, tag: &str) -> Option<String> {
    let rest = url.strip_prefix("https://github.com/")?;
    let at = rest.find(GITHUB_LATEST_DOWNLOAD)?;
    let repo = &rest[..at];
    let asset = &rest[at + GITHUB_LATEST_DOWNLOAD.len()..];
    Some(format!(
        "https://github.com/{repo}/releases/download/{tag}/{asset}"
    ))
}

/// The `<owner>/<repo>` of a `https://github.com/<owner>/<repo>/...` URL.
fn github_repo_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://github.com/")?;
    let owner_end = rest.find('/')?;
    let repo_end = rest[owner_end + 1..]
        .find('/')
        .map_or(rest.len(), |i| owner_end + 1 + i);
    Some(&rest[..repo_end])
}

/// Resolve the browser-download URL of the newest release asset in `repo`
/// whose name contains `asset_pattern` and ends in `.deb`. Resolving at
/// install time (instead of pinning a URL in the recipe) is what keeps the
/// recipe from breaking every time upstream cuts a release.
fn resolve_latest_deb_asset(repo: &str, asset_pattern: &str) -> anyhow::Result<String> {
    use anyhow::Context as _;
    // Authenticated and retried by download::get_json; see there for why.
    let release = latest_github_release(repo)?;
    release["assets"]
        .as_array()
        .with_context(|| format!("GitHub release for {repo} has no 'assets' array"))?
        .iter()
        .find_map(|a| {
            let name = a["name"].as_str()?;
            let is_deb = std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("deb"));
            (name.contains(asset_pattern) && is_deb)
                .then(|| a["browser_download_url"].as_str())?
                .map(std::string::ToString::to_string)
        })
        .with_context(|| {
            format!("No .deb asset matching '{asset_pattern}' in the latest {repo} release")
        })
}

/// Run `argv` with the terminal inherited, failing on a non-zero exit.
///
/// Same exception as `run()` above: the binary installer shells out to
/// `sudo mv` / `sudo apt-get`, which need the terminal for a password
/// prompt, so these steps cannot go through the capturing central runner.
fn exec_inherited(argv: &[&str], verbose: bool) -> anyhow::Result<()> {
    exec_inherited_env(argv, &[], verbose)
}

/// [`exec_inherited`] with extra environment variables for the child.
fn exec_inherited_env(argv: &[&str], env: &[(&str, &str)], verbose: bool) -> anyhow::Result<()> {
    use anyhow::Context as _;
    if verbose {
        println!("Running: {}", argv.join(" "));
    }
    let status = std::process::Command::new(argv[0])
        .args(&argv[1..])
        .envs(env.iter().copied())
        .status()
        .with_context(|| format!("failed to spawn: {}", argv.join(" ")))?;
    if !status.success() {
        anyhow::bail!(
            "{} exited with code {}",
            argv.join(" "),
            status
                .code()
                .map_or_else(|| "unknown".to_string(), |c| c.to_string())
        );
    }
    Ok(())
}

/// SHA-256 of a file's bytes, hex-encoded, streamed rather than read whole
/// (a `.deb` payload runs to hundreds of megabytes).
pub fn file_sha256(path: &std::path::Path) -> anyhow::Result<String> {
    use anyhow::Context as _;
    use sha2::Digest as _;
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {} for hashing", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .with_context(|| format!("Failed to read {} for hashing", path.display()))?;
    Ok(hex::encode(hasher.finalize()))
}

/// A binary recipe's payload, downloaded and unpacked into a work directory
/// but not yet installed.
#[derive(Debug)]
struct FetchedBinary {
    /// SHA-256 of the file as downloaded (the archive, for an archive).
    download_sha256: String,
    /// The executable ready to move into place, or the `.deb` for apt.
    payload: String,
}

/// Download `url` into `base` and unpack `recipe`'s executable from it,
/// following [`fetch_plan`]. With `expected_sha256`, a download whose hash
/// differs is an error: the lock said which bytes to install, and these
/// are not they.
///
/// Shared by `run_binary` (base [`INSTALL_WORK_DIR`], the paths
/// `describe_binary` shows) and `tool lock` (a private directory), so a
/// locked hash is always the hash of exactly what an install downloads.
fn fetch_binary(
    recipe: &BinaryRecipe,
    url: &str,
    expected_sha256: Option<&str>,
    base: &str,
    verbose: bool,
) -> anyhow::Result<FetchedBinary> {
    let plan = fetch_plan(recipe, url, base);
    exec_inherited(
        &plan.curl.iter().map(String::as_str).collect::<Vec<_>>(),
        verbose,
    )?;
    let download_sha256 = file_sha256(std::path::Path::new(&plan.download))?;
    if let Some(expected) = expected_sha256
        && expected != download_sha256
    {
        std::fs::remove_file(&plan.download).ok();
        anyhow::bail!(
            "{url} does not match rsconstruct.lock: sha256 {download_sha256}, locked {expected}"
        );
    }
    for step in &plan.unpack {
        step.run(verbose)?;
    }
    Ok(FetchedBinary {
        download_sha256,
        payload: plan.payload,
    })
}

/// The URL `pkg`'s recipe downloads when no lock entry says otherwise: a
/// GitHub-release `.deb` resolved to today's latest asset, a vendor URL as
/// is, and anything else straight from the recipe.
fn unpinned_binary_url(recipe: &BinaryRecipe) -> anyhow::Result<String> {
    match recipe.archive {
        ArchiveKind::Deb {
            source:
                DebSource::GithubRelease {
                    repo,
                    asset_pattern,
                },
        } => resolve_latest_deb_asset(repo, asset_pattern),
        ArchiveKind::Deb {
            source: DebSource::Url(url),
        } => Ok(url.to_string()),
        _ => Ok(recipe.url.to_string()),
    }
}

fn run_binary(pkg: &str, ctx: &InstallCtx) -> anyhow::Result<()> {
    let recipe = binary_recipe(pkg)
        .ok_or_else(|| anyhow::anyhow!("no binary install recipe for '{pkg}'"))?;
    if matches!(recipe.archive, ArchiveKind::ZipTree { .. }) {
        anyhow::bail!(
            "the '{pkg}' binary is a whole release tree, installed only into a project \
             (`rsconstruct tool install project {pkg}`)"
        );
    }
    // A lock entry decides the exact asset and its bytes; without one (no
    // project, or a recipe that cannot be pinned) the recipe decides.
    let pin = ctx.binary_pins.get(pkg);
    let url = match pin {
        Some(pin) => pin.url.clone(),
        None => unpinned_binary_url(&recipe)?,
    };
    let fetched = fetch_binary(
        &recipe,
        &url,
        pin.map(|p| p.sha256.as_str()),
        INSTALL_WORK_DIR,
        ctx.verbose,
    )?;
    let argv = install_argv(&recipe, &fetched.payload);
    let result = exec_inherited(
        &argv.iter().map(String::as_str).collect::<Vec<_>>(),
        ctx.verbose,
    );
    // apt copies the .deb into its cache; `mv` consumes the executable.
    if matches!(recipe.archive, ArchiveKind::Deb { .. }) {
        std::fs::remove_file(&fetched.payload).ok();
    }
    result
}

/// Whether `pkg`'s binary recipe can be recorded in rsconstruct.lock. A
/// vendor `.deb` URL that always serves the current build (Chrome's) names
/// no release that could be installed again later, so it cannot.
pub fn binary_is_pinnable(pkg: &str) -> bool {
    binary_recipe(pkg).is_some_and(|r| {
        !matches!(
            r.archive,
            ArchiveKind::Deb {
                source: DebSource::Url(_)
            }
        )
    })
}

/// Whether `pkg`'s binary installs into a project's tool directory: every
/// recipe but a `.deb`, which apt installs onto the machine.
pub fn binary_is_repo_local(pkg: &str) -> bool {
    binary_recipe(pkg).is_some_and(|r| !matches!(r.archive, ArchiveKind::Deb { .. }))
}

/// Resolve `pkg`'s binary to the exact asset of today's latest release and
/// record what installing it puts on disk: the download's SHA-256, plus the
/// executable's SHA-256 (or, for a `.deb`, its package name and version) so
/// an installed copy can later be checked against the lock.
pub fn lock_binary(
    ctx: &crate::build_context::BuildContext,
    pkg: &str,
    verbose: bool,
) -> anyhow::Result<crate::tool_lock::BinaryPin> {
    use anyhow::Context as _;
    let recipe = binary_recipe(pkg)
        .ok_or_else(|| anyhow::anyhow!("no binary install recipe for '{pkg}'"))?;
    if !binary_is_pinnable(pkg) {
        anyhow::bail!("the '{pkg}' binary is a vendor URL with no versioned release to lock");
    }
    let url = if recipe.url.contains(GITHUB_LATEST_DOWNLOAD) {
        let repo = github_repo_of(recipe.url)
            .with_context(|| format!("cannot tell the GitHub repo of {}", recipe.url))?;
        let release = latest_github_release(repo)?;
        let tag = release["tag_name"]
            .as_str()
            .with_context(|| format!("the latest {repo} release has no tag_name"))?;
        pin_github_latest_url(recipe.url, tag)
            .with_context(|| format!("cannot pin {} to release {tag}", recipe.url))?
    } else {
        unpinned_binary_url(&recipe)?
    };
    let base = std::env::temp_dir().join(format!("rsconstruct-lock-{}-{pkg}", std::process::id()));
    std::fs::create_dir_all(&base)
        .with_context(|| format!("Failed to create {}", base.display()))?;
    let result = (|| {
        let fetched = fetch_binary(&recipe, &url, None, &base.to_string_lossy(), verbose)?;
        let mut pin = crate::tool_lock::BinaryPin {
            url: url.clone(),
            sha256: fetched.download_sha256,
            executable_sha256: None,
            deb_package: None,
            deb_version: None,
        };
        if matches!(recipe.archive, ArchiveKind::Deb { .. }) {
            let mut cmd = std::process::Command::new("dpkg-deb");
            cmd.args(["-f", &fetched.payload, "Package", "Version"]);
            let out = crate::processor::run_command_capture(ctx, &cmd)
                .with_context(|| format!("failed to run `dpkg-deb -f {}`", fetched.payload))?;
            if !out.status.success() {
                anyhow::bail!(
                    "`dpkg-deb -f {}` failed: {}",
                    fetched.payload,
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            let fields = String::from_utf8_lossy(&out.stdout).into_owned();
            let field = |name: &str| {
                fields.lines().find_map(|l| {
                    l.strip_prefix(name)
                        .and_then(|v| v.strip_prefix(':'))
                        .map(|v| v.trim().to_string())
                })
            };
            pin.deb_package = field("Package");
            pin.deb_version = field("Version");
            if pin.deb_package.is_none() || pin.deb_version.is_none() {
                anyhow::bail!("{url}: the .deb names no Package/Version");
            }
        } else {
            pin.executable_sha256 = Some(file_sha256(std::path::Path::new(&fetched.payload))?);
        }
        Ok(pin)
    })();
    std::fs::remove_dir_all(&base).ok();
    result
}

struct BinaryRecipe {
    url: &'static str,
    archive: ArchiveKind,
    dest: &'static str,
}

enum ArchiveKind {
    TarGz {
        inner: &'static str,
    },
    /// A `.zip` holding the binary as the member `inner`.
    Zip {
        inner: &'static str,
    },
    Gunzip,
    /// The download is the binary itself, no extraction needed.
    Raw,
    /// A `.zip` whose contents are an install prefix (`bin/` beside the
    /// `include/` or `lib/` the executable finds relative to its own path),
    /// kept whole. Installed only into a project's tool directory, where a
    /// package has a prefix of its own; `exe` is the executable's member.
    ZipTree {
        exe: &'static str,
    },
    /// The download is a `.deb` installed with apt, not a bare binary dropped
    /// into /usr/local/bin: apt pulls the dependency tree a raw binary would
    /// be missing.
    ///
    /// `source` says where the `.deb` comes from — a GitHub release whose
    /// asset is resolved at install time (so the recipe doesn't pin a version
    /// that goes stale the moment upstream cuts a release), or a vendor URL
    /// that is already stable.
    Deb {
        source: DebSource,
    },
}

/// Where a [`ArchiveKind::Deb`] payload is fetched from.
enum DebSource {
    /// Newest asset in `repo`'s latest release whose name contains
    /// `asset_pattern` and ends in `.deb`.
    GithubRelease {
        repo: &'static str,
        asset_pattern: &'static str,
    },
    /// A vendor URL that always points at the current build.
    Url(&'static str),
}

fn binary_recipe(pkg: &str) -> Option<BinaryRecipe> {
    match pkg {
        // Pinned like actionlint: oxlint lives in the oxc monorepo, whose
        // "latest" release is whichever crate was tagged most recently, so
        // there is no `releases/latest/download/...` URL that reliably names
        // an oxlint asset. Bump this deliberately. The tarball holds one
        // file named after the target triple, not `oxlint`.
        "oxlint" => Some(BinaryRecipe {
            url: "https://github.com/oxc-project/oxc/releases/download/oxlint_v1.86.0/oxlint-x86_64-unknown-linux-gnu.tar.gz",
            archive: ArchiveKind::TarGz {
                inner: "oxlint-x86_64-unknown-linux-gnu",
            },
            dest: "oxlint",
        }),
        "rumdl" => Some(BinaryRecipe {
            url: "https://github.com/rvben/rumdl/releases/download/v0.2.66/rumdl-v0.2.66-x86_64-unknown-linux-gnu.tar.gz",
            archive: ArchiveKind::TarGz { inner: "rumdl" },
            dest: "rumdl",
        }),
        // Pinned rather than tracking latest, unlike taplo/hadolint below.
        // zola 0.23 renamed config keys (highlight_code -> [markdown.highlighting])
        // and swapped the syntax highlighter, so an unpinned upgrade can fail a
        // site build on config alone -- a breakage that lands in the consuming
        // project's CI, not here. Bump this deliberately.
        "zola" => Some(BinaryRecipe {
            url: "https://github.com/getzola/zola/releases/download/v0.23.3/zola-v0.23.3-x86_64-unknown-linux-gnu.tar.gz",
            archive: ArchiveKind::TarGz { inner: "zola" },
            dest: "zola",
        }),
        "taplo" => Some(BinaryRecipe {
            url: "https://github.com/tamasfe/taplo/releases/latest/download/taplo-linux-x86_64.gz",
            archive: ArchiveKind::Gunzip,
            dest: "taplo",
        }),
        // Pinned like zola: actionlint's release assets embed the version in
        // the filename (actionlint_1.7.12_linux_amd64.tar.gz), so there is no
        // stable `releases/latest/download/...` URL to track the way taplo
        // and hadolint do. Bump this deliberately.
        "actionlint" => Some(BinaryRecipe {
            url: "https://github.com/rhysd/actionlint/releases/download/v1.7.12/actionlint_1.7.12_linux_amd64.tar.gz",
            archive: ArchiveKind::TarGz {
                inner: "actionlint",
            },
            dest: "actionlint",
        }),
        // Pinned like actionlint: selene's release assets embed the version
        // in the filename (selene-0.32.0-linux.zip). Bump this deliberately.
        // The full build, not selene-light: only the full one carries the
        // Roblox standard library, and the size difference is irrelevant.
        "selene" => Some(BinaryRecipe {
            url: "https://github.com/Kampfkarren/selene/releases/download/0.32.0/selene-0.32.0-linux.zip",
            archive: ArchiveKind::Zip { inner: "selene" },
            dest: "selene",
        }),
        "hadolint" => Some(BinaryRecipe {
            url: "https://github.com/hadolint/hadolint/releases/latest/download/hadolint-Linux-x86_64",
            archive: ArchiveKind::Raw,
            dest: "hadolint",
        }),
        // Tracks latest like hadolint: every release in the biome repo is a
        // CLI release, so `releases/latest` always carries the raw
        // `biome-linux-x64` asset. The glibc build, not the musl one, to match
        // the rest of the registry.
        "biome" => Some(BinaryRecipe {
            url: "https://github.com/biomejs/biome/releases/latest/download/biome-linux-x64",
            archive: ArchiveKind::Raw,
            dest: "biome",
        }),
        // Pinned like actionlint: shellcheck's release assets embed the
        // version in the filename and in the tarball's top directory. The
        // binary is statically linked. Bump this deliberately.
        "shellcheck" => Some(BinaryRecipe {
            url: "https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.linux.x86_64.tar.gz",
            archive: ArchiveKind::TarGz {
                inner: "shellcheck-v0.11.0/shellcheck",
            },
            dest: "shellcheck",
        }),
        // Tracks latest like hadolint: every jq release carries the raw,
        // statically linked `jq-linux-amd64` asset under that same name.
        "jq" => Some(BinaryRecipe {
            url: "https://github.com/jqlang/jq/releases/latest/download/jq-linux-amd64",
            archive: ArchiveKind::Raw,
            dest: "jq",
        }),
        // Pinned like actionlint: pandoc's release assets embed the version
        // in the filename and in the tarball's top directory. The binary is
        // statically linked; pandoc-server and pandoc-lua beside it are not
        // needed. A new pandoc can change generated documents, so bump this
        // deliberately.
        "pandoc" => Some(BinaryRecipe {
            url: "https://github.com/jgm/pandoc/releases/download/3.13/pandoc-3.13-linux-amd64.tar.gz",
            archive: ArchiveKind::TarGz {
                inner: "pandoc-3.13/bin/pandoc",
            },
            dest: "pandoc",
        }),
        // Pinned like actionlint: protoc's release assets embed the version
        // in the filename. Kept whole: protoc finds the well-known types
        // (google/protobuf/*.proto) in the include/ beside its bin/. Code it
        // generates checks the protobuf runtime it is compiled or imported
        // against, so bump this deliberately.
        "protoc" => Some(BinaryRecipe {
            url: "https://github.com/protocolbuffers/protobuf/releases/download/v36.2/protoc-36.2-linux-x86_64.zip",
            archive: ArchiveKind::ZipTree { exe: "bin/protoc" },
            dest: "protoc",
        }),
        // Pinned like actionlint: qpdf's release assets embed the version in
        // the filename. Kept whole: qpdf loads libqpdf and the libraries
        // bundled with it from the lib/ beside its bin/ (RUNPATH
        // $ORIGIN/../lib).
        "qpdf" => Some(BinaryRecipe {
            url: "https://github.com/qpdf/qpdf/releases/download/v12.4.2/qpdf-12.4.2-bin-linux-x86_64.zip",
            archive: ArchiveKind::ZipTree { exe: "bin/qpdf" },
            dest: "qpdf",
        }),
        // Tracks latest like hadolint: every uv release carries this asset
        // under the same name. The glibc build, like the rest of the registry.
        "uv" => Some(BinaryRecipe {
            url: "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-unknown-linux-gnu.tar.gz",
            archive: ArchiveKind::TarGz {
                inner: "uv-x86_64-unknown-linux-gnu/uv",
            },
            dest: "uv",
        }),
        // checkpatch.pl ships only inside the kernel tree — there is no release
        // artifact and no distro package. Fetching the single script from
        // torvalds/linux master is the automatable equivalent of "copy it out
        // of a kernel checkout", and keeps the tool out of the manual-only
        // bucket that `tool install machine --all` rejects.
        //
        // Pinned to one kernel commit, not master: icheckpatch is a port of
        // this exact script and its tests compare the two line for line, so
        // a checkpatch.pl that moved on would fail them for changes the port
        // has not followed yet. Moving the pin means porting the diff
        // between the two commits into src/engines/checkpatch/ - do both in
        // the same change.
        "checkpatch.pl" => Some(BinaryRecipe {
            url: "https://raw.githubusercontent.com/torvalds/linux/e5f92606156a6a823992294d214c285b49cd72e9/scripts/checkpatch.pl",
            archive: ArchiveKind::Raw,
            dest: "checkpatch.pl",
        }),
        // drawio ships an official .deb but has no apt repo, and its snap
        // needs a running snapd — unavailable in containers and flaky on CI
        // runners, which made this the one tool `--all` could not provision.
        "drawio" => Some(BinaryRecipe {
            url: "https://github.com/jgraph/drawio-desktop/releases/latest",
            archive: ArchiveKind::Deb {
                source: DebSource::GithubRelease {
                    repo: "jgraph/drawio-desktop",
                    asset_pattern: "amd64",
                },
            },
            dest: "drawio",
        }),
        // Chrome is not in the Ubuntu archive — `apt install
        // google-chrome-stable` only works after adding Google's repo. The
        // vendor .deb is the official route and is not version-pinned.
        "google-chrome-stable" => Some(BinaryRecipe {
            url: "https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb",
            archive: ArchiveKind::Deb {
                source: DebSource::Url(
                    "https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb",
                ),
            },
            dest: "google-chrome",
        }),
        _ => None,
    }
}

/// Information about an external tool: its name, runtime category, and install methods.
pub struct ToolInfo {
    /// Bare binary name, as found on `$PATH` — never a path.
    ///
    /// This is the detection key: it goes straight to `which::which` (see
    /// `builder/tools.rs` and `tool_lock.rs`), so it decides whether a tool
    /// reports `installed` or `missing`, and which binary gets version-locked.
    ///
    /// `which` switches behaviour on the presence of a separator, so a name
    /// containing `/` is NOT looked up on `$PATH` — it is resolved relative to
    /// the *current working directory*. A vendored-looking entry such as
    /// `gems/bin/mdl` therefore only resolves when rsconstruct happens to be
    /// invoked from the one directory with that subtree beneath it, and reports
    /// `missing` everywhere else — including for a user who has the tool
    /// installed and on `$PATH` at, say, `~/install/gems/bin/mdl`. Two such
    /// entries were removed for exactly this reason; keep names bare.
    ///
    /// In both modes `which` also requires the executable bit, so a present but
    /// non-executable file reads as `missing`.
    ///
    /// To pin a project-local binary, set the per-processor `command` config
    /// field (`config/processor_configs.rs`) instead — that is the value that
    /// actually gets executed. This field stays a bare name so detection works
    /// from any cwd.
    pub name: &'static str,
    /// Runtime category ("python", "node", "ruby", "rust", "perl", "jvm", "system")
    pub runtime: &'static str,
    /// The preferred way to install the tool into the project. None when
    /// there is no project route (a system package, a `.deb`).
    pub project: Option<InstallMethod>,
    /// The preferred way to install the tool onto the machine.
    pub machine: Option<InstallMethod>,
    /// Other known ways to install it (brew for macOS, a crate where a
    /// release binary is preferred, ...). Listed by `tool list -M`, never
    /// chosen by an install.
    pub alternatives: &'static [InstallMethod],
    /// The preferred target overall, when it is not the default (the
    /// project if the tool has a project install, else the machine; see
    /// [`ToolInfo::preferred_target`]). Set only to override that default.
    pub prefer: Option<InstallTarget>,
}

/// The tools no single processor owns: language runtimes and compilers that
/// several processors run, and tools a project selects through config
/// (`required_tools`, a cc profile's cross toolchain) or that rsconstruct
/// runs itself. A tool exactly one processor runs is declared in that
/// processor's file instead (see `inventory::collect!(ToolInfo)` below);
/// `central_registry_holds_only_unowned_tools` keeps the split honest.
pub static TOOLS: &[ToolInfo] = &[
    // Python
    // uv builds the project's .venv (`tool install project` runs `uv
    // sync`), so it cannot come from that venv: its release binary goes into
    // the project's tool directory, installed ahead of the sync.
    ToolInfo {
        name: "uv",
        runtime: "python",
        project: Some(InstallMethod::new("binary", "uv")),
        machine: Some(InstallMethod::new("pip", "uv")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "python3",
        runtime: "python",
        project: None,
        machine: Some(InstallMethod::new("apt", "python3")),
        alternatives: &[],
        prefer: None,
    },
    // Node
    ToolInfo {
        name: "node",
        runtime: "node",
        project: None,
        machine: Some(InstallMethod::new("apt", "nodejs")),
        alternatives: &[],
        prefer: None,
    },
    // Ruby
    ToolInfo {
        name: "ruby",
        runtime: "ruby",
        project: None,
        machine: Some(InstallMethod::new("apt", "ruby")),
        alternatives: &[],
        prefer: None,
    },
    // Rust
    // rustup is the canonical route, but `apt install cargo` is a real,
    // automatable install and is what makes cargo reachable from
    // `tool install machine --all` on a bare machine. A host that already has a
    // rustup toolchain never reaches the install path: `which` finds cargo
    // first.
    ToolInfo {
        name: "cargo",
        runtime: "rust",
        project: None,
        machine: Some(InstallMethod::new("apt", "cargo")),
        alternatives: &[],
        prefer: None,
    },
    // Perl
    ToolInfo {
        name: "perl",
        runtime: "perl",
        project: None,
        machine: Some(InstallMethod::new("apt", "perl")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "perltidy",
        runtime: "perl",
        project: None,
        machine: Some(InstallMethod::new("apt", "perltidy")),
        alternatives: &[],
        prefer: None,
    },
    // System
    // ARM bare-metal cross toolchain. Debian splits it the same way as the
    // native one: the compiler driver comes from gcc-arm-none-eabi, while ar
    // and objcopy come from binutils-arm-none-eabi.
    ToolInfo {
        name: "arm-none-eabi-gcc",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "gcc-arm-none-eabi")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "arm-none-eabi-ar",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "binutils-arm-none-eabi")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "arm-none-eabi-objcopy",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "binutils-arm-none-eabi")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "gcc",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "gcc")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "g++",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "g++")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "clang",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "clang")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "clang++",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "clang")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "make",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "make")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "pdflatex",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "texlive-latex-base")),
        alternatives: &[],
        prefer: None,
    },
    ToolInfo {
        name: "dot",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "graphviz")),
        alternatives: &[],
        prefer: None,
    },
    // Debian/Ubuntu package the Clojure CLI, which is what makes this
    // installable without the official shell installer; brew is the macOS
    // route. See https://clojure.org/guides/install_clojure
    ToolInfo {
        name: "clojure",
        runtime: "jvm",
        project: None,
        machine: Some(InstallMethod::new("apt", "clojure")),
        alternatives: &[InstallMethod::new("brew", "clojure/tools/clojure")],
        prefer: None,
    },
    // Built-in / coreutils
    ToolInfo {
        name: "true",
        runtime: "system",
        project: None,
        machine: Some(InstallMethod::new("apt", "coreutils")),
        alternatives: &[],
        prefer: None,
    },
];

// A processor owns its tools: each tool only it runs is declared in its own
// file with `inventory::submit!`, next to its plugin entry, so the install
// methods sit beside the code that runs the tool. Consumers must iterate
// `all_tools()`, never TOOLS directly.
inventory::collect!(ToolInfo);

/// Every known tool: the central table plus processor-file submissions.
pub fn all_tools() -> impl Iterator<Item = &'static ToolInfo> {
    TOOLS.iter().chain(inventory::iter::<ToolInfo>)
}

/// Look up a tool by name.
pub fn tool_info(tool: &str) -> Option<&'static ToolInfo> {
    all_tools().find(|t| t.name == tool)
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    /// A tool name must have exactly one registry entry — a processor
    /// submitting a name the central table already carries (or two
    /// processors submitting the same name) would make `tool_info`'s answer
    /// depend on iteration order.
    #[test]
    fn tool_names_are_unique_across_central_and_submitted() {
        let mut seen = std::collections::HashSet::new();
        let mut dups: Vec<&str> = Vec::new();
        for t in all_tools() {
            if !seen.insert(t.name) {
                dups.push(t.name);
            }
        }
        dups.sort_unstable();
        assert!(dups.is_empty(), "duplicate tool registry entries: {dups:?}");
    }

    /// A slot names a method that can install to the slot's target (no apt
    /// in `project`, no `manual` anywhere), every tool fills at least one
    /// slot, and `prefer` only overrides: it names a filled slot other than
    /// the one the default would pick.
    #[test]
    fn every_slot_holds_a_method_for_its_target() {
        let mut bad: Vec<String> = Vec::new();
        for t in all_tools() {
            for target in [InstallTarget::Project, InstallTarget::Machine] {
                if let Some(m) = t.slot(target)
                    && !m.targets().contains(&target)
                {
                    bad.push(format!(
                        "{}: {} slot holds {} {}, which cannot install to the {}",
                        t.name,
                        target.as_str(),
                        m.method,
                        m.package,
                        target.as_str(),
                    ));
                }
            }
            if t.project.is_none() && t.machine.is_none() {
                bad.push(format!(
                    "{}: neither a project nor a machine install",
                    t.name
                ));
            }
            if let Some(prefer) = t.prefer {
                let default = if t.project.is_some() {
                    InstallTarget::Project
                } else {
                    InstallTarget::Machine
                };
                if t.slot(prefer).is_none() {
                    bad.push(format!(
                        "{}: prefers the {}, but has no {} install",
                        t.name,
                        prefer.as_str(),
                        prefer.as_str(),
                    ));
                } else if prefer == default {
                    bad.push(format!(
                        "{}: `prefer` repeats the default ({}); leave it None",
                        t.name,
                        prefer.as_str(),
                    ));
                }
            }
        }
        assert!(bad.is_empty(), "install slots: {bad:#?}");
    }

    /// An alternative is never installed, so it must still be a method
    /// `run` knows, and must not repeat a slot.
    #[test]
    fn alternatives_are_known_and_distinct_from_the_slots() {
        let mut bad: Vec<String> = Vec::new();
        for t in all_tools() {
            for alt in t.alternatives {
                if alt.targets().is_empty() {
                    bad.push(format!(
                        "{}: alternative {} {} installs nowhere",
                        t.name, alt.method, alt.package
                    ));
                }
                let repeats = [t.project.as_ref(), t.machine.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|m| m.method == alt.method && m.package == alt.package);
                if repeats {
                    bad.push(format!(
                        "{}: alternative {} {} repeats a slot",
                        t.name, alt.method, alt.package
                    ));
                }
            }
        }
        assert!(bad.is_empty(), "install alternatives: {bad:#?}");
    }

    /// The names of the `ToolInfo` entries a processor file submits.
    fn submitted_tool_names(source: &str) -> Vec<&str> {
        source
            .split("crate::tools::ToolInfo {")
            .skip(1)
            .filter_map(|rest| rest.split_once("name: \"")?.1.split_once('"'))
            .map(|(name, _)| name)
            .collect()
    }

    /// Processors own their tools. Every processor is built with its default
    /// config and asked which tools it runs. A tool exactly one processor
    /// runs must be declared in that processor's file, not in the central
    /// table; a tool a processor file declares must be run by no other
    /// processor, or it is shared and belongs in the central table.
    #[test]
    fn central_registry_holds_only_unowned_tools() {
        let processors = crate::builder::create_all_default_processors().unwrap();
        let mut runners: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for (pname, processor) in &processors {
            for tool in processor.required_tools() {
                runners.entry(tool).or_default().push(pname.clone());
            }
        }
        let mut bad: Vec<String> = Vec::new();
        for tool in TOOLS {
            if let Some([only]) = runners.get(tool.name).map(Vec::as_slice) {
                bad.push(format!(
                    "{}: only {only} runs it, so it belongs in that processor's file",
                    tool.name
                ));
            }
        }
        for plugin in crate::registries::all_plugins() {
            let pname = plugin.pname();
            let file = format!("src/{}.rs", pname.replace('.', "/"));
            let source = std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("{pname}: cannot read {file}: {e}"));
            for name in submitted_tool_names(&source) {
                let others: Vec<&String> = runners
                    .get(name)
                    .into_iter()
                    .flatten()
                    .filter(|p| **p != pname)
                    .collect();
                if !others.is_empty() {
                    bad.push(format!(
                        "{name}: declared in {file} but also run by {others:?}, so it is \
                         shared and belongs in the central table"
                    ));
                }
            }
        }
        bad.sort();
        assert!(bad.is_empty(), "tool registry ownership: {bad:#?}");
    }
}

/// Return the default install command for a tool, if known.
pub fn tool_install_command(tool: &str) -> Option<String> {
    tool_info(tool)
        .and_then(ToolInfo::preferred_method)
        .map(InstallMethod::command)
}

/// Return the runtime category for a tool, if known.
pub fn tool_runtime(tool: &str) -> Option<&'static str> {
    tool_info(tool).map(|t| t.runtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sudo_argv()` returns either `["sudo"]` or `[]` based on runtime
    /// state. Empty when running as root or when `sudo` isn't on PATH.
    #[test]
    fn sudo_argv_matches_runtime() {
        let prefix = sudo_argv();
        if crate::platform::needs_sudo() {
            assert_eq!(prefix, &["sudo"]);
        } else {
            assert!(prefix.is_empty(), "{prefix:?}");
        }
    }

    /// Only bin dirs that actually exist are collected, from all three
    /// layouts: `$GEM_HOME/bin`, `~/.gem/ruby/<v>/bin` (upstream) and
    /// `~/.local/share/gem/ruby/<v>/bin` (Debian/Ubuntu).
    #[test]
    fn user_gem_bin_dirs_collects_existing_layouts() {
        let home = tempfile::TempDir::new().unwrap();
        let none_yet = user_gem_bin_dirs(home.path(), None);
        assert!(none_yet.is_empty(), "{none_yet:?}");

        let upstream = home.path().join(".gem/ruby/3.2.0/bin");
        let debian = home.path().join(".local/share/gem/ruby/3.3.0/bin");
        let gem_home = home.path().join("mygems");
        std::fs::create_dir_all(&upstream).unwrap();
        std::fs::create_dir_all(&debian).unwrap();
        std::fs::create_dir_all(gem_home.join("bin")).unwrap();
        // A version dir without bin/ must not be collected.
        std::fs::create_dir_all(home.path().join(".gem/ruby/2.7.0")).unwrap();

        let dirs = user_gem_bin_dirs(home.path(), Some(gem_home.as_os_str()));
        assert_eq!(dirs, vec![gem_home.join("bin"), upstream, debian]);
    }

    /// Appending keeps the original PATH order, adds only what's missing,
    /// and reports None when there is nothing to add.
    #[test]
    fn path_with_appended_dirs_appends_only_missing() {
        use std::path::PathBuf;
        let path = std::ffi::OsString::from("/usr/bin:/home/u/.gem/ruby/3.2.0/bin");
        let dirs = vec![
            PathBuf::from("/home/u/.gem/ruby/3.2.0/bin"),
            PathBuf::from("/home/u/mygems/bin"),
        ];
        let new_path = path_with_appended_dirs(&path, &dirs).unwrap();
        assert_eq!(
            new_path,
            "/usr/bin:/home/u/.gem/ruby/3.2.0/bin:/home/u/mygems/bin"
        );

        assert!(path_with_appended_dirs(&new_path, &dirs).is_none());
        assert!(path_with_appended_dirs(&path, &[]).is_none());
    }

    /// `node_modules/.bin` goes first so the locked version shadows a global
    /// install; an occurrence further back is moved, not duplicated; and a
    /// PATH that already leads with it is left alone.
    #[test]
    fn path_with_prepended_dir_puts_dir_first_once() {
        use std::path::Path;
        let bin = Path::new("/proj/node_modules/.bin");
        let path = std::ffi::OsString::from("/usr/bin:/proj/node_modules/.bin:/bin");
        let new_path = path_with_prepended_dir(&path, bin).unwrap();
        assert_eq!(new_path, "/proj/node_modules/.bin:/usr/bin:/bin");

        assert!(path_with_prepended_dir(&new_path, bin).is_none());

        let without = std::ffi::OsString::from("/usr/bin:/bin");
        assert_eq!(
            path_with_prepended_dir(&without, bin).unwrap(),
            "/proj/node_modules/.bin:/usr/bin:/bin"
        );
    }

    /// A missing path is judged by its nearest existing ancestor — that is
    /// what decides whether the path could be created.
    #[test]
    fn ancestor_writability_walks_to_existing_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(nearest_existing_ancestor_is_writable(
            &dir.path().join("a/b/c")
        ));
        if !crate::platform::is_root() {
            crate::platform::set_permissions_mode(dir.path(), 0o555).unwrap();
            assert!(!nearest_existing_ancestor_is_writable(
                &dir.path().join("a/b/c")
            ));
            crate::platform::set_permissions_mode(dir.path(), 0o755).unwrap();
        }
    }

    /// `apt` describe must run `apt-get update` first, then call
    /// `apt-get install -y <pkg>` exactly once with the package name
    /// after the flags. Whether `sudo` is prepended depends on runtime
    /// state.
    #[test]
    fn apt_describe_shape_is_correct() {
        let steps = describe("apt", &["cowsay"], &no_pins());
        assert_eq!(steps.len(), 2);

        let update = &steps[0];
        let upd_idx = update
            .iter()
            .position(|s| s == "apt-get")
            .expect("apt-get in update argv");
        assert_eq!(update[upd_idx + 1], "update");
        assert_eq!(update.len(), upd_idx + 2);

        let install = &steps[1];
        let pkgmgr_idx = install
            .iter()
            .position(|s| s == "apt-get")
            .expect("apt-get in install argv");
        assert_eq!(install[pkgmgr_idx + 1], "install");
        assert_eq!(install[pkgmgr_idx + 2], "-y");
        assert_eq!(install[pkgmgr_idx + 3], "cowsay");
        if install[0] == "sudo" {
            assert_eq!(pkgmgr_idx, 1);
        } else {
            assert_eq!(pkgmgr_idx, 0);
        }
    }

    /// `apt` batch describe collapses many packages into one apt-get
    /// install call (still preceded by a single `apt-get update`).
    #[test]
    fn apt_batch_describe_collapses() {
        let steps = describe("apt", &["foo", "bar", "baz"], &no_pins());
        assert_eq!(steps.len(), 2);
        let install = &steps[1];
        let pkgmgr_idx = install
            .iter()
            .position(|s| s == "apt-get")
            .expect("apt-get in argv");
        assert_eq!(&install[pkgmgr_idx + 3..], &["foo", "bar", "baz"]);
    }

    /// `cargo` describe is `cargo install --locked --force <crates>`: no
    /// sudo, `--locked` so the crate builds with its published Cargo.lock,
    /// and `--force` so a same-named binary cargo did not install (a
    /// prebuilt tarball in a CI cache) is replaced instead of refused.
    #[test]
    fn cargo_describe_is_locked_and_forced_without_sudo() {
        let steps = describe("cargo", &["cargo-deny", "mdbook"], &no_pins());
        assert_eq!(steps.len(), 1);
        assert_eq!(
            steps[0],
            vec![
                "cargo",
                "install",
                "--locked",
                "--force",
                "cargo-deny",
                "mdbook"
            ]
        );
    }

    #[test]
    fn github_latest_urls_pin_to_a_tag() {
        assert_eq!(
            pin_github_latest_url(
                "https://github.com/tamasfe/taplo/releases/latest/download/taplo-linux-x86_64.gz",
                "0.10.0"
            )
            .as_deref(),
            Some("https://github.com/tamasfe/taplo/releases/download/0.10.0/taplo-linux-x86_64.gz")
        );
        assert_eq!(
            pin_github_latest_url("https://dl.google.com/x.deb", "1"),
            None
        );
        assert_eq!(
            github_repo_of("https://github.com/hadolint/hadolint/releases/latest/download/h"),
            Some("hadolint/hadolint")
        );
    }

    /// Chrome's vendor URL always serves the current build, so it is the
    /// one recipe the lock cannot record; GitHub-release recipes can be.
    #[test]
    fn only_versioned_binary_recipes_are_pinnable() {
        assert!(!binary_is_pinnable("google-chrome-stable"));
        for pkg in ["rumdl", "taplo", "hadolint", "drawio", "checkpatch.pl"] {
            assert!(binary_is_pinnable(pkg), "{pkg}");
        }
        assert!(!binary_is_pinnable("no-such-recipe"));
    }

    /// `pip` describe never uses sudo, regardless of runtime state.
    #[test]
    fn pip_describe_never_has_sudo() {
        let steps = describe("pip", &["ruff"], &no_pins());
        assert_eq!(steps.len(), 1);
        let argv = &steps[0];
        assert_eq!(argv[0], "pip");
        assert!(!argv.contains(&"sudo".to_string()));
    }

    /// `uv-sync` is one `uv sync --locked --no-install-project`, whatever
    /// names it is handed: it reads pyproject.toml and uv.lock, leaves the
    /// project itself to the build, and never uses sudo.
    #[test]
    fn uv_sync_describe_is_a_locked_sync() {
        let steps = describe("uv-sync", &["ruff", "pytest"], &no_pins());
        assert_eq!(
            steps,
            vec![
                ["uv", "sync", "--locked", "--no-install-project"]
                    .map(String::from)
                    .to_vec()
            ]
        );
    }

    /// `binary` describe yields a multi-step plan (download, extract,
    /// chmod, mv) with no shell metacharacters anywhere.
    #[test]
    fn binary_describe_has_no_shell_metachars() {
        for pkg in &[
            "taplo",
            "rumdl",
            "zola",
            "oxlint",
            "selene",
            "shellcheck",
            "jq",
            "pandoc",
            "protoc",
            "qpdf",
            "uv",
        ] {
            let steps = describe("binary", &[pkg], &no_pins());
            assert!(steps.len() >= 3, "binary {pkg} should have >=3 steps");
            for step in &steps {
                for arg in step {
                    for forbidden in &['|', '>', '<', ';', '&'] {
                        assert!(
                            !arg.contains(*forbidden),
                            "binary {pkg} step contains shell metachar '{forbidden}': {arg:?}"
                        );
                    }
                }
            }
        }
    }

    /// An empty `[binary]` lock table, for describing outside a project.
    fn no_pins() -> std::collections::BTreeMap<String, crate::tool_lock::BinaryPin> {
        std::collections::BTreeMap::new()
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| (*s).to_string()).collect()
    }

    /// The install plan `tool install` prints must name the URL the install
    /// downloads: the locked one when rsconstruct.lock has an entry. It used
    /// to show the recipe's `releases/latest` URL regardless.
    #[test]
    fn binary_describe_names_the_pinned_url() {
        let latest =
            "https://github.com/tamasfe/taplo/releases/latest/download/taplo-linux-x86_64.gz";
        let pinned =
            "https://github.com/tamasfe/taplo/releases/download/0.10.0/taplo-linux-x86_64.gz";
        let mut pins = no_pins();
        pins.insert(
            "taplo".to_string(),
            crate::tool_lock::BinaryPin {
                url: pinned.to_string(),
                sha256: "ab12".to_string(),
                executable_sha256: None,
                deb_package: None,
                deb_version: None,
            },
        );

        let steps = describe("binary", &["taplo"], &pins);
        assert_eq!(
            steps[0].last().map(String::as_str),
            Some(pinned),
            "{steps:?}"
        );
        assert!(
            steps[1][0].starts_with('#') && steps[1][0].contains("ab12"),
            "the locked hash is part of the plan: {steps:?}"
        );

        let steps = describe("binary", &["taplo"], &no_pins());
        assert_eq!(
            steps[0].last().map(String::as_str),
            Some(latest),
            "{steps:?}"
        );
    }

    /// The described steps are the fetch's steps. Before they were derived
    /// from one plan, the preview showed `gunzip -f /tmp/taplo.dl` (gunzip
    /// rejects a name without `.gz`) and left out the rename of a tar
    /// member that is not named after the tool.
    #[test]
    fn binary_describe_shows_the_steps_fetch_takes() {
        let steps = describe("binary", &["taplo"], &no_pins());
        assert_eq!(steps.len(), 6, "{steps:?}");
        assert_eq!(steps[0][0], "curl");
        assert_eq!(
            &steps[1..5],
            &[
                argv(&["mv", "/tmp/taplo.dl", "/tmp/taplo.dl.gz"]),
                argv(&["gunzip", "-f", "/tmp/taplo.dl.gz"]),
                argv(&["mv", "/tmp/taplo.dl", "/tmp/taplo"]),
                argv(&["chmod", "+x", "/tmp/taplo"]),
            ]
        );
        assert!(
            steps[5].ends_with(&argv(&["mv", "/tmp/taplo", "/usr/local/bin/taplo"])),
            "{steps:?}"
        );

        let steps = describe("binary", &["oxlint"], &no_pins());
        assert!(
            steps.contains(&argv(&[
                "mv",
                "/tmp/oxlint-x86_64-unknown-linux-gnu",
                "/tmp/oxlint"
            ])),
            "{steps:?}"
        );
        assert!(
            steps.contains(&argv(&["rm", "/tmp/oxlint.dl"])),
            "{steps:?}"
        );

        // A member already named after the tool is not renamed onto itself.
        let steps = describe("binary", &["rumdl"], &no_pins());
        assert!(
            !steps.contains(&argv(&["mv", "/tmp/rumdl", "/tmp/rumdl"])),
            "{steps:?}"
        );
    }

    /// Run the plan for real against local files: a gzip asset comes out as
    /// `<base>/<dest>`, a tarball member not named `dest` is renamed to it,
    /// nothing is left behind, and a locked hash that differs is refused.
    #[test]
    fn fetch_binary_unpacks_gzip_and_renamed_tar_members() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().to_string_lossy().into_owned();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let is_executable = |path: &str| {
            let meta = std::fs::metadata(path).unwrap();
            crate::platform::get_mode(&meta) & 0o111 != 0
        };

        std::fs::write(src.join("payload"), b"#!/bin/sh\necho gz\n").unwrap();
        let status = std::process::Command::new("gzip")
            .arg("-k")
            .arg(src.join("payload"))
            .status()
            .unwrap();
        assert!(status.success());
        let recipe = BinaryRecipe {
            url: "",
            archive: ArchiveKind::Gunzip,
            dest: "fake-gz-tool",
        };
        let url = format!("file://{}", src.join("payload.gz").display());
        let fetched = fetch_binary(&recipe, &url, None, &base, false).unwrap();
        assert_eq!(fetched.payload, format!("{base}/fake-gz-tool"));
        assert_eq!(
            std::fs::read(&fetched.payload).unwrap(),
            b"#!/bin/sh\necho gz\n"
        );
        assert!(is_executable(&fetched.payload));
        assert!(!tmp.path().join("fake-gz-tool.dl").exists());
        assert!(!tmp.path().join("fake-gz-tool.dl.gz").exists());
        assert_eq!(
            fetched.download_sha256,
            file_sha256(&src.join("payload.gz")).unwrap()
        );

        std::fs::write(src.join("tool-x86_64-unknown-linux-gnu"), b"tar member").unwrap();
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(src.join("tool.tar.gz"))
            .arg("-C")
            .arg(&src)
            .arg("tool-x86_64-unknown-linux-gnu")
            .status()
            .unwrap();
        assert!(status.success());
        let recipe = BinaryRecipe {
            url: "",
            archive: ArchiveKind::TarGz {
                inner: "tool-x86_64-unknown-linux-gnu",
            },
            dest: "fake-tar-tool",
        };
        let url = format!("file://{}", src.join("tool.tar.gz").display());
        let fetched = fetch_binary(&recipe, &url, None, &base, false).unwrap();
        assert_eq!(fetched.payload, format!("{base}/fake-tar-tool"));
        assert_eq!(std::fs::read(&fetched.payload).unwrap(), b"tar member");
        assert!(is_executable(&fetched.payload));
        assert!(!tmp.path().join("tool-x86_64-unknown-linux-gnu").exists());
        assert!(!tmp.path().join("fake-tar-tool.dl").exists());

        let err = fetch_binary(&recipe, &url, Some("0000"), &base, false).unwrap_err();
        assert!(
            err.to_string().contains("does not match rsconstruct.lock"),
            "{err}"
        );
        assert!(!tmp.path().join("fake-tar-tool.dl").exists());
    }

    /// A release kept whole unpacks to a tree beside the work files: the
    /// executable stays at its member path, next to the directories it
    /// reads at run time, and the download is gone.
    #[test]
    fn fetch_binary_keeps_a_zip_tree_whole() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().to_string_lossy().into_owned();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("bin")).unwrap();
        std::fs::create_dir_all(src.join("include/google")).unwrap();
        std::fs::write(src.join("bin/tool"), b"#!/bin/sh\necho tree\n").unwrap();
        std::fs::write(src.join("include/google/a.proto"), b"syntax").unwrap();
        let status = std::process::Command::new("zip")
            .args(["-q", "-r", "../tool.zip", "bin", "include"])
            .current_dir(&src)
            .status()
            .unwrap();
        assert!(status.success());
        let recipe = BinaryRecipe {
            url: "",
            archive: ArchiveKind::ZipTree { exe: "bin/tool" },
            dest: "fake-tree-tool",
        };
        let url = format!("file://{}", tmp.path().join("tool.zip").display());
        let fetched = fetch_binary(&recipe, &url, None, &base, false).unwrap();
        let tree = tree_dir(&base, "fake-tree-tool");
        assert_eq!(fetched.payload, format!("{tree}/bin/tool"));
        let meta = std::fs::metadata(&fetched.payload).unwrap();
        assert!(crate::platform::get_mode(&meta) & 0o111 != 0);
        assert_eq!(
            std::fs::read(format!("{tree}/include/google/a.proto")).unwrap(),
            b"syntax"
        );
        assert!(!tmp.path().join("fake-tree-tool.dl").exists());
    }

    /// protoc and qpdf need the directories beside their executable, so
    /// their release is kept whole and installs only into a project; a
    /// machine install of it is refused rather than half done.
    #[test]
    fn zip_tree_binaries_install_only_into_a_project() {
        for pkg in ["protoc", "qpdf"] {
            assert_eq!(
                InstallMethod::new("binary", pkg).targets(),
                &[InstallTarget::Project],
                "{pkg}"
            );
            let ctx = InstallCtx {
                verbose: false,
                use_eatmydata: false,
                binary_pins: no_pins(),
            };
            let err = run_binary(pkg, &ctx).unwrap_err();
            assert!(err.to_string().contains("only into a project"), "{err}");
        }
    }
}
