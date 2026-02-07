mod cargo;
mod clang_tidy;
mod cppcheck;
mod make;
mod pylint;
mod ruff;
mod shellcheck;
mod sleep;
mod spellcheck;

pub use cargo::CargoProcessor;
pub use clang_tidy::ClangTidyProcessor;
pub use cppcheck::CppcheckProcessor;
pub use make::MakeProcessor;
pub use pylint::PylintProcessor;
pub use ruff::RuffProcessor;
pub use shellcheck::ShellcheckProcessor;
pub use sleep::SleepProcessor;
pub use spellcheck::SpellcheckProcessor;
