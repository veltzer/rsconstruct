//! `ShellCheck`'s AST (`ShellCheck.AST`).
//!
//! The variant names are `ShellCheck`'s own with the underscore dropped
//! (`T_SimpleCommand` is `TSimpleCommand`, `TA_Binary` is `TaBinary`, the
//! lexer tokens such as `T_DGREAT` are `TLexDgreat`), so every check here
//! can be read side by side with its Haskell original. A token is an id plus its inner variant; ids map to source
//! spans in the parse result.
//!
//! Traversal visits children in the order Haskell's derived `Traversable`
//! instance does: fields left to right, lists in order, and only fields of
//! the token type (the name of a coprocess is a `Maybe Token` field and is
//! not visited). Equality ignores ids, as `ShellCheck`'s `Eq Token` does.

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
pub enum CaseType {
    Break,
    FallThrough,
    Continue,
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
    TaBinary(String, B, B),
    TaAssignment(String, B, B),
    TaVariable(String, L),
    TaExpansion(L),
    TaSequence(L),
    TaParenthesis(B),
    TaTrinary(B, B, B),
    TaUnary(String, B),
    TcAnd(ConditionType, String, B, B),
    TcBinary(ConditionType, String, B, B),
    TcGroup(ConditionType, B),
    TcNullary(ConditionType, B),
    TcOr(ConditionType, String, B, B),
    TcUnary(ConditionType, String, B),
    TcEmpty(ConditionType),
    TLexAndIf,
    TAndIf(B, B),
    TArithmetic(B),
    TArray(L),
    TIndexedElement(L, B),
    TUnparsedIndex(SourcePos, String),
    TAssignment(AssignmentMode, String, L, B),
    TBackgrounded(B),
    TBackticked(L),
    TBang,
    TBanged(B),
    TBraceExpansion(L),
    TBraceGroup(L),
    TLexClobber,
    TCase,
    TCaseExpression(B, Vec<(CaseType, L, L)>),
    TCondition(ConditionType, B),
    TLexDgreat,
    TLexDsemi,
    TDo,
    TDollarArithmetic(B),
    TDollarBraced(bool, B),
    TDollarBracket(B),
    TDollarDoubleQuoted(L),
    TDollarExpansion(L),
    TDollarSingleQuoted(String),
    TDollarBraceCommandExpansion(Piped, L),
    TDone,
    TDoubleQuoted(L),
    TLexEof,
    TElif,
    TElse,
    TEsac,
    TExtglob(String, L),
    TFdRedirect(String, B),
    TFi,
    TFor,
    TForArithmetic(B, B, B, L),
    TForIn(String, L, L),
    /// keyword, parentheses, name, body
    TFunction(bool, bool, String, B),
    TLexGreatand,
    TGlob(String),
    TGreater,
    THereDoc(Dashed, Quoted, String, L),
    THereString(B),
    TIf,
    TIfExpression(Vec<(L, L)>, L),
    TIn,
    TIoFile(B, B),
    TIoDuplicate(B, String),
    TLexLessand,
    TLexLessgreat,
    TLbrace,
    TLess,
    TLiteral(String),
    TLparen,
    TNormalWord(L),
    TLexOrIf,
    TOrIf(B, B),
    TParamSubSpecialChar(String),
    /// pipe separators, commands
    TPipeline(L, L),
    TProcSub(String, L),
    TRbrace,
    TRedirecting(L, B),
    TRparen,
    TScript(B, L),
    TSelect,
    TSelectIn(String, L, L),
    TSemi,
    TSimpleCommand(L, L),
    TSingleQuoted(String),
    TSubshell(L),
    TThen,
    TUntil,
    TUntilExpression(L, L),
    TWhile,
    TWhileExpression(L, L),
    TAnnotation(Vec<Annotation>, B),
    TPipe(String),
    /// The name is not a traversed child.
    TCoProc(Option<B>, B),
    TCoProcBody(B),
    TInclude(B),
    TSourceCommand(B, B),
    TBatsTest(String, B),
}

