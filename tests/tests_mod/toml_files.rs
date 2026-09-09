use crate::common::*;
use tempfile::TempDir;

/// `toml files` names the three merge-chain files in precedence order, with
/// the user config resolved from `$XDG_CONFIG_HOME`, and reports which of them
/// exist. It needs no rsconstruct.toml to run.
#[test]
fn toml_files_lists_chain_and_existence() {
    let project = TempDir::new().unwrap();
    let xdg = TempDir::new().unwrap();
    let user_dir = xdg.path().join("rsconstruct");
    std::fs::create_dir_all(&user_dir).unwrap();
    std::fs::write(user_dir.join("config.toml"), "[build]\n").unwrap();
    write_file(project.path(), "rsconstruct.toml", "[build]\n");

    let output = run_rsconstruct_with_env(
        project.path(),
        &["toml", "files"],
        &[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    let user_line = stdout
        .lines()
        .find(|l| l.contains("user "))
        .expect("user line");
    assert!(user_line.contains(&user_dir.join("config.toml").display().to_string()));
    assert!(user_line.contains("present"), "{user_line}");

    let project_line = stdout
        .lines()
        .find(|l| l.contains("project "))
        .expect("project line");
    assert!(project_line.contains("rsconstruct.toml"));
    assert!(project_line.contains("present"), "{project_line}");

    let overlay_line = stdout
        .lines()
        .find(|l| l.contains("local overlay"))
        .expect("overlay line");
    assert!(overlay_line.contains("rsconstruct.local.toml"));
    assert!(overlay_line.contains("absent"), "{overlay_line}");

    let order: Vec<usize> = ["user ", "project ", "local overlay"]
        .iter()
        .map(|k| stdout.find(k).unwrap())
        .collect();
    assert!(
        order[0] < order[1] && order[1] < order[2],
        "precedence order: {stdout}"
    );
}

/// Without a project config the command still runs and reports the file as
/// absent instead of failing like the commands that require one.
#[test]
fn toml_files_runs_without_project_config() {
    let project = TempDir::new().unwrap();
    let output = run_rsconstruct(project.path(), &["toml", "files"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let project_line = stdout.lines().find(|l| l.contains("project ")).unwrap();
    assert!(project_line.contains("absent"), "{project_line}");
}

/// `--json` emits one object per file with the precedence, path and
/// existence flag, so scripts can find the user config path without parsing
/// the table.
#[test]
fn toml_files_json_is_machine_readable() {
    let project = TempDir::new().unwrap();
    let xdg = TempDir::new().unwrap();
    write_file(project.path(), "rsconstruct.local.toml", "[build]\n");
    let output = run_rsconstruct_with_env(
        project.path(),
        &["--json", "toml", "files"],
        &[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let files: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    let by_role = |role: &str| {
        files
            .iter()
            .find(|f| f["role"] == role)
            .unwrap_or_else(|| panic!("no entry with role {role}"))
    };
    assert_eq!(by_role("user")["precedence"], 1);
    assert_eq!(by_role("user")["exists"], false);
    assert!(
        by_role("user")["path"]
            .as_str()
            .unwrap()
            .starts_with(xdg.path().to_str().unwrap())
    );
    assert_eq!(by_role("project")["precedence"], 2);
    assert_eq!(by_role("project")["exists"], false);
    assert_eq!(by_role("local overlay")["precedence"], 3);
    assert_eq!(by_role("local overlay")["exists"], true);
    assert!(by_role("tool lock")["precedence"].is_null());
}
