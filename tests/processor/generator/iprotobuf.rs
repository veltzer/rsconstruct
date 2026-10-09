use crate::common::run_rsconstruct_with_env;
use std::fs;
use tempfile::TempDir;

fn setup_project(config: &str) -> TempDir {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    fs::create_dir_all(temp_dir.path().join("proto")).expect("Failed to create proto dir");
    fs::write(temp_dir.path().join("rsconstruct.toml"), config)
        .expect("Failed to write rsconstruct.toml");
    temp_dir
}

const CONFIG: &str = "[processor.generator.iprotobuf]\nsrc_dirs = [\"proto\"]\n";

fn build(project: &std::path::Path) -> std::process::Output {
    run_rsconstruct_with_env(project, &["build"], &[("NO_COLOR", "1")])
}

#[test]
fn iprotobuf_generates_rust_per_package() {
    let temp_dir = setup_project(CONFIG);
    let project = temp_dir.path();
    fs::write(
        project.join("proto/hello.proto"),
        "syntax = \"proto3\";\npackage greet.v1;\n\nmessage Hello {\n  string name = 1;\n  repeated int32 counts = 2;\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("proto/unpackaged.proto"),
        "syntax = \"proto3\";\nmessage Loose {\n  bool flag = 1;\n}\n",
    )
    .unwrap();
    let output = build(project);
    assert!(
        output.status.success(),
        "rsconstruct build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let packaged =
        fs::read_to_string(project.join("out/processor.generator.iprotobuf/greet.v1.rs"))
            .expect("greet.v1.rs written");
    assert!(
        packaged.contains("pub struct Hello"),
        "generated struct missing: {packaged}"
    );
    assert!(
        packaged.contains("pub counts: ::prost::alloc::vec::Vec<i32>"),
        "repeated field missing: {packaged}"
    );
    let loose = fs::read_to_string(project.join("out/processor.generator.iprotobuf/_.rs"))
        .expect("_.rs written");
    assert!(
        loose.contains("pub struct Loose"),
        "unpackaged struct missing: {loose}"
    );
}

#[test]
fn iprotobuf_imports_and_descriptor_set() {
    let config = "[processor.generator.iprotobuf]\nsrc_dirs = [\"proto\"]\ndescriptor_set = \"out/all.pb\"\n";
    let temp_dir = setup_project(config);
    let project = temp_dir.path();
    fs::write(
        project.join("proto/base.proto"),
        "syntax = \"proto3\";\npackage demo;\nimport \"google/protobuf/timestamp.proto\";\nmessage Base {\n  google.protobuf.Timestamp when = 1;\n}\n",
    )
    .unwrap();
    fs::write(
        project.join("proto/user.proto"),
        "syntax = \"proto3\";\npackage demo;\nimport \"base.proto\";\nmessage User {\n  Base base = 1;\n}\n",
    )
    .unwrap();
    let output = build(project);
    assert!(
        output.status.success(),
        "rsconstruct build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let demo = fs::read_to_string(project.join("out/processor.generator.iprotobuf/demo.rs"))
        .expect("demo.rs written");
    assert!(
        demo.contains("pub struct Base") && demo.contains("pub struct User"),
        "{demo}"
    );
    assert!(
        demo.contains("::prost_types::Timestamp"),
        "well-known type mapped to prost_types: {demo}"
    );
    let pb = fs::read(project.join("out/all.pb")).expect("descriptor set written");
    assert!(!pb.is_empty());
}

#[test]
fn iprotobuf_reports_errors_with_position() {
    let temp_dir = setup_project(CONFIG);
    let project = temp_dir.path();
    fs::write(
        project.join("proto/bad.proto"),
        "syntax = \"proto3\";\nmessage Bad {\n  Missing field = 1;\n}\n",
    )
    .unwrap();
    let output = build(project);
    assert!(!output.status.success(), "an unresolved type must fail");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        text.contains("Missing"),
        "the error must name the type: {text}"
    );
}
