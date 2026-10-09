//! luacheck's parser: tokens to the AST of [`super::ast`], plus the
//! comments, code lines, line-ending kinds and hanging semicolons the later
//! stages read.

use std::collections::HashMap;

use super::ast::{Ast, Item, Node, NodeId, Range, Tag};
use super::decoder::Chars;
use super::lexer::{Lexer, Tok, sym};

/// A syntax error, with an optional second location (the opening token of
/// an unclosed block, a previous label).
#[derive(Clone, Debug)]
pub struct SyntaxError {
    pub msg: Vec<u8>,
    pub range: Range,
    pub prev: Option<Range>,
}

pub const fn syntax_error<T>(
    msg: Vec<u8>,
    range: Range,
    prev: Option<Range>,
) -> Result<T, SyntaxError> {
    Err(SyntaxError { msg, range, prev })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Comment,
    String,
}

pub struct Comment {
    pub contents: Vec<u8>,
    pub range: Range,
}

/// Everything a parse produces besides the AST.
pub struct ParseOutput {
    pub ast: Ast,
    pub root: NodeId,
    pub comments: Vec<Comment>,
    pub code_lines: HashMap<usize, bool>,
    pub line_endings: HashMap<usize, LineEnding>,
    pub hanging_semicolons: Vec<Range>,
}

/// The lexer's line tables, available whether or not the parse succeeded.
pub struct LineTables {
    pub line_offsets: Vec<Option<usize>>,
    pub line_lengths: Vec<Option<usize>>,
}

fn closing_of(token: Tok) -> Option<Tok> {
    let text = match token {
        Tok::Sym(s) => s.text().to_vec(),
        _ => return None,
    };
    Some(sym(match text.as_slice() {
        b"(" => ")",
        b"[" => "]",
        b"{" => "}",
        b"do" | b"if" | b"else" | b"elseif" | b"while" | b"for" | b"function" => "end",
        b"repeat" => "until",
        _ => return None,
    }))
}

fn is_closing_token(token: Tok) -> bool {
    token == Tok::Eof
        || token == sym("end")
        || token == sym("else")
        || token == sym("elseif")
        || token == sym("until")
}

fn unary_operator(token: Tok) -> Option<&'static str> {
    let Tok::Sym(s) = token else { return None };
    Some(match s.text() {
        b"not" => "not",
        b"-" => "unm",
        b"~" => "bnot",
        b"#" => "len",
        _ => return None,
    })
}

const UNARY_PRIORITY: u32 = 12;

fn binary_operator(token: Tok) -> Option<&'static str> {
    let Tok::Sym(s) = token else { return None };
    Some(match s.text() {
        b"+" => "add",
        b"-" => "sub",
        b"*" => "mul",
        b"%" => "mod",
        b"^" => "pow",
        b"/" => "div",
        b"//" => "idiv",
        b"&" => "band",
        b"|" => "bor",
        b"~" => "bxor",
        b"<<" => "shl",
        b">>" => "shr",
        b".." => "concat",
        b"~=" => "ne",
        b"==" => "eq",
        b"<" => "lt",
        b"<=" => "le",
        b">" => "gt",
        b">=" => "ge",
        b"and" => "and",
        b"or" => "or",
        _ => return None,
    })
}

fn compound_operator(token: Tok) -> Option<&'static str> {
    let Tok::Sym(s) = token else { return None };
    Some(match s.text() {
        b"+" => "add",
        b"-" => "sub",
        b"*" => "mul",
        b"%" => "mod",
        b"^" => "pow",
        b"/" => "div",
        b"//" => "idiv",
        b"&" => "band",
        b"|" => "bor",
        b"~" => "bxor",
        b"<<" => "shl",
        b">>" => "shr",
        b".." => "concat",
        _ => return None,
    })
}

fn priorities(op: &str) -> (u32, u32) {
    match op {
        "add" | "sub" => (10, 10),
        "mul" | "mod" | "div" | "idiv" => (11, 11),
        "pow" => (14, 13),
        "band" => (6, 6),
        "bor" => (4, 4),
        "bxor" => (5, 5),
        "shl" | "shr" => (7, 7),
        "concat" => (9, 8),
        "and" => (2, 2),
        "or" => (1, 1),
        _ => (3, 3),
    }
}

/// An opening token on the guesser's stack.
#[derive(Clone)]
struct OpeningToken {
    range: Range,
    token: Tok,
    closing_token: Option<Tok>,
    eligible: bool,
    indentation: i64,
    error_token: Option<Tok>,
    error_range: Option<Range>,
}

/// luacheck's `UnpairedTokenGuesser`: on a second parse, tracks opening
/// tokens and their indentation to point a missing `end`/`until` error at a
/// better place than end of file.
struct Guesser {
    error_offset: usize,
    error_closing_token: Tok,
    stack: Vec<OpeningToken>,
    guessed: Option<OpeningToken>,
}

