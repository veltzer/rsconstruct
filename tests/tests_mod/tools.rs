use crate::common::{run_rsconstruct_with_env, setup_project_with_config, setup_test_project};
use serde_json::Value;

#[test]
fn tools_list_shows_all_registry_tools() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // `tool list` shows the whole tool registry regardless of config, like
    // `processor list`. It lists tools no processor in this project needs.
    let output = run_rsconstruct_with_env(project_path, &["tool", "list"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "tool list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty(),
        "tool list should show at least one tool"
    );
    // It includes tools the minimal test project does not require.
    assert!(
        stdout.contains("clojure"),
        "registry list should include all known tools, e.g. clojure"
    );
    // Each tool names the processors that run it at their default config,
    // whatever the project enables; a tool no processor runs says so.
    let row = |tool: &str| {
        stdout
            .lines()
            .find(|l| l.split('│').nth(1).is_some_and(|c| c.trim() == tool))
            .unwrap_or_else(|| panic!("no row for {tool}:\n{stdout}"))
            .to_string()
    };
    assert!(row("ruff").contains("checker.ruff"), "{}", row("ruff"));
    assert!(
        row("gcc").contains("generator.cc_single_file, mass_generator.cc"),
        "{}",
        row("gcc")
    );
    assert!(row("clojure").contains("(none)"), "{}", row("clojure"));
    // A tool many processors run names two of them and counts the rest,
    // instead of stretching the table across the screen.
    assert!(row("python3").contains(" more"), "{}", row("python3"));
}

