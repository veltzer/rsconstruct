//! The project's Python environment as a cache-key component.
//!
//! A tool that runs on the project's Python environment (`./.venv`) reads
//! more than its inputs: pyrefly resolves imports against the installed
//! packages, pytest imports them, and a build script that does
//! `import yaml` produces whatever the installed `PyYAML` makes of its input.
//! Upgrading those packages (`uv lock --upgrade && uv sync`) changes the
//! result while leaving every input file, every config field and the tool's
//! own binary untouched, so without this component the cached verdicts and
//! outputs survived the upgrade.
//!
//! The component is a fingerprint of what is *installed*, not of what
//! `uv.lock` declares: the sorted `*.dist-info` / `*.egg-info` directory
//! names in `site-packages` (each one names a distribution and its version)
//! plus the interpreter version from `pyvenv.cfg`. A lock refreshed but not
//! yet synced changes nothing, because the tools still run against the old
//! packages; a `uv pip install` that bypasses the lock is noticed. See
//! docs/src/internal/python-env-fingerprint.md for the options that were
//! weighed and why this one was chosen.
//!
//! Which processors carry it: those whose plugin defaults say so (the
//! Python checkers, `hash_python_env = true` in their `ProcessorDefaults`),
//! plus any processor whose tool runs the project's Python — the venv's
//! interpreter itself, or a script whose shebang reaches it (see
//! [`runs_project_python`]). The per-processor `hash_python_env` field
//! overrides both ways; `[build] hash_python_env = false` turns the whole
//! mechanism off.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::processor::ProcessorMap;

/// The project venv directory, relative to the project root (the working
/// directory — rsconstruct always runs there, and `tools::augment_path_with_repo_venv`
/// puts `./.venv/bin` first on PATH from the same place).
pub const VENV_DIR: &str = ".venv";

/// The interpreter named by `[build] python`. Set by `Config::load` before
/// the processor sections are deserialized, so processor defaults built
/// during deserialization already see it. Process-wide rather than on the
/// `BuildContext` because the readers include static plugin defaults and
/// `required_tools()`, neither of which is handed a context.
static INTERPRETER: std::sync::LazyLock<std::sync::RwLock<String>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(crate::config::default_python()));

/// The Python interpreter to run and require (`[build] python`).
pub fn interpreter() -> String {
    INTERPRETER.read().unwrap().clone()
}

/// Record `[build] python`. Called on every config load, so a config edit
/// under `rsconstruct watch` takes effect on the next rebuild.
pub fn set_interpreter(name: &str) {
    *INTERPRETER.write().unwrap() = name.to_string();
}

/// How many bytes of a tool file are read to find its shebang line. A
/// shebang longer than this is not a real one (Linux caps it at 256).
const SHEBANG_READ_LIMIT: usize = 512;

/// The fingerprint of the Python environment rooted at `venv`, or `None`
/// when there is no environment there (no `pyvenv.cfg`): a project without
/// a venv has no Python state to track, and its keys stay as they were.
///
/// Pure function of the directory listing and one small file, so it costs
/// one `readdir` of `site-packages` and is identical on every machine that
/// has the same packages installed — which keeps the key shareable through
/// a remote cache.
pub fn fingerprint(venv: &Path) -> Option<String> {
    let cfg = fs::read_to_string(venv.join("pyvenv.cfg")).ok()?;
    let mut parts: Vec<String> = interpreter_identity(&cfg);
    parts.push("--".to_string());
    let mut dists: BTreeSet<String> = BTreeSet::new();
    for dir in site_packages_dirs(venv) {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".dist-info") || name.ends_with(".egg-info") {
                dists.insert(name);
            }
        }
    }
    parts.extend(dists);
    let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
    Some(crate::checksum::hash_parts(&refs))
}

/// The interpreter-identifying lines of a `pyvenv.cfg`: `implementation`
/// and the `version*` keys (`version` from `python -m venv`,
/// `version_info` from uv). The rest — `home`, the uv release that created
/// the venv, `include-system-site-packages` — says nothing about what code
/// runs and would invalidate every key on an unrelated change.
fn interpreter_identity(cfg: &str) -> Vec<String> {
    let mut lines: Vec<String> = cfg
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            (key == "implementation" || key.starts_with("version"))
                .then(|| format!("{key}={}", value.trim()))
        })
        .collect();
    lines.sort();
    lines
}

