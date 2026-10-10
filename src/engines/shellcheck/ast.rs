//! `ShellCheck`'s AST (`ShellCheck.AST`).
//!
//! The variant names are `ShellCheck`'s own (`T_SimpleCommand`, `TA_Binary`,
//! ...), so every check here can be read side by side with its Haskell
//! original. A token is an id plus its inner variant; ids map to source
//! spans in the parse result.
//!
//! Traversal visits children in the order Haskell's derived `Traversable`
//! instance does: fields left to right, lists in order, and only fields of
//! the token type (the name of a coprocess is a `Maybe Token` field and is
//! not visited). Equality ignores ids, as `ShellCheck`'s `Eq Token` does.

#![allow(non_camel_case_types)]

use std::collections::HashMap;
use std::fmt;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Id(pub usize);

/// A hasher for the small dense integers the analysis keys its maps by
/// (token ids, graph nodes): a multiplication spreads them over the hash
/// bits. The default `SipHash` guards against crafted keys, which an id
/// cannot be, and cost a twentieth of the analysis.
#[derive(Default)]
pub struct IntHasher(u64);

impl Hasher for IntHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}

/// A map keyed by an `Id` or a graph node.
pub type IntMap<K, V> = HashMap<K, V, BuildHasherDefault<IntHasher>>;