#[test]
fn tools_list_json_names_each_tools_processors() {
    let temp_dir = setup_test_project();
    let output = run_rsconstruct_with_env(temp_dir.path(), &["--json", "tool", "list"], &[]);
    assert!(
        output.status.success(),
        "tool list --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entries: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    let processors = |tool: &str| -> Vec<String> {
        let entry = entries
            .iter()
            .find(|e| e["tool"] == tool)
            .unwrap_or_else(|| panic!("no entry for {tool}"));
        entry["processor"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(processors("ruff"), ["processor.checker.ruff"]);
    assert!(processors("clojure").is_empty());
}

/// `--json tool list` says where each install goes: per tool its preferred
/// target, per method the targets it can reach and the slots it fills.
/// ruff's pip install fills both slots, so the project is preferred; a
/// `.deb` (drawio) reaches only the machine; brew is clojure's alternative.
#[test]
fn tools_list_json_names_install_targets() {
    let temp_dir = setup_test_project();
    let output = run_rsconstruct_with_env(temp_dir.path(), &["--json", "tool", "list"], &[]);
    assert!(
        output.status.success(),
        "tool list --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entries: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    let entry = |tool: &str| -> &Value {
        entries
            .iter()
            .find(|e| e["tool"] == tool)
            .unwrap_or_else(|| panic!("no entry for {tool}"))
    };
    let method = |tool: &str, name: &str| -> &Value {
        entry(tool)["install_methods"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["method"] == name)
            .unwrap_or_else(|| panic!("{tool} has no {name} method"))
    };
    assert_eq!(entry("ruff")["preferred"], "project");
    assert_eq!(
        method("ruff", "pip")["slots"],
        serde_json::json!(["project", "machine"])
    );
    assert_eq!(
        method("ruff", "pip")["targets"],
        serde_json::json!(["project", "machine"])
    );
    assert_eq!(entry("drawio")["preferred"], "machine");
    assert_eq!(
        method("drawio", "binary")["targets"],
        serde_json::json!(["machine"])
    );
    assert_eq!(entry("clojure")["preferred"], "machine");
    assert_eq!(method("clojure", "brew")["slots"], serde_json::json!([]));
}

#[test]
fn tools_list_shows_configured_tools() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(
        project_path,
        &["tool", "list-configured"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        output.status.success(),
        "tool list-configured failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Template processor requires python3, so list-configured always has output.
    assert!(
        !stdout.is_empty(),
        "tool list-configured should show at least one tool"
    );
    assert!(
        stdout.contains("("),
        "Expected processor name in parentheses for each tool"
    );
}

/// A tera stanza plus a ruff stanza with `enabled = false`. ruff is a
/// pip-installed registry tool, so it is exactly what a disabled stanza
/// must keep out of the lock, the install set and the tool listings.
const TERA_AND_DISABLED_RUFF: &str = "[processor.generator.tera]\nsrc_dirs = [\"tera.templates\"]\n\n[processor.checker.ruff]\nsrc_dirs = [\"src\"]\nenabled = false\n";

/// `list-configured` shows the tools of the enabled processors; `--all` is
/// the one way to see a disabled stanza's tool. Before the enabled filter
/// was real, both forms listed everything and `--all` was a no-op.
#[test]
fn tools_list_configured_hides_disabled_processors_unless_all() {
    let temp_dir = setup_project_with_config(TERA_AND_DISABLED_RUFF);
    let project_path = temp_dir.path();
    std::fs::create_dir_all(project_path.join("tera.templates")).unwrap();

    let output_default = run_rsconstruct_with_env(
        project_path,
        &["tool", "list-configured"],
        &[("NO_COLOR", "1")],
    );
    let output_all = run_rsconstruct_with_env(
        project_path,
        &["tool", "list-configured", "-a"],
        &[("NO_COLOR", "1")],
    );
    assert!(output_default.status.success());
    assert!(output_all.status.success());

    let stdout_default = String::from_utf8_lossy(&output_default.stdout);
    let stdout_all = String::from_utf8_lossy(&output_all.stdout);
    assert!(
        !stdout_default.contains("ruff"),
        "a disabled processor's tool was listed without --all: {stdout_default}"
    );
    assert!(
        stdout_all.contains("ruff") && stdout_all.contains("processor.checker.ruff"),
        "--all must list the disabled processor's tool: {stdout_all}"
    );

    // The JSON form follows the same rule.
    let json_default = run_rsconstruct_with_env(
        project_path,
        &["--json", "tool", "list-configured"],
        &[("NO_COLOR", "1")],
    );
    let entries: Vec<Value> = serde_json::from_slice(&json_default.stdout).unwrap();
    assert!(!entries.iter().any(|e| e["tool"] == "ruff"), "{entries:?}");
    let json_all = run_rsconstruct_with_env(
        project_path,
        &["--json", "tool", "list-configured", "--all"],
        &[("NO_COLOR", "1")],
    );
    let entries: Vec<Value> = serde_json::from_slice(&json_all.stdout).unwrap();
    assert!(entries.iter().any(|e| e["tool"] == "ruff"), "{entries:?}");
}

/// `tool lock` and `tool check` consider the enabled processors only: the
/// lock never gains an entry for a disabled stanza's tool, and the exact
/// check in CI does not ask for one. (With the filter broken, `tool lock`
/// would resolve ruff on PyPI and write a `[pip] ruff` entry.)
#[test]
fn tool_lock_and_check_skip_disabled_processors() {
    let temp_dir = setup_project_with_config(TERA_AND_DISABLED_RUFF);
    let project_path = temp_dir.path();
    std::fs::create_dir_all(project_path.join("tera.templates")).unwrap();

    let output = run_rsconstruct_with_env(project_path, &["tool", "lock"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "tool lock failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lock = std::fs::read_to_string(project_path.join("rsconstruct.lock")).unwrap();
    assert!(
        !lock.contains("ruff"),
        "the lock gained an entry for a disabled processor's tool:\n{lock}"
    );

    let output = run_rsconstruct_with_env(
        project_path,
        &["tool", "check"],
        &[("NO_COLOR", "1"), ("CI", "true")],
    );
    assert!(
        output.status.success(),
        "tool check asked for a disabled processor's tool: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The lock check a CI build runs sees what the missing-tool preflight
/// sees: the processors with products in the graph. A disabled stanza whose
/// tool is neither locked nor installed, a processor that matched no file,
/// and one excluded with `-x` must not fail the build; the same stanza,
/// enabled and with a file to check, must.
#[test]
fn ci_build_checks_the_lock_for_the_processors_that_build_only() {
    let temp_dir = setup_project_with_config(TERA_AND_DISABLED_RUFF);
    let project_path = temp_dir.path();
    std::fs::create_dir_all(project_path.join("tera.templates")).unwrap();
    std::fs::create_dir_all(project_path.join("src")).unwrap();
    std::fs::write(project_path.join("src/a.py"), "x = 1\n").unwrap();
    // An empty lock: ruff is not in it.
    let lock = "version = 1\n";
    std::fs::write(project_path.join("rsconstruct.lock"), lock).unwrap();
    let ci = &[("NO_COLOR", "1"), ("CI", "true")];

    let output = run_rsconstruct_with_env(project_path, &["build"], ci);
    assert!(
        output.status.success(),
        "CI build asked for a disabled processor's tool: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Control: enabled and with a .py file to check, ruff is needed, and
    // the build fails on it — on the preflight when ruff is not installed,
    // on the lock when it is (no `[pip] ruff` entry).
    let enabled = TERA_AND_DISABLED_RUFF.replace("enabled = false", "enabled = true");
    std::fs::write(project_path.join("rsconstruct.toml"), &enabled).unwrap();
    let output = run_rsconstruct_with_env(project_path, &["build"], ci);
    assert!(
        !output.status.success(),
        "an enabled processor with an unlocked tool passed the CI build"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ruff"), "{stderr}");

    // Excluded with -x, ruff has no products in the graph and is not asked for.
    let output =
        run_rsconstruct_with_env(project_path, &["build", "-x", "processor.checker.ruff"], ci);
    assert!(
        output.status.success(),
        "CI build asked for an excluded processor's tool: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Matching no file, the enabled stanza needs no tool either.
    std::fs::remove_file(project_path.join("src/a.py")).unwrap();
    let output = run_rsconstruct_with_env(project_path, &["build"], ci);
    assert!(
        output.status.success(),
        "CI build asked for the tool of a processor with no products: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let after = std::fs::read_to_string(project_path.join("rsconstruct.lock")).unwrap();
    assert_eq!(after, lock, "CI must not rewrite the lock");
}

#[test]
fn tools_list_json() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(
        project_path,
        &["--json", "tool", "list"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        output.status.success(),
        "tool list --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).expect("Expected valid JSON array");

    // Check that every entry has the expected fields
    for entry in &entries {
        assert!(
            entry.get("tool").is_some(),
            "Entry should have 'tool' field"
        );
        assert!(
            entry.get("processor").is_some(),
            "Entry should have 'processor' field"
        );
        assert!(
            entry["processor"].is_array(),
            "'processor' should be an array"
        );
    }
}

#[test]
fn tools_check_succeeds() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    // First create the lock file so check has something to verify against
    let lock_output =
        run_rsconstruct_with_env(project_path, &["tool", "lock"], &[("NO_COLOR", "1")]);
    assert!(
        lock_output.status.success(),
        "tool lock failed: {}",
        String::from_utf8_lossy(&lock_output.stderr)
    );

    // Now check should succeed since versions match the just-created lock file
    let output = run_rsconstruct_with_env(project_path, &["tool", "check"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "tool check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `tool lock` writes rsconstruct.lock (here with no entries: the tera
/// processor's tools are apt packages, which are not locked), and a second
/// run changes nothing. rsconstruct itself is not recorded.
#[test]
fn tool_lock_writes_rsconstruct_lock_once() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let first = run_rsconstruct_with_env(project_path, &["tool", "lock"], &[("NO_COLOR", "1")]);
    assert!(
        first.status.success(),
        "tool lock failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(stdout.contains("Created rsconstruct.lock"), "{stdout}");
    let lock = std::fs::read_to_string(project_path.join("rsconstruct.lock")).unwrap();
    assert!(lock.contains("version = 1\n"), "{lock}");
    assert!(!lock.contains("[rsconstruct]"), "{lock}");

    let second = run_rsconstruct_with_env(project_path, &["tool", "lock"], &[("NO_COLOR", "1")]);
    assert!(second.status.success());
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(stdout.contains("up to date"), "{stdout}");
    let again = std::fs::read_to_string(project_path.join("rsconstruct.lock")).unwrap();
    assert_eq!(lock, again);
}

/// In CI a build without rsconstruct.lock fails; with one it proceeds.
#[test]
fn ci_build_requires_the_lock() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(
        project_path,
        &["build"],
        &[("NO_COLOR", "1"), ("CI", "true")],
    );
    assert!(
        !output.status.success(),
        "build without a lock passed in CI"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("No rsconstruct.lock found"), "{stderr}");

    let lock = run_rsconstruct_with_env(project_path, &["tool", "lock"], &[("NO_COLOR", "1")]);
    assert!(lock.status.success());
    let output = run_rsconstruct_with_env(
        project_path,
        &["build"],
        &[("NO_COLOR", "1"), ("CI", "true")],
    );
    assert!(
        output.status.success(),
        "CI build with a lock failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A lock written before 2026-10-26 carries an `[rsconstruct]` table (the
/// release `tool lock` ran as, back when CI downloaded that release). It is
/// accepted and ignored: in CI such a lock passes untouched, whatever
/// version it names, and outside CI the next `tool lock` drops the table
/// and reports doing so.
#[test]
fn legacy_rsconstruct_table_is_accepted_and_dropped_on_rewrite() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();
    let lock = "version = 1\n\n[rsconstruct]\nversion = \"0.0.1\"\n";
    std::fs::write(project_path.join("rsconstruct.lock"), lock).unwrap();

    for args in [
        &["tool", "install-deps"][..],
        &["tool", "check"],
        &["build"],
    ] {
        let output =
            run_rsconstruct_with_env(project_path, args, &[("NO_COLOR", "1"), ("CI", "true")]);
        assert!(
            output.status.success(),
            "{args:?} failed in CI: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let after = std::fs::read_to_string(project_path.join("rsconstruct.lock")).unwrap();
    assert_eq!(after, lock, "CI must not rewrite the lock");

    let output = run_rsconstruct_with_env(project_path, &["tool", "lock"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("- rsconstruct 0.0.1"), "{stdout}");
    assert!(stdout.contains("Updated rsconstruct.lock"), "{stdout}");
    let rewritten = std::fs::read_to_string(project_path.join("rsconstruct.lock")).unwrap();
    assert!(!rewritten.contains("[rsconstruct]"), "{rewritten}");
}

/// In CI a lock that does not match the project is an error, never
/// silently rewritten: here it pins a crate nothing needs.
#[test]
fn ci_rejects_a_lock_that_does_not_match() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();
    let stale = "version = 1\n\n[cargo]\ncargo-unused = \"1.0.0\"\n";
    std::fs::write(project_path.join("rsconstruct.lock"), stale).unwrap();

    let output = run_rsconstruct_with_env(
        project_path,
        &["tool", "install-deps"],
        &[("NO_COLOR", "1"), ("CI", "true")],
    );
    assert!(
        !output.status.success(),
        "install-deps accepted a stale lock in CI"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cargo cargo-unused"), "{stderr}");
    let after = std::fs::read_to_string(project_path.join("rsconstruct.lock")).unwrap();
    assert_eq!(after, stale, "CI must not rewrite the lock");

    // Outside CI the same lock is repaired: the unused entry goes.
    let output = run_rsconstruct_with_env(project_path, &["tool", "lock"], &[("NO_COLOR", "1")]);
    assert!(output.status.success());
    let repaired = std::fs::read_to_string(project_path.join("rsconstruct.lock")).unwrap();
    assert!(!repaired.contains("cargo-unused"), "{repaired}");
}

/// `--upgrade <name>` takes only names the project locks. An unknown name
/// is an error that leaves the lock alone, and `rsconstruct` is one:
/// rsconstruct itself is not locked (CI downloads the latest release).
#[test]
fn tool_lock_upgrade_named_rejects_names_the_project_does_not_lock() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();
    let old = "version = 1\n";
    std::fs::write(project_path.join("rsconstruct.lock"), old).unwrap();

    for name in ["nosuch", "rsconstruct"] {
        let output = run_rsconstruct_with_env(
            project_path,
            &["tool", "lock", "--upgrade", name],
            &[("NO_COLOR", "1")],
        );
        assert!(!output.status.success(), "{name} was accepted");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("not a tool this project locks: {name}")),
            "{stderr}"
        );
        let unchanged = std::fs::read_to_string(project_path.join("rsconstruct.lock")).unwrap();
        assert_eq!(unchanged, old);
    }
}

/// A leftover .tools.versions is an error that says what to do.
#[test]
fn legacy_tools_versions_is_rejected() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();
    std::fs::write(project_path.join(".tools.versions"), "{}").unwrap();

    let output = run_rsconstruct_with_env(project_path, &["tool", "lock"], &[("NO_COLOR", "1")]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Delete .tools.versions"), "{stderr}");
}

#[test]
fn tools_stats_shows_summary() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(project_path, &["tool", "stats"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "tool stats failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Tool"), "Expected 'Tool' table header");
    assert!(
        stdout.contains("Runtime summary:"),
        "Expected 'Runtime summary:' section"
    );
    assert!(stdout.contains("Total:"), "Expected 'Total:' summary line");
    assert!(stdout.contains("installed"), "Expected 'installed' count");
}

#[test]
fn tools_stats_json() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(
        project_path,
        &["--json", "tool", "stats"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        output.status.success(),
        "tool stats --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(&stdout).expect("Expected valid JSON");

    // Verify top-level structure
    assert!(parsed.get("tools").is_some(), "Expected 'tools' field");
    assert!(
        parsed.get("runtimes").is_some(),
        "Expected 'runtimes' field"
    );
    assert!(parsed.get("summary").is_some(), "Expected 'summary' field");

    // Verify tools array entries
    let tools = parsed["tools"]
        .as_array()
        .expect("'tools' should be an array");
    assert!(!tools.is_empty(), "tools array should not be empty");
    for tool in tools {
        assert!(tool.get("name").is_some(), "Tool entry should have 'name'");
        assert!(
            tool.get("installed").is_some(),
            "Tool entry should have 'installed'"
        );
        assert!(
            tool.get("runtime").is_some(),
            "Tool entry should have 'runtime'"
        );
        assert!(
            tool.get("processor").is_some(),
            "Tool entry should have 'processors'"
        );
    }

    // Verify runtimes array entries
    let runtimes = parsed["runtimes"]
        .as_array()
        .expect("'runtimes' should be an array");
    for rt in runtimes {
        assert!(
            rt.get("runtime").is_some(),
            "Runtime entry should have 'runtime'"
        );
        assert!(
            rt.get("total").is_some(),
            "Runtime entry should have 'total'"
        );
        assert!(
            rt.get("installed").is_some(),
            "Runtime entry should have 'installed'"
        );
        assert!(
            rt.get("missing").is_some(),
            "Runtime entry should have 'missing'"
        );
    }

    // Verify summary
    let summary = &parsed["summary"];
    assert!(
        summary.get("total_tools").is_some(),
        "Summary should have 'total_tools'"
    );
    assert!(
        summary.get("installed").is_some(),
        "Summary should have 'installed'"
    );
    assert!(
        summary.get("missing").is_some(),
        "Summary should have 'missing'"
    );

    // Verify consistency: total_tools == tools.len()
    let total_tools = summary["total_tools"].as_u64().unwrap();
    assert_eq!(
        total_tools as usize,
        tools.len(),
        "summary.total_tools should match tools array length"
    );

    // Verify consistency: installed + missing == total_tools
    let installed = summary["installed"].as_u64().unwrap();
    let missing = summary["missing"].as_u64().unwrap();
    assert_eq!(
        installed + missing,
        total_tools,
        "installed + missing should equal total_tools"
    );
}

/// Every install method named in the registry must be one that `install`
/// actually implements. A method string with no arm in `tools::run`
/// (there used to be a bogus "system") is not a config error the user can
/// see — it surfaces only when someone tries to install that tool, as an
/// "unknown install method" failure at the worst possible moment.
#[test]
fn tools_list_uses_only_implemented_install_methods() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(
        project_path,
        &["--json", "tool", "list"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        output.status.success(),
        "tool list --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Kept in sync with the `match method` arms in tools::run().
    const IMPLEMENTED: &[&str] = &[
        "apt",
        "dnf",
        "pacman",
        "brew",
        "snap",
        "pip",
        "npm",
        "npm-ci",
        "npm-install",
        "cargo",
        "gem",
        "binary",
        "manual",
    ];

    let parsed: Value =
        serde_json::from_slice(&output.stdout).expect("tool list --json should emit valid JSON");
    let tools = parsed
        .as_array()
        .expect("tool list --json should be an array");
    assert!(!tools.is_empty(), "registry should not be empty");

    for tool in tools {
        let name = tool["tool"].as_str().unwrap_or("<unnamed>");
        let methods = tool["install_methods"]
            .as_array()
            .expect("tool should have install_methods");
        assert!(
            !methods.is_empty(),
            "tool '{name}' has no install method at all"
        );
        for m in methods {
            let method = m["method"]
                .as_str()
                .expect("install method should have a 'method' string");
            assert!(
                IMPLEMENTED.contains(&method),
                "tool '{name}' declares install method '{method}', which tools::run() does not implement",
            );
        }
    }
}

/// Registry names are detection keys handed straight to `which::which`, which
/// only searches `$PATH` for names with no path separator. A name containing
/// `/` is resolved relative to the current working directory instead, so it
/// reports `missing` unless rsconstruct happens to run from the one directory
/// with that subtree beneath it — including for users who have the tool
/// installed and on `$PATH`. The registry once carried `gems/bin/mdl` and
/// `node_modules/.bin/markdownlint` for exactly this bug. Vendored paths belong
/// in the per-processor `command` config field, not here.
#[test]
fn tools_list_names_are_bare_binaries_not_paths() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(
        project_path,
        &["--json", "tool", "list"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        output.status.success(),
        "tool list --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value =
        serde_json::from_slice(&output.stdout).expect("tool list --json should emit valid JSON");
    let tools = parsed
        .as_array()
        .expect("tool list --json should be an array");
    assert!(!tools.is_empty(), "registry should not be empty");

    for tool in tools {
        let name = tool["tool"]
            .as_str()
            .expect("tool should have a 'tool' name string");
        assert!(
            !name.contains('/'),
            "tool '{name}' is a path, not a bare binary name; which() would resolve it \
             relative to the cwd and report it missing from anywhere else. Use the bare \
             binary name here and put the vendored path in the processor's `command` config.",
        );
    }
}

/// `tool install --all` must be able to install every registry entry.
/// A manual-only entry makes `--all` a hard error, which would break CI
/// provisioning, so the registry must not contain one.
#[test]
fn tools_install_all_has_no_manual_only_entries() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(
        project_path,
        &["--json", "tool", "list"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        output.status.success(),
        "tool list --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: Value =
        serde_json::from_slice(&output.stdout).expect("tool list --json should emit valid JSON");
    let manual_only: Vec<&str> = parsed
        .as_array()
        .expect("array")
        .iter()
        .filter(|tool| {
            tool["install_methods"].as_array().is_some_and(|ms| {
                !ms.is_empty() && ms.iter().all(|m| m["method"].as_str() == Some("manual"))
            })
        })
        .map(|tool| tool["tool"].as_str().unwrap_or("<unnamed>"))
        .collect();

    assert!(
        manual_only.is_empty(),
        "these tools have only a manual install method, so `tool install --all` cannot provision them: {manual_only:?}",
    );
}

/// `--all` walks the registry instead of the config, so it must reject a
/// tool name rather than silently ignoring one of the two.
#[test]
fn tools_install_all_conflicts_with_tool_name() {
    let temp_dir = setup_test_project();
    let project_path = temp_dir.path();

    let output = run_rsconstruct_with_env(
        project_path,
        &["tool", "install", "--all", "ruff"],
        &[("NO_COLOR", "1")],
    );
    assert!(
        !output.status.success(),
        "`tool install --all ruff` should be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "expected a clap conflict error, got: {stderr}",
    );
}

/// A locked release binary goes into the project's tool directory, linked
/// from the tool store, and is the copy every later command uses — ahead of
/// anything else on PATH. The store is pre-filled here with a stand-in
/// `rumdl` under the version the lock names, so nothing is downloaded.
/// `cache clear` keeps the tools, and a lock that names another version is
/// reported by `tool check` instead of passing on whatever is installed.
#[test]
fn locked_tools_are_installed_into_the_project_from_the_store() {
    let project = setup_project_with_config("[processor.checker.rumdl]\nsrc_dirs = [\"docs\"]\n");
    let root = project.path();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/a.md"), "# A\n").unwrap();
    let lock = |exe_sha: &str| {
        format!(
            "version = 1\n\n[binary.rumdl]\nurl = \"https://example.invalid/rumdl.tar.gz\"\n\
             sha256 = \"aa\"\nexecutable_sha256 = \"{exe_sha}\"\n"
        )
    };
    std::fs::write(root.join("rsconstruct.lock"), lock("feed")).unwrap();

    let scratch = tempfile::TempDir::new().unwrap();
    let store = scratch.path().join("store");
    let ran = scratch.path().join("ran");
    let stored = store.join("binary/rumdl@sha256-feed/bin/rumdl");
    std::fs::create_dir_all(stored.parent().unwrap()).unwrap();
    std::fs::write(
        &stored,
        format!("#!/bin/sh\necho \"$@\" >> {}\n", ran.display()),
    )
    .unwrap();
    crate::common::make_executable(&stored);
    let store_env = store.to_str().unwrap();
    let run = |args: &[&str], ci: bool| {
        let mut env = vec![("NO_COLOR", "1"), ("RSCONSTRUCT_TOOL_STORE", store_env)];
        if ci {
            env.push(("CI", "true"));
        }
        run_rsconstruct_with_env(root, args, &env)
    };
    let ok = |out: &std::process::Output, what: &str| {
        assert!(
            out.status.success(),
            "{what} failed:\n{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    };

    let out = run(&["tool", "install"], false);
    ok(&out, "tool install");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("binary rumdl sha256-feed (linked from the tool store)"),
        "{stdout}"
    );
    let linked = root.join(".rsconstruct/tools/bin/rumdl");
    let ino =
        |p: &std::path::Path| std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(p).unwrap());
    assert_eq!(
        ino(&linked),
        ino(&stored),
        "the project's copy is not a hard link"
    );

    ok(&run(&["tool", "check"], false), "tool check");
    ok(&run(&["build"], true), "CI build");
    let calls = std::fs::read_to_string(&ran).expect("the project's rumdl never ran");
    assert!(calls.contains("a.md"), "{calls}");

    ok(&run(&["cache", "clear"], false), "cache clear");
    assert!(linked.exists(), "cache clear removed the project's tools");
    ok(
        &run(&["tool", "check"], false),
        "tool check after cache clear",
    );

    std::fs::write(root.join("rsconstruct.lock"), lock("beef")).unwrap();
    let out = run(&["tool", "check"], false);
    assert!(!out.status.success(), "tool check accepted a drifted tool");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("binary rumdl — locked sha256-beef, installed sha256-feed"),
        "{stderr}"
    );
}
