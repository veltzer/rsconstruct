//! Column selection for the CLI's tables: `--fields`, `--all` and the
//! `[tables]` section of the user config.

use crate::common::*;
use std::fs;
use tempfile::TempDir;

fn user_config(contents: &str) -> TempDir {
    let xdg = TempDir::new().unwrap();
    fs::create_dir_all(xdg.path().join("rsconstruct")).unwrap();
    fs::write(xdg.path().join("rsconstruct/config.toml"), contents).unwrap();
    xdg
}

/// The header row of the first table in `stdout`: the line after the top
/// border, with the box-drawing stripped and the cells trimmed.
fn header_cells(stdout: &str) -> Vec<String> {
    let line = stdout
        .lines()
        .skip_while(|l| !l.starts_with('╭'))
        .nth(1)
        .unwrap_or_else(|| panic!("no table in:\n{stdout}"));
    line.split('│')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .collect()
}

fn processor_list(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> String {
    let mut full = args.to_vec();
    full.extend(["processor", "list"]);
    let mut env_vars = vec![("NO_COLOR", "1")];
    env_vars.extend_from_slice(env);
    let output = run_rsconstruct_with_env(dir, &full, &env_vars);
    assert!(
        output.status.success(),
        "processor list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// By default the table hides its verbose-only column; `--verbose` and
/// `--all` both reveal it, and `--all` does so with no `--verbose`.
#[test]
fn all_shows_every_column() {
    let dir = TempDir::new().unwrap();
    let default = header_cells(&processor_list(dir.path(), &[], &[]));
    assert_eq!(
        default,
        [
            "Name",
            "Type",
            "Native",
            "Rust",
            "Batch",
            "Cache",
            "Rust alternative"
        ]
    );
    let all = header_cells(&processor_list(dir.path(), &["--all"], &[]));
    assert_eq!(
        all,
        [
            "Name",
            "Type",
            "Native",
            "Rust",
            "Batch",
            "Cache",
            "Rust alternative",
            "Description"
        ]
    );
    let verbose = header_cells(&processor_list(dir.path(), &["--verbose"], &[]));
    assert_eq!(verbose, all);
}

/// `--fields` prints the table's name, its command and every column name,
/// and no data rows at all.
#[test]
fn fields_prints_names_only() {
    let dir = TempDir::new().unwrap();
    let stdout = processor_list(dir.path(), &["--fields"], &[]);
    assert!(
        stdout.contains("processor_list (rsconstruct processor list)"),
        "{stdout}"
    );
    assert_eq!(
        header_cells(&stdout),
        [
            "Name",
            "Type",
            "Native",
            "Rust",
            "Batch",
            "Cache",
            "Rust alternative",
            "Description"
        ]
    );
    assert!(
        !stdout.contains("processor.checker.ruff"),
        "--fields must print no data rows:\n{stdout}"
    );
    let rows = stdout.lines().filter(|l| l.starts_with('│')).count();
    assert_eq!(rows, 1, "only the header row:\n{stdout}");
}

/// `[tables]` in the user config picks the columns and their order; `--all`
/// overrides it; a table without an entry keeps its default.
#[test]
fn user_config_chooses_columns() {
    let dir = TempDir::new().unwrap();
    let xdg = user_config("[tables]\nprocessor_list = [\"Type\", \"Name\", \"Description\"]\n");
    let env = [("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())];

    let chosen = header_cells(&processor_list(dir.path(), &[], &env));
    assert_eq!(chosen, ["Type", "Name", "Description"]);

    let all = header_cells(&processor_list(dir.path(), &["--all"], &env));
    assert_eq!(all.len(), 8, "{all:?}");

    let output = run_rsconstruct_with_env(dir.path(), &["processor", "types"], &env);
    assert!(output.status.success());
    let other = header_cells(&String::from_utf8_lossy(&output.stdout));
    assert_eq!(other, ["Type", "Batch", "Description"]);
}

/// A `[tables]` entry sits beside `[build]` in the same file, and the file
/// still applies without a project config.
#[test]
fn user_config_tables_beside_build() {
    let dir = TempDir::new().unwrap();
    let xdg =
        user_config("[build]\nreject_dot_src_dirs = true\n\n[tables]\nhook_list = [\"Hook\"]\n");
    let env = [
        ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
        ("NO_COLOR", "1"),
    ];
    let output = run_rsconstruct_with_env(dir.path(), &["hook"], &env);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        header_cells(&String::from_utf8_lossy(&output.stdout)),
        ["Hook"]
    );
}

/// A key that names no table, or a column the table does not have, is a
/// config error that lists the valid names, so a typo is caught on the next
/// command instead of silently showing the default columns.
#[test]
fn user_config_rejects_unknown_table_or_field() {
    let dir = TempDir::new().unwrap();
    let xdg = user_config("[tables]\nprocessor_lst = [\"Name\"]\n");
    let output = run_rsconstruct_with_env(
        dir.path(),
        &["processor", "list"],
        &[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("processor_lst") && stderr.contains("no table has this name"),
        "{stderr}"
    );
    assert!(
        stderr.contains("processor_list") && stderr.contains("--fields"),
        "the error must list the real names and point at --fields: {stderr}"
    );

    let xdg = user_config("[tables]\nprocessor_list = [\"Name\", \"Colour\"]\n");
    let output = run_rsconstruct_with_env(
        dir.path(),
        &["processor", "list"],
        &[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(
        stderr.contains("\"Colour\"") && stderr.contains("\"Rust alternative\""),
        "{stderr}"
    );

    let xdg = user_config("[tables]\nprocessor_list = \"Name\"\n");
    let output = run_rsconstruct_with_env(
        dir.path(),
        &["processor", "list"],
        &[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("list of field names"), "{stderr}");
}

/// A table with a `Total` row keeps the total aligned with the chosen
/// columns.
#[test]
fn total_row_follows_the_chosen_columns() {
    let temp_dir =
        setup_project_with_config("[processor.generator.tera]\nsrc_dirs = [\"tera.templates\"]\n");
    write_file(temp_dir.path(), "tera.templates/hello.txt.tera", "hello");
    let xdg = user_config("[tables]\nproduct_status = [\"new\", \"Processor\"]\n");
    let output = run_rsconstruct_with_env(
        temp_dir.path(),
        &["status"],
        &[
            ("XDG_CONFIG_HOME", xdg.path().to_str().unwrap()),
            ("NO_COLOR", "1"),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(header_cells(&stdout), ["new", "Processor"]);
    let total = stdout
        .lines()
        .find(|l| l.contains("Total"))
        .unwrap_or_else(|| panic!("no Total row:\n{stdout}"));
    let cells: Vec<&str> = total
        .split('│')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect();
    assert_eq!(cells, ["1", "Total"], "{stdout}");
}

/// Tables print processor names without the `processor.` prefix; JSON keeps
/// the full name, which is what config files and scripts use.
#[test]
fn tables_drop_the_processor_prefix() {
    let dir = TempDir::new().unwrap();
    let stdout = processor_list(dir.path(), &[], &[]);
    assert!(stdout.contains("│ checker.ruff "), "{stdout}");
    assert!(
        !stdout.lines().any(|l| l.contains("│ processor.")),
        "no cell starts with the prefix:\n{stdout}"
    );
    let output = run_rsconstruct_with_env(
        dir.path(),
        &["--json", "processor", "list"],
        &[("NO_COLOR", "1")],
    );
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e["name"] == "processor.checker.ruff")
    );
}

/// The Cache column says whether a cache hit stands in for a run: yes for a
/// generator whose files are restored and a checker whose pass is recorded,
/// no for the package installers, whose stamp outlives the environment they
/// installed into.
#[test]
fn cache_column_marks_the_installers() {
    let dir = TempDir::new().unwrap();
    let output = run_rsconstruct_with_env(
        dir.path(),
        &["--json", "processor", "list"],
        &[("NO_COLOR", "1")],
    );
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).unwrap();
    let caches = |name: &str| -> bool {
        entries
            .iter()
            .find(|e| e["name"] == name)
            .unwrap_or_else(|| panic!("no {name}"))["caches_outputs"]
            .as_bool()
            .unwrap()
    };
    assert!(caches("processor.generator.tera"));
    assert!(caches("processor.checker.ruff"));
    assert!(caches("processor.mass_generator.cargo"));
    for installer in ["pip", "npm", "gem"] {
        assert!(
            !caches(&format!("processor.mass_generator.{installer}")),
            "{installer} installs into an environment the cache cannot restore"
        );
    }
    let stdout = processor_list(dir.path(), &[], &[]);
    let pip = stdout
        .lines()
        .find(|l| l.contains("│ mass_generator.pip "))
        .unwrap_or_else(|| panic!("no pip row:\n{stdout}"));
    let cells: Vec<&str> = pip
        .split('│')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect();
    assert_eq!(cells[5], "No", "{pip}");
}
