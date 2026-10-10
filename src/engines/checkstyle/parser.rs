//! checkstyle's Java recognizer: `GeneratedJavaRecognizer`, the LL(2)
//! parser ANTLR 2.7.7 generates from `java.g`, rule by rule.
//!
//! Each rule follows the generated method: the same lookahead tests (the
//! `SET_n` tables are the generated `_tokenSet_n`), the same syntactic
//! predicates (mark, guess, rewind; a recognition error inside a guess just
//! fails it), the same semantic predicates, and the same tree building
//! through `ASTFactory` (`super::ast`). Actions guarded by
//! `inputState.guessing == 0` run only outside guesses; the two that are not
//! guarded (`switchBlockDepth++`/`--` around a switch block) run in guesses
//! too, and a guess that fails between them leaves the depth raised, as in
//! checkstyle. Trees built during a guess are thrown away, so they are not
//! built here at all.
//!
//! Errors are ANTLR's exceptions: `NoViableAltException` ("unexpected token:
//! x"), `MismatchedTokenException` ("expecting X, found 'x'") and the
//! `SemanticException` of a failed `{...}?` predicate, plus whatever the
//! token stream raised when the parser read that far.

// The token type constants are the generated grammar's names
// (`LITERAL_void`, ...), matched on as they are in the generated methods;
// importing them all keeps each rule readable next to the method it ports.
#![allow(non_upper_case_globals)]
#[allow(clippy::wildcard_imports)]
use super::tables::*;

use super::ast::{Ast, NodeId, Pair};
use super::lexer::{EOF_TYPE, StreamError, Token};

/// A set of token types (an ANTLR parser `BitSet`; types are below 256).
pub struct TokenSet(pub [u64; 4]);

impl TokenSet {
    pub const fn contains(&self, ty: u16) -> bool {
        let ty = ty as usize;
        ty < 256 && (self.0[ty / 64] >> (ty % 64)) & 1 == 1
    }
}

/// A parse failure, as checkstyle's root cause prints it.
#[derive(Clone, Debug)]
pub enum ParseError {
    /// A `RecognitionException` with a position.
    Recognition {
        line: i32,
        col: i32,
        message: String,
    },
    /// A `SemanticException` of the parser, which has no position.
    Semantic(String),
    /// The token stream failed.
    Stream(StreamError),
}

type R<T> = Result<T, ParseError>;
type Node = Option<NodeId>;

const fn is_builtin(t: u16) -> bool {
    t >= LITERAL_void && t <= LITERAL_double
}

/// `IDENT | "record" | "yield"`: what `id` starts with.
const fn is_id(t: u16) -> bool {
    matches!(t, IDENT | LITERAL_record | LITERAL_yield)
}

/// `classTypeSpec`'s first tokens (`id` or an annotation).
const fn is_class_type_start(t: u16) -> bool {
    is_id(t) || t == AT
}

const fn is_modifier_keyword(t: u16) -> bool {
    matches!(
        t,
        LITERAL_private
            | LITERAL_public
            | LITERAL_protected
            | LITERAL_static
            | LITERAL_transient
            | LITERAL_native
            | LITERAL_synchronized
            | LITERAL_volatile
    )
}

const fn is_literal_constant(t: u16) -> bool {
    matches!(
        t,
        NUM_INT | CHAR_LITERAL | STRING_LITERAL | NUM_FLOAT | NUM_LONG | NUM_DOUBLE
    )
}

/// The tokens an expression starts with (the generated switches list them
/// before every `expression()` call).
const fn is_expression_start(t: u16) -> bool {
    is_builtin(t)
        || is_literal_constant(t)
        || matches!(
            t,
            IDENT
                | LPAREN
                | LITERAL_this
                | LITERAL_super
                | LITERAL_switch
                | PLUS
                | MINUS
                | INC
                | DEC
                | BNOT
                | LNOT
                | LITERAL_true
                | LITERAL_false
                | LITERAL_null
                | LITERAL_new
                | AT
                | LITERAL_record
                | TEXT_BLOCK_CONTENT
                | LITERAL_yield
        )
}

/// The tokens a member of a class body starts with (`field()`).
const fn is_field_start(t: u16) -> bool {
    is_builtin(t)
        || is_modifier_keyword(t)
        || matches!(
            t,
            FINAL
                | ABSTRACT
                | STRICTFP
                | IDENT
                | LITERAL_class
                | LITERAL_interface
                | LCURLY
                | LITERAL_default
                | LT
                | ENUM
                | AT
                | LITERAL_record
                | LITERAL_yield
        )
}

/// The statement keywords common to several follow sets.
const fn is_statement_keyword(t: u16) -> bool {
    matches!(
        t,
        LITERAL_if
            | LITERAL_while
            | LITERAL_do
            | LITERAL_break
            | LITERAL_continue
            | LITERAL_return
            | LITERAL_switch
            | LITERAL_throw
            | LITERAL_for
    )
}

/// What may follow `annotation()` when no `(` follows its name.
const fn annotation_follow(t: u16) -> bool {
    is_builtin(t)
        || is_modifier_keyword(t)
        || matches!(
            t,
            FINAL
                | ABSTRACT
                | STRICTFP
                | LITERAL_package
                | SEMI
                | LBRACK
                | IDENT
                | LITERAL_class
                | LITERAL_interface
                | RCURLY
                | COMMA
                | RPAREN
                | LITERAL_this
                | LITERAL_default
                | QUESTION
                | LT
                | ENUM
                | AT
                | ELLIPSIS
                | LITERAL_record
                | LITERAL_yield
        )
}

/// What may follow an assignment expression without an assignment
/// operator (`assignmentExpression`'s exit switch).
const fn assignment_follow(t: u16) -> bool {
    is_builtin(t)
        || is_modifier_keyword(t)
        || is_literal_constant(t)
        || is_statement_keyword(t)
        || matches!(
            t,
            FINAL
                | ABSTRACT
                | STRICTFP
                | SEMI
                | RBRACK
                | IDENT
                | STAR
                | LITERAL_class
                | LITERAL_interface
                | LCURLY
                | RCURLY
                | COMMA
                | LPAREN
                | RPAREN
                | LITERAL_this
                | LITERAL_super
                | COLON
                | LITERAL_else
                | LITERAL_case
                | LITERAL_default
                | LITERAL_try
                | LT
                | GT
                | LE
                | GE
                | LITERAL_instanceof
                | SL
                | SR
                | BSR
                | PLUS
                | MINUS
                | DIV
                | MOD
                | INC
                | DEC
                | BNOT
                | LNOT
                | LITERAL_true
                | LITERAL_false
                | LITERAL_null
                | LITERAL_new
                | ASSERT
                | ENUM
                | AT
                | LITERAL_record
                | TEXT_BLOCK_CONTENT
                | LITERAL_yield
        )
}

const fn is_assignment_operator(t: u16) -> bool {
    matches!(
        t,
        ASSIGN
            | PLUS_ASSIGN
            | MINUS_ASSIGN
            | STAR_ASSIGN
            | DIV_ASSIGN
            | MOD_ASSIGN
            | SR_ASSIGN
            | BSR_ASSIGN
            | SL_ASSIGN
            | BAND_ASSIGN
            | BXOR_ASSIGN
            | BOR_ASSIGN
    )
}

/// What may follow a conditional expression without `?`: the assignment
/// follow set plus the assignment operators and `->`.
const fn conditional_follow(t: u16) -> bool {
    assignment_follow(t) || is_assignment_operator(t) || t == LAMBDA
}

/// What may follow a `try` statement without `finally`.
const fn try_follow(t: u16) -> bool {
    is_builtin(t)
        || is_modifier_keyword(t)
        || is_literal_constant(t)
        || is_statement_keyword(t)
        || matches!(
            t,
            FINAL
                | ABSTRACT
                | STRICTFP
                | SEMI
                | IDENT
                | STAR
                | LITERAL_class
                | LCURLY
                | RCURLY
                | LPAREN
                | RPAREN
                | LITERAL_this
                | LITERAL_super
                | COLON
                | LITERAL_else
                | LITERAL_case
                | LITERAL_default
                | LITERAL_try
                | LT
                | GT
                | LE
                | GE
                | LITERAL_instanceof
                | SL
                | SR
                | BSR
                | PLUS
                | MINUS
                | DIV
                | MOD
                | INC
                | DEC
                | BNOT
                | LNOT
                | LITERAL_true
                | LITERAL_false
                | LITERAL_null
                | LITERAL_new
                | ASSERT
                | AT
                | LITERAL_record
                | TEXT_BLOCK_CONTENT
                | LITERAL_yield
        )
}

/// What may follow a variable declarator without `=`.
const fn var_initializer_follow(t: u16) -> bool {
    is_builtin(t)
        || is_modifier_keyword(t)
        || matches!(
            t,
            FINAL
                | ABSTRACT
                | STRICTFP
                | SEMI
                | IDENT
                | LITERAL_class
                | LITERAL_interface
                | LCURLY
                | RCURLY
                | COMMA
                | RPAREN
                | LITERAL_default
                | LT
                | ENUM
                | AT
                | LITERAL_record
                | LITERAL_yield
        )
}

/// What may follow a switch label without `:`.
const fn switch_label_follow(t: u16) -> bool {
    is_builtin(t)
        || is_modifier_keyword(t)
        || is_literal_constant(t)
        || is_statement_keyword(t)
        || matches!(
            t,
            FINAL
                | ABSTRACT
                | STRICTFP
                | SEMI
                | IDENT
                | LITERAL_class
                | LCURLY
                | RCURLY
                | LPAREN
                | LITERAL_this
                | LITERAL_super
                | LITERAL_case
                | LITERAL_default
                | LITERAL_try
                | PLUS
                | MINUS
                | INC
                | DEC
                | BNOT
                | LNOT
                | LITERAL_true
                | LITERAL_false
                | LITERAL_null
                | LITERAL_new
                | ASSERT
                | AT
                | LAMBDA
                | LITERAL_record
                | TEXT_BLOCK_CONTENT
                | LITERAL_yield
        )
}

/// What `statement()` dispatches to `traditionalStatement()`.
const fn is_traditional_statement_start(t: u16) -> bool {
    is_builtin(t)
        || is_modifier_keyword(t)
        || is_literal_constant(t)
        || is_statement_keyword(t)
        || matches!(
            t,
            FINAL
                | ABSTRACT
                | STRICTFP
                | SEMI
                | IDENT
                | LITERAL_class
                | LCURLY
                | LPAREN
                | LITERAL_this
                | LITERAL_super
                | LITERAL_default
                | LITERAL_try
                | PLUS
                | MINUS
                | INC
                | DEC
                | BNOT
                | LNOT
                | LITERAL_true
                | LITERAL_false
                | LITERAL_null
                | LITERAL_new
                | AT
                | LITERAL_record
                | TEXT_BLOCK_CONTENT
                | LITERAL_yield
        )
}

pub struct Parser<'a> {
    tokens: &'a [Token],
    stream_error: Option<&'a StreamError>,
    p: usize,
    guessing: u32,
    pub ast: Ast,
    lt_counter: i32,
    gt_to_reconcile: i32,
    current_gt_sequence: Option<NodeId>,
    switch_block_depth: i32,
}

impl<'a> Parser<'a> {
    pub fn new(tokens: &'a [Token], stream_error: Option<&'a StreamError>) -> Self {
        Self {
            tokens,
            stream_error,
            p: 0,
            guessing: 0,
            ast: Ast::default(),
            lt_counter: 0,
            gt_to_reconcile: 0,
            current_gt_sequence: None,
            switch_block_depth: 0,
        }
    }

    // ---- LLkParser ------------------------------------------------------------

