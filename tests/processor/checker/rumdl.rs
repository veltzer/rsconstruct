test_checker!(rumdl, tool: "rumdl", processor: "processor.checker.rumdl",
    files: [("test.md", "# Test\n\nHello world.\n")]);

/// Every file rumdl's config discovery reads from the working directory is
/// an input when it exists: its own files and `pyproject.toml`, and the
/// markdownlint / markdownlint-cli2 configuration it falls back to.
#[test]
fn rumdl_default_dep_auto_tracks_every_rumdl_config_file() {
    let defconfig = crate::common::processor_defconfig("processor.checker.rumdl");
    let dep_auto = defconfig["dep_auto"]
        .as_array()
        .expect("dep_auto is a list")
        .iter()
        .map(|v| v.as_str().expect("dep_auto entries are strings"))
        .collect::<Vec<_>>();
    for expected in [
        ".rumdl.toml",
        "rumdl.toml",
        ".config/rumdl.toml",
        "pyproject.toml",
        ".markdownlint.json",
        ".markdownlint.yaml",
        ".markdownlint-cli2.yaml",
    ] {
        assert!(
            dep_auto.contains(&expected),
            "{expected} missing from rumdl dep_auto: {dep_auto:?}"
        );
    }
}
