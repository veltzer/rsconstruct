use std::fs;
use std::path::Path;

use crate::common::{make_executable, run_rsconstruct, write_file};

/// A fake `make` that mimics the parts of kbuild the linux_module processor
/// relies on, without needing real kernel headers:
///   * `modules` — reads the generated Kbuild in M=<dir>, extracts the module
///     name from `obj-m := <name>.o`, and writes `<dir>/<name>.ko`.
///   * `clean`   — recursively deletes every `*.ko` and `*.o` under M=<dir>
///     (kbuild's clean is destructive over the whole module tree — this is
///     exactly the behavior that used to wipe an output written under the
///     module dir).
///
/// `modules` first writes the intermediate `<source>.o` kbuild would, and
/// fails (exit 1) when a `.fail-modules` marker sits in M=<dir>, so tests
/// can watch what a failed build leaves behind. It ignores `-C <kdir>`,
/// ARCH=, CROSS_COMPILE=, V=, W=.
const FAKE_MAKE: &str = r#"#!/usr/bin/env python3
import os
import sys

m_dir = None
subcommand = None
for arg in sys.argv[1:]:
    if arg.startswith("M="):
        m_dir = arg[2:]
    elif arg in ("modules", "clean"):
        subcommand = arg

if subcommand == "modules":
    kbuild = os.path.join(m_dir, "Kbuild")
    name = None
    with open(kbuild) as handle:
        for line in handle:
            if line.startswith("obj-m"):
                # obj-m := <name>.o
                name = line.split(":=", 1)[1].strip()[:-len(".o")]
            elif "-objs" in line:
                for obj in line.split(":=", 1)[1].split():
                    with open(os.path.join(m_dir, obj), "wb") as handle_o:
                        handle_o.write(b"FAKE-OBJ")
    if os.path.exists(os.path.join(m_dir, ".fail-modules")):
        sys.stderr.write("fake make: error: compilation failed\n")
        sys.exit(1)
    with open(os.path.join(m_dir, name + ".ko"), "wb") as handle:
        handle.write(b"FAKE-KO-BYTES")
elif subcommand == "clean":
    for root, _dirs, files in os.walk(m_dir):
        for filename in files:
            if filename.endswith((".ko", ".o")):
                os.unlink(os.path.join(root, filename))
sys.exit(0)
"#;

/// Write the fake make into the project as `bin/<name>` and return its
/// absolute path as a string, so the manifest's `make:` can point straight
/// at it.
fn install_fake_make_named(project: &Path, name: &str) -> String {
    let make_path = project.join("bin").join(name);
    fs::create_dir_all(make_path.parent().unwrap()).unwrap();
    fs::write(&make_path, FAKE_MAKE).unwrap();
    make_executable(&make_path);
    make_path.to_string_lossy().into_owned()
}

/// The fake make under its usual name.
fn install_fake_make(project: &Path) -> String {
    install_fake_make_named(project, "make")
}

