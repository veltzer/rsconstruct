test_checker!(cmake, tool: "cmakelint", processor: "processor.checker.cmake",
    files: [("CMakeLists.txt", "cmake_minimum_required(VERSION 3.10)\nproject(demo)\n")]);

/// cmakelint reads `$PWD/.cmakelintrc` by default, so it is an input when
/// it exists.
#[test]
fn cmake_default_dep_auto_tracks_cmakelintrc() {
    let defconfig = crate::common::processor_defconfig("processor.checker.cmake");
    assert_eq!(defconfig["dep_auto"], serde_json::json!([".cmakelintrc"]));
}
