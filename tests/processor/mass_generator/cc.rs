use crate::common::{run_rsconstruct_json, run_rsconstruct_with_env};
use std::fs;
use tempfile::TempDir;

/// Helper: set up a project with the cc processor enabled and a cc.yaml file.
fn setup_cc_project(project_path: &std::path::Path, cc_yaml: &str) {
    fs::create_dir_all(project_path.join("src")).unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.cc]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(project_path.join("cc.yaml"), cc_yaml).unwrap();
}

#[test]
fn cc_no_project_discovered() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // Enable cc processor but don't create cc.yaml
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.cc]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();

    let result = run_rsconstruct_json(project_path, &["build"]);
    assert!(result.exit_success, "Build should succeed with no cc.yaml");
    assert_eq!(result.total_products, 0, "No products should be discovered");
}

#[test]
fn cc_build_static_library() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(
        project_path,
        r#"
libraries:
  - name: mymath
    lib_type: static
    sources: [src/math.c]
"#,
    );

    fs::write(
        project_path.join("src/math.c"),
        "int add(int a, int b) { return a + b; }\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        project_path
            .join("out/processor.mass_generator.cc/lib/libmymath.a")
            .exists(),
        "Static library should exist"
    );
}

#[test]
fn cc_build_shared_library() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(
        project_path,
        r#"
libraries:
  - name: mymath
    lib_type: shared
    sources: [src/math.c]
"#,
    );

    fs::write(
        project_path.join("src/math.c"),
        "int add(int a, int b) { return a + b; }\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        project_path
            .join("out/processor.mass_generator.cc/lib/libmymath.so")
            .exists(),
        "Shared library should exist"
    );
}

#[test]
fn cc_build_program_with_library() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(
        project_path,
        r#"
libraries:
  - name: mymath
    lib_type: static
    sources: [src/math.c]

programs:
  - name: main
    sources: [src/main.c]
    link: [mymath]
"#,
    );

    fs::write(
        project_path.join("src/math.c"),
        "int add(int a, int b) { return a + b; }\n",
    )
    .unwrap();

    fs::write(
        project_path.join("src/main.c"),
        r#"extern int add(int, int);
int main() { return add(1, 2) - 3; }
"#,
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        project_path
            .join("out/processor.mass_generator.cc/lib/libmymath.a")
            .exists(),
        "Static library should exist"
    );
    assert!(
        project_path
            .join("out/processor.mass_generator.cc/bin/main")
            .exists(),
        "Executable should exist"
    );

    // Run the executable - should return 0 (add(1,2) - 3 == 0)
    let run_output =
        std::process::Command::new(project_path.join("out/processor.mass_generator.cc/bin/main"))
            .output()
            .expect("Failed to run executable");
    assert!(run_output.status.success(), "Executable should return 0");
}

#[test]
fn cc_incremental_skip() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(
        project_path,
        r#"
programs:
  - name: hello
    sources: [src/hello.c]
"#,
    );

    fs::write(
        project_path.join("src/hello.c"),
        "int main() { return 0; }\n",
    )
    .unwrap();

    // First build
    let output1 = run_rsconstruct_with_env(project_path, &["build", "-v"], &[("NO_COLOR", "1")]);
    assert!(output1.status.success());
    let stdout1 = String::from_utf8_lossy(&output1.stdout);
    assert!(
        stdout1.contains("Processing:"),
        "First build should process: {}",
        stdout1
    );

    // Second build - should skip
    let output2 =
        run_rsconstruct_with_env(project_path, &["build", "--verbose"], &[("NO_COLOR", "1")]);
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(
        stdout2.contains("[processor.mass_generator.cc] Skipping (unchanged):"),
        "Second build should skip: {}",
        stdout2
    );
}

#[test]
fn cc_single_invocation_mode() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("src")).unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.cc]\nsingle_invocation = true\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("cc.yaml"),
        r#"
programs:
  - name: hello
    sources: [src/hello.c]
"#,
    )
    .unwrap();
    fs::write(
        project_path.join("src/hello.c"),
        "#include <stdio.h>\nint main() { printf(\"hello\\n\"); return 0; }\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build with single_invocation failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        project_path
            .join("out/processor.mass_generator.cc/bin/hello")
            .exists(),
        "Executable should exist"
    );
}