/// Every `site-packages` directory of the venv: `lib/python3.X/site-packages`,
/// one per interpreter version that ever populated it.
fn site_packages_dirs(venv: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(venv.join("lib")) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("site-packages");
            if entry.file_name().to_string_lossy().starts_with("python") && candidate.is_dir() {
                dirs.push(candidate);
            }
        }
    }
    dirs.sort();
    dirs
}

/// Whether `tool` — a command as a processor names it, resolved through
/// PATH — runs the Python of the venv at `venv`:
///
/// - the tool resolves inside the venv and is an interpreter
///   (`.venv/bin/python`, `python3`, `python3.14`), or
/// - the tool is a script whose shebang names an interpreter inside the
///   venv (what `uv sync` writes into `.venv/bin/pytest`), or names
///   `python*` through `/usr/bin/env` (resolved through PATH, where the
///   venv comes first — `scripts/build.py` with `#!/usr/bin/env python`).
///
/// A system script with `#!/usr/bin/python3` is Python too, but not the
/// project's: it never sees the venv's packages, so it is not keyed by them.
/// A tool that is not on PATH is not this function's problem — the tool
/// preflight reports it.
pub fn runs_project_python(tool: &str, venv: &Path) -> bool {
    let Ok(path) = which::which(tool) else {
        return false;
    };
    if path.starts_with(venv) && is_interpreter_name(&path) {
        return true;
    }
    shebang_runs_project_python(&path, venv)
}

/// `python`, `python3`, `python3.14` — not `python-config` or `pythonw`.
fn is_interpreter_name(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy()) else {
        return false;
    };
    name.strip_prefix("python")
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

fn shebang_runs_project_python(path: &Path, venv: &Path) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::with_capacity(SHEBANG_READ_LIMIT);
    if file
        .take(SHEBANG_READ_LIMIT as u64)
        .read_to_end(&mut head)
        .is_err()
    {
        return false;
    }
    let Some(rest) = head.strip_prefix(b"#!") else {
        return false;
    };
    let line_end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
    let Ok(line) = std::str::from_utf8(&rest[..line_end]) else {
        return false;
    };
    shebang_line_runs_project_python(line, venv)
}

/// The decision for one shebang line (the text after `#!`).
fn shebang_line_runs_project_python(line: &str, venv: &Path) -> bool {
    let mut words = line.split_whitespace();
    let Some(interpreter) = words.next() else {
        return false;
    };
    let interpreter = Path::new(interpreter);
    if interpreter.file_name().is_some_and(|n| n == "env") {
        // `env [-S] [-flags] [NAME=VALUE...] python3 ...`: the first word
        // that is neither a flag nor an assignment is the program.
        return words
            .find(|w| !w.starts_with('-') && !w.contains('='))
            .is_some_and(|program| is_interpreter_name(Path::new(program)));
    }
    interpreter.starts_with(venv) && is_interpreter_name(interpreter)
}

