#[macro_use]
mod common;

#[path = "tests_mod/analyzers.rs"]
mod analyzers;
#[path = "tests_mod/build.rs"]
mod build;
#[path = "tests_mod/cache.rs"]
mod cache;
#[path = "tests_mod/complete.rs"]
mod complete;
#[path = "tests_mod/doctor.rs"]
mod doctor;
#[path = "tests_mod/dry_run.rs"]
mod dry_run;
#[path = "tests_mod/exit_codes.rs"]
mod exit_codes;
#[path = "tests_mod/explain.rs"]
mod explain;
#[path = "tests_mod/graph.rs"]
mod graph;
#[path = "tests_mod/incremental.rs"]
mod incremental;
#[path = "tests_mod/init.rs"]
mod init;
#[path = "tests_mod/iset_pset.rs"]
mod iset_pset;
#[path = "tests_mod/local_overlay.rs"]
mod local_overlay;
#[path = "tests_mod/pages.rs"]
mod pages;
#[path = "tests_mod/processor_cmd.rs"]
mod processor_cmd;
#[path = "tests_mod/product.rs"]
mod product;
#[path = "tests_mod/rsconstructignore.rs"]
mod rsconstructignore;
#[path = "tests_mod/scheduler.rs"]
mod scheduler;
#[path = "tests_mod/status.rs"]
mod status;
#[path = "tests_mod/toml_files.rs"]
mod toml_files;
#[path = "tests_mod/tools.rs"]
mod tools;
#[path = "tests_mod/watch.rs"]
mod watch;

// Per-processor tests mirror `src/processor/`: one directory per processor
// type, one file per processor, so `generic` can exist once per type.
mod processor {
    pub mod checker {
        pub mod actionlint;
        pub mod ascii;
        pub mod aspell;
        pub mod biome;
        pub mod black;
        pub mod checkstyle;
        pub mod clang_tidy;
        pub mod clippy;
        pub mod cmake;
        pub mod cppcheck;
        pub mod doctest;
        pub mod duplicate_files;
        pub mod eslint;
        pub mod hadolint;
        pub mod htmlhint;
        pub mod htmllint;
        pub mod ixmllint;
        pub mod iyamllint;
        pub mod iyamlschema;
        pub mod jq;
        pub mod jshint;
        pub mod jslint;
        pub mod json_schema;
        pub mod jsonlint;
        pub mod license_header;
        pub mod luacheck;
        pub mod make;
        pub mod markdownlint;
        pub mod mdl;
        pub mod mypy;
        pub mod oxlint;
        pub mod perlcritic;
        pub mod php_lint;
        pub mod pylint;
        pub mod pyrefly;
        pub mod pytest;
        pub mod ruff;
        pub mod rumdl;
        pub mod script;
        pub mod shellcheck;
        pub mod slidev;
        pub mod standard;
        pub mod stylelint;
        pub mod svglint;
        pub mod svgo;
        pub mod taplo;
        pub mod terms;
        pub mod tidy;
        pub mod xmllint;
        pub mod yamllint;
        pub mod yq;
        pub mod zspell;
    }
    pub mod creator {
        pub mod cargo;
        pub mod cc;
        pub mod gem;
        pub mod generic;
        pub mod jekyll;
        pub mod linux_module;
        pub mod mdbook;
        pub mod npm;
        pub mod pip;
        pub mod sphinx;
    }
    pub mod generator {
        pub mod a2x;
        pub mod cc_single_file;
        pub mod drawio;
        pub mod generic;
        pub mod isass;
        pub mod jinja2;
        pub mod libreoffice;
        pub mod mako;
        pub mod marp;
        pub mod mermaid;
        pub mod pandoc;
        pub mod pdflatex;
        pub mod pdfunite;
        pub mod protobuf;
        pub mod requirements;
        pub mod rust_single_file;
        pub mod sass;
        pub mod tags;
        pub mod tera;
    }
    pub mod mass_generator {
        pub mod generic;
        pub mod zola;
    }
    pub mod markdown;
    pub mod shared_output_dir;
}

/// Every test file on disk must be registered above — a file missing from
/// this hand-maintained module list silently never runs, with no warning
/// from anything. This asserts the list matches the directories: the flat
/// `tests_mod/`, and `processor/` with one block per type directory.
#[test]
fn every_test_file_is_registered() {
    let this = include_str!("main.rs");
    let mut missing: Vec<String> = Vec::new();

    fn rs_stems(dir: &str) -> Vec<String> {
        let mut stems = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                stems.push(stem.to_string());
            }
        }
        stems
    }

    for stem in rs_stems("tests/tests_mod") {
        if !this.contains(&format!("tests_mod/{stem}.rs"))
            && !this.contains(&format!("mod {stem};"))
        {
            missing.push(format!("tests/tests_mod/{stem}.rs"));
        }
    }
    for stem in rs_stems("tests/processor") {
        if !this.contains(&format!("    pub mod {stem};")) {
            missing.push(format!("tests/processor/{stem}.rs"));
        }
    }
    // A per-type file must be declared inside its own type block: the same
    // stem (`generic`) exists under several types, so a match anywhere in
    // the file would not prove this one is registered.
    for entry in std::fs::read_dir("tests/processor").unwrap() {
        let path = entry.unwrap().path();
        if !path.is_dir() {
            continue;
        }
        let ty = path.file_name().unwrap().to_str().unwrap().to_string();
        let block_start = format!("    pub mod {ty} {{\n");
        let block = this
            .find(&block_start)
            .map(|start| {
                let rest = &this[start + block_start.len()..];
                let end = rest.find("\n    }").unwrap_or(rest.len());
                &rest[..end]
            })
            .unwrap_or("");
        for stem in rs_stems(&format!("tests/processor/{ty}")) {
            if !block.contains(&format!("pub mod {stem};")) {
                missing.push(format!("tests/processor/{ty}/{stem}.rs"));
            }
        }
    }
    missing.sort();
    assert!(
        missing.is_empty(),
        "test files exist on disk but are not registered in tests/main.rs \
         (they silently never run): {missing:#?}"
    );
}
