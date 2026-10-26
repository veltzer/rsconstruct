test_checker!(hadolint, tool: "hadolint", processor: "processor.checker.hadolint",
    files: [("Dockerfile", "FROM ubuntu:22.04\nCMD [\"echo\", \"hello\"]\n")]);

/// hadolint reads `$PWD/.hadolint.yaml`, then `$PWD/.hadolint.yml`: both
/// are inputs when they exist, the same list as idockerfile.
#[test]
fn hadolint_default_dep_auto_tracks_hadolint_config() {
    let defconfig = crate::common::processor_defconfig("processor.checker.hadolint");
    assert_eq!(
        defconfig["dep_auto"],
        serde_json::json!([".hadolint.yaml", ".hadolint.yml"])
    );
    let idockerfile = crate::common::processor_defconfig("processor.checker.idockerfile");
    assert_eq!(defconfig["dep_auto"], idockerfile["dep_auto"]);
}