/// Regression test for the root-level manifest bug: when linux-module.yaml sits
/// at the repo root, the module directory is the repo root, so the output dir
/// (out/processor.mass_generator.linux_module/) lives *inside* it. kbuild's `make clean` recursively
/// deletes .ko files under the module dir — which used to wipe the just-copied
/// output, leaving the declared output missing ("Failed to read output:
/// out/processor.mass_generator.linux_module/<name>.ko"). The processor must capture the .ko before
/// cleaning and write it out afterwards, so a root-level module builds cleanly.
#[test]
fn linux_module_root_level_manifest_output_survives_clean() {
    let temp = tempfile::TempDir::new().unwrap();
    let project = temp.path();

    let make = install_fake_make(project);
    // A dummy kdir — the fake make ignores -C, so it need not be a real one.
    fs::create_dir_all(project.join("kdir")).unwrap();
    let kdir = project.join("kdir").to_string_lossy().into_owned();

    write_file(project, "top.c", "int top;\n");
    write_file(project, "helper.c", "int helper;\n");
    write_file(
        project,
        "linux-module.yaml",
        &format!(
            "make: {make}\nkdir: {kdir}\nmodules:\n  - name: mymod\n    sources: [top.c, helper.c]\n"
        ),
    );
    write_file(
        project,
        "rsconstruct.toml",
        "[processor.mass_generator.linux_module]\nsrc_dirs = [\".\"]\n",
    );

    let output = run_rsconstruct(project, &["build"]);
    assert!(
        output.status.success(),
        "build failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // The declared output exists and holds the built module.
    let ko = project.join("out/processor.mass_generator.linux_module/mymod.ko");
    assert!(
        ko.exists(),
        "output module not produced at {}",
        ko.display()
    );
    assert_eq!(fs::read(&ko).unwrap(), b"FAKE-KO-BYTES");

    // The source tree is left clean: no leftover .ko or generated Kbuild.
    assert!(
        !project.join("mymod.ko").exists(),
        "source .ko not cleaned up"
    );
    assert!(
        !project.join("Kbuild").exists(),
        "generated Kbuild not removed"
    );
}

/// A manifest in a subdirectory keeps working: the module dir is the subdir,
/// the output lands under out/processor.mass_generator.linux_module/<subdir>/, and clean over the subdir
/// does not touch it. This guards the reordered clean/write against a
/// regression in the common (subdir) layout.
#[test]
fn linux_module_subdir_manifest_builds() {
    let temp = tempfile::TempDir::new().unwrap();
    let project = temp.path();

    let make = install_fake_make(project);
    fs::create_dir_all(project.join("kdir")).unwrap();
    let kdir = project.join("kdir").to_string_lossy().into_owned();

    write_file(project, "drivers/hello/main.c", "int hello;\n");
    write_file(
        project,
        "drivers/hello/linux-module.yaml",
        &format!("make: {make}\nkdir: {kdir}\nmodules:\n  - name: hello\n    sources: [main.c]\n"),
    );
    write_file(
        project,
        "rsconstruct.toml",
        "[processor.mass_generator.linux_module]\nsrc_dirs = [\"drivers\"]\n",
    );

    let output = run_rsconstruct(project, &["build"]);
    assert!(
        output.status.success(),
        "build failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let ko = project.join("out/processor.mass_generator.linux_module/drivers/hello/hello.ko");
    assert!(
        ko.exists(),
        "output module not produced at {}",
        ko.display()
    );
    assert_eq!(fs::read(&ko).unwrap(), b"FAKE-KO-BYTES");
    assert!(
        !project.join("drivers/hello/hello.ko").exists(),
        "source .ko not cleaned up"
    );
    assert!(
        !project.join("drivers/hello/Kbuild").exists(),
        "generated Kbuild not removed"
    );
}

/// A failed `make modules` must leave the source directory as it found it:
/// the generated `Kbuild` and kbuild's intermediates (`.o` files) are
/// cleaned up on the error path too, not only after a successful build.
#[test]
fn linux_module_failed_build_cleans_the_source_directory() {
    let temp = tempfile::TempDir::new().unwrap();
    let project = temp.path();

    let make = install_fake_make(project);
    fs::create_dir_all(project.join("kdir")).unwrap();
    let kdir = project.join("kdir").to_string_lossy().into_owned();

    write_file(project, "drivers/bad/main.c", "int bad;\n");
    write_file(project, "drivers/bad/.fail-modules", "");
    write_file(
        project,
        "drivers/bad/linux-module.yaml",
        &format!("make: {make}\nkdir: {kdir}\nmodules:\n  - name: bad\n    sources: [main.c]\n"),
    );
    write_file(
        project,
        "rsconstruct.toml",
        "[processor.mass_generator.linux_module]\nsrc_dirs = [\"drivers\"]\n",
    );

    let output = run_rsconstruct(project, &["build"]);
    assert!(!output.status.success(), "the build must fail with make");
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("make modules for bad failed"), "{text}");
    assert!(text.contains("compilation failed"), "{text}");

    assert!(
        !project.join("drivers/bad/Kbuild").exists(),
        "generated Kbuild left behind after a failed build"
    );
    assert!(
        !project.join("drivers/bad/main.o").exists(),
        "kbuild intermediate left behind after a failed build"
    );
    assert!(
        project.join("drivers/bad/main.c").exists(),
        "the source itself must survive the cleanup"
    );
}

/// `required_tools()` names the `make` each manifest runs, not a hardcoded
/// `make`: a manifest pointing at `bin/kmake` declares that tool, so the
/// build's tool check and the declared-tools guard see what actually runs.
#[test]
fn linux_module_manifest_make_is_the_declared_tool() {
    let temp = tempfile::TempDir::new().unwrap();
    let project = temp.path();

    let kmake = install_fake_make_named(project, "kmake");
    fs::create_dir_all(project.join("kdir")).unwrap();
    let kdir = project.join("kdir").to_string_lossy().into_owned();

    write_file(project, "drivers/hello/main.c", "int hello;\n");
    write_file(
        project,
        "drivers/hello/linux-module.yaml",
        &format!("make: {kmake}\nkdir: {kdir}\nmodules:\n  - name: hello\n    sources: [main.c]\n"),
    );
    write_file(
        project,
        "rsconstruct.toml",
        "[processor.mass_generator.linux_module]\nsrc_dirs = [\"drivers\"]\n",
    );

    let output = run_rsconstruct(project, &["build"]);
    assert!(
        output.status.success(),
        "build failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        fs::read(project.join("out/processor.mass_generator.linux_module/drivers/hello/hello.ko"))
            .unwrap(),
        b"FAKE-KO-BYTES"
    );
}

/// `dep_auto` is an input like `dep_inputs`: a file it names that exists is
/// an input of every module, so editing it rebuilds the modules.
#[test]
fn linux_module_dep_auto_file_is_an_input_of_every_module() {
    let temp = tempfile::TempDir::new().unwrap();
    let project = temp.path();

    let make = install_fake_make(project);
    fs::create_dir_all(project.join("kdir")).unwrap();
    let kdir = project.join("kdir").to_string_lossy().into_owned();

    write_file(project, "drivers/hello/main.c", "int hello;\n");
    write_file(
        project,
        "drivers/hello/linux-module.yaml",
        &format!("make: {make}\nkdir: {kdir}\nmodules:\n  - name: hello\n    sources: [main.c]\n"),
    );
    write_file(project, "kernel.cfg", "CONFIG_A=y\n");
    write_file(
        project,
        "rsconstruct.toml",
        "[processor.mass_generator.linux_module]\nsrc_dirs = [\"drivers\"]\ndep_auto = [\"kernel.cfg\"]\n",
    );

    let first = run_rsconstruct(project, &["build", "-v"]);
    assert!(
        first.status.success(),
        "first build failed: {}\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr),
    );

    let unchanged = run_rsconstruct(project, &["build", "-v"]);
    assert!(unchanged.status.success());
    let stdout = String::from_utf8_lossy(&unchanged.stdout);
    assert!(
        stdout.contains("Skipping (unchanged):"),
        "nothing changed, so nothing rebuilds: {stdout}"
    );

    write_file(project, "kernel.cfg", "CONFIG_A=n\n");
    let edited = run_rsconstruct(project, &["build", "-v"]);
    assert!(edited.status.success());
    let stdout = String::from_utf8_lossy(&edited.stdout);
    assert!(
        stdout.contains("Processing:"),
        "an edit to the dep_auto file must rebuild the module: {stdout}"
    );
}
