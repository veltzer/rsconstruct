//! In-process implementations of external tools, each a port of a specific
//! release, named after the tool it reproduces. A processor in
//! `src/processor/` is the front end; the engine here does the work.

pub mod actionlint;
pub mod cpplint;
pub mod hadolint;
pub mod luacheck;
pub mod svglint;
pub mod tidy;
pub mod xmllint;
pub mod yamllint;
