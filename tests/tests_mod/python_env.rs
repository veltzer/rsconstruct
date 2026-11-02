//! The project's Python environment as a cache-key component: upgrading
//! the packages in `./.venv` rebuilds the processors that depend on them,
//! and only those. See `src/python_env.rs` and
//! docs/src/internal/python-env-fingerprint.md.

use crate::common::{make_executable, run_rsconstruct};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const SITE_PACKAGES: &str = ".venv/lib/python3.14/site-packages";

/// A project with a fake `./.venv` holding one installed distribution, a
/// `.venv/bin/python` that is a shell script (rsconstruct puts `.venv/bin`
/// first on PATH, so `command = "python"` runs it), a non-Python tool
/// `tools/plain.sh`, and one source file for a `checker.script` stanza.
fn venv_project(processor_config: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let root = temp_dir.path();
    fs::create_dir_all(root.join(SITE_PACKAGES)).unwrap();
    fs::create_dir_all(root.join(SITE_PACKAGES).join("pyyaml-6.0.2.dist-info")).unwrap();
    fs::write(
        root.join(".venv/pyvenv.cfg"),
        "home = /usr/bin\nimplementation = CPython\nversion_info = 3.14.0\n",
    )
    .unwrap();
    fs::create_dir_all(root.join(".venv/bin")).unwrap();
    fs::write(root.join(".venv/bin/python"), "#!/bin/sh\nexit 0\n").unwrap();
    make_executable(&root.join(".venv/bin/python"));
    fs::create_dir_all(root.join("tools")).unwrap();
    fs::write(root.join("tools/plain.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    make_executable(&root.join("tools/plain.sh"));
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/a.txt"), "content\n").unwrap();
    fs::write(
        root.join("rsconstruct.toml"),
        format!(
            "[processor.checker.script]\nsrc_dirs = [\"src\"]\nsrc_extensions = [\".txt\"]\n{processor_config}"
        ),
    )
    .unwrap();
    temp_dir
}

/// What `uv lock --upgrade && uv sync` does to site-packages: the
/// distribution's `dist-info` directory is replaced by the new version's.
fn upgrade_package(root: &Path) {
    let sp = root.join(SITE_PACKAGES);
    fs::remove_dir(sp.join("pyyaml-6.0.2.dist-info")).unwrap();
    fs::create_dir(sp.join("pyyaml-6.0.3.dist-info")).unwrap();
}

/// Build twice (the second must be all cache hits), upgrade a package, and
/// build again. Returns whether the third build re-ran anything.
fn reruns_after_upgrade(root: &Path) -> bool {
    for round in ["first", "second"] {
        let out = run_rsconstruct(root, &["build", "-v"]);
        assert!(
            out.status.success(),
            "{round} build failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        if round == "second" {
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                !stdout.contains("Processing:"),
                "nothing changed, so nothing may re-run: {stdout}"
            );
        }
    }
    upgrade_package(root);
    let out = run_rsconstruct(root, &["build", "-v"]);
    assert!(
        out.status.success(),
        "build after the upgrade failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).contains("Processing:")
}

/// A tool that is the venv's interpreter is detected as running the
/// project's Python: a package upgrade re-runs it.
#[test]
fn venv_python_command_reruns_after_package_upgrade() {
    let temp_dir = venv_project("command = \"python\"\n");
    assert!(
        reruns_after_upgrade(temp_dir.path()),
        "a package upgrade must re-run a processor that runs the venv's python"
    );
}

/// A tool that does not run Python keeps its cached result.
#[test]
fn non_python_command_is_not_keyed_by_the_venv() {
    let temp_dir = venv_project("command = \"tools/plain.sh\"\n");
    assert!(
        !reruns_after_upgrade(temp_dir.path()),
        "a package upgrade must not re-run a processor whose tool is not Python"
    );
}

/// `hash_python_env = true` keys a processor detection cannot see: a
/// shell wrapper that runs Python behind the scenes.
#[test]
fn explicit_true_keys_a_non_python_command() {
    let temp_dir = venv_project("command = \"tools/plain.sh\"\nhash_python_env = true\n");
    assert!(
        reruns_after_upgrade(temp_dir.path()),
        "hash_python_env = true must key the processor by the venv"
    );
}

/// `hash_python_env = false` overrides detection.
#[test]
fn explicit_false_overrides_detection() {
    let temp_dir = venv_project("command = \"python\"\nhash_python_env = false\n");
    assert!(
        !reruns_after_upgrade(temp_dir.path()),
        "hash_python_env = false must keep the venv out of the key"
    );
}

/// `[build] hash_python_env = false` turns the mechanism off everywhere.
#[test]
fn build_switch_disables_python_env_keys() {
    let temp_dir = venv_project("command = \"python\"\nhash_python_env = true\n");
    let root = temp_dir.path();
    let config = fs::read_to_string(root.join("rsconstruct.toml")).unwrap();
    fs::write(
        root.join("rsconstruct.toml"),
        format!("[build]\nhash_python_env = false\n\n{config}"),
    )
    .unwrap();
    assert!(
        !reruns_after_upgrade(root),
        "[build] hash_python_env = false must keep the venv out of every key"
    );
}

/// The Python checkers are keyed by default (ruff and pyrefly are native
/// binaries, so detection alone would miss them): `processor defconfig`
/// reports the plugin default.
#[test]
fn python_checkers_default_to_hash_python_env() {
    for pname in [
        "processor.checker.ruff",
        "processor.checker.pyrefly",
        "processor.checker.pytest",
        "processor.mass_generator.sphinx",
    ] {
        let defconfig = crate::common::processor_defconfig(pname);
        assert_eq!(
            defconfig.get("hash_python_env"),
            Some(&serde_json::Value::Bool(true)),
            "{pname} must default hash_python_env to true: {defconfig}"
        );
    }
    let defconfig = crate::common::processor_defconfig("processor.checker.shellcheck");
    assert_eq!(
        defconfig.get("hash_python_env"),
        None,
        "a non-Python checker leaves hash_python_env unset (detected): {defconfig}"
    );
}
