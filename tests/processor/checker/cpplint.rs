test_checker!(cpplint, tool: "cpplint", processor: "processor.checker.cpplint",
    files: [("test.cc", "// Copyright 2024 Example Project\n#include <cstdio>\n\nint main() {\n  std::printf(\"hello\\n\");\n  return 0;\n}\n")]);

/// cpplint reads `CPPLINT.cfg` from the file's directory and every parent,
/// so the project root's copy is an input of every product when it exists.
#[test]
fn cpplint_default_dep_auto_tracks_cpplint_cfg() {
    let defconfig = crate::common::processor_defconfig("processor.checker.cpplint");
    assert_eq!(defconfig["dep_auto"], serde_json::json!(["CPPLINT.cfg"]));
}
