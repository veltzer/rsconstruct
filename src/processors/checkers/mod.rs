mod cpplint;
mod pylint;
mod ruff;
mod shellcheck;
mod sleep;
mod spellcheck;

pub use cpplint::CpplintProcessor;
pub use pylint::PylintProcessor;
pub use ruff::RuffProcessor;
pub use shellcheck::ShellcheckProcessor;
pub use sleep::SleepProcessor;
pub use spellcheck::SpellcheckProcessor;
