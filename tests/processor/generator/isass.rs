use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// A project with `sass/style.scss` that `@use`s the partial `sass/_vars.scss`.
fn setup_isass_project(config: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();
    fs::create_dir_all(project_path.join("sass")).expect("Failed to create sass dir");
    fs::write(project_path.join("rsconstruct.toml"), config).expect("Failed to write config");
    fs::write(project_path.join("sass/_vars.scss"), "$color: red;\n").unwrap();
    fs::write(
        project_path.join("sass/style.scss"),
        "@use \"vars\";\nbody { color: vars.$color; }\n",
    )
    .unwrap();
    temp_dir
}

fn build_ok(project_path: &Path) {
    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "rsconstruct build failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

const OUT: &str = "out/processor.generator.isass";

#[test]
fn isass_basic_compile() {
    let temp_dir = setup_isass_project("[processor.generator.isass]\nsrc_dirs = [\"sass\"]\n");
    let project_path = temp_dir.path();
    build_ok(project_path);

    let css = fs::read_to_string(project_path.join(OUT).join("style.css"))
        .expect("style.css was not created");
    assert!(
        css.contains("red"),
        "CSS should carry the variable's value: {css}"
    );
}

/// Like the `sass` CLI, a partial (`_vars.scss`) is not compiled to a `.css`
/// of its own: it exists to be `@use`d by other stylesheets.
#[test]
fn isass_skips_partials_by_default() {
    let temp_dir = setup_isass_project("[processor.generator.isass]\nsrc_dirs = [\"sass\"]\n");
    let project_path = temp_dir.path();
    build_ok(project_path);

    assert!(project_path.join(OUT).join("style.css").exists());
    assert!(
        !project_path.join(OUT).join("_vars.css").exists(),
        "the partial must not get its own CSS"
    );
}

#[test]
fn isass_compiles_partials_when_asked() {
    let temp_dir = setup_isass_project(
        "[processor.generator.isass]\nsrc_dirs = [\"sass\"]\nskip_partials = false\n",
    );
    let project_path = temp_dir.path();
    build_ok(project_path);

    assert!(project_path.join(OUT).join("style.css").exists());
    assert!(
        project_path.join(OUT).join("_vars.css").exists(),
        "skip_partials = false must compile the partial too"
    );
}
