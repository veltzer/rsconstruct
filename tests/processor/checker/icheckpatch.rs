use crate::common::run_rsconstruct_with_env;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const CONFIG: &str = "[processor.checker.icheckpatch]\nsrc_dirs = [\"src\"]\n";

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

/// Not batched, as the docs say: files are checked in parallel as separate
/// products. A batch would run them one after another on a single worker.
#[test]
fn icheckpatch_does_not_batch() {
    let info = crate::common::processor_info("processor.checker.icheckpatch");
    assert_eq!(info["supports_batch"], false);
}

fn combined_output(output: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

const CLEAN: &str = "// SPDX-License-Identifier: GPL-2.0\n#include <linux/module.h>\n\nstatic int count;\n\nint get_count(void)\n{\n\treturn count;\n}\n";

const BAD: &str = "// SPDX-License-Identifier: GPL-2.0\nint f(void) {\n    return 0 ;\n}\n";

#[test]
fn icheckpatch_clean_file_passes() {
    let temp_dir = setup_project(CONFIG, &[("src/ok.c", CLEAN)]);
    let output = build(temp_dir.path());
    assert!(
        output.status.success(),
        "a clean file must pass: {}",
        combined_output(&output)
    );
}

#[test]
fn icheckpatch_empty_file_passes() {
    let temp_dir = setup_project(CONFIG, &[("src/empty.c", "")]);
    let output = build(temp_dir.path());
    assert!(
        output.status.success(),
        "an empty file must pass: {}",
        combined_output(&output)
    );
}

/// A failing file reports what `checkpatch.pl --no-tree -f` prints for it.
#[test]
fn icheckpatch_reports_what_checkpatch_prints() {
    crate::common::require_tool("checkpatch.pl");
    let temp_dir = setup_project(CONFIG, &[("src/bad.c", BAD)]);
    let output = build(temp_dir.path());
    assert!(!output.status.success(), "a file with errors must fail");
    let text = combined_output(&output);
    let expected = std::process::Command::new("checkpatch.pl")
        .args(["--no-tree", "-f", "src/bad.c"])
        .current_dir(temp_dir.path())
        .output()
        .expect("run checkpatch.pl");
    assert!(
        !expected.status.success(),
        "checkpatch.pl fails the file too"
    );
    let stdout = String::from_utf8_lossy(&expected.stdout);
    for line in stdout.lines().filter(|l| !l.is_empty()) {
        assert!(
            text.contains(line),
            "checkpatch prints {line:?}, icheckpatch does not: {text}"
        );
    }
}
