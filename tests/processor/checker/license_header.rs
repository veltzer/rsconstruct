use crate::common::run_rsconstruct_with_env;
use std::fs;
use tempfile::TempDir;

/// The demos-os-linux header shape: a comment block every source starts with.
const CONFIG: &str = r#"[processor.checker.license_header]
src_dirs = ["src"]
src_extensions = [".c"]
header_lines = [
    "/*",
    " * This file is part of the demo package.",
    " */",
]
optional_prefix_lines = ["// SPDX-License-Identifier: GPL-2.0"]
"#;

const HEADER: &str = "/*\n * This file is part of the demo package.\n */\n";

fn project(files: &[(&str, &str)]) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), CONFIG).unwrap();
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    for (name, content) in files {
        fs::write(temp_dir.path().join("src").join(name), content).unwrap();
    }
    temp_dir
}

fn build(temp_dir: &TempDir) -> (bool, String) {
    let output = run_rsconstruct_with_env(temp_dir.path(), &["build", "-k"], &[("NO_COLOR", "1")]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), combined)
}

#[test]
fn license_header_accepts_header_at_top_and_after_spdx() {
    let plain = format!("{HEADER}int x;\n");
    // Kernel sources put the SPDX line first; the header follows it.
    let kernel = format!("// SPDX-License-Identifier: GPL-2.0\n{HEADER}int y;\n");
    let temp_dir = project(&[("plain.c", &plain), ("kernel.c", &kernel)]);
    let (ok, out) = build(&temp_dir);
    assert!(ok, "both files carry the header: {out}");
}

#[test]
fn license_header_rejects_missing_or_partial_header() {
    // Every header line is required: the old check passed a file if any
    // one line appeared anywhere.
    let partial = "/*\n * This file is part of another package.\n */\nint x;\n";
    // The header must be at the top, not further down.
    let late = format!("int x;\n{HEADER}");
    let temp_dir = project(&[("partial.c", partial), ("late.c", &late)]);
    let (ok, out) = build(&temp_dir);
    assert!(!ok, "build should fail: {out}");
    assert!(
        out.contains("src/partial.c:2: license header line 2 differs"),
        "partial header not reported: {out}"
    );
    assert!(
        out.contains("src/late.c:1: license header line 1 differs"),
        "late header not reported: {out}"
    );
}

/// The kernel use: the header is the SPDX line itself, so every source must
/// open with it.
#[test]
fn license_header_checks_spdx_line_as_the_header() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(
        temp_dir.path().join("rsconstruct.toml"),
        "[processor.checker.license_header]\nsrc_dirs = [\"src\"]\nsrc_extensions = [\".c\"]\n\
         header_lines = [\"// SPDX-License-Identifier: GPL-2.0\"]\n",
    )
    .unwrap();
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::write(
        temp_dir.path().join("src/good.c"),
        "// SPDX-License-Identifier: GPL-2.0\nint x;\n",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("src/other.c"),
        "// SPDX-License-Identifier: MIT\nint y;\n",
    )
    .unwrap();
    fs::write(temp_dir.path().join("src/none.c"), "int z;\n").unwrap();
    let (ok, out) = build(&temp_dir);
    assert!(!ok, "build should fail: {out}");
    assert!(!out.contains("src/good.c"), "good.c has the line: {out}");
    assert!(
        out.contains("src/other.c:1: license header line 1 differs"),
        "wrong SPDX license not reported: {out}"
    );
    assert!(
        out.contains("src/none.c:1: license header line 1 differs"),
        "missing SPDX line not reported: {out}"
    );
}

/// A kernel source with a broken license block after its SPDX line is
/// reported at the block, not at the SPDX line.
#[test]
fn license_header_reports_block_mismatch_after_spdx() {
    let broken = "// SPDX-License-Identifier: GPL-2.0\n/*\n * Some other package.\n */\n";
    let temp_dir = project(&[("broken.c", broken)]);
    let (ok, out) = build(&temp_dir);
    assert!(!ok, "build should fail: {out}");
    assert!(
        out.contains("src/broken.c:3: license header line 2 differs"),
        "mismatch should be reported inside the block: {out}"
    );
}

/// The prefix must match exactly: another SPDX line is not the declared
/// prefix, and a file ending right after the header without a newline does
/// not have the header's last line.
#[test]
fn license_header_matches_prefix_and_newlines_exactly() {
    let mit = format!("// SPDX-License-Identifier: MIT\n{HEADER}int x;\n");
    let unterminated = HEADER.trim_end_matches('\n');
    let temp_dir = project(&[("mit.c", &mit), ("unterminated.c", unterminated)]);
    let (ok, out) = build(&temp_dir);
    assert!(!ok, "build should fail: {out}");
    assert!(
        out.contains("src/mit.c:1: license header line 1 differs"),
        "unknown SPDX line not reported: {out}"
    );
    assert!(
        out.contains(
            "src/unterminated.c:3: file ends without a newline after license header line 3"
        ),
        "missing final newline not reported: {out}"
    );
}

/// `skip_shebang` (default on) lets scripts keep their `#!` line first.
#[test]
fn license_header_skip_shebang_is_configurable() {
    let script = format!("#!/bin/sh\n{HEADER}int x;\n");
    let temp_dir = project(&[("script.c", &script)]);
    let (ok, out) = build(&temp_dir);
    assert!(ok, "shebang is skipped by default: {out}");

    let config = format!("{CONFIG}skip_shebang = false\n");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config).unwrap();
    let (ok, out) = build(&temp_dir);
    assert!(
        !ok,
        "with skip_shebang = false the #! line is not skipped: {out}"
    );
    assert!(
        out.contains("src/script.c:1: license header line 1 differs"),
        "unexpected error: {out}"
    );
}

#[test]
fn license_header_requires_header_lines() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::write(
        temp_dir.path().join("rsconstruct.toml"),
        "[processor.checker.license_header]\nsrc_dirs = [\"src\"]\nheader_lines = []\n",
    )
    .unwrap();
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::write(temp_dir.path().join("src/a.c"), "int x;\n").unwrap();
    let (ok, out) = build(&temp_dir);
    assert!(!ok, "an empty header_lines must not pass every file: {out}");
    assert!(
        out.contains("required field 'header_lines' must not be empty"),
        "unexpected error: {out}"
    );
}