use Inner::{
    TAndIf, TAnnotation, TArithmetic, TArray, TAssignment, TBackgrounded, TBackticked, TBang,
    TBanged, TBatsTest, TBraceExpansion, TBraceGroup, TCase, TCaseExpression, TCoProc, TCoProcBody,
    TCondition, TDo, TDollarArithmetic, TDollarBraceCommandExpansion, TDollarBraced,
    TDollarBracket, TDollarDoubleQuoted, TDollarExpansion, TDollarSingleQuoted, TDone,
    TDoubleQuoted, TElif, TElse, TEsac, TExtglob, TFdRedirect, TFi, TFor, TForArithmetic, TForIn,
    TFunction, TGlob, TGreater, THereDoc, THereString, TIf, TIfExpression, TIn, TInclude,
    TIndexedElement, TIoDuplicate, TIoFile, TLbrace, TLess, TLexAndIf, TLexClobber, TLexDgreat,
    TLexDsemi, TLexEof, TLexGreatand, TLexLessand, TLexLessgreat, TLexOrIf, TLiteral, TLparen,
    TNormalWord, TOrIf, TParamSubSpecialChar, TPipe, TPipeline, TProcSub, TRbrace, TRedirecting,
    TRparen, TScript, TSelect, TSelectIn, TSemi, TSimpleCommand, TSingleQuoted, TSourceCommand,
    TSubshell, TThen, TUnparsedIndex, TUntil, TUntilExpression, TWhile, TWhileExpression,
    TaAssignment, TaBinary, TaExpansion, TaParenthesis, TaSequence, TaTrinary, TaUnary, TaVariable,
    TcAnd, TcBinary, TcEmpty, TcGroup, TcNullary, TcOr, TcUnary,
};

impl Token {
    pub const fn new(id: Id, inner: Inner) -> Self {
        Self { id, inner }
    }

    pub fn for_each_child<'a>(&'a self, visit: &mut dyn FnMut(&'a Self)) {
        match &self.inner {
            TaBinary(_, a, b) | TaAssignment(_, a, b) => {
                visit(a);
                visit(b);
            }
            TaVariable(_, l)
            | TaExpansion(l)
            | TaSequence(l)
            | TArray(l)
            | TBackticked(l)
            | TBraceExpansion(l)
            | TBraceGroup(l)
            | TDollarDoubleQuoted(l)
            | TDollarExpansion(l)
            | TDollarBraceCommandExpansion(_, l)
            | TDoubleQuoted(l)
            | TExtglob(_, l)
            | THereDoc(_, _, _, l)
            | TNormalWord(l)
            | TProcSub(_, l)
            | TSubshell(l) => {
                for child in l {
                    visit(child);
                }
            }
            TaParenthesis(a)
            | TaUnary(_, a)
            | TcGroup(_, a)
            | TcNullary(_, a)
            | TcUnary(_, _, a)
            | TArithmetic(a)
            | TBackgrounded(a)
            | TBanged(a)
            | TCondition(_, a)
            | TDollarArithmetic(a)
            | TDollarBraced(_, a)
            | TDollarBracket(a)
            | TFdRedirect(_, a)
            | TFunction(_, _, _, a)
            | THereString(a)
            | TIoDuplicate(a, _)
            | TAnnotation(_, a)
            | TCoProc(_, a)
            | TCoProcBody(a)
            | TInclude(a)
            | TBatsTest(_, a) => visit(a),
            TaTrinary(a, b, c) => {
                visit(a);
                visit(b);
                visit(c);
            }
            TcAnd(_, _, a, b)
            | TcBinary(_, _, a, b)
            | TcOr(_, _, a, b)
            | TAndIf(a, b)
            | TOrIf(a, b)
            | TIoFile(a, b)
            | TSourceCommand(a, b) => {
                visit(a);
                visit(b);
            }
            TIndexedElement(l, a) | TAssignment(_, _, l, a) | TRedirecting(l, a) => {
                for child in l {
                    visit(child);
                }
                visit(a);
            }
            TCaseExpression(w, cases) => {
                visit(w);
                for (_, a, b) in cases {
                    for child in a.iter().chain(b) {
                        visit(child);
                    }
                }
            }
            TForArithmetic(a, b, c, l) => {
                visit(a);
                visit(b);
                visit(c);
                for child in l {
                    visit(child);
                }
            }
            TForIn(_, a, b)
            | TSelectIn(_, a, b)
            | TPipeline(a, b)
            | TSimpleCommand(a, b)
            | TUntilExpression(a, b)
            | TWhileExpression(a, b) => {
                for child in a.iter().chain(b) {
                    visit(child);
                }
            }
            TIfExpression(branches, elses) => {
                for (a, b) in branches {
                    for child in a.iter().chain(b) {
                        visit(child);
                    }
                }
                for child in elses {
                    visit(child);
                }
            }
            TScript(a, l) => {
                visit(a);
                for child in l {
                    visit(child);
                }
            }
            TcEmpty(_)
            | TLexAndIf
            | TUnparsedIndex(..)
            | TBang
            | TLexClobber
            | TCase
            | TLexDgreat
            | TLexDsemi
            | TDo
            | TDollarSingleQuoted(_)
            | TDone
            | TLexEof
            | TElif
            | TElse
            | TEsac
            | TFi
            | TFor
            | TLexGreatand
            | TGlob(_)
            | TGreater
            | TIf
            | TIn
            | TLexLessand
            | TLexLessgreat
            | TLbrace
            | TLess
            | TLiteral(_)
            | TLparen
            | TLexOrIf
            | TParamSubSpecialChar(_)
            | TRbrace
            | TRparen
            | TSelect
            | TSemi
            | TSingleQuoted(_)
            | TThen
            | TUntil
            | TWhile
            | TPipe(_) => {}
        }
    }