#[test]
fn cc_build_both_library_types() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(
        project_path,
        r#"
libraries:
  - name: myutil
    lib_type: both
    sources: [src/util.c]
"#,
    );

    fs::write(
        project_path.join("src/util.c"),
        "int helper() { return 42; }\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        project_path
            .join("out/processor.mass_generator.cc/lib/libmyutil.a")
            .exists(),
        "Static library should exist"
    );
    assert!(
        project_path
            .join("out/processor.mass_generator.cc/lib/libmyutil.so")
            .exists(),
        "Shared library should exist"
    );
}

#[test]
fn cc_clean_removes_output_dir() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(
        project_path,
        r#"
programs:
  - name: hello
    sources: [src/hello.c]
"#,
    );

    fs::write(
        project_path.join("src/hello.c"),
        "int main() { return 0; }\n",
    )
    .unwrap();

    // Build
    let build_output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(build_output.status.success());
    assert!(
        project_path
            .join("out/processor.mass_generator.cc/bin/hello")
            .exists()
    );

    // Clean
    let clean_output =
        run_rsconstruct_with_env(project_path, &["clean", "outputs"], &[("NO_COLOR", "1")]);
    assert!(clean_output.status.success());
    assert!(
        !project_path
            .join("out/processor.mass_generator.cc/bin/hello")
            .exists(),
        "Output should be removed after clean"
    );
}

/// Two sources with the same stem in different directories must compile to
/// distinct object files — a flat stem-derived name would silently drop one
/// translation unit from the link.
#[test]
fn cc_same_stem_sources_do_not_collide() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(
        project_path,
        r#"
programs:
  - name: app
    sources: [src/main.c, src/a/util.c, src/b/util.c]
"#,
    );

    fs::create_dir_all(project_path.join("src/a")).unwrap();
    fs::create_dir_all(project_path.join("src/b")).unwrap();
    fs::write(
        project_path.join("src/main.c"),
        "int from_a(void); int from_b(void);\nint main(void) { return from_a() + from_b(); }\n",
    )
    .unwrap();
    fs::write(
        project_path.join("src/a/util.c"),
        "int from_a(void) { return 1; }\n",
    )
    .unwrap();
    fs::write(
        project_path.join("src/b/util.c"),
        "int from_b(void) { return -1; }\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "Build failed (same-stem object collision?): stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        project_path
            .join("out/processor.mass_generator.cc/bin/app")
            .exists(),
        "Program should link"
    );
}

/// `[processor.mass_generator.cc]` flags are project-wide defaults for every cc.yaml.
///
/// Regression test: the config-level `cflags`/`cxxflags`/`ldflags`/
/// `include_dirs` fields used to be fully dead — declared, documented, part
/// of the config hash, and read by nothing. The source below only compiles
/// when the config-level define reaches the compiler.
#[test]
fn cc_config_flags_are_manifest_defaults() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("src")).unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.cc]\nsrc_dirs = [\".\"]\ncflags = [\"-DNEED_FLAG=0\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("cc.yaml"),
        r#"
programs:
  - name: app
    sources: [src/main.c]
"#,
    )
    .unwrap();
    fs::write(
        project_path.join("src/main.c"),
        "int main(void) { return NEED_FLAG; }\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "config-level cflags must reach the compiler: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A cc.yaml that sets a field explicitly overrides the config default —
/// including setting it to empty.
#[test]
fn cc_manifest_overrides_config_flags() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    fs::create_dir_all(project_path.join("src")).unwrap();
    // The config default is an invalid flag: the build can only succeed if
    // the manifest's explicit empty cflags wins over the config value.
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.cc]\nsrc_dirs = [\".\"]\ncflags = [\"--definitely-not-a-flag\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("cc.yaml"),
        r#"
cflags: []
programs:
  - name: app
    sources: [src/main.c]
"#,
    )
    .unwrap();
    fs::write(
        project_path.join("src/main.c"),
        "int main(void) { return 0; }\n",
    )
    .unwrap();

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "manifest cflags must override the config default: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A compiler overridden per cc.yaml must be part of the processor's
/// declared tools.
///
/// Regression test: `required_tools()` used to return only the config-level
/// compilers, so a manifest override tripped the debug-build declared-tools
/// assertion ("executed undeclared tool") and made `tool check` verify the
/// wrong binary. Test binaries run the debug build, so this test fails by
/// panic if the override is not declared.
#[test]
fn cc_manifest_compiler_is_declared() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    // A distinctly-named compiler wrapper on PATH.
    let bin_dir = project_path.join("toolbin");
    fs::create_dir_all(&bin_dir).unwrap();
    let wrapper = bin_dir.join("mycustomcc");
    fs::write(&wrapper, "#!/bin/sh\nexec gcc \"$@\"\n").unwrap();
    crate::common::make_executable(&wrapper);

    fs::create_dir_all(project_path.join("src")).unwrap();
    fs::write(
        project_path.join("rsconstruct.toml"),
        "[processor.mass_generator.cc]\nsrc_dirs = [\".\"]\n",
    )
    .unwrap();
    fs::write(
        project_path.join("cc.yaml"),
        r#"
cc: mycustomcc
programs:
  - name: app
    sources: [src/main.c]
"#,
    )
    .unwrap();
    fs::write(
        project_path.join("src/main.c"),
        "int main(void) { return 0; }\n",
    )
    .unwrap();

    let path_env = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = run_rsconstruct_with_env(
        project_path,
        &["build"],
        &[("NO_COLOR", "1"), ("PATH", &path_env)],
    );
    assert!(
        output.status.success(),
        "manifest-overridden compiler must be declared (debug builds assert on undeclared tools): stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        project_path
            .join("out/processor.mass_generator.cc/bin/app")
            .exists(),
        "Program should link"
    );
}

