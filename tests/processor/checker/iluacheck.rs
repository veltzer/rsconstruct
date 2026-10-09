use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.iluacheck]\nsrc_dirs = [\"src\"]\n";

fn setup_project(config: &str, files: &[(&str, &str)]) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    for (name, content) in files {
        let path = temp_dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).expect("Failed to create dir");
        fs::write(&path, content).expect("Failed to write source file");
    }
    temp_dir
}

fn build(project_path: &Path) -> std::process::Output {
    run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")])
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_passes(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} must pass: {}",
        combined_output(output)
    );
}

fn assert_fails_with(output: &std::process::Output, what: &str, needle: &str) {
    assert!(!output.status.success(), "{what} must fail");
    let text = combined_output(output);
    assert!(
        text.contains(needle),
        "{what}: {needle:?} not reported: {text}"
    );
}

#[test]
fn iluacheck_clean_file_passes() {
    let temp_dir = setup_project(CONFIG, &[("src/ok.lua", "local x = 1\nprint(x)\n")]);
    assert_passes(&build(temp_dir.path()), "a clean Lua file");
}

#[test]
fn iluacheck_reports_in_luacheck_format() {
    let source = "local x = 1\ny = 2\nprint(z.w)\n";
    let temp_dir = setup_project(CONFIG, &[("src/bad.lua", source)]);
    let output = build(temp_dir.path());
    assert_fails_with(
        &output,
        "an unused local",
        "src/bad.lua:1:7: (W211) unused variable 'x'",
    );
    assert_fails_with(
        &output,
        "a global assignment",
        "src/bad.lua:2:1: (W111) setting non-standard global variable 'y'",
    );
    assert_fails_with(
        &output,
        "an undefined global",
        "src/bad.lua:3:7: (W113) accessing undefined variable 'z'",
    );
}

/// With `allow_defined_top`, a global set at the top of one file is
/// defined for every file checked together, as in one luacheck run.
#[test]
fn iluacheck_implicit_globals_span_files() {
    let config = "[processor.checker.iluacheck]\nsrc_dirs = [\"src\"]\nallow_defined_top = true\n";
    let temp_dir = setup_project(
        config,
        &[
            ("src/a.lua", "SHARED = {}\n"),
            ("src/b.lua", "print(SHARED)\n"),
        ],
    );
    assert_passes(&build(temp_dir.path()), "a global defined in another file");
}

#[test]
fn iluacheck_reads_luacheckrc() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            ("src/vimrc.lua", "vim.opt.number = true\n"),
            (".luacheckrc", "files[\"src\"] = {globals = {\"vim\"}}\n"),
        ],
    );
    assert_passes(
        &build(temp_dir.path()),
        "a global the configuration declares",
    );
}

#[test]
fn iluacheck_rejects_an_invalid_configuration() {
    let temp_dir = setup_project(
        CONFIG,
        &[
            ("src/a.lua", "print(1)\n"),
            (".luacheckrc", "std = \"nope\"\n"),
        ],
    );
    assert_fails_with(
        &build(temp_dir.path()),
        "an unknown std",
        "in config loaded from .luacheckrc: invalid value of option 'std': unknown std 'nope'",
    );
}

#[test]
fn iluacheck_honours_inline_options() {
    let temp_dir = setup_project(
        CONFIG,
        &[(
            "src/a.lua",
            "-- luacheck: globals FOO\nFOO = 1\nlocal unused = 1 -- luacheck: ignore\n",
        )],
    );
    assert_passes(&build(temp_dir.path()), "findings silenced inline");
}
