//! `${{ }}` expressions: lexer, parser, types and semantic checks, a port
//! of actionlint's `expr_*.go`.

pub mod insecure;
pub mod lexer;
pub mod parser;
pub mod sema;
pub mod types;

/// An error in an expression, positioned within the expression text:
/// 1-based line and column relative to the start of the expression, and the
/// byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprError {
    pub message: String,
    pub offset: usize,
    pub line: u32,
    pub column: u32,
}
