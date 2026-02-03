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
#[path = "tests_mod/graph.rs"]
mod graph;
#[path = "tests_mod/init.rs"]
mod init;
#[path = "tests_mod/processor_cmd.rs"]
mod processor_cmd;
#[path = "tests_mod/rsbignore.rs"]
mod rsbignore;
#[path = "tests_mod/status.rs"]
mod status;
#[path = "tests_mod/tools.rs"]
mod tools;
#[path = "tests_mod/watch.rs"]
mod watch;

mod processors {
    pub mod cc_single_file;
    pub mod sleep;
    pub mod spellcheck;
    pub mod template;
}