/// A Parsec source position: file, 1-based line and column (tabs to 8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourcePos {
    pub file: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quoted {
    Quoted,
    Unquoted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dashed {
    Dashed,
    Undashed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Piped {
    Piped,
    Unpiped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentMode {
    Assign,
    Append,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(
    clippy::enum_variant_names,
    reason = "ShellCheck's own constructor names (CaseBreak, ...)"
)]
pub enum CaseType {
    CaseBreak,
    CaseFallThrough,
    CaseContinue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConditionType {
    DoubleBracket,
    SingleBracket,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Annotation {
    /// Codes in `[from, to)`.
    DisableComment(i64, i64),
    EnableComment(String),
    SourceOverride(String),
    ShellOverride(String),
    SourcePath(String),
    ExternalSources(bool),
    ExtendedAnalysis(bool),
}

#[derive(Clone)]
pub struct Token {
    pub id: Id,
    pub inner: Inner,
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}:{:?}", self.id.0, self.inner)
    }
}

impl PartialEq for Token {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

pub type B = Box<Token>;
pub type L = Vec<Token>;

#[derive(Clone, Debug, PartialEq)]
pub enum Inner {
    TA_Binary(String, B, B),
    TA_Assignment(String, B, B),
    TA_Variable(String, L),
    TA_Expansion(L),
    TA_Sequence(L),
    TA_Parenthesis(B),
    TA_Trinary(B, B, B),
    TA_Unary(String, B),
    TC_And(ConditionType, String, B, B),
    TC_Binary(ConditionType, String, B, B),
    TC_Group(ConditionType, B),
    TC_Nullary(ConditionType, B),
    TC_Or(ConditionType, String, B, B),
    TC_Unary(ConditionType, String, B),
    TC_Empty(ConditionType),
    T_AND_IF,
    T_AndIf(B, B),
    T_Arithmetic(B),
    T_Array(L),
    T_IndexedElement(L, B),
    T_UnparsedIndex(SourcePos, String),
    T_Assignment(AssignmentMode, String, L, B),
    T_Backgrounded(B),
    T_Backticked(L),
    T_Bang,
    T_Banged(B),
    T_BraceExpansion(L),
    T_BraceGroup(L),
    T_CLOBBER,
    T_Case,
    T_CaseExpression(B, Vec<(CaseType, L, L)>),
    T_Condition(ConditionType, B),
    T_DGREAT,
    T_DSEMI,
    T_Do,
    T_DollarArithmetic(B),
    T_DollarBraced(bool, B),
    T_DollarBracket(B),
    T_DollarDoubleQuoted(L),
    T_DollarExpansion(L),
    T_DollarSingleQuoted(String),
    T_DollarBraceCommandExpansion(Piped, L),
    T_Done,
    T_DoubleQuoted(L),
    T_EOF,
    T_Elif,
    T_Else,
    T_Esac,
    T_Extglob(String, L),
    T_FdRedirect(String, B),
    T_Fi,
    T_For,
    T_ForArithmetic(B, B, B, L),
    T_ForIn(String, L, L),
    /// keyword, parentheses, name, body
    T_Function(bool, bool, String, B),
    T_GREATAND,
    T_Glob(String),
    T_Greater,
    T_HereDoc(Dashed, Quoted, String, L),
    T_HereString(B),
    T_If,
    T_IfExpression(Vec<(L, L)>, L),
    T_In,
    T_IoFile(B, B),
    T_IoDuplicate(B, String),
    T_LESSAND,
    T_LESSGREAT,
    T_Lbrace,
    T_Less,
    T_Literal(String),
    T_Lparen,
    T_NormalWord(L),
    T_OR_IF,
    T_OrIf(B, B),
    T_ParamSubSpecialChar(String),
    /// pipe separators, commands
    T_Pipeline(L, L),
    T_ProcSub(String, L),
    T_Rbrace,
    T_Redirecting(L, B),
    T_Rparen,
    T_Script(B, L),
    T_Select,
    T_SelectIn(String, L, L),
    T_Semi,
    T_SimpleCommand(L, L),
    T_SingleQuoted(String),
    T_Subshell(L),
    T_Then,
    T_Until,
    T_UntilExpression(L, L),
    T_While,
    T_WhileExpression(L, L),
    T_Annotation(Vec<Annotation>, B),
    T_Pipe(String),
    /// The name is not a traversed child.
    T_CoProc(Option<B>, B),
    T_CoProcBody(B),
    T_Include(B),
    T_SourceCommand(B, B),
    T_BatsTest(String, B),
}

use Inner::{
    T_AND_IF, T_AndIf, T_Annotation, T_Arithmetic, T_Array, T_Assignment, T_Backgrounded,
    T_Backticked, T_Bang, T_Banged, T_BatsTest, T_BraceExpansion, T_BraceGroup, T_CLOBBER, T_Case,
    T_CaseExpression, T_CoProc, T_CoProcBody, T_Condition, T_DGREAT, T_DSEMI, T_Do,
    T_DollarArithmetic, T_DollarBraceCommandExpansion, T_DollarBraced, T_DollarBracket,
    T_DollarDoubleQuoted, T_DollarExpansion, T_DollarSingleQuoted, T_Done, T_DoubleQuoted, T_EOF,
    T_Elif, T_Else, T_Esac, T_Extglob, T_FdRedirect, T_Fi, T_For, T_ForArithmetic, T_ForIn,
    T_Function, T_GREATAND, T_Glob, T_Greater, T_HereDoc, T_HereString, T_If, T_IfExpression, T_In,
    T_Include, T_IndexedElement, T_IoDuplicate, T_IoFile, T_LESSAND, T_LESSGREAT, T_Lbrace, T_Less,
    T_Literal, T_Lparen, T_NormalWord, T_OR_IF, T_OrIf, T_ParamSubSpecialChar, T_Pipe, T_Pipeline,
    T_ProcSub, T_Rbrace, T_Redirecting, T_Rparen, T_Script, T_Select, T_SelectIn, T_Semi,
    T_SimpleCommand, T_SingleQuoted, T_SourceCommand, T_Subshell, T_Then, T_UnparsedIndex, T_Until,
    T_UntilExpression, T_While, T_WhileExpression, TA_Assignment, TA_Binary, TA_Expansion,
    TA_Parenthesis, TA_Sequence, TA_Trinary, TA_Unary, TA_Variable, TC_And, TC_Binary, TC_Empty,
    TC_Group, TC_Nullary, TC_Or, TC_Unary,
};

impl Token {
    pub const fn new(id: Id, inner: Inner) -> Self {
        Self { id, inner }
    }

    pub fn for_each_child<'a>(&'a self, visit: &mut dyn FnMut(&'a Self)) {
        match &self.inner {
            TA_Binary(_, a, b) | TA_Assignment(_, a, b) => {
                visit(a);
                visit(b);
            }
            TA_Variable(_, l)
            | TA_Expansion(l)
            | TA_Sequence(l)
            | T_Array(l)
            | T_Backticked(l)
            | T_BraceExpansion(l)
            | T_BraceGroup(l)
            | T_DollarDoubleQuoted(l)
            | T_DollarExpansion(l)
            | T_DollarBraceCommandExpansion(_, l)
            | T_DoubleQuoted(l)
            | T_Extglob(_, l)
            | T_HereDoc(_, _, _, l)
            | T_NormalWord(l)
            | T_ProcSub(_, l)
            | T_Subshell(l) => {
                for child in l {
                    visit(child);
                }
            }
            TA_Parenthesis(a)
            | TA_Unary(_, a)
            | TC_Group(_, a)
            | TC_Nullary(_, a)
            | TC_Unary(_, _, a)
            | T_Arithmetic(a)
            | T_Backgrounded(a)
            | T_Banged(a)
            | T_Condition(_, a)
            | T_DollarArithmetic(a)
            | T_DollarBraced(_, a)
            | T_DollarBracket(a)
            | T_FdRedirect(_, a)
            | T_Function(_, _, _, a)
            | T_HereString(a)
            | T_IoDuplicate(a, _)
            | T_Annotation(_, a)
            | T_CoProc(_, a)
            | T_CoProcBody(a)
            | T_Include(a)
            | T_BatsTest(_, a) => visit(a),
            TA_Trinary(a, b, c) => {
                visit(a);
                visit(b);
                visit(c);
            }
            TC_And(_, _, a, b)
            | TC_Binary(_, _, a, b)
            | TC_Or(_, _, a, b)
            | T_AndIf(a, b)
            | T_OrIf(a, b)
            | T_IoFile(a, b)
            | T_SourceCommand(a, b) => {
                visit(a);
                visit(b);
            }
            T_IndexedElement(l, a) | T_Assignment(_, _, l, a) | T_Redirecting(l, a) => {
                for child in l {
                    visit(child);
                }
                visit(a);
            }
            T_CaseExpression(w, cases) => {
                visit(w);
                for (_, a, b) in cases {
                    for child in a.iter().chain(b) {
                        visit(child);
                    }
                }
            }
            T_ForArithmetic(a, b, c, l) => {
                visit(a);
                visit(b);
                visit(c);
                for child in l {
                    visit(child);
                }
            }
            T_ForIn(_, a, b)
            | T_SelectIn(_, a, b)
            | T_Pipeline(a, b)
            | T_SimpleCommand(a, b)
            | T_UntilExpression(a, b)
            | T_WhileExpression(a, b) => {
                for child in a.iter().chain(b) {
                    visit(child);
                }
            }
            T_IfExpression(branches, elses) => {
                for (a, b) in branches {
                    for child in a.iter().chain(b) {
                        visit(child);
                    }
                }
                for child in elses {
                    visit(child);
                }
            }
            T_Script(a, l) => {
                visit(a);
                for child in l {
                    visit(child);
                }
            }
            TC_Empty(_)
            | T_AND_IF
            | T_UnparsedIndex(..)
            | T_Bang
            | T_CLOBBER
            | T_Case
            | T_DGREAT
            | T_DSEMI
            | T_Do
            | T_DollarSingleQuoted(_)
            | T_Done
            | T_EOF
            | T_Elif
            | T_Else
            | T_Esac
            | T_Fi
            | T_For
            | T_GREATAND
            | T_Glob(_)
            | T_Greater
            | T_If
            | T_In
            | T_LESSAND
            | T_LESSGREAT
            | T_Lbrace
            | T_Less
            | T_Literal(_)
            | T_Lparen
            | T_OR_IF
            | T_ParamSubSpecialChar(_)
            | T_Rbrace
            | T_Rparen
            | T_Select
            | T_Semi
            | T_SingleQuoted(_)
            | T_Then
            | T_Until
            | T_While
            | T_Pipe(_) => {}
        }
    }

    /// Mutable children, in the same order.
    pub fn for_each_child_mut(&mut self, visit: &mut dyn FnMut(&mut Self)) {
        match &mut self.inner {
            TA_Binary(_, a, b) | TA_Assignment(_, a, b) => {
                visit(a);
                visit(b);
            }
            TA_Variable(_, l)
            | TA_Expansion(l)
            | TA_Sequence(l)
            | T_Array(l)
            | T_Backticked(l)
            | T_BraceExpansion(l)
            | T_BraceGroup(l)
            | T_DollarDoubleQuoted(l)
            | T_DollarExpansion(l)
            | T_DollarBraceCommandExpansion(_, l)
            | T_DoubleQuoted(l)
            | T_Extglob(_, l)
            | T_HereDoc(_, _, _, l)
            | T_NormalWord(l)
            | T_ProcSub(_, l)
            | T_Subshell(l) => {
                for child in l {
                    visit(child);
                }
            }
            TA_Parenthesis(a)
            | TA_Unary(_, a)
            | TC_Group(_, a)
            | TC_Nullary(_, a)
            | TC_Unary(_, _, a)
            | T_Arithmetic(a)
            | T_Backgrounded(a)
            | T_Banged(a)
            | T_Condition(_, a)
            | T_DollarArithmetic(a)
            | T_DollarBraced(_, a)
            | T_DollarBracket(a)
            | T_FdRedirect(_, a)
            | T_Function(_, _, _, a)
            | T_HereString(a)
            | T_IoDuplicate(a, _)
            | T_Annotation(_, a)
            | T_CoProc(_, a)
            | T_CoProcBody(a)
            | T_Include(a)
            | T_BatsTest(_, a) => visit(a),
            TA_Trinary(a, b, c) => {
                visit(a);
                visit(b);
                visit(c);
            }
            TC_And(_, _, a, b)
            | TC_Binary(_, _, a, b)
            | TC_Or(_, _, a, b)
            | T_AndIf(a, b)
            | T_OrIf(a, b)
            | T_IoFile(a, b)
            | T_SourceCommand(a, b) => {
                visit(a);
                visit(b);
            }
            T_IndexedElement(l, a) | T_Assignment(_, _, l, a) | T_Redirecting(l, a) => {
                for child in l {
                    visit(child);
                }
                visit(a);
            }
            T_CaseExpression(w, cases) => {
                visit(w);
                for (_, a, b) in cases {
                    for child in a.iter_mut().chain(b) {
                        visit(child);
                    }
                }
            }
            T_ForArithmetic(a, b, c, l) => {
                visit(a);
                visit(b);
                visit(c);
                for child in l {
                    visit(child);
                }
            }
            T_ForIn(_, a, b)
            | T_SelectIn(_, a, b)
            | T_Pipeline(a, b)
            | T_SimpleCommand(a, b)
            | T_UntilExpression(a, b)
            | T_WhileExpression(a, b) => {
                for child in a.iter_mut().chain(b) {
                    visit(child);
                }
            }
            T_IfExpression(branches, elses) => {
                for (a, b) in branches {
                    for child in a.iter_mut().chain(b) {
                        visit(child);
                    }
                }
                for child in elses {
                    visit(child);
                }
            }
            T_Script(a, l) => {
                visit(a);
                for child in l {
                    visit(child);
                }
            }
            _ => {}
        }
    }
}

/// `doAnalysis`: calls `f` on every token, parents before children.
pub fn do_analysis<'a>(t: &'a Token, f: &mut dyn FnMut(&'a Token)) {
    f(t);
    t.for_each_child(&mut |c| do_analysis(c, f));
}

/// `doStackAnalysis`: `start` before the children, `end` after.
pub fn do_stack_analysis<'a>(
    t: &'a Token,
    start: &mut dyn FnMut(&'a Token),
    end: &mut dyn FnMut(&'a Token),
) {
    start(t);
    t.for_each_child(&mut |c| do_stack_analysis(c, start, end));
    end(t);
}

/// `doTransform`: rebuilds the tree bottom-up with `f` applied to every
/// token after its children.
pub fn do_transform(t: &mut Token, f: &mut dyn FnMut(&mut Token)) {
    t.for_each_child_mut(&mut |c| do_transform(c, f));
    f(t);
}