/// The processors whose products carry the Python-environment component.
///
/// `explicit` reads a processor's resolved `hash_python_env` field: the
/// user's value, or its plugin default. `Some(true)`/`Some(false)` decide;
/// `None` (neither set it) falls back to detection: the processor is keyed
/// when any of its required tools runs the project's Python. Detection is
/// done once per distinct tool, since one tool (`python`) is typically
/// shared by many processors.
pub fn processors_keyed_by_python_env(
    processors: &ProcessorMap,
    enabled: &dyn Fn(&str) -> bool,
    explicit: &dyn Fn(&str) -> Option<bool>,
    venv: &Path,
) -> HashSet<String> {
    let mut detected: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    let mut keyed = HashSet::new();
    for (name, processor) in processors {
        if !enabled(name) {
            continue;
        }
        let wanted = explicit(name).unwrap_or_else(|| {
            processor.required_tools().iter().any(|tool| {
                *detected
                    .entry(tool.clone())
                    .or_insert_with(|| runs_project_python(tool, venv))
            })
        });
        if wanted {
            keyed.insert(name.clone());
        }
    }
    keyed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_venv(root: &Path, cfg: &str, dists: &[&str]) -> PathBuf {
        let venv = root.join(VENV_DIR);
        let sp = venv.join("lib").join("python3.14").join("site-packages");
        fs::create_dir_all(&sp).unwrap();
        fs::write(venv.join("pyvenv.cfg"), cfg).unwrap();
        for d in dists {
            fs::create_dir_all(sp.join(d)).unwrap();
        }
        venv
    }

    const CFG: &str = "home = /usr/bin\nimplementation = CPython\nuv = 0.13.6\nversion_info = 3.14.0\ninclude-system-site-packages = false\n";

    #[test]
    fn no_venv_has_no_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(fingerprint(&dir.path().join(VENV_DIR)), None);
    }

    #[test]
    fn package_upgrade_changes_the_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let venv = make_venv(
            dir.path(),
            CFG,
            &["pyyaml-6.0.2.dist-info", "requests-2.32.3.dist-info"],
        );
        let before = fingerprint(&venv).unwrap();
        let sp = venv.join("lib/python3.14/site-packages");
        fs::remove_dir(sp.join("pyyaml-6.0.2.dist-info")).unwrap();
        fs::create_dir(sp.join("pyyaml-6.0.3.dist-info")).unwrap();
        assert_ne!(before, fingerprint(&venv).unwrap());
    }

    #[test]
    fn package_contents_and_other_files_do_not_change_it() {
        let dir = tempfile::tempdir().unwrap();
        let venv = make_venv(dir.path(), CFG, &["pyyaml-6.0.2.dist-info"]);
        let before = fingerprint(&venv).unwrap();
        let sp = venv.join("lib/python3.14/site-packages");
        fs::create_dir_all(sp.join("yaml/__pycache__")).unwrap();
        fs::write(sp.join("yaml/__pycache__/x.pyc"), "x").unwrap();
        fs::write(sp.join("distutils-precedence.pth"), "x").unwrap();
        assert_eq!(before, fingerprint(&venv).unwrap());
    }

    #[test]
    fn interpreter_version_changes_it_but_uv_release_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let venv = make_venv(dir.path(), CFG, &["pyyaml-6.0.2.dist-info"]);
        let before = fingerprint(&venv).unwrap();
        fs::write(
            venv.join("pyvenv.cfg"),
            CFG.replace("uv = 0.13.6", "uv = 0.14.0"),
        )
        .unwrap();
        assert_eq!(before, fingerprint(&venv).unwrap());
        fs::write(venv.join("pyvenv.cfg"), CFG.replace("3.14.0", "3.14.1")).unwrap();
        assert_ne!(before, fingerprint(&venv).unwrap());
    }

    #[test]
    fn fingerprint_is_independent_of_the_venv_location() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let dists = ["a-1.dist-info", "b-2.dist-info"];
        assert_eq!(
            fingerprint(&make_venv(a.path(), CFG, &dists)),
            fingerprint(&make_venv(b.path(), CFG, &dists))
        );
    }

    #[test]
    fn interpreter_names() {
        for yes in ["python", "python3", "python3.14"] {
            assert!(is_interpreter_name(Path::new(yes)), "{yes}");
        }
        for no in ["python-config", "pythonw", "ipython", "pytest", "py"] {
            assert!(!is_interpreter_name(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn shebang_lines() {
        let venv = Path::new("/p/.venv");
        assert!(shebang_line_runs_project_python(
            "/p/.venv/bin/python",
            venv
        ));
        assert!(shebang_line_runs_project_python(
            "/p/.venv/bin/python3 -I",
            venv
        ));
        assert!(shebang_line_runs_project_python(
            "/usr/bin/env python",
            venv
        ));
        assert!(shebang_line_runs_project_python(
            "/usr/bin/env -S python3 -u",
            venv
        ));
        assert!(shebang_line_runs_project_python(
            "/usr/bin/env PYTHONHASHSEED=0 python3",
            venv
        ));
        assert!(!shebang_line_runs_project_python("/usr/bin/python3", venv));
        assert!(!shebang_line_runs_project_python("/usr/bin/env bash", venv));
        assert!(!shebang_line_runs_project_python("/bin/sh", venv));
        assert!(!shebang_line_runs_project_python(
            "/other/.venv/bin/python",
            venv
        ));
        assert!(!shebang_line_runs_project_python("", venv));
    }

    #[test]
    fn script_files_are_detected_by_shebang() {
        let dir = tempfile::tempdir().unwrap();
        let venv = dir.path().join(VENV_DIR);
        let py = dir.path().join("build.py");
        fs::write(&py, "#!/usr/bin/env python\nimport yaml\n").unwrap();
        let sh = dir.path().join("build.sh");
        fs::write(&sh, "#!/bin/sh\nexit 0\n").unwrap();
        let elf = dir.path().join("ruff");
        fs::write(&elf, b"\x7fELF\x02\x01\x01\0").unwrap();
        assert!(shebang_runs_project_python(&py, &venv));
        assert!(!shebang_runs_project_python(&sh, &venv));
        assert!(!shebang_runs_project_python(&elf, &venv));
    }
}