    /// Mutable children, in the same order.
    pub fn for_each_child_mut(&mut self, visit: &mut dyn FnMut(&mut Self)) {
        match &mut self.inner {
            TaBinary(_, a, b) | TaAssignment(_, a, b) => {
                visit(a);
                visit(b);
            }
            TaVariable(_, l)
            | TaExpansion(l)
            | TaSequence(l)
            | TArray(l)
            | TBackticked(l)
            | TBraceExpansion(l)
            | TBraceGroup(l)
            | TDollarDoubleQuoted(l)
            | TDollarExpansion(l)
            | TDollarBraceCommandExpansion(_, l)
            | TDoubleQuoted(l)
            | TExtglob(_, l)
            | THereDoc(_, _, _, l)
            | TNormalWord(l)
            | TProcSub(_, l)
            | TSubshell(l) => {
                for child in l {
                    visit(child);
                }
            }
            TaParenthesis(a)
            | TaUnary(_, a)
            | TcGroup(_, a)
            | TcNullary(_, a)
            | TcUnary(_, _, a)
            | TArithmetic(a)
            | TBackgrounded(a)
            | TBanged(a)
            | TCondition(_, a)
            | TDollarArithmetic(a)
            | TDollarBraced(_, a)
            | TDollarBracket(a)
            | TFdRedirect(_, a)
            | TFunction(_, _, _, a)
            | THereString(a)
            | TIoDuplicate(a, _)
            | TAnnotation(_, a)
            | TCoProc(_, a)
            | TCoProcBody(a)
            | TInclude(a)
            | TBatsTest(_, a) => visit(a),
            TaTrinary(a, b, c) => {
                visit(a);
                visit(b);
                visit(c);
            }
            TcAnd(_, _, a, b)
            | TcBinary(_, _, a, b)
            | TcOr(_, _, a, b)
            | TAndIf(a, b)
            | TOrIf(a, b)
            | TIoFile(a, b)
            | TSourceCommand(a, b) => {
                visit(a);
                visit(b);
            }
            TIndexedElement(l, a) | TAssignment(_, _, l, a) | TRedirecting(l, a) => {
                for child in l {
                    visit(child);
                }
                visit(a);
            }
            TCaseExpression(w, cases) => {
                visit(w);
                for (_, a, b) in cases {
                    for child in a.iter_mut().chain(b) {
                        visit(child);
                    }
                }
            }
            TForArithmetic(a, b, c, l) => {
                visit(a);
                visit(b);
                visit(c);
                for child in l {
                    visit(child);
                }
            }
            TForIn(_, a, b)
            | TSelectIn(_, a, b)
            | TPipeline(a, b)
            | TSimpleCommand(a, b)
            | TUntilExpression(a, b)
            | TWhileExpression(a, b) => {
                for child in a.iter_mut().chain(b) {
                    visit(child);
                }
            }
            TIfExpression(branches, elses) => {
                for (a, b) in branches {
                    for child in a.iter_mut().chain(b) {
                        visit(child);
                    }
                }
                for child in elses {
                    visit(child);
                }
            }
            TScript(a, l) => {
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