    /// `LT(i)`: the token buffer fills up to it, which is where a failure of
    /// the token stream surfaces.
    fn lt(&self, i: usize) -> R<&'a Token> {
        let index = self.p + i - 1;
        if let Some(token) = self.tokens.get(index) {
            return Ok(token);
        }
        if let Some(error) = self.stream_error {
            return Err(ParseError::Stream(error.clone()));
        }
        self.tokens
            .last()
            .filter(|t| t.ty == EOF_TYPE)
            .ok_or_else(|| ParseError::Semantic("token stream ended without end of file".into()))
    }

    fn la(&self, i: usize) -> R<u16> {
        Ok(self.lt(i)?.ty)
    }

    const fn g0(&self) -> bool {
        self.guessing == 0
    }

    fn no_viable(&self) -> ParseError {
        match self.lt(1) {
            Ok(token) => ParseError::Recognition {
                line: token.line,
                col: token.col,
                message: format!(
                    "unexpected token: {}",
                    token.text.as_deref().unwrap_or("null")
                ),
            },
            Err(e) => e,
        }
    }

    fn fail<T>(&self) -> R<T> {
        Err(self.no_viable())
    }

    /// `match(t)`.
    fn match_(&mut self, ty: u16) -> R<()> {
        let token = self.lt(1)?;
        if token.ty != ty {
            let name = TOKEN_NAMES.get(usize::from(ty)).copied().unwrap_or("?");
            return Err(ParseError::Recognition {
                line: token.line,
                col: token.col,
                message: format!(
                    "expecting {name}, found '{}'",
                    token.text.as_deref().unwrap_or("null")
                ),
            });
        }
        self.p += 1;
        Ok(())
    }

    /// A syntactic predicate: try `rule` without building anything, then
    /// rewind. A recognition failure is "no"; a token stream failure is
    /// not caught.
    fn synpred(&mut self, rule: impl FnOnce(&mut Self) -> R<()>) -> R<bool> {
        let mark = self.p;
        self.guessing += 1;
        let result = rule(self);
        match result {
            Ok(()) => {}
            Err(ParseError::Stream(e)) => return Err(ParseError::Stream(e)),
            Err(_) => {
                self.p = mark;
                self.guessing -= 1;
                return Ok(false);
            }
        }
        self.p = mark;
        self.guessing -= 1;
        Ok(true)
    }

    // ---- tree building --------------------------------------------------------

    /// `astFactory.create(LT(1))`, outside guesses.
    fn create(&mut self) -> R<Node> {
        let token = self.lt(1)?;
        Ok(if self.g0() {
            Some(self.ast.token_node(token))
        } else {
            None
        })
    }

    /// `astFactory.create(TYPE, "TYPE")`, outside guesses.
    fn imaginary(&mut self, ty: u16, name: &str) -> Node {
        if self.g0() {
            Some(self.ast.imaginary(ty, name))
        } else {
            None
        }
    }

    fn add(&mut self, pair: &mut Pair, node: Node) {
        if self.g0() {
            self.ast.add_ast_child(pair, node);
        }
    }

    fn root(&mut self, pair: &mut Pair, node: Node) {
        if self.g0() {
            self.ast.make_ast_root(pair, node);
        }
    }

    fn make(&mut self, nodes: &[Node]) -> Node {
        if self.g0() {
            self.ast.make(nodes)
        } else {
            None
        }
    }

    fn set_type(&mut self, node: Node, ty: u16) {
        if let (true, Some(n)) = (self.g0(), node) {
            self.ast.set_type(n, ty);
        }
    }

    /// The common rule ending `#rule = #(#[TYPE,"TYPE"], #rule)`.
    fn wrap(&mut self, pair: &mut Pair, ty: u16, name: &str) {
        if self.g0() {
            let root = self.imaginary(ty, name);
            let tree = self.ast.make(&[root, pair.root]);
            self.ast.set_rule_tree(pair, tree);
        }
    }

    /// `#rule = tree` (with `currentAST` updated as the generated code does).
    fn set_tree(&self, pair: &mut Pair, tree: Node) {
        if self.g0() {
            self.ast.set_rule_tree(pair, tree);
        }
    }

    /// Match one token and add it as a child.
    fn token(&mut self, pair: &mut Pair, ty: u16) -> R<Node> {
        let node = self.create()?;
        self.add(pair, node);
        self.match_(ty)?;
        Ok(node)
    }

    /// Match one token and make it the root.
    fn token_root(&mut self, pair: &mut Pair, ty: u16) -> R<Node> {
        let node = self.create()?;
        self.root(pair, node);
        self.match_(ty)?;
        Ok(node)
    }

    /// Match one token without adding it anywhere (a labelled `x:T` in a
    /// `!` rule).
    fn token_alone(&mut self, ty: u16) -> R<Node> {
        let node = self.create()?;
        self.match_(ty)?;
        Ok(node)
    }

    /// The `GT`/`SR`/`BSR` the type argument productions consumed.
    fn consume_current_gt_sequence(&mut self, node: Node) {
        let Some(node) = node else {
            return;
        };
        self.current_gt_sequence = Some(node);
        let len = self.ast.text(node).encode_utf16().count() as i32;
        self.gt_to_reconcile += len;
        self.lt_counter -= len;
    }

    /// `emitSingleGt`: a `GENERIC_END` for one of the pending `>`.
    fn emit_single_gt(&mut self) -> Node {
        self.gt_to_reconcile -= 1;
        let sequence = self.current_gt_sequence?;
        let len = self.ast.text(sequence).encode_utf16().count() as i32;
        let token = Token {
            ty: GENERIC_END,
            text: Some(">".into()),
            line: self.ast.line(sequence),
            col: self.ast.column(sequence) + (len - self.gt_to_reconcile),
        };
        Some(self.ast.token_node(&token))
    }

    const fn are_lts_and_gts_balanced(&self, current_lt_level: i32) -> bool {
        current_lt_level != 0 || self.lt_counter == current_lt_level
    }

    // ---- rules ----------------------------------------------------------------

    pub fn compilation_unit(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        let package = (l1 == LITERAL_package || l1 == AT)
            && SET_0.contains(self.la(2)?)
            && self.synpred(|p| {
                p.annotations()?;
                p.match_(LITERAL_package)
            })?;
        if package {
            let n = self.package_definition()?;
            self.add(&mut cur, n);
        } else if !(SET_1.contains(l1) && SET_2.contains(self.la(2)?)) {
            return self.fail();
        }
        loop {
            let l1 = self.la(1)?;
            if (l1 == SEMI || l1 == LITERAL_import) && SET_2.contains(self.la(2)?) {
                let n = self.import_definition()?;
                self.add(&mut cur, n);
            } else {
                break;
            }
        }
        while SET_3.contains(self.la(1)?) {
            let n = self.type_definition()?;
            self.add(&mut cur, n);
        }
        self.match_(EOF_TYPE)?;
        Ok(cur.root)
    }

    fn package_definition(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.annotations()?;
        self.add(&mut cur, n);
        let p = self.token_root(&mut cur, LITERAL_package)?;
        self.set_type(p, PACKAGE_DEF);
        let n = self.identifier()?;
        self.add(&mut cur, n);
        self.token(&mut cur, SEMI)?;
        Ok(cur.root)
    }

    fn import_definition(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        match self.la(1)? {
            LITERAL_import => {
                let i = self.token_root(&mut cur, LITERAL_import)?;
                self.set_type(i, IMPORT);
                match self.la(1)? {
                    LITERAL_static => {
                        self.token(&mut cur, LITERAL_static)?;
                        self.set_type(i, STATIC_IMPORT);
                    }
                    t if is_id(t) => {}
                    _ => return self.fail(),
                }
                let n = self.identifier_star()?;
                self.add(&mut cur, n);
                self.token(&mut cur, SEMI)?;
            }
            SEMI => {
                self.token(&mut cur, SEMI)?;
            }
            _ => return self.fail(),
        }
        Ok(cur.root)
    }

    fn type_definition(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if is_modifier_keyword(t)
            || matches!(
                t,
                FINAL
                    | ABSTRACT
                    | STRICTFP
                    | LITERAL_class
                    | LITERAL_interface
                    | LITERAL_default
                    | ENUM
                    | AT
                    | LITERAL_record
            )
        {
            let m = self.modifiers()?;
            let n = self.type_definition_internal(m)?;
            self.add(&mut cur, n);
        } else if t == SEMI {
            self.token(&mut cur, SEMI)?;
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn identifier(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.id()?;
        self.add(&mut cur, n);
        while self.la(1)? == DOT {
            self.token_root(&mut cur, DOT)?;
            let n = self.id()?;
            self.add(&mut cur, n);
        }
        Ok(cur.root)
    }

    fn identifier_star(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.id()?;
        self.add(&mut cur, n);
        while self.la(1)? == DOT && SET_0.contains(self.la(2)?) {
            self.token_root(&mut cur, DOT)?;
            let n = self.id()?;
            self.add(&mut cur, n);
        }
        match self.la(1)? {
            DOT => {
                self.token_root(&mut cur, DOT)?;
                self.token(&mut cur, STAR)?;
            }
            SEMI => {}
            _ => return self.fail(),
        }
        Ok(cur.root)
    }

    fn modifiers(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        loop {
            let l1 = self.la(1)?;
            if SET_5.contains(l1) {
                let n = self.modifier()?;
                self.add(&mut cur, n);
            } else if l1 == AT
                && SET_0.contains(self.la(2)?)
                && self.lt(2)?.text.as_deref() != Some("interface")
            {
                let n = self.annotation()?;
                self.add(&mut cur, n);
            } else {
                break;
            }
        }
        self.wrap(&mut cur, MODIFIERS, "MODIFIERS");
        Ok(cur.root)
    }

    fn type_definition_internal(&mut self, modifiers: Node) -> R<Node> {
        let mut cur = Pair::default();
        let n = match self.la(1)? {
            LITERAL_class => self.class_definition(modifiers)?,
            LITERAL_interface => self.interface_definition(modifiers)?,
            ENUM => self.enum_definition(modifiers)?,
            AT => self.annotation_definition(modifiers)?,
            LITERAL_record => self.record_definition(modifiers)?,
            _ => return self.fail(),
        };
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn class_definition(&mut self, modifiers: Node) -> R<Node> {
        let mut cur = Pair::default();
        let c = self.token_alone(LITERAL_class)?;
        let id = self.id()?;
        let tp = match self.la(1)? {
            LT => self.type_parameters()?,
            LITERAL_extends | LCURLY | LITERAL_implements => None,
            _ => return self.fail(),
        };
        let sc = self.super_class_clause()?;
        let ic = self.implements_clause()?;
        let cb = self.class_block()?;
        if self.g0() {
            let root = self.imaginary(CLASS_DEF, "CLASS_DEF");
            let tree = self.make(&[root, modifiers, c, id, tp, sc, ic, cb]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn interface_definition(&mut self, modifiers: Node) -> R<Node> {
        let mut cur = Pair::default();
        let i = self.token_alone(LITERAL_interface)?;
        let id = self.id()?;
        let tp = match self.la(1)? {
            LT => self.type_parameters()?,
            LITERAL_extends | LCURLY => None,
            _ => return self.fail(),
        };
        let ie = self.interface_extends()?;
        let cb = self.class_block()?;
        if self.g0() {
            let root = self.imaginary(INTERFACE_DEF, "INTERFACE_DEF");
            let tree = self.make(&[root, modifiers, i, id, tp, ie, cb]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn enum_definition(&mut self, modifiers: Node) -> R<Node> {
        let mut cur = Pair::default();
        let e = self.token_alone(ENUM)?;
        let id = self.id()?;
        let ic = self.implements_clause()?;
        let eb = self.enum_block()?;
        if self.g0() {
            let root = self.imaginary(ENUM_DEF, "ENUM_DEF");
            let tree = self.make(&[root, modifiers, e, id, ic, eb]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn annotation_definition(&mut self, modifiers: Node) -> R<Node> {
        let mut cur = Pair::default();
        let a = self.token_alone(AT)?;
        let i = self.token_alone(LITERAL_interface)?;
        let id = self.id()?;
        let ab = self.annotation_block()?;
        if self.g0() {
            let root = self.imaginary(ANNOTATION_DEF, "ANNOTATION_DEF");
            let tree = self.make(&[root, modifiers, a, i, id, ab]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn record_definition(&mut self, modifiers: Node) -> R<Node> {
        let mut cur = Pair::default();
        let r = self.token_alone(LITERAL_record)?;
        let id = self.id()?;
        let tp = match self.la(1)? {
            LT => self.type_parameters()?,
            LPAREN => None,
            _ => return self.fail(),
        };
        let rc = self.record_components_list()?;
        let ic = self.implements_clause()?;
        let rb = self.record_body_declaration()?;
        if self.g0() {
            let root = self.imaginary(RECORD_DEF, "RECORD_DEF");
            let tree = self.make(&[root, modifiers, r, id, tp, ic, rc, rb]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn type_spec(&mut self, add_imag_node: bool) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        let n = if is_class_type_start(t) {
            self.class_type_spec(add_imag_node)?
        } else if is_builtin(t) {
            self.built_in_type_spec(add_imag_node)?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    /// The `(({LA(1) == AT}? annotations | ) LBRACK^ RBRACK)*` loop of
    /// `classTypeSpec` and `builtInTypeSpec`.
    fn array_brackets_with_annotations(&mut self, cur: &mut Pair) -> R<()> {
        loop {
            let l1 = self.la(1)?;
            if !((l1 == LBRACK || l1 == AT) && SET_6.contains(self.la(2)?)) {
                break;
            }
            if l1 == AT {
                let n = self.annotations()?;
                self.add(cur, n);
            } else if !(l1 == LBRACK && self.la(2)? == RBRACK) {
                return self.fail();
            }
            let lb = self.token_root(cur, LBRACK)?;
            self.set_type(lb, ARRAY_DECLARATOR);
            self.token(cur, RBRACK)?;
        }
        Ok(())
    }

    fn class_type_spec(&mut self, add_imag_node: bool) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.class_or_interface_type(add_imag_node)?;
        self.add(&mut cur, n);
        self.array_brackets_with_annotations(&mut cur)?;
        if add_imag_node {
            self.wrap(&mut cur, TYPE, "TYPE");
        }
        Ok(cur.root)
    }

    fn built_in_type_spec(&mut self, add_imag_node: bool) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.built_in_type()?;
        self.add(&mut cur, n);
        self.array_brackets_with_annotations(&mut cur)?;
        if add_imag_node {
            self.wrap(&mut cur, TYPE, "TYPE");
        }
        Ok(cur.root)
    }

    /// `({LA(1) == AT}? annotations | )` before brackets or `...` in
    /// `variableLengthParameterTypeSpec`.
    fn vararg_annotations(&mut self, cur: &mut Pair) -> R<()> {
        let l1 = self.la(1)?;
        if SET_7.contains(l1) && SET_6.contains(self.la(2)?) && l1 == AT {
            let n = self.annotations()?;
            self.add(cur, n);
        } else if !((l1 == LBRACK || l1 == ELLIPSIS) && SET_6.contains(self.la(2)?)) {
            return self.fail();
        }
        Ok(())
    }

    fn variable_length_parameter_type_spec(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        let n = if is_class_type_start(t) {
            self.class_or_interface_type(false)?
        } else if is_builtin(t) {
            self.built_in_type()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        self.vararg_annotations(&mut cur)?;
        while self.la(1)? == LBRACK {
            let lb = self.token_root(&mut cur, LBRACK)?;
            self.set_type(lb, ARRAY_DECLARATOR);
            self.token(&mut cur, RBRACK)?;
            self.vararg_annotations(&mut cur)?;
        }
        Ok(cur.root)
    }

    /// `({LA(1) == AT}? annotations | )` before an `id` in
    /// `classOrInterfaceType`.
    fn type_name_annotations(&mut self, cur: &mut Pair) -> R<()> {
        let l1 = self.la(1)?;
        if SET_8.contains(l1) && SET_9.contains(self.la(2)?) && l1 == AT {
            let n = self.annotations()?;
            self.add(cur, n);
        } else if !(SET_0.contains(l1) && SET_9.contains(self.la(2)?)) {
            return self.fail();
        }
        Ok(())
    }

    fn optional_type_arguments(&mut self, cur: &mut Pair, add_imag_node: bool) -> R<()> {
        let l1 = self.la(1)?;
        if l1 == LT && SET_9.contains(self.la(2)?) {
            let n = self.type_arguments(add_imag_node)?;
            self.add(cur, n);
        } else if !(SET_9.contains(l1) && SET_10.contains(self.la(2)?)) {
            return self.fail();
        }
        Ok(())
    }

    fn class_or_interface_type(&mut self, add_imag_node: bool) -> R<Node> {
        let mut cur = Pair::default();
        self.type_name_annotations(&mut cur)?;
        let n = self.id()?;
        self.add(&mut cur, n);
        self.optional_type_arguments(&mut cur, add_imag_node)?;
        while self.la(1)? == DOT && SET_8.contains(self.la(2)?) {
            self.token_root(&mut cur, DOT)?;
            self.type_name_annotations(&mut cur)?;
            let n = self.id()?;
            self.add(&mut cur, n);
            self.optional_type_arguments(&mut cur, add_imag_node)?;
        }
        Ok(cur.root)
    }

    fn built_in_type(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if !is_builtin(t) {
            return self.fail();
        }
        self.token(&mut cur, t)?;
        Ok(cur.root)
    }

    fn id(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        match self.la(1)? {
            IDENT => {
                self.token(&mut cur, IDENT)?;
            }
            LITERAL_record => {
                let n = self.keyword_as_id(LITERAL_record)?;
                self.add(&mut cur, n);
            }
            LITERAL_yield => {
                let n = self.keyword_as_id(LITERAL_yield)?;
                self.add(&mut cur, n);
            }
            _ => return self.fail(),
        }
        Ok(cur.root)
    }

    /// `recordKey` and `yieldKey`: the keyword as an `IDENT`.
    fn keyword_as_id(&mut self, keyword: u16) -> R<Node> {
        let mut cur = Pair::default();
        self.token(&mut cur, keyword)?;
        let root = cur.root;
        self.set_type(root, IDENT);
        Ok(cur.root)
    }

    fn type_arguments(&mut self, add_imag_node: bool) -> R<Node> {
        let mut cur = Pair::default();
        // `{currentLtLevel = ltCounter;}` is an action: guesses keep 0.
        let current_lt_level = if self.g0() { self.lt_counter } else { 0 };
        let lt = self.token(&mut cur, LT)?;
        if self.g0() {
            self.set_type(lt, GENERIC_START);
            self.lt_counter += 1;
        }
        let l1 = self.la(1)?;
        if SET_11.contains(l1) && SET_9.contains(self.la(2)?) {
            let n = self.type_argument(add_imag_node)?;
            self.add(&mut cur, n);
            while self.la(1)? == COMMA && SET_11.contains(self.la(2)?) && self.gt_to_reconcile == 0
            {
                self.token(&mut cur, COMMA)?;
                let n = self.type_argument(add_imag_node)?;
                self.add(&mut cur, n);
            }
        } else if !(SET_9.contains(l1) && SET_10.contains(self.la(2)?)) {
            return self.fail();
        }
        let l1 = self.la(1)?;
        if SET_12.contains(l1) && SET_9.contains(self.la(2)?) {
            let n = self.type_arguments_or_parameters_end()?;
            self.add(&mut cur, n);
        } else if !(SET_9.contains(l1) && SET_10.contains(self.la(2)?)) {
            return self.fail();
        }
        if self.g0() && self.gt_to_reconcile > 0 {
            let gt = self.emit_single_gt();
            self.add(&mut cur, gt);
        }
        if !self.are_lts_and_gts_balanced(current_lt_level) {
            return Err(ParseError::Semantic(
                "areLtsAndGtsBalanced(currentLtLevel)".into(),
            ));
        }
        self.wrap(&mut cur, TYPE_ARGUMENTS, "TYPE_ARGUMENTS");
        Ok(cur.root)
    }

    fn type_argument(&mut self, add_imag_node: bool) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_11.contains(l1) && SET_9.contains(self.la(2)?) && l1 == AT {
            let n = self.annotations()?;
            self.add(&mut cur, n);
        } else if !(SET_11.contains(l1) && SET_9.contains(self.la(2)?)) {
            return self.fail();
        }
        let t = self.la(1)?;
        let n = if is_class_type_start(t) {
            self.class_type_spec(add_imag_node)?
        } else if is_builtin(t) {
            self.built_in_type_spec(add_imag_node)?
        } else if t == QUESTION {
            self.wildcard_type(add_imag_node)?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        self.wrap(&mut cur, TYPE_ARGUMENT, "TYPE_ARGUMENT");
        Ok(cur.root)
    }

    fn wildcard_type(&mut self, add_imag_node: bool) -> R<Node> {
        let mut cur = Pair::default();
        let q = self.token(&mut cur, QUESTION)?;
        self.set_type(q, WILDCARD_TYPE);
        let l1 = self.la(1)?;
        let bounds = (l1 == LITERAL_extends || l1 == LITERAL_super)
            && SET_13.contains(self.la(2)?)
            && self.synpred(|p| match p.la(1)? {
                LITERAL_extends => p.match_(LITERAL_extends),
                LITERAL_super => p.match_(LITERAL_super),
                _ => p.fail(),
            })?;
        if bounds {
            let n = self.type_argument_bounds(add_imag_node)?;
            self.add(&mut cur, n);
        } else if !(SET_9.contains(l1) && SET_10.contains(self.la(2)?)) {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn type_argument_bounds(&mut self, add_imag_node: bool) -> R<Node> {
        let mut cur = Pair::default();
        match self.la(1)? {
            LITERAL_extends => {
                let e = self.token_root(&mut cur, LITERAL_extends)?;
                self.set_type(e, TYPE_UPPER_BOUNDS);
            }
            LITERAL_super => {
                let s = self.token_root(&mut cur, LITERAL_super)?;
                self.set_type(s, TYPE_LOWER_BOUNDS);
            }
            _ => return self.fail(),
        }
        let t = self.la(1)?;
        let n = if is_class_type_start(t) {
            self.class_or_interface_type(add_imag_node)?
        } else if is_builtin(t) {
            self.built_in_type()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        while self.la(1)? == LBRACK && self.la(2)? == RBRACK {
            let lb = self.token_root(&mut cur, LBRACK)?;
            self.set_type(lb, ARRAY_DECLARATOR);
            self.token(&mut cur, RBRACK)?;
        }
        Ok(cur.root)
    }

    fn type_arguments_or_parameters_end(&mut self) -> R<Node> {
        let ty = self.la(1)?;
        if !matches!(ty, GT | SR | BSR) {
            return self.fail();
        }
        let node = self.token_alone(ty)?;
        if self.g0() {
            self.consume_current_gt_sequence(node);
        }
        Ok(None)
    }

    fn type_(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_13.contains(l1) && SET_14.contains(self.la(2)?) && l1 == AT {
            let n = self.annotations()?;
            self.add(&mut cur, n);
        } else if !(SET_13.contains(l1) && SET_14.contains(self.la(2)?)) {
            return self.fail();
        }
        let t = self.la(1)?;
        let n = if is_class_type_start(t) {
            self.class_or_interface_type(false)?
        } else if is_builtin(t) {
            self.built_in_type()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn declaration(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let m = self.modifiers()?;
        let t = self.type_spec(false)?;
        let v = self.variable_definitions(m, t)?;
        self.set_tree(&mut cur, v);
        Ok(cur.root)
    }

    /// The `dupTree(mods)`, `dupList(t)` copies each declarator gets.
    fn dup_mods_and_type(&mut self, mods: Node, t: Node) -> (Node, Node) {
        if self.g0() {
            (self.ast.dup_tree(mods), self.ast.dup_list(t))
        } else {
            (None, None)
        }
    }

    fn variable_definitions(&mut self, mods: Node, t: Node) -> R<Node> {
        let mut cur = Pair::default();
        let (m, ty) = self.dup_mods_and_type(mods, t);
        let n = self.variable_declarator(m, ty)?;
        self.add(&mut cur, n);
        while self.la(1)? == COMMA {
            self.token(&mut cur, COMMA)?;
            let (m, ty) = self.dup_mods_and_type(mods, t);
            let n = self.variable_declarator(m, ty)?;
            self.add(&mut cur, n);
        }
        Ok(cur.root)
    }

    fn modifier(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if !(is_modifier_keyword(t) || matches!(t, FINAL | ABSTRACT | STRICTFP | LITERAL_default)) {
            return self.fail();
        }
        self.token(&mut cur, t)?;
        Ok(cur.root)
    }

    fn annotation(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let at = self.token_alone(AT)?;
        let i = self.identifier()?;
        let (mut l, mut args, mut r) = (None, None, None);
        let t = self.la(1)?;
        if t == LPAREN {
            l = self.token_alone(LPAREN)?;
            let t = self.la(1)?;
            if is_expression_start(t) || t == LCURLY {
                args = self.annotation_arguments()?;
            } else if t != RPAREN {
                return self.fail();
            }
            r = self.token_alone(RPAREN)?;
        } else if !annotation_follow(t) {
            return self.fail();
        }
        if self.g0() {
            let root = self.imaginary(ANNOTATION, "ANNOTATION");
            let tree = self.make(&[root, at, i, l, args, r]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn annotations(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        while self.la(1)? == AT && SET_0.contains(self.la(2)?) {
            let n = self.annotation()?;
            self.add(&mut cur, n);
        }
        self.wrap(&mut cur, ANNOTATIONS, "ANNOTATIONS");
        Ok(cur.root)
    }

    fn annotation_arguments(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        let n = if SET_15.contains(l1) && SET_16.contains(self.la(2)?) {
            self.annotation_member_value_initializer()?
        } else if SET_0.contains(l1) && self.la(2)? == ASSIGN {
            self.annotation_member_value_pairs()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn annotation_member_value_initializer(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        let n = if SET_17.contains(l1)
            && SET_18.contains(self.la(2)?)
            && self.synpred(|p| p.annotation_expression().map(|_| ()))?
        {
            self.annotation_expression()?
        } else if l1 == AT && SET_0.contains(self.la(2)?) {
            self.annotation()?
        } else if l1 == LCURLY {
            self.annotation_member_array_initializer()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn annotation_member_value_pairs(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.annotation_member_value_pair()?;
        self.add(&mut cur, n);
        while self.la(1)? == COMMA {
            self.token(&mut cur, COMMA)?;
            let n = self.annotation_member_value_pair()?;
            self.add(&mut cur, n);
        }
        Ok(cur.root)
    }

    fn annotation_member_value_pair(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let i = self.id()?;
        let a = self.token_alone(ASSIGN)?;
        let v = self.annotation_member_value_initializer()?;
        if self.g0() {
            let root = self.imaginary(ANNOTATION_MEMBER_VALUE_PAIR, "ANNOTATION_MEMBER_VALUE_PAIR");
            let tree = self.make(&[root, i, a, v]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn annotation_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.conditional_expression()?;
        self.add(&mut cur, n);
        self.wrap(&mut cur, EXPR, "EXPR");
        Ok(cur.root)
    }

    fn annotation_member_array_initializer(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let lc = self.token_root(&mut cur, LCURLY)?;
        self.set_type(lc, ANNOTATION_ARRAY_INIT);
        let t = self.la(1)?;
        if is_expression_start(t) {
            let n = self.annotation_member_array_value_initializer()?;
            self.add(&mut cur, n);
            while self.la(1)? == COMMA && SET_17.contains(self.la(2)?) {
                self.token(&mut cur, COMMA)?;
                let n = self.annotation_member_array_value_initializer()?;
                self.add(&mut cur, n);
            }
            match self.la(1)? {
                COMMA => {
                    self.token(&mut cur, COMMA)?;
                }
                RCURLY => {}
                _ => return self.fail(),
            }
        } else if t != RCURLY {
            return self.fail();
        }
        self.token(&mut cur, RCURLY)?;
        Ok(cur.root)
    }

    fn annotation_member_array_value_initializer(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        let n = if SET_17.contains(l1)
            && SET_19.contains(self.la(2)?)
            && self.synpred(|p| p.annotation_expression().map(|_| ()))?
        {
            self.annotation_expression()?
        } else if l1 == AT && SET_0.contains(self.la(2)?) {
            self.annotation()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    /// `(lambdaExpression)=> lambdaExpression | <other>` where the other
    /// alternative is taken under `other_l1`/`other_l2`.
    fn lambda_or(
        &mut self,
        cur: &mut Pair,
        other_l2: &TokenSet,
        other: fn(&mut Self) -> R<Node>,
    ) -> R<()> {
        let l1 = self.la(1)?;
        let n = if SET_20.contains(l1)
            && SET_21.contains(self.la(2)?)
            && self.synpred(|p| p.lambda_expression().map(|_| ()))?
        {
            self.lambda_expression()?
        } else if SET_17.contains(l1) && other_l2.contains(self.la(2)?) {
            other(self)?
        } else {
            return self.fail();
        };
        self.add(cur, n);
        Ok(())
    }

    fn conditional_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.logical_or_expression()?;
        self.add(&mut cur, n);
        let t = self.la(1)?;
        if t == QUESTION {
            self.token_root(&mut cur, QUESTION)?;
            self.lambda_or(&mut cur, &SET_22, Self::assignment_expression)?;
            self.token(&mut cur, COLON)?;
            self.lambda_or(&mut cur, &SET_23, Self::conditional_expression)?;
        } else if !conditional_follow(t) {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn type_parameters(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let current_lt_level = if self.g0() { self.lt_counter } else { 0 };
        let lt = self.token(&mut cur, LT)?;
        if self.g0() {
            self.set_type(lt, GENERIC_START);
            self.lt_counter += 1;
        }
        let n = self.type_parameter()?;
        self.add(&mut cur, n);
        while self.la(1)? == COMMA {
            self.token(&mut cur, COMMA)?;
            let n = self.type_parameter()?;
            self.add(&mut cur, n);
        }
        let t = self.la(1)?;
        if matches!(t, GT | SR | BSR) {
            let n = self.type_arguments_or_parameters_end()?;
            self.add(&mut cur, n);
        } else if !(is_builtin(t)
            || matches!(
                t,
                IDENT
                    | LITERAL_extends
                    | LCURLY
                    | LITERAL_implements
                    | LPAREN
                    | AT
                    | LITERAL_record
                    | LITERAL_yield
            ))
        {
            return self.fail();
        }
        if self.g0() && self.gt_to_reconcile == 1 {
            let gt = self.emit_single_gt();
            self.add(&mut cur, gt);
        }
        if !self.are_lts_and_gts_balanced(current_lt_level) {
            return Err(ParseError::Semantic(
                "areLtsAndGtsBalanced(currentLtLevel)".into(),
            ));
        }
        self.wrap(&mut cur, TYPE_PARAMETERS, "TYPE_PARAMETERS");
        Ok(cur.root)
    }

    fn record_components_list(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token(&mut cur, LPAREN)?;
        let n = self.record_components()?;
        self.add(&mut cur, n);
        self.token(&mut cur, RPAREN)?;
        Ok(cur.root)
    }

    fn implements_clause(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        match self.la(1)? {
            LITERAL_implements => {
                let i = self.token_root(&mut cur, LITERAL_implements)?;
                self.set_type(i, IMPLEMENTS_CLAUSE);
                let n = self.class_or_interface_type(false)?;
                self.add(&mut cur, n);
                while self.la(1)? == COMMA {
                    self.token(&mut cur, COMMA)?;
                    let n = self.class_or_interface_type(false)?;
                    self.add(&mut cur, n);
                }
            }
            LCURLY => {}
            _ => return self.fail(),
        }
        Ok(cur.root)
    }

    fn record_body_declaration(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token(&mut cur, LCURLY)?;
        loop {
            let l1 = self.la(1)?;
            if SET_24.contains(l1)
                && SET_25.contains(self.la(2)?)
                && self.synpred(|p| p.compact_constructor_declaration().map(|_| ()))?
            {
                let n = self.compact_constructor_declaration()?;
                self.add(&mut cur, n);
            } else if SET_26.contains(l1) && SET_27.contains(self.la(2)?) {
                let n = self.field()?;
                self.add(&mut cur, n);
            } else if l1 == SEMI {
                self.token(&mut cur, SEMI)?;
            } else {
                break;
            }
        }
        self.token(&mut cur, RCURLY)?;
        self.wrap(&mut cur, OBJBLOCK, "OBJBLOCK");
        Ok(cur.root)
    }

    fn record_components(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_13.contains(l1)
            && SET_28.contains(self.la(2)?)
            && self.synpred(|p| p.record_component().map(|_| ()))?
        {
            let n = self.record_component()?;
            self.add(&mut cur, n);
            loop {
                let more = self.la(1)? == COMMA
                    && SET_13.contains(self.la(2)?)
                    && self.synpred(|p| {
                        p.match_(COMMA)?;
                        p.record_component().map(|_| ())
                    })?;
                if !more {
                    break;
                }
                self.token(&mut cur, COMMA)?;
                let n = self.record_component()?;
                self.add(&mut cur, n);
            }
            match self.la(1)? {
                COMMA => {
                    self.token(&mut cur, COMMA)?;
                    let n = self.record_component_variable_length()?;
                    self.add(&mut cur, n);
                }
                RPAREN => {}
                _ => return self.fail(),
            }
        } else if SET_13.contains(l1) && SET_29.contains(self.la(2)?) {
            let n = self.record_component_variable_length()?;
            self.add(&mut cur, n);
        } else if l1 != RPAREN {
            return self.fail();
        }
        self.wrap(&mut cur, RECORD_COMPONENTS, "RECORD_COMPONENTS");
        Ok(cur.root)
    }

    fn record_component(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let a = self.annotations()?;
        let t = self.type_spec(false)?;
        let i = self.id()?;
        let d = self.declarator_brackets(t)?;
        if self.g0() {
            let root = self.imaginary(RECORD_COMPONENT_DEF, "RECORD_COMPONENT_DEF");
            let ty = self.imaginary(TYPE, "TYPE");
            let ty = self.make(&[ty, d]);
            let tree = self.make(&[root, a, ty, i]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn record_component_variable_length(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let annotations = self.annotations()?;
        let spec = self.variable_length_parameter_type_spec()?;
        let ellipsis = self.token_alone(ELLIPSIS)?;
        let name = self.id()?;
        let brackets = self.declarator_brackets(spec)?;
        if self.g0() {
            let root = self.imaginary(RECORD_COMPONENT_DEF, "RECORD_COMPONENT_DEF");
            let ty = self.imaginary(TYPE, "TYPE");
            let ty = self.make(&[ty, brackets]);
            let tree = self.make(&[root, annotations, ty, ellipsis, name]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn declarator_brackets(&mut self, typ: Node) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if (l1 == LBRACK || l1 == AT) && SET_6.contains(self.la(2)?) {
            let mut an = None;
            if l1 == AT {
                an = self.annotations()?;
            } else if !(l1 == LBRACK && self.la(2)? == RBRACK) {
                return self.fail();
            }
            let lb = self.token_alone(LBRACK)?;
            self.set_type(lb, ARRAY_DECLARATOR);
            let rb = self.token_alone(RBRACK)?;
            let inner = self.make(&[lb, typ, an, rb]);
            let db = self.declarator_brackets(inner)?;
            self.set_tree(&mut cur, db);
        } else if SET_30.contains(l1) && SET_31.contains(self.la(2)?) {
            self.set_tree(&mut cur, typ);
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn compact_constructor_declaration(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let m = self.modifiers()?;
        let i = self.id()?;
        let c = self.constructor_body()?;
        if self.g0() {
            let root = self.imaginary(COMPACT_CTOR_DEF, "COMPACT_CTOR_DEF");
            let tree = self.make(&[root, m, i, c]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    /// `throwsClause` when `throws` is next; the method/ctor head's switch.
    fn optional_throws(&mut self, follow: &[u16]) -> R<Node> {
        let t = self.la(1)?;
        if t == LITERAL_throws {
            self.throws_clause()
        } else if follow.contains(&t) {
            Ok(None)
        } else {
            self.fail()
        }
    }

    /// `( compoundStatement | SEMI )` ending a method: the body and the
    /// semicolon, one of them `None`.
    fn method_body(&mut self) -> R<(Node, Node)> {
        match self.la(1)? {
            LCURLY => Ok((self.compound_statement()?, None)),
            SEMI => Ok((None, self.token_alone(SEMI)?)),
            _ => self.fail(),
        }
    }

    /// The `typeSpec` part of `field` and `enumConstantField`: a method
    /// definition or variable definitions.
    fn method_or_variables(&mut self, mods: Node, tp: Node, s6_optional: bool) -> R<Node> {
        let t = self.type_spec(false)?;
        let l1 = self.la(1)?;
        if SET_0.contains(l1) && self.la(2)? == LPAREN {
            let id = self.id()?;
            let lp = self.token_alone(LPAREN)?;
            let param = self.parameter_declaration_list()?;
            let rp = self.token_alone(RPAREN)?;
            let rt = self.declarator_brackets(t)?;
            let tc = self.optional_throws(&[SEMI, LCURLY])?;
            let (body, semi) = self.method_body()?;
            if !self.g0() {
                return Ok(None);
            }
            let root = self.imaginary(METHOD_DEF, "METHOD_DEF");
            let ty = self.imaginary(TYPE, "TYPE");
            let ty = self.make(&[ty, rt]);
            Ok(self.make(&[root, mods, tp, ty, id, lp, param, rp, tc, body, semi]))
        } else if SET_0.contains(l1)
            && (if s6_optional { &SET_37 } else { &SET_47 }).contains(self.la(2)?)
        {
            let v = self.variable_definitions(mods, t)?;
            let s6 = if s6_optional {
                let l1 = self.la(1)?;
                if l1 == SEMI && SET_38.contains(self.la(2)?) {
                    self.token_alone(SEMI)?
                } else if SET_38.contains(l1) && SET_39.contains(self.la(2)?) {
                    None
                } else {
                    return self.fail();
                }
            } else {
                self.token_alone(SEMI)?
            };
            if let (true, Some(v)) = (self.g0(), v) {
                self.ast.add_child(v, s6);
            }
            Ok(v)
        } else {
            self.fail()
        }
    }

    fn field(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_32.contains(l1) && SET_33.contains(self.la(2)?) {
            let mods = self.modifiers()?;
            let l1 = self.la(1)?;
            if SET_34.contains(l1) && SET_35.contains(self.la(2)?) {
                let td = self.type_definition_internal(mods)?;
                self.set_tree(&mut cur, td);
            } else if SET_36.contains(l1) && SET_14.contains(self.la(2)?) {
                let t = self.la(1)?;
                let tp = if t == LT {
                    self.type_parameters()?
                } else if is_class_type_start(t) || is_builtin(t) {
                    None
                } else {
                    return self.fail();
                };
                let l1 = self.la(1)?;
                if SET_0.contains(l1) && self.la(2)? == LPAREN {
                    let h = self.ctor_head()?;
                    let s = self.constructor_body()?;
                    if self.g0() {
                        let root = self.imaginary(CTOR_DEF, "CTOR_DEF");
                        let tree = self.make(&[root, mods, tp, h, s]);
                        self.set_tree(&mut cur, tree);
                    }
                } else if SET_13.contains(l1) && SET_28.contains(self.la(2)?) {
                    let tree = self.method_or_variables(mods, tp, true)?;
                    self.set_tree(&mut cur, tree);
                } else {
                    return self.fail();
                }
            } else {
                return self.fail();
            }
        } else if l1 == LITERAL_static && self.la(2)? == LCURLY {
            let si = self.token_alone(LITERAL_static)?;
            let s3 = self.compound_statement()?;
            if let (true, Some(si)) = (self.g0(), si) {
                self.ast.set_type(si, STATIC_INIT);
                self.ast.set_text(si, "STATIC_INIT");
                let tree = self.make(&[Some(si), s3]);
                self.set_tree(&mut cur, tree);
            }
        } else if l1 == LCURLY {
            let s4 = self.compound_statement()?;
            if self.g0() {
                let root = self.imaginary(INSTANCE_INIT, "INSTANCE_INIT");
                let tree = self.make(&[root, s4]);
                self.set_tree(&mut cur, tree);
            }
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn constructor_body(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let lc = self.token_root(&mut cur, LCURLY)?;
        self.set_type(lc, SLIST);
        let l1 = self.la(1)?;
        if SET_40.contains(l1)
            && SET_41.contains(self.la(2)?)
            && self.synpred(|p| p.explicit_constructor_invocation().map(|_| ()))?
        {
            let n = self.explicit_constructor_invocation()?;
            self.add(&mut cur, n);
        } else if !(SET_42.contains(l1) && SET_43.contains(self.la(2)?)) {
            return self.fail();
        }
        while SET_44.contains(self.la(1)?) {
            let n = self.statement()?;
            self.add(&mut cur, n);
        }
        self.token(&mut cur, RCURLY)?;
        Ok(cur.root)
    }

    fn super_class_clause(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        match self.la(1)? {
            LITERAL_extends => {
                let e = self.token_root(&mut cur, LITERAL_extends)?;
                self.set_type(e, EXTENDS_CLAUSE);
                let n = self.class_or_interface_type(false)?;
                self.add(&mut cur, n);
            }
            LCURLY | LITERAL_implements => {}
            _ => return self.fail(),
        }
        Ok(cur.root)
    }

    /// The `( member | SEMI )* RCURLY` of `classBlock` and the like.
    fn members(
        &mut self,
        cur: &mut Pair,
        starts: fn(u16) -> bool,
        member: fn(&mut Self) -> R<Node>,
    ) -> R<()> {
        loop {
            let t = self.la(1)?;
            if starts(t) {
                let n = member(self)?;
                self.add(cur, n);
            } else if t == SEMI {
                self.token(cur, SEMI)?;
            } else {
                break;
            }
        }
        self.token(cur, RCURLY)?;
        Ok(())
    }

    fn class_block(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token(&mut cur, LCURLY)?;
        self.members(&mut cur, is_field_start, Self::field)?;
        self.wrap(&mut cur, OBJBLOCK, "OBJBLOCK");
        Ok(cur.root)
    }

    fn interface_extends(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        match self.la(1)? {
            LITERAL_extends => {
                let e = self.token_root(&mut cur, LITERAL_extends)?;
                self.set_type(e, EXTENDS_CLAUSE);
                let n = self.class_or_interface_type(false)?;
                self.add(&mut cur, n);
                while self.la(1)? == COMMA {
                    self.token(&mut cur, COMMA)?;
                    let n = self.class_or_interface_type(false)?;
                    self.add(&mut cur, n);
                }
            }
            LCURLY => {}
            _ => return self.fail(),
        }
        Ok(cur.root)
    }

    fn enum_block(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token(&mut cur, LCURLY)?;
        let t = self.la(1)?;
        if is_class_type_start(t) {
            let n = self.enum_constant()?;
            self.add(&mut cur, n);
            while self.la(1)? == COMMA && SET_8.contains(self.la(2)?) {
                self.token(&mut cur, COMMA)?;
                let n = self.enum_constant()?;
                self.add(&mut cur, n);
            }
            match self.la(1)? {
                COMMA => {
                    self.token(&mut cur, COMMA)?;
                }
                SEMI | RCURLY => {}
                _ => return self.fail(),
            }
        } else if !matches!(t, SEMI | RCURLY) {
            return self.fail();
        }
        match self.la(1)? {
            SEMI => {
                self.token(&mut cur, SEMI)?;
                loop {
                    let t = self.la(1)?;
                    if is_field_start(t) {
                        let n = self.field()?;
                        self.add(&mut cur, n);
                    } else if t == SEMI {
                        self.token(&mut cur, SEMI)?;
                    } else {
                        break;
                    }
                }
            }
            RCURLY => {}
            _ => return self.fail(),
        }
        self.token(&mut cur, RCURLY)?;
        self.wrap(&mut cur, OBJBLOCK, "OBJBLOCK");
        Ok(cur.root)
    }

    fn annotation_block(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token(&mut cur, LCURLY)?;
        self.members(
            &mut cur,
            |t| is_field_start(t) && t != LCURLY && t != LT,
            Self::annotation_field,
        )?;
        self.wrap(&mut cur, OBJBLOCK, "OBJBLOCK");
        Ok(cur.root)
    }

    fn type_parameter(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_8.contains(l1) && SET_45.contains(self.la(2)?) && l1 == AT {
            let n = self.annotations()?;
            self.add(&mut cur, n);
        } else if !(SET_0.contains(l1) && SET_45.contains(self.la(2)?)) {
            return self.fail();
        }
        let n = self.id()?;
        self.add(&mut cur, n);
        let l1 = self.la(1)?;
        if l1 == LITERAL_extends && SET_8.contains(self.la(2)?) {
            let n = self.type_parameter_bounds()?;
            self.add(&mut cur, n);
        } else if !(SET_45.contains(l1) && SET_46.contains(self.la(2)?)) {
            return self.fail();
        }
        self.wrap(&mut cur, TYPE_PARAMETER, "TYPE_PARAMETER");
        Ok(cur.root)
    }

    fn type_parameter_bounds(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let e = self.token_root(&mut cur, LITERAL_extends)?;
        let n = self.class_or_interface_type(true)?;
        self.add(&mut cur, n);
        while self.la(1)? == BAND {
            let b = self.token(&mut cur, BAND)?;
            self.set_type(b, TYPE_EXTENSION_AND);
            let n = self.class_or_interface_type(true)?;
            self.add(&mut cur, n);
        }
        self.set_type(e, TYPE_UPPER_BOUNDS);
        Ok(cur.root)
    }

    fn annotation_field(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let mods = self.modifiers()?;
        let l1 = self.la(1)?;
        if SET_34.contains(l1) && SET_35.contains(self.la(2)?) {
            let td = self.type_definition_internal(mods)?;
            self.set_tree(&mut cur, td);
        } else if SET_13.contains(l1) && SET_28.contains(self.la(2)?) {
            let t = self.type_spec(false)?;
            let l1 = self.la(1)?;
            if SET_0.contains(l1) && self.la(2)? == LPAREN {
                let i = self.id()?;
                let lp = self.token_alone(LPAREN)?;
                let rp = self.token_alone(RPAREN)?;
                let rt = self.declarator_brackets(t)?;
                let d = match self.la(1)? {
                    LITERAL_default => self.annotation_default()?,
                    SEMI => None,
                    _ => return self.fail(),
                };
                let s = self.token_alone(SEMI)?;
                if self.g0() {
                    let root = self.imaginary(ANNOTATION_FIELD_DEF, "ANNOTATION_FIELD_DEF");
                    let ty = self.imaginary(TYPE, "TYPE");
                    let ty = self.make(&[ty, rt]);
                    let tree = self.make(&[root, mods, ty, i, lp, rp, d, s]);
                    self.set_tree(&mut cur, tree);
                }
            } else if SET_0.contains(l1) && SET_47.contains(self.la(2)?) {
                let v = self.variable_definitions(mods, t)?;
                let s6 = self.token_alone(SEMI)?;
                if let (true, Some(v)) = (self.g0(), v) {
                    self.ast.add_child(v, s6);
                }
                self.set_tree(&mut cur, v);
            } else {
                return self.fail();
            }
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn annotation_default(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, LITERAL_default)?;
        let n = self.annotation_member_value_initializer()?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn enum_constant(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let an = self.annotations()?;
        let i = self.id()?;
        let (mut l, mut args, mut r) = (None, None, None);
        match self.la(1)? {
            LPAREN => {
                l = self.token_alone(LPAREN)?;
                args = self.arg_list()?;
                r = self.token_alone(RPAREN)?;
            }
            SEMI | LCURLY | RCURLY | COMMA => {}
            _ => return self.fail(),
        }
        let b = match self.la(1)? {
            LCURLY => self.enum_constant_block()?,
            SEMI | RCURLY | COMMA => None,
            _ => return self.fail(),
        };
        if self.g0() {
            let root = self.imaginary(ENUM_CONSTANT_DEF, "ENUM_CONSTANT_DEF");
            let tree = self.make(&[root, an, i, l, args, r, b]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn arg_list(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_17.contains(l1) && l1 != RPAREN {
            let n = self.expression_list()?;
            self.add(&mut cur, n);
        } else if l1 == RPAREN {
            let elist = self.imaginary(ELIST, "ELIST");
            self.set_tree(&mut cur, elist);
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn enum_constant_block(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token(&mut cur, LCURLY)?;
        self.members(&mut cur, is_field_start, Self::enum_constant_field)?;
        self.wrap(&mut cur, OBJBLOCK, "OBJBLOCK");
        Ok(cur.root)
    }

    fn enum_constant_field(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if is_field_start(t) && t != LCURLY {
            let mods = self.modifiers()?;
            let l1 = self.la(1)?;
            if SET_34.contains(l1) && SET_35.contains(self.la(2)?) {
                let td = self.type_definition_internal(mods)?;
                self.set_tree(&mut cur, td);
            } else if SET_36.contains(l1) && SET_28.contains(self.la(2)?) {
                let t = self.la(1)?;
                let tp = if t == LT {
                    self.type_parameters()?
                } else if is_class_type_start(t) || is_builtin(t) {
                    None
                } else {
                    return self.fail();
                };
                let tree = self.method_or_variables(mods, tp, false)?;
                self.set_tree(&mut cur, tree);
            } else {
                return self.fail();
            }
        } else if t == LCURLY {
            let s4 = self.compound_statement()?;
            if self.g0() {
                let root = self.imaginary(INSTANCE_INIT, "INSTANCE_INIT");
                let tree = self.make(&[root, s4]);
                self.set_tree(&mut cur, tree);
            }
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn parameter_declaration_list(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_48.contains(l1)
            && SET_49.contains(self.la(2)?)
            && self.synpred(|p| p.parameter_declaration().map(|_| ()))?
        {
            let n = self.parameter_declaration()?;
            self.add(&mut cur, n);
            loop {
                let more = self.la(1)? == COMMA
                    && SET_48.contains(self.la(2)?)
                    && self.synpred(|p| {
                        p.match_(COMMA)?;
                        p.parameter_declaration().map(|_| ())
                    })?;
                if !more {
                    break;
                }
                self.token(&mut cur, COMMA)?;
                let n = self.parameter_declaration()?;
                self.add(&mut cur, n);
            }
            match self.la(1)? {
                COMMA => {
                    self.token(&mut cur, COMMA)?;
                    let n = self.variable_length_parameter_declaration()?;
                    self.add(&mut cur, n);
                }
                RPAREN => {}
                _ => return self.fail(),
            }
        } else if SET_50.contains(l1) && SET_51.contains(self.la(2)?) {
            let n = self.variable_length_parameter_declaration()?;
            self.add(&mut cur, n);
        } else if l1 != RPAREN {
            return self.fail();
        }
        self.wrap(&mut cur, PARAMETERS, "PARAMETERS");
        Ok(cur.root)
    }

    /// `({LA(1) == AT}? annotations | ) identifier` in `throwsClause`.
    fn throws_entry(&mut self, cur: &mut Pair) -> R<()> {
        let l1 = self.la(1)?;
        if SET_8.contains(l1) && SET_52.contains(self.la(2)?) && l1 == AT {
            let n = self.annotations()?;
            self.add(cur, n);
        } else if !(SET_0.contains(l1) && SET_53.contains(self.la(2)?)) {
            return self.fail();
        }
        let n = self.identifier()?;
        self.add(cur, n);
        Ok(())
    }

    fn throws_clause(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, LITERAL_throws)?;
        self.throws_entry(&mut cur)?;
        while self.la(1)? == COMMA {
            self.token(&mut cur, COMMA)?;
            self.throws_entry(&mut cur)?;
        }
        Ok(cur.root)
    }

    fn compound_statement(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let lc = self.token_root(&mut cur, LCURLY)?;
        self.set_type(lc, SLIST);
        while SET_44.contains(self.la(1)?) {
            let n = self.statement()?;
            self.add(&mut cur, n);
        }
        self.token(&mut cur, RCURLY)?;
        Ok(cur.root)
    }

    fn ctor_head(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.id()?;
        self.add(&mut cur, n);
        self.token(&mut cur, LPAREN)?;
        let n = self.parameter_declaration_list()?;
        self.add(&mut cur, n);
        self.token(&mut cur, RPAREN)?;
        let n = self.optional_throws(&[LCURLY])?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    /// `this(...)` / `super(...)` call: the keyword as root, then
    /// `( args ) ;`.
    fn constructor_call(&mut self, cur: &mut Pair, keyword: u16, ty: u16) -> R<()> {
        let k = self.token_root(cur, keyword)?;
        self.token(cur, LPAREN)?;
        let n = self.arg_list()?;
        self.add(cur, n);
        self.token(cur, RPAREN)?;
        self.token(cur, SEMI)?;
        self.set_type(k, ty);
        Ok(())
    }

    fn explicit_constructor_invocation(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_54.contains(l1) && SET_55.contains(self.la(2)?) {
            match self.la(1)? {
                LT => {
                    let n = self.type_arguments(false)?;
                    self.add(&mut cur, n);
                }
                LITERAL_this | LITERAL_super => {}
                _ => return self.fail(),
            }
            match self.la(1)? {
                LITERAL_this => self.constructor_call(&mut cur, LITERAL_this, CTOR_CALL)?,
                LITERAL_super => self.constructor_call(&mut cur, LITERAL_super, SUPER_CTOR_CALL)?,
                _ => return self.fail(),
            }
        } else if SET_56.contains(l1) && SET_57.contains(self.la(2)?) {
            let n = self.primary_expression()?;
            self.add(&mut cur, n);
            self.token(&mut cur, DOT)?;
            match self.la(1)? {
                LT => {
                    let n = self.type_arguments(false)?;
                    self.add(&mut cur, n);
                }
                LITERAL_super => {}
                _ => return self.fail(),
            }
            self.constructor_call(&mut cur, LITERAL_super, SUPER_CTOR_CALL)?;
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn statement(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        let n = if is_traditional_statement_start(t) {
            self.traditional_statement()?
        } else if t == ASSERT {
            self.assert_statement()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn primary_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if is_literal_constant(t) || t == TEXT_BLOCK_CONTENT {
            let n = self.constant()?;
            self.add(&mut cur, n);
        } else if matches!(
            t,
            LITERAL_true | LITERAL_false | LITERAL_this | LITERAL_null | LITERAL_super
        ) {
            self.token(&mut cur, t)?;
        } else if t == LITERAL_new {
            let n = self.new_expression()?;
            self.add(&mut cur, n);
        } else if t == LPAREN {
            self.token(&mut cur, LPAREN)?;
            self.lambda_or(&mut cur, &SET_58, Self::assignment_expression)?;
            self.token(&mut cur, RPAREN)?;
        } else {
            let l1 = t;
            if SET_13.contains(l1)
                && SET_59.contains(self.la(2)?)
                && self.synpred(|p| {
                    p.type_spec(false)?;
                    p.match_(DOUBLE_COLON)
                })?
            {
                let n = self.type_spec(false)?;
                self.add(&mut cur, n);
            } else if SET_0.contains(l1) && SET_59.contains(self.la(2)?) {
                let n = self.id()?;
                self.add(&mut cur, n);
            } else if is_builtin(l1) && SET_59.contains(self.la(2)?) {
                let n = self.built_in_type()?;
                self.add(&mut cur, n);
                while self.la(1)? == LBRACK && self.la(2)? == RBRACK {
                    let lbt = self.token_root(&mut cur, LBRACK)?;
                    self.set_type(lbt, ARRAY_DECLARATOR);
                    self.token(&mut cur, RBRACK)?;
                }
                let l1 = self.la(1)?;
                if l1 == DOT && self.la(2)? == LITERAL_class {
                    self.token_root(&mut cur, DOT)?;
                    self.token(&mut cur, LITERAL_class)?;
                } else if !(SET_59.contains(l1) && SET_60.contains(self.la(2)?)) {
                    return self.fail();
                }
            } else {
                return self.fail();
            }
        }
        Ok(cur.root)
    }

    fn variable_declarator(&mut self, mods: Node, t: Node) -> R<Node> {
        let mut cur = Pair::default();
        let id = self.id()?;
        let d = self.declarator_brackets(t)?;
        let v = self.var_initializer()?;
        if self.g0() {
            let root = self.imaginary(VARIABLE_DEF, "VARIABLE_DEF");
            let ty = self.imaginary(TYPE, "TYPE");
            let ty = self.make(&[ty, d]);
            let tree = self.make(&[root, mods, ty, id, v]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn var_initializer(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if t == ASSIGN {
            self.token_root(&mut cur, ASSIGN)?;
            let n = self.initializer()?;
            self.add(&mut cur, n);
        } else if !var_initializer_follow(t) {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn initializer(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        let n = if is_expression_start(t) {
            self.expression()?
        } else if t == LCURLY {
            self.array_initializer()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn array_initializer(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let lc = self.token_root(&mut cur, LCURLY)?;
        self.set_type(lc, ARRAY_INIT);
        let t = self.la(1)?;
        if is_expression_start(t) || t == LCURLY {
            let n = self.initializer()?;
            self.add(&mut cur, n);
            while self.la(1)? == COMMA && SET_15.contains(self.la(2)?) {
                self.token(&mut cur, COMMA)?;
                let n = self.initializer()?;
                self.add(&mut cur, n);
            }
        } else if !matches!(t, RCURLY | COMMA) {
            return self.fail();
        }
        match self.la(1)? {
            COMMA => {
                self.token(&mut cur, COMMA)?;
            }
            RCURLY => {}
            _ => return self.fail(),
        }
        self.token(&mut cur, RCURLY)?;
        Ok(cur.root)
    }

    fn expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_20.contains(l1)
            && SET_21.contains(self.la(2)?)
            && self.synpred(|p| p.lambda_expression().map(|_| ()))?
        {
            let n = self.lambda_expression()?;
            self.add(&mut cur, n);
        } else if SET_17.contains(l1) && SET_61.contains(self.la(2)?) && l1 != RPAREN {
            let n = self.assignment_expression()?;
            self.add(&mut cur, n);
            self.wrap(&mut cur, EXPR, "EXPR");
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn parameter_declaration(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let pm = self.parameter_modifier()?;
        let l1 = self.la(1)?;
        let t = if SET_13.contains(l1) && SET_62.contains(self.la(2)?) {
            self.type_spec(false)?
        } else if SET_63.contains(l1) && SET_64.contains(self.la(2)?) {
            None
        } else {
            return self.fail();
        };
        let id = self.parameter_ident()?;
        let pd = self.declarator_brackets(t)?;
        if self.g0() {
            let root = self.imaginary(PARAMETER_DEF, "PARAMETER_DEF");
            let ty = self.imaginary(TYPE, "TYPE");
            let ty = self.make(&[ty, pd]);
            let tree = self.make(&[root, pm, ty, id]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn variable_length_parameter_declaration(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let pm = self.parameter_modifier()?;
        let t = self.variable_length_parameter_type_spec()?;
        let td = self.token_alone(ELLIPSIS)?;
        let id = self.id()?;
        let pd = self.declarator_brackets(t)?;
        if self.g0() {
            let root = self.imaginary(PARAMETER_DEF, "PARAMETER_DEF");
            let ty = self.imaginary(TYPE, "TYPE");
            let ty = self.make(&[ty, pd]);
            let tree = self.make(&[root, pm, ty, td, id]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn parameter_modifier(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        while self.la(1)? == AT && SET_0.contains(self.la(2)?) {
            let n = self.annotation()?;
            self.add(&mut cur, n);
        }
        let t = self.la(1)?;
        if t == FINAL {
            self.token(&mut cur, FINAL)?;
        } else if !(is_builtin(t)
            || matches!(
                t,
                IDENT | LITERAL_this | AT | LITERAL_record | LITERAL_yield
            ))
        {
            return self.fail();
        }
        while self.la(1)? == AT && SET_0.contains(self.la(2)?) {
            let n = self.annotation()?;
            self.add(&mut cur, n);
        }
        self.wrap(&mut cur, MODIFIERS, "MODIFIERS");
        Ok(cur.root)
    }

    fn parameter_ident(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if t == LITERAL_this {
            self.token(&mut cur, LITERAL_this)?;
        } else if is_id(t) {
            let n = self.id()?;
            self.add(&mut cur, n);
            match self.la(1)? {
                DOT => {
                    self.token_root(&mut cur, DOT)?;
                    self.token(&mut cur, LITERAL_this)?;
                }
                LBRACK | COMMA | RPAREN | AT => {}
                _ => return self.fail(),
            }
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn catch_parameter_declaration(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let pm = self.parameter_modifier()?;
        let mct = self.multi_catch_types()?;
        let id = self.id()?;
        if self.g0() {
            let root = self.imaginary(PARAMETER_DEF, "PARAMETER_DEF");
            let ty = self.imaginary(TYPE, "TYPE");
            let ty = self.make(&[ty, mct]);
            let tree = self.make(&[root, pm, ty, id]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn multi_catch_types(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.type_spec(false)?;
        self.add(&mut cur, n);
        while self.la(1)? == BOR {
            self.token_root(&mut cur, BOR)?;
            let n = self.type_spec(false)?;
            self.add(&mut cur, n);
        }
        Ok(cur.root)
    }

    /// `"break"^ (id)? SEMI` and `"continue"^ (id)? SEMI`.
    fn jump_statement(&mut self, cur: &mut Pair, keyword: u16) -> R<()> {
        self.token_root(cur, keyword)?;
        let t = self.la(1)?;
        if is_id(t) {
            let n = self.id()?;
            self.add(cur, n);
        } else if t != SEMI {
            return self.fail();
        }
        self.token(cur, SEMI)?;
        Ok(())
    }

    /// `keyword^ LPAREN expression RPAREN`, the head of `if`, `while` and
    /// `synchronized`.
    fn parenthesized_head(&mut self, cur: &mut Pair, keyword: u16) -> R<()> {
        self.token_root(cur, keyword)?;
        self.token(cur, LPAREN)?;
        let n = self.expression()?;
        self.add(cur, n);
        self.token(cur, RPAREN)?;
        Ok(())
    }

    fn traditional_statement(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        match self.la(1)? {
            LCURLY => {
                let n = self.compound_statement()?;
                self.add(&mut cur, n);
            }
            LITERAL_if => {
                self.parenthesized_head(&mut cur, LITERAL_if)?;
                let n = self.statement()?;
                self.add(&mut cur, n);
                let l1 = self.la(1)?;
                if l1 == LITERAL_else && SET_44.contains(self.la(2)?) {
                    let n = self.else_statement()?;
                    self.add(&mut cur, n);
                } else if !(SET_65.contains(l1) && SET_66.contains(self.la(2)?)) {
                    return self.fail();
                }
            }
            LITERAL_for => {
                let n = self.for_statement()?;
                self.add(&mut cur, n);
            }
            LITERAL_while => {
                self.parenthesized_head(&mut cur, LITERAL_while)?;
                let n = self.statement()?;
                self.add(&mut cur, n);
            }
            LITERAL_do => {
                self.token_root(&mut cur, LITERAL_do)?;
                let n = self.statement()?;
                self.add(&mut cur, n);
                let w = self.token(&mut cur, LITERAL_while)?;
                self.set_type(w, DO_WHILE);
                self.token(&mut cur, LPAREN)?;
                let n = self.expression()?;
                self.add(&mut cur, n);
                self.token(&mut cur, RPAREN)?;
                self.token(&mut cur, SEMI)?;
            }
            LITERAL_break => self.jump_statement(&mut cur, LITERAL_break)?,
            LITERAL_continue => self.jump_statement(&mut cur, LITERAL_continue)?,
            LITERAL_return => {
                self.token_root(&mut cur, LITERAL_return)?;
                let t = self.la(1)?;
                if is_expression_start(t) {
                    let n = self.expression()?;
                    self.add(&mut cur, n);
                } else if t != SEMI {
                    return self.fail();
                }
                self.token(&mut cur, SEMI)?;
            }
            LITERAL_try => {
                let n = self.try_block()?;
                self.add(&mut cur, n);
            }
            LITERAL_throw => {
                let n = self.throw_statement()?;
                self.add(&mut cur, n);
            }
            SEMI => {
                let s = self.token(&mut cur, SEMI)?;
                self.set_type(s, EMPTY_STAT);
            }
            _ => self.other_statement(&mut cur)?,
        }
        Ok(cur.root)
    }

    /// The `default:` branch of `traditionalStatement`'s switch.
    fn other_statement(&mut self, cur: &mut Pair) -> R<()> {
        let l1 = self.la(1)?;
        if l1 == LITERAL_yield && SET_17.contains(self.la(2)?) && self.switch_block_depth > 0 {
            let n = self.yield_statement()?;
            self.add(cur, n);
            return Ok(());
        }
        if SET_67.contains(l1)
            && SET_68.contains(self.la(2)?)
            && self.synpred(|p| p.declaration().map(|_| ()))?
        {
            let n = self.declaration()?;
            self.add(cur, n);
            self.token(cur, SEMI)?;
            return Ok(());
        }
        if l1 == LITERAL_record && SET_0.contains(self.la(2)?) {
            let mods = self.imaginary(MODIFIERS, "MODIFIERS");
            let n = self.record_definition(mods)?;
            self.add(cur, n);
            return Ok(());
        }
        if l1 == LITERAL_switch && self.la(2)? == LPAREN {
            let n = self.switch_expression()?;
            self.add(cur, n);
            return Ok(());
        }
        if SET_17.contains(l1)
            && SET_69.contains(self.la(2)?)
            && self.la(2)? != COLON
            && self.synpred(|p| {
                if p.la(2)? == COLON {
                    return Err(ParseError::Semantic("LA(2) != COLON".into()));
                }
                p.expression()?;
                if p.la(1)? == SEMI {
                    p.match_(SEMI)?;
                }
                Ok(())
            })?
        {
            let n = self.expression()?;
            self.add(cur, n);
            let l1 = self.la(1)?;
            if l1 == SEMI && SET_65.contains(self.la(2)?) {
                self.token(cur, SEMI)?;
            } else if !(SET_65.contains(l1) && SET_66.contains(self.la(2)?)) {
                return self.fail();
            }
            return Ok(());
        }
        if SET_70.contains(l1) && SET_71.contains(self.la(2)?) {
            let m = self.modifiers()?;
            let n = self.class_definition(m)?;
            self.add(cur, n);
        } else if SET_0.contains(l1) && self.la(2)? == COLON {
            let n = self.id()?;
            self.add(cur, n);
            let c = self.token_root(cur, COLON)?;
            self.set_type(c, LABELED_STAT);
            let n = self.statement()?;
            self.add(cur, n);
        } else if l1 == LITERAL_synchronized && self.la(2)? == LPAREN {
            self.parenthesized_head(cur, LITERAL_synchronized)?;
            let n = self.compound_statement()?;
            self.add(cur, n);
        } else {
            return self.fail();
        }
        Ok(())
    }

    fn assert_statement(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, ASSERT)?;
        let n = self.expression()?;
        self.add(&mut cur, n);
        match self.la(1)? {
            COLON => {
                self.token(&mut cur, COLON)?;
                let n = self.expression()?;
                self.add(&mut cur, n);
            }
            SEMI => {}
            _ => return self.fail(),
        }
        self.token(&mut cur, SEMI)?;
        Ok(cur.root)
    }

    fn yield_statement(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l = self.token_alone(LITERAL_yield)?;
        let e = self.expression()?;
        let s = self.token_alone(SEMI)?;
        let tree = self.make(&[l, e, s]);
        self.set_tree(&mut cur, tree);
        Ok(cur.root)
    }

    fn switch_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.parenthesized_head(&mut cur, LITERAL_switch)?;
        let n = self.switch_block()?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn else_statement(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, LITERAL_else)?;
        let n = self.statement()?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn for_statement(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, LITERAL_for)?;
        self.token(&mut cur, LPAREN)?;
        let l1 = self.la(1)?;
        let n = if SET_72.contains(l1)
            && SET_73.contains(self.la(2)?)
            && self.synpred(|p| {
                p.for_init()?;
                p.match_(SEMI)
            })? {
            self.traditional_for_clause()?
        } else if SET_67.contains(l1) && SET_68.contains(self.la(2)?) {
            self.for_each_clause()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        self.token(&mut cur, RPAREN)?;
        let n = self.statement()?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn try_block(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, LITERAL_try)?;
        match self.la(1)? {
            LPAREN => {
                let n = self.resource_specification()?;
                self.add(&mut cur, n);
            }
            LCURLY => {}
            _ => return self.fail(),
        }
        let n = self.compound_statement()?;
        self.add(&mut cur, n);
        while self.la(1)? == LITERAL_catch {
            let n = self.handler()?;
            self.add(&mut cur, n);
        }
        let t = self.la(1)?;
        if t == LITERAL_finally {
            let n = self.finally_handler()?;
            self.add(&mut cur, n);
        } else if !try_follow(t) {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn throw_statement(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, LITERAL_throw)?;
        let n = self.expression()?;
        self.add(&mut cur, n);
        self.token(&mut cur, SEMI)?;
        Ok(cur.root)
    }

    fn for_init(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_67.contains(l1)
            && SET_68.contains(self.la(2)?)
            && self.synpred(|p| p.declaration().map(|_| ()))?
        {
            let n = self.declaration()?;
            self.add(&mut cur, n);
        } else if SET_17.contains(l1) && SET_74.contains(self.la(2)?) {
            let n = self.expression_list()?;
            self.add(&mut cur, n);
        } else if l1 != SEMI {
            return self.fail();
        }
        self.wrap(&mut cur, FOR_INIT, "FOR_INIT");
        Ok(cur.root)
    }

    fn traditional_for_clause(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.for_init()?;
        self.add(&mut cur, n);
        self.token(&mut cur, SEMI)?;
        let n = self.for_cond()?;
        self.add(&mut cur, n);
        self.token(&mut cur, SEMI)?;
        let n = self.for_iter()?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn for_each_clause(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.for_each_declarator()?;
        self.add(&mut cur, n);
        self.token(&mut cur, COLON)?;
        let n = self.expression()?;
        self.add(&mut cur, n);
        self.wrap(&mut cur, FOR_EACH_CLAUSE, "FOR_EACH_CLAUSE");
        Ok(cur.root)
    }

    fn for_cond(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if is_expression_start(t) {
            let n = self.expression()?;
            self.add(&mut cur, n);
        } else if t != SEMI {
            return self.fail();
        }
        self.wrap(&mut cur, FOR_CONDITION, "FOR_CONDITION");
        Ok(cur.root)
    }

    fn for_iter(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if is_expression_start(t) {
            let n = self.expression_list()?;
            self.add(&mut cur, n);
        } else if t != RPAREN {
            return self.fail();
        }
        self.wrap(&mut cur, FOR_ITERATOR, "FOR_ITERATOR");
        Ok(cur.root)
    }

    fn for_each_declarator(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let m = self.modifiers()?;
        let t = self.type_spec(false)?;
        let id = self.id()?;
        let d = self.declarator_brackets(t)?;
        if self.g0() {
            let root = self.imaginary(VARIABLE_DEF, "VARIABLE_DEF");
            let ty = self.imaginary(TYPE, "TYPE");
            let ty = self.make(&[ty, d]);
            let tree = self.make(&[root, m, ty, id]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn is_case_or_default(&self) -> R<bool> {
        let t = self.la(1)?;
        Ok(t == LITERAL_case || t == LITERAL_default)
    }

    fn switch_block(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.switch_block_depth += 1;
        self.token(&mut cur, LCURLY)?;
        let rules = self.is_case_or_default()?
            && SET_75.contains(self.la(2)?)
            && self.synpred(|p| {
                if !p.is_case_or_default()? {
                    return p.fail();
                }
                while p.is_case_or_default()? {
                    p.switch_rule()?;
                }
                Ok(())
            })?;
        if rules {
            if !self.is_case_or_default()? {
                return self.fail();
            }
            while self.is_case_or_default()? {
                let n = self.switch_rule()?;
                self.add(&mut cur, n);
            }
        } else {
            let groups = SET_76.contains(self.la(1)?)
                && SET_77.contains(self.la(2)?)
                && self.synpred(|p| {
                    while p.is_case_or_default()? {
                        p.cases_group()?;
                    }
                    Ok(())
                })?;
            if !groups {
                return self.fail();
            }
            while self.is_case_or_default()? {
                let n = self.cases_group()?;
                self.add(&mut cur, n);
            }
        }
        self.switch_block_depth -= 1;
        self.token(&mut cur, RCURLY)?;
        Ok(cur.root)
    }

    fn switch_rule(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let (mut se, mut sb, mut st) = (None, None, None);
        let applies = self.is_case_or_default()? && SET_75.contains(self.la(2)?);
        if applies && self.synpred(|p| p.switch_labeled_expression().map(|_| ()))? {
            se = self.switch_labeled_expression()?;
            self.add(&mut cur, se);
        } else if applies && self.synpred(|p| p.switch_labeled_block().map(|_| ()))? {
            sb = self.switch_labeled_block()?;
            self.add(&mut cur, sb);
        } else if applies && self.synpred(|p| p.switch_labeled_throw().map(|_| ()))? {
            st = self.switch_labeled_throw()?;
            self.add(&mut cur, st);
        } else {
            return self.fail();
        }
        if self.g0() {
            let root = self.imaginary(SWITCH_RULE, "SWITCH_RULE");
            let tree = self.make(&[root, se, sb, st]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn cases_group(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let mut count = 0;
        loop {
            if self.is_case_or_default()? && SET_78.contains(self.la(2)?) {
                let n = self.switch_label()?;
                self.add(&mut cur, n);
            } else if count >= 1 {
                break;
            } else {
                return self.fail();
            }
            count += 1;
        }
        let l1 = self.la(1)?;
        if SET_44.contains(l1) && SET_79.contains(self.la(2)?) {
            let n = self.case_s_list()?;
            self.add(&mut cur, n);
        } else if !(SET_76.contains(l1) && SET_77.contains(self.la(2)?)) {
            return self.fail();
        }
        self.wrap(&mut cur, CASE_GROUP, "CASE_GROUP");
        Ok(cur.root)
    }

    fn switch_label(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        match self.la(1)? {
            LITERAL_case => {
                self.token_root(&mut cur, LITERAL_case)?;
                let n = self.case_constant()?;
                self.add(&mut cur, n);
                while self.la(1)? == COMMA {
                    self.token(&mut cur, COMMA)?;
                    let n = self.case_constant()?;
                    self.add(&mut cur, n);
                }
            }
            LITERAL_default => {
                self.token_root(&mut cur, LITERAL_default)?;
            }
            _ => return self.fail(),
        }
        let t = self.la(1)?;
        if t == COLON {
            self.token(&mut cur, COLON)?;
        } else if !switch_label_follow(t) {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn case_s_list(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let mut count = 0;
        loop {
            let l1 = self.la(1)?;
            if SET_44.contains(l1) && SET_79.contains(self.la(2)?) && l1 != LITERAL_default {
                let n = self.statement()?;
                self.add(&mut cur, n);
            } else if count >= 1 {
                break;
            } else {
                return self.fail();
            }
            count += 1;
        }
        self.wrap(&mut cur, SLIST, "SLIST");
        Ok(cur.root)
    }

    /// `switchLabel LAMBDA <body>`.
    fn switch_labeled(&mut self, body: fn(&mut Self) -> R<Node>) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.switch_label()?;
        self.add(&mut cur, n);
        self.token(&mut cur, LAMBDA)?;
        let n = body(self)?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn switch_labeled_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.switch_label()?;
        self.add(&mut cur, n);
        self.token(&mut cur, LAMBDA)?;
        let n = self.expression()?;
        self.add(&mut cur, n);
        self.token(&mut cur, SEMI)?;
        Ok(cur.root)
    }

    fn switch_labeled_block(&mut self) -> R<Node> {
        self.switch_labeled(Self::compound_statement)
    }

    fn switch_labeled_throw(&mut self) -> R<Node> {
        self.switch_labeled(Self::throw_statement)
    }

    fn case_constant(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.conditional_expression()?;
        self.add(&mut cur, n);
        self.wrap(&mut cur, EXPR, "EXPR");
        Ok(cur.root)
    }

    fn expression_list(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.expression()?;
        self.add(&mut cur, n);
        while self.la(1)? == COMMA {
            self.token(&mut cur, COMMA)?;
            let n = self.expression()?;
            self.add(&mut cur, n);
        }
        self.wrap(&mut cur, ELIST, "ELIST");
        Ok(cur.root)
    }

    fn resource_specification(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token(&mut cur, LPAREN)?;
        let n = self.resources()?;
        self.add(&mut cur, n);
        match self.la(1)? {
            SEMI => {
                self.token(&mut cur, SEMI)?;
            }
            RPAREN => {}
            _ => return self.fail(),
        }
        self.token(&mut cur, RPAREN)?;
        self.wrap(&mut cur, RESOURCE_SPECIFICATION, "RESOURCE_SPECIFICATION");
        Ok(cur.root)
    }

    fn handler(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, LITERAL_catch)?;
        self.token(&mut cur, LPAREN)?;
        let n = self.catch_parameter_declaration()?;
        self.add(&mut cur, n);
        self.token(&mut cur, RPAREN)?;
        let n = self.compound_statement()?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn finally_handler(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, LITERAL_finally)?;
        let n = self.compound_statement()?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn resources(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.resource()?;
        self.add(&mut cur, n);
        while self.la(1)? == SEMI && SET_80.contains(self.la(2)?) {
            self.token(&mut cur, SEMI)?;
            let n = self.resource()?;
            self.add(&mut cur, n);
        }
        self.wrap(&mut cur, RESOURCES, "RESOURCES");
        Ok(cur.root)
    }

    fn resource(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if SET_50.contains(l1)
            && SET_81.contains(self.la(2)?)
            && self.synpred(|p| p.try_resource_declaration().map(|_| ()))?
        {
            let n = self.try_resource_declaration()?;
            self.add(&mut cur, n);
        } else if SET_56.contains(l1) && SET_82.contains(self.la(2)?) {
            while SET_56.contains(self.la(1)?) && SET_57.contains(self.la(2)?) {
                let n = self.primary_expression()?;
                self.add(&mut cur, n);
                self.token_root(&mut cur, DOT)?;
            }
            let n = self.id()?;
            self.add(&mut cur, n);
            self.wrap(&mut cur, RESOURCE, "RESOURCE");
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn try_resource_declaration(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let m = self.parameter_modifier()?;
        let t = self.type_spec(false)?;
        let (dm, dt) = self.dup_mods_and_type(m, t);
        let v = self.try_resource_declarator(dm, dt)?;
        self.set_tree(&mut cur, v);
        Ok(cur.root)
    }

    fn try_resource_declarator(&mut self, mods: Node, t: Node) -> R<Node> {
        let mut cur = Pair::default();
        let id = self.id()?;
        let d = self.declarator_brackets(t)?;
        let v = self.var_initializer()?;
        if self.g0() {
            let root = self.imaginary(RESOURCE, "RESOURCE");
            let ty = self.imaginary(TYPE, "TYPE");
            let ty = self.make(&[ty, d]);
            let tree = self.make(&[root, mods, ty, id, v]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn lambda_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.lambda_parameters()?;
        self.add(&mut cur, n);
        self.token_root(&mut cur, LAMBDA)?;
        let n = self.lambda_body()?;
        self.add(&mut cur, n);
        Ok(cur.root)
    }

    fn assignment_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.conditional_expression()?;
        self.add(&mut cur, n);
        let t = self.la(1)?;
        if is_assignment_operator(t) {
            self.token_root(&mut cur, t)?;
            self.lambda_or(&mut cur, &SET_61, Self::assignment_expression)?;
        } else if !assignment_follow(t) {
            return self.fail();
        }
        Ok(cur.root)
    }

    /// A left-associative binary level: `next ((op)^ next)*`, the loop
    /// taken when `is_op(LA(1))` and, where the generated test has one,
    /// `LA(2)` is in `l2`.
    fn binary_level(
        &mut self,
        next: fn(&mut Self) -> R<Node>,
        is_op: fn(u16) -> bool,
        l2: Option<&TokenSet>,
    ) -> R<Node> {
        let mut cur = Pair::default();
        let n = next(self)?;
        self.add(&mut cur, n);
        loop {
            let l1 = self.la(1)?;
            let taken = is_op(l1)
                && match l2 {
                    Some(set) => set.contains(self.la(2)?),
                    None => true,
                };
            if !taken {
                break;
            }
            self.token_root(&mut cur, l1)?;
            let n = next(self)?;
            self.add(&mut cur, n);
        }
        Ok(cur.root)
    }

    fn logical_or_expression(&mut self) -> R<Node> {
        self.binary_level(Self::logical_and_expression, |t| t == LOR, None)
    }

    fn logical_and_expression(&mut self) -> R<Node> {
        self.binary_level(Self::inclusive_or_expression, |t| t == LAND, None)
    }

    fn inclusive_or_expression(&mut self) -> R<Node> {
        self.binary_level(Self::exclusive_or_expression, |t| t == BOR, None)
    }

    fn exclusive_or_expression(&mut self) -> R<Node> {
        self.binary_level(Self::and_expression, |t| t == BXOR, None)
    }

    fn and_expression(&mut self) -> R<Node> {
        self.binary_level(Self::equality_expression, |t| t == BAND, None)
    }

    fn equality_expression(&mut self) -> R<Node> {
        self.binary_level(
            Self::relational_expression,
            |t| t == NOT_EQUAL || t == EQUAL,
            None,
        )
    }

    fn relational_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.shift_expression()?;
        self.add(&mut cur, n);
        let l1 = self.la(1)?;
        if l1 == LITERAL_instanceof && SET_13.contains(self.la(2)?) {
            self.token_root(&mut cur, LITERAL_instanceof)?;
            let l1 = self.la(1)?;
            let n = if SET_13.contains(l1)
                && SET_28.contains(self.la(2)?)
                && self.synpred(|p| {
                    p.type_spec(true)?;
                    p.match_(IDENT)
                })? {
                self.pattern_definition()?
            } else if SET_13.contains(l1) && SET_83.contains(self.la(2)?) {
                self.instanceof_type_spec()?
            } else {
                return self.fail();
            };
            self.add(&mut cur, n);
        } else if !(SET_77.contains(l1) && SET_60.contains(self.la(2)?)) {
            return self.fail();
        }
        loop {
            let l1 = self.la(1)?;
            if !(matches!(l1, LT | GT | LE | GE) && SET_17.contains(self.la(2)?)) {
                break;
            }
            self.token_root(&mut cur, l1)?;
            let n = self.shift_expression()?;
            self.add(&mut cur, n);
        }
        Ok(cur.root)
    }

    fn shift_expression(&mut self) -> R<Node> {
        self.binary_level(
            Self::additive_expression,
            |t| matches!(t, SL | SR | BSR),
            Some(&SET_17),
        )
    }

    fn pattern_definition(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let v = self.pattern_variable_definition()?;
        self.set_tree(&mut cur, v);
        Ok(cur.root)
    }

    fn instanceof_type_spec(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.type_spec(true)?;
        self.set_tree(&mut cur, t);
        Ok(cur.root)
    }

    fn pattern_variable_definition(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.type_spec(true)?;
        let i = self.token_alone(IDENT)?;
        if self.g0() {
            let root = self.imaginary(PATTERN_VARIABLE_DEF, "PATTERN_VARIABLE_DEF");
            let tree = self.make(&[root, t, i]);
            self.set_tree(&mut cur, tree);
        }
        Ok(cur.root)
    }

    fn additive_expression(&mut self) -> R<Node> {
        self.binary_level(
            Self::multiplicative_expression,
            |t| t == PLUS || t == MINUS,
            Some(&SET_17),
        )
    }

    fn multiplicative_expression(&mut self) -> R<Node> {
        self.binary_level(
            Self::unary_expression,
            |t| SET_84.contains(t),
            Some(&SET_17),
        )
    }

    fn unary_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        match t {
            INC | DEC | MINUS | PLUS => {
                let op = self.token_root(&mut cur, t)?;
                if t == MINUS {
                    self.set_type(op, UNARY_MINUS);
                } else if t == PLUS {
                    self.set_type(op, UNARY_PLUS);
                }
                let n = self.unary_expression()?;
                self.add(&mut cur, n);
            }
            _ if is_expression_start(t) => {
                let n = self.unary_expression_not_plus_minus()?;
                self.add(&mut cur, n);
            }
            _ => return self.fail(),
        }
        Ok(cur.root)
    }

    fn unary_expression_not_plus_minus(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if t == BNOT || t == LNOT {
            self.token_root(&mut cur, t)?;
            let n = self.unary_expression()?;
            self.add(&mut cur, n);
        } else if t == LITERAL_switch {
            let n = self.switch_expression()?;
            self.add(&mut cur, n);
        } else if is_expression_start(t) && !matches!(t, PLUS | MINUS | INC | DEC) {
            let n = self.cast_expression()?;
            self.add(&mut cur, n);
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    /// `lp:LPAREN^ {#lp.setType(TYPECAST);}`.
    fn typecast_open(&mut self, cur: &mut Pair) -> R<()> {
        let lp = self.token_root(cur, LPAREN)?;
        self.set_type(lp, TYPECAST);
        Ok(())
    }

    fn cast_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        if l1 == LPAREN
            && is_builtin(self.la(2)?)
            && self.synpred(|p| {
                p.match_(LPAREN)?;
                p.built_in_type_spec(true)?;
                p.match_(RPAREN)?;
                p.unary_expression().map(|_| ())
            })?
        {
            self.typecast_open(&mut cur)?;
            let n = self.built_in_type_spec(true)?;
            self.add(&mut cur, n);
            self.token(&mut cur, RPAREN)?;
            let n = self.unary_expression()?;
            self.add(&mut cur, n);
        } else if l1 == LPAREN
            && SET_8.contains(self.la(2)?)
            && self.synpred(|p| {
                p.match_(LPAREN)?;
                p.type_cast_parameters()?;
                p.match_(RPAREN)?;
                p.lambda_expression().map(|_| ())
            })?
        {
            self.typecast_open(&mut cur)?;
            let n = self.type_cast_parameters()?;
            self.add(&mut cur, n);
            self.token(&mut cur, RPAREN)?;
            let n = self.lambda_expression()?;
            self.add(&mut cur, n);
        } else if l1 == LPAREN
            && SET_8.contains(self.la(2)?)
            && self.synpred(|p| {
                p.match_(LPAREN)?;
                p.type_cast_parameters()?;
                p.match_(RPAREN)?;
                p.unary_expression_not_plus_minus().map(|_| ())
            })?
        {
            self.typecast_open(&mut cur)?;
            let n = self.type_cast_parameters()?;
            self.add(&mut cur, n);
            self.token(&mut cur, RPAREN)?;
            let n = self.unary_expression_not_plus_minus()?;
            self.add(&mut cur, n);
        } else if SET_56.contains(l1) && SET_23.contains(self.la(2)?) {
            let n = self.postfix_expression()?;
            self.add(&mut cur, n);
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn type_cast_parameters(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.class_type_spec(true)?;
        self.add(&mut cur, n);
        while self.la(1)? == BAND {
            let b = self.token(&mut cur, BAND)?;
            self.set_type(b, TYPE_EXTENSION_AND);
            let n = self.class_type_spec(true)?;
            self.add(&mut cur, n);
        }
        Ok(cur.root)
    }

    fn postfix_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let n = self.primary_expression()?;
        self.add(&mut cur, n);
        loop {
            match self.la(1)? {
                DOT => {
                    self.token_root(&mut cur, DOT)?;
                    let t = self.la(1)?;
                    if is_id(t) || matches!(t, LITERAL_this | LITERAL_super | LT) {
                        if t == LT {
                            let n = self.type_arguments(false)?;
                            self.add(&mut cur, n);
                        }
                        match self.la(1)? {
                            t if is_id(t) => {
                                let n = self.id()?;
                                self.add(&mut cur, n);
                            }
                            LITERAL_this => {
                                self.token(&mut cur, LITERAL_this)?;
                            }
                            LITERAL_super => {
                                self.token(&mut cur, LITERAL_super)?;
                            }
                            _ => return self.fail(),
                        }
                    } else if t == LITERAL_class {
                        self.token(&mut cur, LITERAL_class)?;
                    } else if t == LITERAL_new {
                        let n = self.new_expression()?;
                        self.add(&mut cur, n);
                    } else {
                        return self.fail();
                    }
                }
                DOUBLE_COLON => {
                    let dc = self.token_root(&mut cur, DOUBLE_COLON)?;
                    self.set_type(dc, METHOD_REF);
                    let t = self.la(1)?;
                    if t == LT {
                        let n = self.type_arguments(false)?;
                        self.add(&mut cur, n);
                    } else if !(is_id(t) || t == LITERAL_new) {
                        return self.fail();
                    }
                    match self.la(1)? {
                        t if is_id(t) => {
                            let n = self.id()?;
                            self.add(&mut cur, n);
                        }
                        LITERAL_new => {
                            self.token(&mut cur, LITERAL_new)?;
                        }
                        _ => return self.fail(),
                    }
                }
                _ => {
                    let l1 = self.la(1)?;
                    if l1 == LBRACK && self.la(2)? == RBRACK {
                        while self.la(1)? == LBRACK && self.la(2)? == RBRACK {
                            let lbc = self.token_root(&mut cur, LBRACK)?;
                            self.set_type(lbc, ARRAY_DECLARATOR);
                            self.token(&mut cur, RBRACK)?;
                        }
                        let l1 = self.la(1)?;
                        if l1 == DOT && self.la(2)? == LITERAL_class {
                            self.token_root(&mut cur, DOT)?;
                            self.token(&mut cur, LITERAL_class)?;
                        } else if !(SET_59.contains(l1) && SET_60.contains(self.la(2)?)) {
                            return self.fail();
                        }
                    } else if l1 == LBRACK && SET_17.contains(self.la(2)?) {
                        let lb = self.token_root(&mut cur, LBRACK)?;
                        self.set_type(lb, INDEX_OP);
                        let n = self.expression()?;
                        self.add(&mut cur, n);
                        self.token(&mut cur, RBRACK)?;
                    } else if l1 == LPAREN && SET_85.contains(self.la(2)?) {
                        let lp = self.token_root(&mut cur, LPAREN)?;
                        self.set_type(lp, METHOD_CALL);
                        let n = self.arg_list()?;
                        self.add(&mut cur, n);
                        self.token(&mut cur, RPAREN)?;
                    } else {
                        break;
                    }
                }
            }
        }
        let l1 = self.la(1)?;
        if l1 == INC && SET_77.contains(self.la(2)?) {
            let n = self.token_root(&mut cur, INC)?;
            self.set_type(n, POST_INC);
        } else if l1 == DEC && SET_77.contains(self.la(2)?) {
            let n = self.token_root(&mut cur, DEC)?;
            self.set_type(n, POST_DEC);
        } else if !(SET_77.contains(l1) && SET_60.contains(self.la(2)?)) {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn new_expression(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        self.token_root(&mut cur, LITERAL_new)?;
        let t = self.la(1)?;
        if t == LT {
            let n = self.type_arguments(false)?;
            self.add(&mut cur, n);
        } else if !(is_class_type_start(t) || is_builtin(t)) {
            return self.fail();
        }
        let n = self.type_()?;
        self.add(&mut cur, n);
        match self.la(1)? {
            LPAREN => {
                self.token(&mut cur, LPAREN)?;
                let n = self.arg_list()?;
                self.add(&mut cur, n);
                self.token(&mut cur, RPAREN)?;
                let l1 = self.la(1)?;
                if l1 == LCURLY && SET_38.contains(self.la(2)?) {
                    let n = self.class_block()?;
                    self.add(&mut cur, n);
                } else if !(SET_59.contains(l1) && SET_60.contains(self.la(2)?)) {
                    return self.fail();
                }
            }
            LBRACK | AT => {
                let n = self.new_array_declarator()?;
                self.add(&mut cur, n);
                let l1 = self.la(1)?;
                if l1 == LCURLY && SET_86.contains(self.la(2)?) {
                    let n = self.array_initializer()?;
                    self.add(&mut cur, n);
                } else if !(SET_59.contains(l1) && SET_60.contains(self.la(2)?)) {
                    return self.fail();
                }
            }
            _ => return self.fail(),
        }
        Ok(cur.root)
    }

    fn constant(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if is_literal_constant(t) {
            self.token(&mut cur, t)?;
        } else if t == TEXT_BLOCK_CONTENT {
            let n = self.text_block()?;
            self.add(&mut cur, n);
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn new_array_declarator(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let mut count = 0;
        loop {
            let l1 = self.la(1)?;
            if (l1 == LBRACK || l1 == AT) && SET_87.contains(self.la(2)?) {
                if l1 == AT {
                    let n = self.annotations()?;
                    self.add(&mut cur, n);
                } else if l1 != LBRACK {
                    return self.fail();
                }
                let lb = self.token_root(&mut cur, LBRACK)?;
                self.set_type(lb, ARRAY_DECLARATOR);
                let t = self.la(1)?;
                if is_expression_start(t) {
                    let n = self.expression()?;
                    self.add(&mut cur, n);
                } else if t != RBRACK {
                    return self.fail();
                }
                self.token(&mut cur, RBRACK)?;
            } else if count >= 1 {
                break;
            } else {
                return self.fail();
            }
            count += 1;
        }
        Ok(cur.root)
    }

    fn text_block(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let c = self.token_alone(TEXT_BLOCK_CONTENT)?;
        let end = self.token_alone(TEXT_BLOCK_LITERAL_END)?;
        let begin = self.token_alone(TEXT_BLOCK_LITERAL_BEGIN)?;
        let tree = self.make(&[begin, c, end]);
        self.set_tree(&mut cur, tree);
        Ok(cur.root)
    }

    fn lambda_parameters(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let t = self.la(1)?;
        if is_id(t) {
            let n = self.id()?;
            self.add(&mut cur, n);
        } else if t == LPAREN {
            self.token(&mut cur, LPAREN)?;
            let l1 = self.la(1)?;
            if SET_88.contains(l1) && SET_89.contains(self.la(2)?) {
                let n = self.parameter_declaration_list()?;
                self.add(&mut cur, n);
            } else if !(l1 == RPAREN && self.la(2)? == LAMBDA) {
                return self.fail();
            }
            self.token(&mut cur, RPAREN)?;
        } else {
            return self.fail();
        }
        Ok(cur.root)
    }

    fn lambda_body(&mut self) -> R<Node> {
        let mut cur = Pair::default();
        let l1 = self.la(1)?;
        let n = if SET_17.contains(l1) && SET_23.contains(self.la(2)?) {
            self.expression()?
        } else if SET_44.contains(l1) && SET_23.contains(self.la(2)?) {
            self.statement()?
        } else {
            return self.fail();
        };
        self.add(&mut cur, n);
        Ok(cur.root)
    }
}
