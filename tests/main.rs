#[macro_use]
mod common;

#[path = "tests_mod/build.rs"]
mod build;
#[path = "tests_mod/cache.rs"]
mod cache;
#[path = "tests_mod/complete.rs"]
mod complete;
#[path = "tests_mod/config.rs"]
mod config;
#[path = "tests_mod/dry_run.rs"]
mod dry_run;
#[path = "tests_mod/explain.rs"]
mod explain;
#[path = "tests_mod/exit_codes.rs"]
mod exit_codes;
#[path = "tests_mod/graph.rs"]
mod graph;
#[path = "tests_mod/init.rs"]
mod init;
#[path = "tests_mod/processor_cmd.rs"]
mod processor_cmd;
#[path = "tests_mod/rsconstructignore.rs"]
mod rsconstructignore;
#[path = "tests_mod/status.rs"]
mod status;
#[path = "tests_mod/tools.rs"]
mod tools;
#[path = "tests_mod/watch.rs"]
mod watch;

mod processors {
    pub mod a2x;
    pub mod ascii;
    pub mod aspell;
    pub mod black;
    pub mod cargo;
    pub mod cc;
    pub mod clippy;
    pub mod cc_single_file;
    pub mod clang_tidy;
    pub mod cppcheck;
    pub mod doctest;
    pub mod drawio;
    pub mod gem;
    pub mod generator;
    pub mod jinja2;
    pub mod jq;
    pub mod json_schema;
    pub mod jsonlint;
    pub mod libreoffice;
    pub mod luacheck;
    pub mod mako;
    pub mod make;
    pub mod markdown;
    pub mod markdownlint;
    pub mod marp;
    pub mod mdbook;
    pub mod mdl;
    pub mod mermaid;
    pub mod mypy;
    pub mod npm;
    pub mod pandoc;
    pub mod pdflatex;
    pub mod pdfunite;
    pub mod pip;
    pub mod protobuf;
    pub mod pylint;
    pub mod pytest;
    pub mod pyrefly;
    pub mod ruff;
    pub mod rumdl;
    pub mod rust_single_file;
    pub mod sass;
    pub mod script;
    pub mod shellcheck;
    pub mod zspell;
    pub mod sphinx;
    pub mod taplo;
    pub mod tera;
    pub mod yamllint;
    pub mod tags;
    pub mod eslint;
    pub mod jshint;
    pub mod htmlhint;
    pub mod tidy;
    pub mod stylelint;
    pub mod jslint;
    pub mod standard;
    pub mod htmllint;
    pub mod iyamlschema;
    pub mod php_lint;
    pub mod perlcritic;
    pub mod xmllint;
    pub mod svglint;
    pub mod checkstyle;
    pub mod yq;
    pub mod cmake;
    pub mod creator;
    pub mod hadolint;
    pub mod jekyll;
    pub mod slidev;
    pub mod shared_output_dir;
}