/// Write `content` to `rel` under the project, creating parent dirs.
fn write_project_file(project_path: &std::path::Path, rel: &str, content: &str) {
    let path = project_path.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Run `rsconstruct build` with JSON output, asserting it succeeded.
fn build_json(project_path: &std::path::Path) -> crate::common::BuildResult {
    let result = run_rsconstruct_json(project_path, &["build"]);
    assert!(
        result.exit_success,
        "build failed: {:?} {:?}",
        result.errors, result.products
    );
    result
}

/// A cc.yaml product lists the manifest first and its sources after it,
/// so the C/C++ analyzers used to look at `inputs[0]`, see a `.yaml`, and
/// match nothing: editing a header included by a listed source rebuilt
/// nothing. With `[analyzer.cpp]` every listed source is scanned, a header
/// one of them includes rebuilds the targets built from it, and a header
/// nothing includes does not.
#[test]
fn cc_header_edit_rebuilds_with_cpp_analyzer() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    write_project_file(
        project_path,
        "rsconstruct.toml",
        "[processor.mass_generator.cc]\nsrc_dirs = [\".\"]\n\n[analyzer.cpp]\ninclude_paths = [\"include\"]\n",
    );
    write_project_file(
        project_path,
        "cc.yaml",
        r#"
include_dirs: [include]

libraries:
  - name: mymath
    lib_type: static
    sources: [src/math.c]

programs:
  - name: main
    sources: [src/main.c]
    link: [mymath]
"#,
    );
    write_project_file(project_path, "include/mymath.h", "int add(int a, int b);\n");
    write_project_file(project_path, "include/unrelated.h", "#define UNUSED 1\n");
    write_project_file(
        project_path,
        "src/math.c",
        "#include \"mymath.h\"\nint add(int a, int b) { return a + b; }\n",
    );
    write_project_file(
        project_path,
        "src/main.c",
        "#include \"mymath.h\"\nint main(void) { return add(1, 2) - 3; }\n",
    );

    let first = build_json(project_path);
    assert!(first.total_products > 0, "the manifest must plan products");
    assert_eq!(
        first.success, first.total_products,
        "first build builds everything: {:?}",
        first.products
    );

    let second = build_json(project_path);
    assert_eq!(
        second.skipped, second.total_products,
        "unchanged project must skip everything: {:?}",
        second.products
    );

    // Every target compiles a source that includes mymath.h, so all of them
    // are dirty once it changes.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_project_file(
        project_path,
        "include/mymath.h",
        "/* edited */\nint add(int a, int b);\n",
    );
    let third = build_json(project_path);
    assert_eq!(
        third.success, third.total_products,
        "editing an included header must rebuild the targets that include it: {:?}",
        third.products
    );

    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_project_file(project_path, "include/unrelated.h", "#define UNUSED 2\n");
    let fourth = build_json(project_path);
    assert_eq!(
        fourth.skipped, fourth.total_products,
        "editing a header nothing includes must rebuild nothing: {:?}",
        fourth.products
    );
}

