//! Tests for loading the config:
//! - Missing `src_dirs` entries are a config error; `[build]
//!   allow_missing_src_dirs = true` restores the old skip.
//! - The user config sets `[build]` defaults under `rsconstruct.toml`.
//! - A config error names the file and line to edit.

use crate::common::{
    run_rsconstruct, run_rsconstruct_with_env, setup_project_with_config, write_file,
};
use std::fs;
use tempfile::TempDir;

/// A `src_dirs` entry that doesn't exist is a config error, not a silent skip:
/// a listed directory that is not there is a stale stanza or a copy of
/// another repo's config, and either way a processor that checks nothing.
#[test]
fn missing_src_dirs_is_a_config_error() {
    let temp_dir = setup_project_with_config(
        "[processor.generator.tera]\nsrc_dirs = [\"missing.templates\"]\n",
    );
    let output = run_rsconstruct(temp_dir.path(), &["build"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a missing src_dirs entry must fail as a config error: {stderr}"
    );
    assert!(
        stderr.contains("processor.generator.tera") && stderr.contains("missing.templates"),
        "error must name the processor and the entry: {stderr}"
    );
}

/// With the skip restored, a missing entry does not disturb its siblings:
/// the instance with a real directory still builds.
#[test]
fn allow_missing_src_dirs_does_not_disturb_sibling_instances() {
    let temp_dir = setup_project_with_config(concat!(
        "[build]\n",
        "allow_missing_src_dirs = true\n",
        "\n",
        "[processor.generator.tera.real]\n",
        "src_dirs = [\"tera.templates\"]\n",
        "\n",
        "[processor.generator.tera.ghost]\n",
        "src_dirs = [\"missing.templates\"]\n",
    ));
    let project = temp_dir.path();
    write_file(project, "tera.templates/hello.txt.tera", "hello");

    let output = run_rsconstruct(project, &["build"]);
    assert!(
        output.status.success(),
        "build should skip the missing dir: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The instance with a real directory still produced its output.
    assert!(project.join("hello.txt").exists());
}

#[test]
fn tool_check_is_deferred_to_processors_with_products() {
    // A declared processor whose tool is absent must not fail the build
    // when it has no products in this repo: its directory exists but holds
    // no file it matches.
    let temp_dir = setup_project_with_config(concat!(
        "[processor.generator.tera]\n",
        "src_dirs = [\"tera.templates\"]\n",
        "\n",
        "[processor.checker.script.ghost_check]\n",
        "command = \"scripts/does_not_exist.py\"\n",
        "src_dirs = [\"ghost_src\"]\n",
        "src_extensions = [\".md\"]\n",
    ));
    let project = temp_dir.path();
    write_file(project, "tera.templates/out.txt.tera", "ok");
    fs::create_dir_all(project.join("ghost_src")).unwrap();

    let output = run_rsconstruct(project, &["build"]);
    assert!(
        output.status.success(),
        "tool-less zero-product processor should not fail the build: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.join("out.txt").exists());
}

/// Deferring the tool check to processors with products must not weaken it:
/// a processor that DOES match files still fails when its tool is absent.
#[test]
fn missing_tool_fails_when_processor_has_products() {
    let temp_dir = setup_project_with_config(concat!(
        "[processor.checker.script.real_check]\n",
        "command = \"scripts/does_not_exist.py\"\n",
        "src_dirs = [\"checked\"]\n",
        "src_extensions = [\".md\"]\n",
    ));
    let project = temp_dir.path();
    write_file(project, "checked/doc.md", "# doc");

    let output = run_rsconstruct(project, &["build"]);
    assert!(
        !output.status.success(),
        "missing tool must fail when the processor has products"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Missing required tools"),
        "expected strict tool preflight, got: {stderr}"
    );
}

/// The other half of the preflight rule: a repo-local script command is
/// something the build requires to EXIST, but never something `tool install`
/// can provision. Treating the path as a registry tool made `tool install`
/// fail with "No install method for '`scripts/check_md.py`'" for a script
/// checked into the repo, which broke CI across every repo using the shared
/// config.
#[test]
fn tools_install_skips_repo_local_script_commands() {
    let temp_dir = setup_project_with_config(concat!(
        "[processor.checker.script.check_md]\n",
        "command = \"scripts/check_md.py\"\n",
        "src_dirs = [\"checked\"]\n",
        "src_extensions = [\".md\"]\n",
    ));
    let project = temp_dir.path();
    write_file(project, "checked/doc.md", "# doc");
    write_file(project, "scripts/check_md.py", "#!/usr/bin/env python\n");

    let output = run_rsconstruct(project, &["tool", "install", "machine"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "a repo-local script command is not an installable tool: {stderr}"
    );
    assert!(
        !stderr.contains("No install method"),
        "script path must not be resolved against the tool registry: {stderr}"
    );
}

/// `rsconstruct.local.toml` was a per-repo overlay merged over the main
/// config until 0.9.177. No repo used it; the file is now an ordinary file
/// that nothing reads, so what it says changes nothing.
#[test]
fn a_local_toml_file_is_not_read() {
    let temp_dir =
        setup_project_with_config("[processor.generator.tera]\nsrc_dirs = [\"tera.templates\"]\n");
    let project = temp_dir.path();
    write_file(project, "tera.templates/gen.txt.tera", "generated");
    write_file(
        project,
        "rsconstruct.local.toml",
        "[processor.generator.tera]\nenabled = false\n",
    );

    let output = run_rsconstruct(project, &["build"]);
    assert!(
        output.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.join("gen.txt").exists());
}

/// The user-level config (`$XDG_CONFIG_HOME/rsconstruct/config.toml`) sets
/// `[build]` defaults under the repo config: it applies when the repo says
/// nothing, and the repo's own `[build]` wins key by key.
#[test]
fn user_config_sets_build_defaults_under_the_repo() {
    let temp_dir = setup_project_with_config("[processor.generator.tera]\nsrc_dirs = [\".\"]\n");
    let project = temp_dir.path();
    write_file(project, "tera.templates/hello.txt.tera", "hello");
    let xdg = TempDir::new().unwrap();
    fs::create_dir_all(xdg.path().join("rsconstruct")).unwrap();
    fs::write(
        xdg.path().join("rsconstruct/config.toml"),
        "[build]\nreject_dot_src_dirs = true\n",
    )
    .unwrap();
    let xdg_path = xdg.path().to_str().unwrap();

    // The user default applies: "." is rejected.
    let output = run_rsconstruct_with_env(project, &["build"], &[("XDG_CONFIG_HOME", xdg_path)]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "the user config's reject_dot_src_dirs must apply: {stderr}"
    );
    assert!(
        stderr.contains("reject_dot_src_dirs"),
        "unexpected error: {stderr}"
    );

    // The repo overrides the user default key by key.
    fs::write(
        project.join("rsconstruct.toml"),
        "[build]\nreject_dot_src_dirs = false\n\n[processor.generator.tera]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    let output = run_rsconstruct_with_env(project, &["build"], &[("XDG_CONFIG_HOME", xdg_path)]);
    assert!(
        output.status.success(),
        "the repo's own [build] must win over the user config: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.join("hello.txt").exists());

    // Without the user file the default is off.
    let empty = TempDir::new().unwrap();
    fs::write(
        project.join("rsconstruct.toml"),
        "[processor.generator.tera]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    let output = run_rsconstruct_with_env(
        project,
        &["build"],
        &[("XDG_CONFIG_HOME", empty.path().to_str().unwrap())],
    );
    assert!(
        output.status.success(),
        "no user file means the built-in default: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The user config may carry `[build]` only: anything that could change what
/// a repo builds must live in the repo, where CI sees it.
#[test]
fn user_config_rejects_sections_other_than_build() {
    let temp_dir =
        setup_project_with_config("[processor.generator.tera]\nsrc_dirs = [\"tera.templates\"]\n");
    let project = temp_dir.path();
    write_file(project, "tera.templates/hello.txt.tera", "hello");
    let xdg = TempDir::new().unwrap();
    fs::create_dir_all(xdg.path().join("rsconstruct")).unwrap();
    fs::write(
        xdg.path().join("rsconstruct/config.toml"),
        "[build]\nreject_dot_src_dirs = true\n\n[processor.checker.ruff]\nsrc_dirs = [\"src\"]\n",
    )
    .unwrap();
    let output = run_rsconstruct_with_env(
        project,
        &["build"],
        &[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a [processor] table in the user config must be refused: {stderr}"
    );
    assert!(
        stderr.contains("only a [build] table and a [tables] table")
            && stderr.contains("[processor]"),
        "error must say what is allowed and what was found: {stderr}"
    );
}

/// `toml check` rejects what every building command rejects: compression
/// with the hardlink restore method used to pass the check and fail later.
#[test]
fn toml_check_rejects_compression_with_hardlink_restore() {
    let temp_dir =
        setup_project_with_config("[cache]\ncompression = true\nrestore_method = \"hardlink\"\n");
    let output = run_rsconstruct(temp_dir.path(), &["toml", "check"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    assert!(stderr.contains("compression"), "{stderr}");
}

/// Discovery converges when a pass declares no new file. With a cap of one
/// pass, a project whose products feed nothing used to fail as
/// "did not converge", because the check looked at new products.
#[test]
fn one_discovery_pass_is_enough_when_nothing_is_routed() {
    let temp_dir = setup_project_with_config(
        "[build]\nmax_discovery_passes = 1\n\n[processor.generator.tera]\nsrc_dirs = [\"tera.templates\"]\n",
    );
    write_file(temp_dir.path(), "tera.templates/hello.txt.tera", "hello");
    let output = run_rsconstruct(temp_dir.path(), &["build"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(temp_dir.path().join("hello.txt").exists());
}

/// What `toml check` prints on stderr for a project with this
/// `rsconstruct.toml` and this user config (none when `user` is empty).
/// Every case here is a config error.
fn check_stderr(main: &str, user: &str) -> String {
    let temp_dir = setup_project_with_config(main);
    let xdg = TempDir::new().unwrap();
    if !user.is_empty() {
        write_file(xdg.path(), "rsconstruct/config.toml", user);
    }
    let output = run_rsconstruct_with_env(
        temp_dir.path(),
        &["toml", "check"],
        &[("XDG_CONFIG_HOME", xdg.path().to_str().unwrap())],
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(2), "{stderr}");
    stderr
}

/// A schema mistake is found in the merged config, which has no positions:
/// the message used to name the section and leave the file and the line to
/// the reader. Each one now leads with the file and line of its key.
#[test]
fn schema_errors_name_the_file_and_line() {
    let main = "# main\n[processor.checker.ruff]\nbogus = 1\n\n[processor.ruff]\n\n[analyzer.python]\nenabeld = true\n";
    let stderr = check_stderr(main, "");
    for expected in [
        "rsconstruct.toml:3: [processor.checker.ruff]: unknown field 'bogus'",
        "rsconstruct.toml:5: [processor.ruff]: unknown processor type",
        "rsconstruct.toml:8: [analyzer.python]: unknown field 'enabeld'",
    ] {
        assert!(
            stderr.contains(expected),
            "missing `{expected}` in: {stderr}"
        );
    }
}

/// A required field that is missing has no line of its own: the stanza's
/// header is where it has to be added.
#[test]
fn a_missing_required_field_is_located_at_its_stanza() {
    let stderr = check_stderr(
        "[build]\nparallel = 1\n\n[processor.explicit.generic.a]\ninputs = [\"x\"]\n",
        "",
    );
    assert!(
        stderr.contains("rsconstruct.toml:4: [processor.explicit.generic.a]: required field"),
        "{stderr}"
    );
}

/// A value of the wrong type in a top-level section was reported against
/// "config (merged from ...; the error is in one of them)". The file that
/// holds it fails the same way on its own, with line, column and the line
/// itself.
#[test]
fn a_type_error_is_reported_in_the_file_that_holds_it() {
    let stderr = check_stderr(
        "[cache]\ncompression = false\n",
        "# user\n[build]\nparalel = 4\n",
    );
    assert!(
        stderr.contains("rsconstruct/config.toml") && stderr.contains("line 3, column 1"),
        "{stderr}"
    );
    assert!(!stderr.contains("merged from"), "{stderr}");

    let stderr = check_stderr(
        "[build]\nparallel = 2\n\n[cache]\nbogus = 1\n",
        "[build]\nparallel = 3\n",
    );
    assert!(
        stderr.contains("Failed to parse config file rsconstruct.toml")
            && stderr.contains("line 5, column 1"),
        "{stderr}"
    );
}

/// The range checks on `[build]` and the cross-field check on `[cache]`
/// run on the loaded config and name the line of the offending key.
#[test]
fn value_checks_name_the_file_and_line() {
    let stderr = check_stderr("[build]\nparallel = 2\n", "[build]\n\nmax_arg_len = 0\n");
    assert!(
        stderr.contains("rsconstruct/config.toml:3: [build] max_arg_len"),
        "{stderr}"
    );
    let stderr = check_stderr(
        "[cache]\nrestore_method = \"hardlink\"\ncompression = true\n",
        "",
    );
    assert!(
        stderr.contains("rsconstruct.toml:3: [cache] compression"),
        "{stderr}"
    );
}

/// A value only the processor's or analyzer's own config type rejects is
/// found when the stanza is built, long after the files were merged, and
/// is still reported at the stanza's header.
#[test]
fn a_stanza_that_cannot_be_built_is_located() {
    let stderr = check_stderr(
        "[build]\nparallel = 1\n\n[processor.checker.ruff.core]\nmax_jobs = -1\n",
        "",
    );
    assert!(
        stderr.contains("rsconstruct.toml:4: [processor.checker.ruff.core]:"),
        "{stderr}"
    );
}

/// An undefined or embedded `${...}` reference names its line.
#[test]
fn variable_errors_name_the_line() {
    let stderr = check_stderr(
        "[vars]\na = \"x\"\n\n[processor.checker.ruff]\nargs = [\"${nope}\"]\n",
        "",
    );
    assert!(
        stderr.contains("rsconstruct.toml: line 5: Undefined variable: ${nope}"),
        "{stderr}"
    );
    let stderr = check_stderr(
        "[vars]\na = \"x\"\n\n[processor.checker.ruff]\n\nargs = [\"${a}/src\"]\n",
        "",
    );
    assert!(
        stderr.contains("rsconstruct.toml: line 6: Unresolved variable reference ${a}"),
        "{stderr}"
    );
}

/// Each file is type-checked on its own. A mistyped value in the user
/// config that the project file overrides is gone from the merged config,
/// so it used to pass until the day the override was removed.
#[test]
fn an_overridden_mistake_is_still_reported() {
    let stderr = check_stderr(
        "[build]\nparallel = 2\n",
        "[tables]\n\n[build]\nparallel = \"x\"\n",
    );
    assert!(
        stderr.contains("rsconstruct/config.toml") && stderr.contains("line 4, column 12"),
        "{stderr}"
    );
}
