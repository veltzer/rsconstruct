//! checkpatch.pl, in Rust: the Linux kernel's style checker as `checkpatch.pl
//! --no-tree -f FILE` runs it, on a Perl-compatible regular expression
//! engine (`perlre`) with Perl's interpolation of the patterns (`re`), the
//! pattern variables (`globals`), the diff it reads (`diff`), its helper subs
//! (`ctx`), its checks (`lines`) and its report (`check`).

mod check;
mod ctx;
mod diff;
pub mod globals;
mod lines;
pub mod perlre;
pub mod re;

pub use check::{Checker, Outcome};
pub use diff::timestamp;

#[cfg(test)]
mod tests;
