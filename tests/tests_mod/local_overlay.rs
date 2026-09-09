//! Tests for two config features:
//! - Missing `src_dirs` entries are a config error; `[build]
//!   allow_missing_src_dirs = true` restores the old skip.
//! - `rsconstruct.local.toml` — a per-repo overlay deep-merged over the main
//!   config at load time.

use crate::common::{
    run_rsconstruct, run_rsconstruct_with_env, setup_project_with_config, write_file,
};
use std::fs;
use tempfile::TempDir;

/// A src_dirs entry that doesn't exist is a config error, not a silent skip:
/// a listed directory that is not there is a stale stanza or a copy of
/// another repo's config, and either way a processor that checks nothing.
#[test]
fn missing_src_dirs_is_a_config_error() {
    let temp_dir =
        setup_project_with_config("[processor.tera]\nsrc_dirs = [\"missing.templates\"]\n");
    let output = run_rsconstruct(temp_dir.path(), &["build"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(2),
        "a missing src_dirs entry must fail as a config error: {stderr}"
    );
    assert!(
        stderr.contains("processor.tera") && stderr.contains("missing.templates"),
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
        "[processor.tera.real]\n",
        "src_dirs = [\"tera.templates\"]\n",
        "\n",
        "[processor.tera.ghost]\n",
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
        "[processor.tera]\n",
        "src_dirs = [\"tera.templates\"]\n",
        "\n",
        "[processor.script.ghost_check]\n",
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
        "[processor.script.real_check]\n",
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
/// something the build requires to EXIST, but never something `tools install`
/// can provision. Treating the path as a registry tool made `tools install`
/// fail with "No install method for 'scripts/check_md.py'" for a script
/// checked into the repo, which broke CI across every repo using the shared
/// config.
#[test]
fn tools_install_skips_repo_local_script_commands() {
    let temp_dir = setup_project_with_config(concat!(
        "[processor.script.check_md]\n",
        "command = \"scripts/check_md.py\"\n",
        "src_dirs = [\"checked\"]\n",
        "src_extensions = [\".md\"]\n",
    ));
    let project = temp_dir.path();
    write_file(project, "checked/doc.md", "# doc");
    write_file(project, "scripts/check_md.py", "#!/usr/bin/env python3\n");

    let output = run_rsconstruct(project, &["tools", "install"]);
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

#[test]
fn local_overlay_disables_processor() {
    let temp_dir = setup_project_with_config("[processor.tera]\nsrc_dirs = [\"tera.templates\"]\n");
    let project = temp_dir.path();
    write_file(project, "tera.templates/gen.txt.tera", "generated");
    fs::write(
        project.join("rsconstruct.local.toml"),
        "[processor.tera]\nenabled = false\n",
    )
    .unwrap();

    let output = run_rsconstruct(project, &["build"]);
    assert!(
        output.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The overlay disabled tera, so nothing was rendered.
    assert!(!project.join("gen.txt").exists());
}

#[test]
fn local_overlay_adds_sections() {
    let temp_dir = setup_project_with_config("[processor.tera]\nsrc_dirs = [\"tera.templates\"]\n");
    let project = temp_dir.path();
    write_file(project, "tera.templates/gen.txt.tera", "generated");
    // The overlay adds a global [build] setting and a whole new processor
    // section — both must merge in.
    fs::write(
        project.join("rsconstruct.local.toml"),
        concat!(
            "[build]\n",
            "max_discovery_passes = 7\n",
            "\n",
            "[processor.zspell]\n",
            "src_dirs = [\"empty_docs\"]\n",
        ),
    )
    .unwrap();
    fs::create_dir_all(project.join("empty_docs")).unwrap();

    let output = run_rsconstruct(project, &["build"]);
    assert!(
        output.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.join("gen.txt").exists());

    // Both overlay sections merged: the [build] value is visible in the
    // merged config, and the added processor section is a known instance.
    let cfg = run_rsconstruct(project, &["processors", "config", "zspell"]);
    assert!(
        cfg.status.success(),
        "overlay-added [processor.zspell] should exist in the merged config: {}",
        String::from_utf8_lossy(&cfg.stderr)
    );
}

#[test]
fn local_overlay_field_wins_over_main() {
    let temp_dir = setup_project_with_config("[processor.tera]\ndep_inputs = [\"config/a.py\"]\n");
    let project = temp_dir.path();
    fs::write(
        project.join("rsconstruct.local.toml"),
        "[processor.tera]\ndep_inputs = [\"config/b.py\"]\n",
    )
    .unwrap();

    let output = run_rsconstruct(project, &["processors", "config", "tera"]);
    assert!(
        output.status.success(),
        "processors config failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("b.py"),
        "local value should win, got: {stdout}"
    );
    assert!(
        !stdout.contains("a.py"),
        "main value should be replaced, got: {stdout}"
    );
}

#[test]
fn local_overlay_without_main_config_fails() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(
        temp_dir.path().join("rsconstruct.local.toml"),
        "[processor.tera]\n",
    )
    .unwrap();

    let output = run_rsconstruct(temp_dir.path(), &["build"]);
    assert!(
        !output.status.success(),
        "build should fail without a main config"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("rsconstruct.local.toml found without rsconstruct.toml"),
        "expected overlay-without-main error, got: {stderr}"
    );
}

/// The user-level config (`$XDG_CONFIG_HOME/rsconstruct/config.toml`) sets
/// `[build]` defaults under the repo config: it applies when the repo says
/// nothing, and the repo's own `[build]` wins key by key.
#[test]
fn user_config_sets_build_defaults_under_the_repo() {
    let temp_dir = setup_project_with_config("[processor.tera]\nsrc_dirs = [\".\"]\n");
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
        "[build]\nreject_dot_src_dirs = false\n\n[processor.tera]\nsrc_dirs = [\".\"]\n",
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
        "[processor.tera]\nsrc_dirs = [\".\"]\n",
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
    let temp_dir = setup_project_with_config("[processor.tera]\nsrc_dirs = [\"tera.templates\"]\n");
    let project = temp_dir.path();
    write_file(project, "tera.templates/hello.txt.tera", "hello");
    let xdg = TempDir::new().unwrap();
    fs::create_dir_all(xdg.path().join("rsconstruct")).unwrap();
    fs::write(
        xdg.path().join("rsconstruct/config.toml"),
        "[build]\nreject_dot_src_dirs = true\n\n[processor.ruff]\nsrc_dirs = [\"src\"]\n",
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
        stderr.contains("only a [build] table") && stderr.contains("[processor]"),
        "error must say what is allowed and what was found: {stderr}"
    );
}
