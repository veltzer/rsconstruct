test_checker!(htmllint, tool: "htmllint", processor: "processor.checker.htmllint",
    files: [(".htmllintrc", "{}\n"), ("test.html", "<!DOCTYPE html>\n<html><head><title>Test</title></head><body></body></html>\n")]);

/// htmllint reads `.htmllintrc` from the working directory, so it is an
/// input when it exists.
#[test]
fn htmllint_default_dep_auto_tracks_htmllintrc() {
    let defconfig = crate::common::processor_defconfig("processor.checker.htmllint");
    assert_eq!(defconfig["dep_auto"], serde_json::json!([".htmllintrc"]));
}