/// Each object gets the flags of its own language at both levels: a
/// target's `cxxflags` reach its C++ objects and its `cflags` its C
/// objects, never the other way round. The sources `#error` on a flag from
/// the wrong language, so the build is green only when the split is exact.
/// Regression test: per-target `cxxflags` were parsed and never used, and
/// per-target `cflags` went to every object.
#[test]
fn cc_target_flags_follow_the_source_language() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    setup_cc_project(
        project_path,
        r#"
libraries:
  - name: helper
    lib_type: static
    sources: [src/helper.cpp]
    cflags: [-DLIB_C=1]
    cxxflags: [-DLIB_CXX=1]

programs:
  - name: app
    sources: [src/main.cpp, src/util.c]
    link: [helper]
    cflags: [-DPROG_C=1]
    cxxflags: [-DPROG_CXX=1]
"#,
    );
    write_project_file(
        project_path,
        "src/helper.cpp",
        "#ifndef LIB_CXX\n#error library cxxflags did not reach a C++ object\n#endif\n\
         #ifdef LIB_C\n#error library cflags leaked into a C++ object\n#endif\n\
         extern \"C\" int helper(void) { return 1; }\n",
    );
    write_project_file(
        project_path,
        "src/main.cpp",
        "#ifndef PROG_CXX\n#error program cxxflags did not reach a C++ object\n#endif\n\
         #ifdef PROG_C\n#error program cflags leaked into a C++ object\n#endif\n\
         extern \"C\" int helper(void);\nextern \"C\" int util(void);\n\
         int main() { return helper() + util() - 2; }\n",
    );
    write_project_file(
        project_path,
        "src/util.c",
        "#ifndef PROG_C\n#error program cflags did not reach a C object\n#endif\n\
         #ifdef PROG_CXX\n#error program cxxflags leaked into a C object\n#endif\n\
         int util(void) { return 1; }\n",
    );

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "per-target flags must follow the source language: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let run_output =
        std::process::Command::new(project_path.join("out/processor.mass_generator.cc/bin/app"))
            .output()
            .expect("Failed to run executable");
    assert!(run_output.status.success(), "Executable should return 0");
}

/// Single-invocation mode used to pass only the global language flags:
/// the manifest's and the program's `include_dirs` and the program's own
/// flags were dropped. Both programs here compile only with them: the C
/// one needs headers from both include-dir levels and a program `cflags`
/// define; the C++ one needs a program `cxxflags` define and would fail on
/// the program's (invalid) `cflags` if they reached a C++ command.
#[test]
fn cc_single_invocation_honours_include_dirs_and_program_flags() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let project_path = temp_dir.path();

    write_project_file(
        project_path,
        "rsconstruct.toml",
        "[processor.mass_generator.cc]\nsingle_invocation = true\nsrc_dirs = [\".\"]\n",
    );
    write_project_file(
        project_path,
        "cc.yaml",
        r#"
include_dirs: [include]

programs:
  - name: app
    sources: [src/main.c]
    include_dirs: [extra]
    cflags: [-DNEED_FLAG=0]

  - name: appxx
    sources: [src/main.cpp]
    cflags: [--definitely-not-a-flag]
    cxxflags: [-DNEED_CXX=0]
"#,
    );
    write_project_file(project_path, "include/a.h", "#define A 0\n");
    write_project_file(project_path, "extra/b.h", "#define B 0\n");
    write_project_file(
        project_path,
        "src/main.c",
        "#include \"a.h\"\n#include \"b.h\"\nint main(void) { return A + B + NEED_FLAG; }\n",
    );
    write_project_file(
        project_path,
        "src/main.cpp",
        "int main() { return NEED_CXX; }\n",
    );

    let output = run_rsconstruct_with_env(project_path, &["build"], &[("NO_COLOR", "1")]);
    assert!(
        output.status.success(),
        "single invocation must pass include dirs and the program's language flags: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for exe in ["app", "appxx"] {
        let path = project_path.join(format!("out/processor.mass_generator.cc/bin/{exe}"));
        assert!(path.exists(), "{exe} should link");
        let run_output = std::process::Command::new(&path)
            .output()
            .expect("Failed to run executable");
        assert!(run_output.status.success(), "{exe} should return 0");
    }
}