pub struct Parser<'a> {
    pub lexer: Lexer<'a>,
    token: Tok,
    token_value: Option<Vec<u8>>,
    line: usize,
    offset: usize,
    end_offset: usize,
    ast: Ast,
    comments: Vec<Comment>,
    code_lines: HashMap<usize, bool>,
    line_endings: HashMap<usize, LineEnding>,
    hanging_semicolons: Vec<Range>,
    guesser: Option<Guesser>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a Chars) -> Self {
        Self {
            lexer: Lexer::new(src),
            token: Tok::Eof,
            token_value: None,
            line: 0,
            offset: 0,
            end_offset: 0,
            ast: Ast::default(),
            comments: Vec::new(),
            code_lines: HashMap::new(),
            line_endings: HashMap::new(),
            hanging_semicolons: Vec::new(),
            guesser: None,
        }
    }

    const fn range(&self) -> Range {
        Range {
            line: self.line,
            offset: self.offset,
            end_offset: self.end_offset,
        }
    }

    fn mark_line_endings(&mut self, kind: LineEnding) {
        for line in self.line..self.lexer.line {
            self.line_endings.insert(line, kind);
        }
    }

    fn skip_token(&mut self) -> Result<(), SyntaxError> {
        loop {
            match self.lexer.next_token() {
                Err(e) => {
                    self.line = e.line;
                    self.offset = e.offset;
                    self.end_offset = e.end_offset;
                    return syntax_error(e.msg, self.range(), None);
                }
                Ok(token) => {
                    self.token = token.tok;
                    self.token_value = token.value;
                    self.line = token.line;
                    self.offset = token.offset;
                    self.end_offset = self.lexer.offset - 1;
                    match token.tok {
                        Tok::ShortComment => {
                            self.comments.push(Comment {
                                contents: self.token_value.clone().unwrap_or_default(),
                                range: self.range(),
                            });
                            self.line_endings.insert(self.line, LineEnding::Comment);
                        }
                        Tok::LongComment => self.mark_line_endings(LineEnding::Comment),
                        tok => {
                            if tok != Tok::Eof {
                                self.mark_line_endings(LineEnding::String);
                                self.code_lines.insert(self.line, true);
                                self.code_lines.insert(self.lexer.line, true);
                            }
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    fn parse_error<T>(
        &self,
        msg: &[u8],
        prev: Option<Range>,
        token_prefix: Option<&str>,
        message_suffix: Option<&str>,
    ) -> Result<T, SyntaxError> {
        let mut token_repr = if self.token == Tok::Eof {
            b"<eof>".to_vec()
        } else {
            self.lexer
                .quoted_substring_or_line(self.line, self.offset, self.end_offset)
        };
        if let Some(prefix) = token_prefix {
            let mut with_prefix = prefix.as_bytes().to_vec();
            with_prefix.push(b' ');
            with_prefix.extend(token_repr);
            token_repr = with_prefix;
        }
        let mut message = msg.to_vec();
        message.extend_from_slice(b" near ");
        message.extend(token_repr);
        if let Some(suffix) = message_suffix {
            message.push(b' ');
            message.extend_from_slice(suffix.as_bytes());
        }
        syntax_error(message, self.range(), prev)
    }

    fn check_token(&self, token: Tok) -> Result<(), SyntaxError> {
        if self.token != token {
            let mut msg = b"expected ".to_vec();
            msg.extend(token.name());
            return self.parse_error(&msg, None, None, None);
        }
        Ok(())
    }

    fn check_and_skip_token(&mut self, token: Tok) -> Result<(), SyntaxError> {
        self.check_token(token)?;
        self.skip_token()
    }

    fn test_and_skip_token(&mut self, token: Tok) -> Result<bool, SyntaxError> {
        if self.token == token {
            self.skip_token()?;
            return Ok(true);
        }
        Ok(false)
    }

    fn get_indentation(&self, line: usize) -> i64 {
        let offset = self.lexer.line_offset(line);
        match self.lexer.src.find(b"^[ \t\x0b\x0c]*", offset) {
            Ok(Some(m)) => m.end as i64 - m.start as i64,
            _ => -1,
        }
    }

    fn on_block_start(&mut self, opening_range: Range, opening_token: Tok) {
        let indentation = self.get_indentation(opening_range.line);
        let Some(guesser) = self.guesser.as_mut() else {
            return;
        };
        let closing_token = closing_of(opening_token);
        guesser.stack.push(OpeningToken {
            range: opening_range,
            token: opening_token,
            closing_token,
            eligible: closing_token == Some(guesser.error_closing_token),
            indentation,
            error_token: None,
            error_range: None,
        });
    }

    fn guesser_check_token(&mut self) -> Result<(), SyntaxError> {
        let top = self.guesser.as_ref().and_then(|g| g.stack.last().cloned());
        if let Some(top) = top
            && top.eligible
            && self.line > top.range.line
        {
            let token_indentation = self.get_indentation(self.line);
            let mut set = false;
            if token_indentation < top.indentation {
                set = true;
            } else if token_indentation == top.indentation {
                let token = self.token;
                let is_if = top.token == sym("if") || top.token == sym("elseif");
                if Some(token) != top.closing_token
                    && (!is_if || (token != sym("elseif") && token != sym("else")))
                {
                    set = true;
                }
            }
            if set {
                let range = self.range();
                let token = self.token;
                let guesser = self.guesser.as_mut().expect("guesser");
                if guesser.guessed.is_none() {
                    let mut opening = top;
                    opening.error_token = Some(token);
                    opening.error_range = Some(range);
                    guesser.guessed = Some(opening);
                }
            }
        }

        let guesser = self.guesser.as_ref().expect("guesser");
        if self.offset == guesser.error_offset
            && let Some(guessed) = guesser.guessed.clone()
        {
            let error_range = guessed.error_range.expect("guessed error range");
            if error_range.offset != self.offset {
                self.line = error_range.line;
                self.offset = error_range.offset;
                self.end_offset = error_range.end_offset;
                self.token = guessed.error_token.expect("guessed error token");
                return self.missing_closing_token_error(
                    Some(guessed.range),
                    Some(guessed.token),
                    guessed.closing_token.expect("closing token"),
                    true,
                );
            }
        }
        Ok(())
    }

    fn on_block_end(&mut self) -> Result<(), SyntaxError> {
        self.guesser_check_token()?;
        let guesser = self.guesser.as_mut().expect("guesser");
        guesser.stack.pop();
        if guesser.stack.is_empty() {
            guesser.guessed = None;
        }
        Ok(())
    }

    fn missing_closing_token_error<T>(
        &self,
        opening_range: Option<Range>,
        opening_token: Option<Tok>,
        closing_token: Tok,
        is_guess: bool,
    ) -> Result<T, SyntaxError> {
        let mut msg = b"expected ".to_vec();
        msg.extend(closing_token.name());
        if let Some(range) = opening_range
            && range.line != self.line
        {
            msg.extend_from_slice(b" (to close ");
            msg.extend(opening_token.map(Tok::name).unwrap_or_default());
            msg.extend_from_slice(format!(" on line {})", range.line).as_bytes());
        }
        let (token_prefix, message_suffix) = if is_guess {
            (
                (self.token == closing_token).then_some("less indented"),
                Some("(indentation-based guess)"),
            )
        } else {
            (None, None)
        };
        self.parse_error(&msg, opening_range, token_prefix, message_suffix)
    }

    fn check_closing_token(
        &self,
        opening_range: Option<Range>,
        opening_token: Option<Tok>,
    ) -> Result<(), SyntaxError> {
        let closing_token = opening_token.and_then(closing_of).unwrap_or(Tok::Eof);
        if self.token == closing_token {
            return Ok(());
        }
        if (opening_token == Some(sym("if")) || opening_token == Some(sym("elseif")))
            && (self.token == sym("else") || self.token == sym("elseif"))
        {
            return Ok(());
        }
        if (closing_token == sym("end") || closing_token == sym("until")) && self.guesser.is_none()
        {
            return Err(self.guess(closing_token));
        }
        self.missing_closing_token_error(opening_range, opening_token, closing_token, false)
    }

    /// Reparses the source with a guesser looking for the opening token of
    /// the missing closing token; always ends in a syntax error.
    fn guess(&self, closing_token: Tok) -> SyntaxError {
        let mut parser = Parser::new(self.lexer.src);
        parser.guesser = Some(Guesser {
            error_offset: self.offset,
            error_closing_token: closing_token,
            stack: Vec::new(),
            guessed: None,
        });
        let result = parser
            .skip_token()
            .and_then(|()| parser.parse_block(None, None, None));
        match result {
            Err(e) => e,
            Ok(_) => panic!("No syntax error in second parse"),
        }
    }

    fn check_and_skip_closing_token(
        &mut self,
        opening_range: Range,
        opening_token: Tok,
    ) -> Result<(), SyntaxError> {
        self.check_closing_token(Some(opening_range), Some(opening_token))?;
        self.skip_token()
    }

    fn check_name(&self) -> Result<Vec<u8>, SyntaxError> {
        self.check_token(Tok::Name)?;
        Ok(self.token_value.clone().unwrap_or_default())
    }

    fn new_outer_node(&mut self, range: Range, tag: Tag, items: Vec<Item>) -> NodeId {
        self.ast.add(Node {
            tag: Some(tag),
            range: Some(range),
            items,
            ..Node::default()
        })
    }

    /// Gives an existing node a tag and the range from `start` to `end`.
    fn set_inner(&mut self, id: NodeId, start: Range, end_offset: usize, tag: Tag) {
        let node = &mut self.ast.nodes[id];
        node.tag = Some(tag);
        node.range = Some(Range {
            line: start.line,
            offset: start.offset,
            end_offset,
        });
    }

    fn new_inner_node(
        &mut self,
        start: Range,
        end_offset: usize,
        tag: Tag,
        items: Vec<Item>,
    ) -> NodeId {
        self.ast.add(Node {
            tag: Some(tag),
            range: Some(Range {
                line: start.line,
                offset: start.offset,
                end_offset,
            }),
            items,
            ..Node::default()
        })
    }

    fn end_of(&self, id: NodeId) -> usize {
        self.ast.range(id).end_offset
    }

    fn parse_expression_list(&mut self, list: &mut Vec<Item>) -> Result<(), SyntaxError> {
        loop {
            let expression = self.parse_expression(None)?;
            list.push(Item::Node(expression));
            if !self.test_and_skip_token(sym(","))? {
                return Ok(());
            }
        }
    }

    fn parse_id(&mut self, tag: Tag) -> Result<NodeId, SyntaxError> {
        let range = self.range();
        let name = self.check_name()?;
        let node = self.new_outer_node(range, tag, vec![Item::Str(name)]);
        self.skip_token()?;
        Ok(node)
    }

    fn atom(&mut self, tag: Tag) -> Result<NodeId, SyntaxError> {
        let range = self.range();
        let items = match self.token_value.clone() {
            Some(value) => vec![Item::Str(value)],
            None => Vec::new(),
        };
        let node = self.new_outer_node(range, tag, items);
        self.skip_token()?;
        Ok(node)
    }

    fn is_simple_expression_start(&self) -> bool {
        matches!(self.token, Tok::Number | Tok::String)
            || [
                sym("nil"),
                sym("true"),
                sym("false"),
                sym("..."),
                sym("{"),
                sym("function"),
            ]
            .contains(&self.token)
    }

    fn parse_literal(&mut self) -> Result<NodeId, SyntaxError> {
        match self.token {
            Tok::Number => self.atom(Tag::Number),
            Tok::String => self.atom(Tag::String),
            t if t == sym("nil") => self.atom(Tag::Nil),
            t if t == sym("true") => self.atom(Tag::True),
            t if t == sym("false") => self.atom(Tag::False),
            t if t == sym("...") => self.atom(Tag::Dots),
            t if t == sym("{") => self.parse_table(),
            t if t == sym("function") => {
                let function_range = self.range();
                self.skip_token()?;
                self.parse_function(function_range)
            }
            _ => unreachable!("not a literal"),
        }
    }

    fn parse_table(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        let mut items: Vec<Item> = Vec::new();
        self.skip_token()?;
        loop {
            if self.token == sym("}") {
                break;
            }
            let first_token_range = self.range();
            let mut key_node = None;
            if self.token == Tok::Name {
                let name = self.token_value.clone().unwrap_or_default();
                self.skip_token()?;
                if self.test_and_skip_token(sym("="))? {
                    key_node = Some(self.new_outer_node(
                        first_token_range,
                        Tag::String,
                        vec![Item::Str(name)],
                    ));
                } else {
                    // `name` begins an expression in the array part:
                    // backtrack the lexer to before it.
                    self.lexer.line = first_token_range.line;
                    self.lexer.offset = first_token_range.offset;
                    self.skip_token()?;
                }
            } else if self.token == sym("[") {
                self.skip_token()?;
                key_node = Some(self.parse_expression(None)?);
                self.check_and_skip_closing_token(first_token_range, sym("["))?;
                self.check_and_skip_token(sym("="))?;
            }
            let value_node = self.parse_expression(None)?;
            match key_node {
                Some(key) => {
                    let end = self.end_of(value_node);
                    let pair = self.new_inner_node(
                        first_token_range,
                        end,
                        Tag::Pair,
                        vec![Item::Node(key), Item::Node(value_node)],
                    );
                    items.push(Item::Node(pair));
                }
                None => items.push(Item::Node(value_node)),
            }
            if !(self.test_and_skip_token(sym(","))? || self.test_and_skip_token(sym(";"))?) {
                break;
            }
        }
        let node = self.new_inner_node(start_range, self.end_offset, Tag::Table, items);
        self.check_and_skip_closing_token(start_range, sym("{"))?;
        Ok(node)
    }

    fn parse_function(&mut self, function_range: Range) -> Result<NodeId, SyntaxError> {
        let paren_range = self.range();
        self.check_and_skip_token(sym("("))?;
        let mut args = Vec::new();
        if self.token != sym(")") {
            loop {
                if self.token == Tok::Name {
                    args.push(self.parse_id(Tag::Id)?);
                } else if self.token == sym("...") {
                    args.push(self.atom(Tag::Dots)?);
                    break;
                } else {
                    return self.parse_error(b"expected argument", None, None, None);
                }
                if !self.test_and_skip_token(sym(","))? {
                    break;
                }
            }
        }
        self.check_and_skip_closing_token(paren_range, sym("("))?;
        let args = self.ast.list(args);
        let body = self.parse_block(Some(function_range), Some(sym("function")), None)?;
        let end_range = self.range();
        self.skip_token()?;
        let node = self.new_inner_node(
            function_range,
            end_range.end_offset,
            Tag::Function,
            vec![Item::Node(args), Item::Node(body)],
        );
        self.ast.nodes[node].end_range = Some(end_range);
        Ok(node)
    }

    /// Call arguments after `(`, `{` or a string, appended to `items`.
    fn parse_call(
        &mut self,
        base: NodeId,
        tag: Tag,
        mut items: Vec<Item>,
    ) -> Result<NodeId, SyntaxError> {
        let base_range = self.ast.range(base);
        if self.token == sym("(") {
            let paren_range = self.range();
            self.skip_token()?;
            if self.token != sym(")") {
                self.parse_expression_list(&mut items)?;
            }
            let node = self.new_inner_node(base_range, self.end_offset, tag, items);
            self.check_and_skip_closing_token(paren_range, sym("("))?;
            Ok(node)
        } else {
            let arg = self.parse_literal()?;
            items.push(Item::Node(arg));
            let end = self.end_of(arg);
            Ok(self.new_inner_node(base_range, end, tag, items))
        }
    }

    fn is_call_start(&self) -> bool {
        self.token == sym("(") || self.token == sym("{") || self.token == Tok::String
    }

    fn parse_simple_expression(
        &mut self,
        kind: Option<&str>,
        no_literals: bool,
    ) -> Result<NodeId, SyntaxError> {
        let mut expression;
        if self.token == sym("(") {
            let paren_range = self.range();
            self.skip_token()?;
            let inner = self.parse_expression(None)?;
            expression = self.new_inner_node(
                paren_range,
                self.end_offset,
                Tag::Paren,
                vec![Item::Node(inner)],
            );
            self.check_and_skip_closing_token(paren_range, sym("("))?;
        } else if self.token == Tok::Name {
            expression = self.parse_id(Tag::Id)?;
        } else {
            if !self.is_simple_expression_start() || no_literals {
                let msg = format!("expected {}", kind.unwrap_or("expression"));
                return self.parse_error(msg.as_bytes(), None, None, None);
            }
            return self.parse_literal();
        }

        loop {
            if self.token == sym(".") {
                self.skip_token()?;
                let index = self.parse_id(Tag::String)?;
                let start = self.ast.range(expression);
                let end = self.end_of(index);
                expression = self.new_inner_node(
                    start,
                    end,
                    Tag::Index,
                    vec![Item::Node(expression), Item::Node(index)],
                );
            } else if self.token == sym("[") {
                let bracket_range = self.range();
                self.skip_token()?;
                let index = self.parse_expression(None)?;
                let start = self.ast.range(expression);
                let node = self.new_inner_node(
                    start,
                    self.end_offset,
                    Tag::Index,
                    vec![Item::Node(expression), Item::Node(index)],
                );
                self.check_and_skip_closing_token(bracket_range, sym("["))?;
                expression = node;
            } else if self.token == sym(":") {
                self.skip_token()?;
                let method_name = self.parse_id(Tag::String)?;
                if !self.is_call_start() {
                    return self.parse_error(b"expected method arguments", None, None, None);
                }
                expression = self.parse_call(
                    expression,
                    Tag::Invoke,
                    vec![Item::Node(expression), Item::Node(method_name)],
                )?;
            } else if self.is_call_start() {
                expression =
                    self.parse_call(expression, Tag::Call, vec![Item::Node(expression)])?;
            } else {
                return Ok(expression);
            }
        }
    }

    fn parse_subexpression(
        &mut self,
        limit: u32,
        kind: Option<&str>,
    ) -> Result<NodeId, SyntaxError> {
        let mut expression = if let Some(op) = unary_operator(self.token) {
            let operator_range = self.range();
            self.skip_token()?;
            let operand = self.parse_subexpression(UNARY_PRIORITY, None)?;
            let end = self.end_of(operand);
            self.new_inner_node(
                operator_range,
                end,
                Tag::Op,
                vec![Item::Str(op.as_bytes().to_vec()), Item::Node(operand)],
            )
        } else {
            self.parse_simple_expression(kind, false)?
        };
        while let Some(op) = binary_operator(self.token) {
            let (left, right) = priorities(op);
            if left <= limit {
                break;
            }
            self.skip_token()?;
            let subexpression = self.parse_subexpression(right, None)?;
            let start = self.ast.range(expression);
            let end = self.end_of(subexpression);
            expression = self.new_inner_node(
                start,
                end,
                Tag::Op,
                vec![
                    Item::Str(op.as_bytes().to_vec()),
                    Item::Node(expression),
                    Item::Node(subexpression),
                ],
            );
        }
        Ok(expression)
    }

    fn parse_expression(&mut self, kind: Option<&str>) -> Result<NodeId, SyntaxError> {
        self.parse_subexpression(0, kind)
    }

    fn parse_if(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        let mut items: Vec<Item> = Vec::new();
        let mut block_start_token = sym("if");
        let mut block_start_range = start_range;
        loop {
            items.push(Item::Node(self.parse_expression(Some("condition"))?));
            let branch_range = self.range();
            self.check_and_skip_token(sym("then"))?;
            let block = self.parse_block(
                Some(block_start_range),
                Some(block_start_token),
                Some(branch_range),
            )?;
            items.push(Item::Node(block));
            if self.token == sym("else") {
                let branch_range = self.range();
                block_start_token = sym("else");
                block_start_range = branch_range;
                self.skip_token()?;
                let block = self.parse_block(
                    Some(block_start_range),
                    Some(block_start_token),
                    Some(branch_range),
                )?;
                items.push(Item::Node(block));
                break;
            } else if self.token == sym("elseif") {
                block_start_token = sym("elseif");
                block_start_range = self.range();
                self.skip_token()?;
            } else {
                break;
            }
        }
        let node = self.new_inner_node(start_range, self.end_offset, Tag::If, items);
        self.skip_token()?;
        Ok(node)
    }

    fn parse_while(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        let condition = self.parse_expression(Some("condition"))?;
        self.check_and_skip_token(sym("do"))?;
        let block = self.parse_block(Some(start_range), Some(sym("while")), None)?;
        let node = self.new_inner_node(
            start_range,
            self.end_offset,
            Tag::While,
            vec![Item::Node(condition), Item::Node(block)],
        );
        self.skip_token()?;
        Ok(node)
    }

    fn parse_do(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        let block = self.parse_block(Some(start_range), Some(sym("do")), None)?;
        self.set_inner(block, start_range, self.end_offset, Tag::Do);
        self.skip_token()?;
        Ok(block)
    }

    fn parse_for(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        let first_var = self.parse_id(Tag::Id)?;
        let tag;
        let mut items: Vec<Item> = Vec::new();
        if self.token == sym("=") {
            tag = Tag::Fornum;
            self.skip_token()?;
            items.push(Item::Node(first_var));
            items.push(Item::Node(self.parse_expression(None)?));
            self.check_and_skip_token(sym(","))?;
            items.push(Item::Node(self.parse_expression(None)?));
            if self.test_and_skip_token(sym(","))? {
                items.push(Item::Node(self.parse_expression(None)?));
            }
            self.check_and_skip_token(sym("do"))?;
            items.push(Item::Node(self.parse_block(
                Some(start_range),
                Some(sym("for")),
                None,
            )?));
        } else if self.token == sym(",") || self.token == sym("in") {
            tag = Tag::Forin;
            let mut iter_vars = vec![first_var];
            while self.test_and_skip_token(sym(","))? {
                iter_vars.push(self.parse_id(Tag::Id)?);
            }
            let iter_vars = self.ast.list(iter_vars);
            items.push(Item::Node(iter_vars));
            self.check_and_skip_token(sym("in"))?;
            let mut exprs = Vec::new();
            self.parse_expression_list(&mut exprs)?;
            let exprs = self.ast.add(Node {
                items: exprs,
                ..Node::default()
            });
            items.push(Item::Node(exprs));
            self.check_and_skip_token(sym("do"))?;
            items.push(Item::Node(self.parse_block(
                Some(start_range),
                Some(sym("for")),
                None,
            )?));
        } else {
            return self.parse_error(b"expected '=', ',' or 'in'", None, None, None);
        }
        let node = self.new_inner_node(start_range, self.end_offset, tag, items);
        self.skip_token()?;
        Ok(node)
    }

    fn parse_repeat(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        let block = self.parse_block(Some(start_range), Some(sym("repeat")), None)?;
        self.skip_token()?;
        let condition = self.parse_expression(Some("condition"))?;
        let end = self.end_of(condition);
        Ok(self.new_inner_node(
            start_range,
            end,
            Tag::Repeat,
            vec![Item::Node(block), Item::Node(condition)],
        ))
    }

    fn parse_function_statement(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        let mut lhs = self.parse_id(Tag::Id)?;
        let mut implicit_self_range = None;
        while implicit_self_range.is_none() && (self.token == sym(".") || self.token == sym(":")) {
            if self.token == sym(":") {
                implicit_self_range = Some(self.range());
            }
            self.skip_token()?;
            let index = self.parse_id(Tag::String)?;
            let start = self.ast.range(lhs);
            let end = self.end_of(index);
            lhs = self.new_inner_node(
                start,
                end,
                Tag::Index,
                vec![Item::Node(lhs), Item::Node(index)],
            );
        }
        let function_node = self.parse_function(start_range)?;
        if let Some(range) = implicit_self_range {
            let self_arg = self.new_outer_node(range, Tag::Id, vec![Item::Str(b"self".to_vec())]);
            self.ast.nodes[self_arg].implicit = true;
            let args = self.ast.kid(function_node, 1);
            self.ast.nodes[args].items.insert(0, Item::Node(self_arg));
        }
        let lhs_list = self.ast.list(vec![lhs]);
        let rhs_list = self.ast.list(vec![function_node]);
        let end = self.end_of(function_node);
        Ok(self.new_inner_node(
            start_range,
            end,
            Tag::Set,
            vec![Item::Node(lhs_list), Item::Node(rhs_list)],
        ))
    }

    fn parse_local(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        if self.token == sym("function") {
            let function_range = self.range();
            self.skip_token()?;
            let var = self.parse_id(Tag::Id)?;
            let function_node = self.parse_function(function_range)?;
            let lhs = self.ast.list(vec![var]);
            let rhs = self.ast.list(vec![function_node]);
            let end = self.end_of(function_node);
            return Ok(self.new_inner_node(
                start_range,
                end,
                Tag::Localrec,
                vec![Item::Node(lhs), Item::Node(rhs)],
            ));
        }
        let mut lhs = Vec::new();
        loop {
            lhs.push(self.parse_id(Tag::Id)?);
            if self.token == sym("<") {
                self.skip_token()?;
                self.check_name()?;
                self.skip_token()?;
                self.check_and_skip_token(sym(">"))?;
            }
            if !self.test_and_skip_token(sym(","))? {
                break;
            }
        }
        let rhs = if self.test_and_skip_token(sym("="))? {
            let mut exprs = Vec::new();
            self.parse_expression_list(&mut exprs)?;
            Some(exprs)
        } else {
            None
        };
        let last = match &rhs {
            Some(exprs) => match exprs.last() {
                Some(Item::Node(n)) => *n,
                _ => unreachable!("expression list"),
            },
            None => *lhs.last().expect("lhs"),
        };
        let end = self.end_of(last);
        let lhs = self.ast.list(lhs);
        let mut items = vec![Item::Node(lhs)];
        if let Some(exprs) = rhs {
            let rhs = self.ast.add(Node {
                items: exprs,
                ..Node::default()
            });
            items.push(Item::Node(rhs));
        }
        Ok(self.new_inner_node(start_range, end, Tag::Local, items))
    }

    fn parse_label(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        let name = self.check_name()?;
        self.skip_token()?;
        let node = self.new_inner_node(
            start_range,
            self.end_offset,
            Tag::Label,
            vec![Item::Str(name)],
        );
        self.check_and_skip_token(sym("::"))?;
        Ok(node)
    }

    fn parse_return(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        if is_closing_token(self.token) || self.token == sym(";") {
            Ok(self.new_outer_node(start_range, Tag::Return, Vec::new()))
        } else {
            let mut returns = Vec::new();
            self.parse_expression_list(&mut returns)?;
            let last = match returns.last() {
                Some(Item::Node(n)) => *n,
                _ => unreachable!("expression list"),
            };
            let end = self.end_of(last);
            Ok(self.new_inner_node(start_range, end, Tag::Return, returns))
        }
    }

    fn parse_break(&mut self) -> Result<NodeId, SyntaxError> {
        let node = self.new_outer_node(self.range(), Tag::Break, Vec::new());
        self.skip_token()?;
        Ok(node)
    }

    fn parse_goto(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        self.skip_token()?;
        let name = self.check_name()?;
        let node = self.new_outer_node(start_range, Tag::Goto, vec![Item::Str(name)]);
        self.skip_token()?;
        Ok(node)
    }

    fn parse_expression_statement(&mut self) -> Result<NodeId, SyntaxError> {
        let start_range = self.range();
        let mut lhs: Option<Vec<NodeId>> = None;
        loop {
            let item_start_range = if lhs.is_some() {
                self.range()
            } else {
                start_range
            };
            let expected = if lhs.is_some() {
                "identifier or field"
            } else {
                "statement"
            };
            let primary = self.parse_simple_expression(Some(expected), true)?;
            if self.ast.is(primary, Tag::Paren) {
                let msg = format!("expected {expected} near '('");
                return syntax_error(msg.into_bytes(), item_start_range, None);
            }
            if self.ast.is(primary, Tag::Call) || self.ast.is(primary, Tag::Invoke) {
                if lhs.is_some() {
                    return self.parse_error(b"expected call or indexing", None, None, None);
                }
                return Ok(primary);
            }
            lhs.get_or_insert_with(Vec::new).push(primary);
            if !self.test_and_skip_token(sym(","))? {
                break;
            }
        }
        let lhs = lhs.expect("lhs");
        if let Some(op) = compound_operator(self.token) {
            let msg = format!("compound assignment not allowed on tuples near {op}=");
            if lhs.len() != 1 {
                return self.parse_error(msg.as_bytes(), None, None, None);
            }
            self.skip_token()?;
            self.check_and_skip_token(sym("="))?;
            let mut rhs = Vec::new();
            self.parse_expression_list(&mut rhs)?;
            if rhs.len() != 1 {
                return self.parse_error(msg.as_bytes(), None, None, None);
            }
            let first = match &rhs[0] {
                Item::Node(n) => *n,
                Item::Str(_) => unreachable!("expression"),
            };
            let end = self.end_of(first);
            let lhs = self.ast.list(lhs);
            let rhs = self.ast.add(Node {
                items: rhs,
                ..Node::default()
            });
            Ok(self.new_inner_node(
                start_range,
                end,
                Tag::OpSet,
                vec![
                    Item::Node(lhs),
                    Item::Node(rhs),
                    Item::Str(op.as_bytes().to_vec()),
                ],
            ))
        } else {
            self.check_and_skip_token(sym("="))?;
            let mut rhs = Vec::new();
            self.parse_expression_list(&mut rhs)?;
            let last = match rhs.last() {
                Some(Item::Node(n)) => *n,
                _ => unreachable!("expression list"),
            };
            let end = self.end_of(last);
            let lhs = self.ast.list(lhs);
            let rhs = self.ast.add(Node {
                items: rhs,
                ..Node::default()
            });
            Ok(self.new_inner_node(
                start_range,
                end,
                Tag::Set,
                vec![Item::Node(lhs), Item::Node(rhs)],
            ))
        }
    }

    fn parse_statement(&mut self) -> Result<NodeId, SyntaxError> {
        let Tok::Sym(s) = self.token else {
            return self.parse_expression_statement();
        };
        match s.text() {
            b"if" => self.parse_if(),
            b"while" => self.parse_while(),
            b"do" => self.parse_do(),
            b"for" => self.parse_for(),
            b"repeat" => self.parse_repeat(),
            b"function" => self.parse_function_statement(),
            b"local" => self.parse_local(),
            b"::" => self.parse_label(),
            b"return" => self.parse_return(),
            b"break" => self.parse_break(),
            b"goto" => self.parse_goto(),
            _ => self.parse_expression_statement(),
        }
    }

    /// Parses statements until a closing token. `block` is the range the
    /// block list carries (the `then`/`else` token of an `if` branch).
    fn parse_block(
        &mut self,
        opening_range: Option<Range>,
        opening_token: Option<Tok>,
        block: Option<Range>,
    ) -> Result<NodeId, SyntaxError> {
        if self.guesser.is_some()
            && let (Some(range), Some(token)) = (opening_range, opening_token)
        {
            self.on_block_start(range, token);
        }
        let block_id = self.ast.add(Node {
            range: block,
            ..Node::default()
        });
        let mut after_statement = false;
        while !is_closing_token(self.token) {
            if self.token == sym(";") {
                if !after_statement {
                    self.hanging_semicolons.push(self.range());
                }
                self.skip_token()?;
                after_statement = false;
            } else {
                if self.guesser.is_some() {
                    self.guesser_check_token()?;
                }
                let statement = self.parse_statement()?;
                after_statement = true;
                self.ast.nodes[block_id].items.push(Item::Node(statement));
                if self.ast.is(statement, Tag::Return) {
                    self.test_and_skip_token(sym(";"))?;
                    break;
                }
            }
        }
        if self.guesser.is_some() && opening_token.is_some() {
            self.on_block_end()?;
        }
        self.check_closing_token(opening_range, opening_token)?;
        Ok(block_id)
    }
}

/// Parses a decoded source.
pub fn parse(src: &Chars) -> (Result<ParseOutput, SyntaxError>, LineTables) {
    let mut parser = Parser::new(src);
    let result = parser
        .skip_token()
        .and_then(|()| parser.parse_block(None, None, None));
    let tables = LineTables {
        line_offsets: std::mem::take(&mut parser.lexer.line_offsets),
        line_lengths: std::mem::take(&mut parser.lexer.line_lengths),
    };
    let result = result.map(|root| ParseOutput {
        ast: std::mem::take(&mut parser.ast),
        root,
        comments: std::mem::take(&mut parser.comments),
        code_lines: std::mem::take(&mut parser.code_lines),
        line_endings: std::mem::take(&mut parser.line_endings),
        hanging_semicolons: std::mem::take(&mut parser.hanging_semicolons),
    });
    (result, tables)
}
