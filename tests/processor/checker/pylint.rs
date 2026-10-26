test_checker!(pylint, tool: "pylint", processor: "processor.checker.pylint",
    files: [("test.py", "\"\"\"Test module.\"\"\"\nX = 1\n")]);

/// Every file pylint's `find_default_config_files` looks up in the working
/// directory is an input when it exists: the rc names in pylint's order,
/// then `pyproject.toml`, `setup.cfg` and `tox.ini`.
#[test]
fn pylint_default_dep_auto_tracks_every_pylint_config_file() {
    let defconfig = crate::common::processor_defconfig("processor.checker.pylint");
    assert_eq!(
        defconfig["dep_auto"],
        serde_json::json!([
            "pylintrc",
            "pylintrc.toml",
            ".pylintrc",
            ".pylintrc.toml",
            "pyproject.toml",
            "setup.cfg",
            "tox.ini"
        ])
    );
}
