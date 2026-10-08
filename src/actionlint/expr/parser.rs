//! The expression syntax tree and parser, a port of actionlint's
//! `expr_ast.go` and `expr_parser.go`.

use super::ExprError;
use super::lexer::{Lexer, Token, TokenKind};
use crate::actionlint::yaml::{go_parse_float, go_parse_int_base0, go_quote};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareKind {
    Less,
    LessEq,
    Greater,
    GreaterEq,
    Eq,
    NotEq,
}

impl CompareKind {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Less => "<",
            Self::LessEq => "<=",
            Self::Greater => ">",
            Self::GreaterEq => ">=",
            Self::Eq => "==",
            Self::NotEq => "!=",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalKind {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprNode {
    /// A context access; the name is lower-cased, the token has the
    /// original spelling.
    Variable {
        name: String,
        tok: Token,
    },
    Null(Token),
    Bool(bool, Token),
    Int(i64, Token),
    Float(f64, Token),
    Str(String, Token),
    /// `receiver.property`, the property lower-cased.
    ObjectDeref {
        receiver: Box<Self>,
        property: String,
    },
    /// `receiver.*`
    ArrayDeref {
        receiver: Box<Self>,
    },
    /// `operand[index]`
    IndexAccess {
        operand: Box<Self>,
        index: Box<Self>,
    },
    Not {
        operand: Box<Self>,
        tok: Token,
    },
    Compare {
        kind: CompareKind,
        left: Box<Self>,
        right: Box<Self>,
    },
    Logical {
        kind: LogicalKind,
        left: Box<Self>,
        right: Box<Self>,
    },
    /// `callee(args...)`, the callee as written.
    FuncCall {
        callee: String,
        args: Vec<Self>,
        tok: Token,
    },
}

impl ExprNode {
    /// The node's first token, which gives its position.
    pub fn token(&self) -> &Token {
        match self {
            Self::Variable { tok, .. }
            | Self::Null(tok)
            | Self::Bool(_, tok)
            | Self::Int(_, tok)
            | Self::Float(_, tok)
            | Self::Str(_, tok)
            | Self::Not { tok, .. }
            | Self::FuncCall { tok, .. } => tok,
            Self::ObjectDeref { receiver, .. } | Self::ArrayDeref { receiver } => receiver.token(),
            Self::IndexAccess { operand, .. } => operand.token(),
            Self::Compare { left, .. } | Self::Logical { left, .. } => left.token(),
        }
    }
}

pub const fn error_at_token(t: &Token, message: String) -> ExprError {
    ExprError {
        message,
        offset: t.offset,
        line: t.line,
        column: t.column,
    }
}

pub struct Parser {
    cur: Token,
    lexer: Lexer,
    err: Option<ExprError>,
}

impl Parser {
    /// Parses one expression from `src`, which must end with `}}` (the
    /// lexer's end marker). On success the lexer has stopped right after
    /// that marker; `offset()` tells where.
    pub fn parse(src: &str) -> (Result<ExprNode, ExprError>, usize) {
        let mut lexer = Lexer::new(src);
        let cur = lexer.next_token();
        let mut p = Self {
            cur,
            lexer,
            err: None,
        };
        let root = p.parse_logical_or();
        if let Some(err) = p.error_value() {
            return (Err(err), p.lexer.offset());
        }
        let Some(root) = root else {
            // Unreachable in practice: a missing node always comes with an error.
            return (
                Err(error_at_token(
                    &p.cur,
                    "failed to parse expression".to_string(),
                )),
                p.lexer.offset(),
            );
        };
        if p.cur.kind != TokenKind::End {
            let mut names = vec![go_quote(p.cur.kind.name())];
            let mut count = 1;
            loop {
                let t = p.lexer.next_token();
                if t.kind == TokenKind::End {
                    break;
                }
                names.push(go_quote(t.kind.name()));
                count += 1;
            }
            p.error(format!(
                "parser did not reach end of input after parsing the expression. {count} remaining token(s) in the input: {}",
                names.join(", ")
            ));
            return (
                Err(p.err.clone().expect("error was just set")),
                p.lexer.offset(),
            );
        }
        (Ok(root), p.lexer.offset())
    }

    fn error_value(&self) -> Option<ExprError> {
        if let Some(e) = self.lexer.err() {
            return Some(e.clone());
        }
        self.err.clone()
    }

    fn error(&mut self, message: String) {
        if self.err.is_none() {
            self.err = Some(error_at_token(&self.cur, message));
        }
    }

    fn unexpected(&mut self, where_: &str, expected: &[TokenKind]) {
        if self.err.is_some() {
            return;
        }
        let names: Vec<String> = expected.iter().map(|k| go_quote(k.name())).collect();
        let what = if self.cur.kind == TokenKind::End {
            "end of input".to_string()
        } else {
            format!("token {}", go_quote(self.cur.kind.name()))
        };
        let msg = format!(
            "unexpected {what} while parsing {where_}. expecting {}",
            names.join(", ")
        );
        self.error(msg);
    }

    fn next(&mut self) -> Token {
        let next = self.lexer.next_token();
        std::mem::replace(&mut self.cur, next)
    }

    fn parse_ident(&mut self) -> Option<ExprNode> {
        let ident = self.next();
        if self.cur.kind == TokenKind::LeftParen {
            self.next();
            let mut args = Vec::new();
            if self.cur.kind == TokenKind::RightParen {
                self.next();
            } else {
                loop {
                    let arg = self.parse_logical_or()?;
                    args.push(arg);
                    match self.cur.kind {
                        TokenKind::Comma => {
                            self.next();
                        }
                        TokenKind::RightParen => {
                            self.next();
                            break;
                        }
                        _ => {
                            self.unexpected(
                                "arguments of function call",
                                &[TokenKind::Comma, TokenKind::RightParen],
                            );
                            return None;
                        }
                    }
                }
            }
            return Some(ExprNode::FuncCall {
                callee: ident.value.clone(),
                args,
                tok: ident,
            });
        }
        Some(match ident.value.as_str() {
            "null" => ExprNode::Null(ident),
            "true" => ExprNode::Bool(true, ident),
            "false" => ExprNode::Bool(false, ident),
            _ => ExprNode::Variable {
                name: ident.value.to_lowercase(),
                tok: ident,
            },
        })
    }

    fn parse_nested_expr(&mut self) -> Option<ExprNode> {
        self.next();
        let nested = self.parse_logical_or()?;
        if self.cur.kind == TokenKind::RightParen {
            self.next();
        } else {
            self.unexpected(
                "closing ')' of nested expression (...)",
                &[TokenKind::RightParen],
            );
            return None;
        }
        Some(nested)
    }

    fn parse_int(&mut self) -> Option<ExprNode> {
        let t = self.cur.clone();
        let parsed = go_parse_int_base0(&t.value).filter(|v| i32::try_from(*v).is_ok());
        let Some(i) = parsed else {
            let reason = if go_parse_int_base0(&t.value).is_some() {
                "value out of range"
            } else {
                "invalid syntax"
            };
            self.error(format!(
                "parsing invalid integer literal {}: strconv.ParseInt: parsing {}: {reason}",
                go_quote(&t.value),
                go_quote(&t.value)
            ));
            return None;
        };
        self.next();
        Some(ExprNode::Int(i, t))
    }

    fn parse_float(&mut self) -> Option<ExprNode> {
        let t = self.cur.clone();
        let Some(f) = go_parse_float(&t.value) else {
            self.error(format!(
                "parsing invalid float literal {}: strconv.ParseFloat: parsing {}: invalid syntax",
                go_quote(&t.value),
                go_quote(&t.value)
            ));
            return None;
        };
        self.next();
        Some(ExprNode::Float(f, t))
    }

    fn parse_string(&mut self) -> ExprNode {
        let t = self.next();
        let inner = &t.value[1..t.value.len() - 1];
        let s = inner.replace("''", "'");
        ExprNode::Str(s, t)
    }

    fn parse_primary_expr(&mut self) -> Option<ExprNode> {
        match self.cur.kind {
            TokenKind::Ident => self.parse_ident(),
            TokenKind::LeftParen => self.parse_nested_expr(),
            TokenKind::Int => self.parse_int(),
            TokenKind::Float => self.parse_float(),
            TokenKind::String => Some(self.parse_string()),
            _ => {
                self.unexpected(
                    "variable access, function call, null, bool, int, float or string",
                    &[
                        TokenKind::Ident,
                        TokenKind::LeftParen,
                        TokenKind::Int,
                        TokenKind::Float,
                        TokenKind::String,
                    ],
                );
                None
            }
        }
    }

    fn parse_postfix_op(&mut self) -> Option<ExprNode> {
        let mut ret = self.parse_primary_expr()?;
        loop {
            match self.cur.kind {
                TokenKind::Dot => {
                    self.next();
                    match self.cur.kind {
                        TokenKind::Star => {
                            self.next();
                            ret = ExprNode::ArrayDeref {
                                receiver: Box::new(ret),
                            };
                        }
                        TokenKind::Ident => {
                            let t = self.next();
                            ret = ExprNode::ObjectDeref {
                                receiver: Box::new(ret),
                                property: t.value.to_lowercase(),
                            };
                        }
                        _ => {
                            self.unexpected(
                                "object property dereference like 'a.b' or array element dereference like 'a.*'",
                                &[TokenKind::Ident, TokenKind::Star],
                            );
                            return None;
                        }
                    }
                }
                TokenKind::LeftBracket => {
                    self.next();
                    let idx = self.parse_logical_or()?;
                    ret = ExprNode::IndexAccess {
                        operand: Box::new(ret),
                        index: Box::new(idx),
                    };
                    if self.cur.kind != TokenKind::RightBracket {
                        self.unexpected(
                            "closing bracket ']' for index access",
                            &[TokenKind::RightBracket],
                        );
                        return None;
                    }
                    self.next();
                }
                _ => return Some(ret),
            }
        }
    }

    fn parse_prefix_op(&mut self) -> Option<ExprNode> {
        if self.cur.kind != TokenKind::Not {
            return self.parse_postfix_op();
        }
        let t = self.next();
        let o = self.parse_prefix_op()?;
        Some(ExprNode::Not {
            operand: Box::new(o),
            tok: t,
        })
    }

    fn parse_compare_bin_op(&mut self) -> Option<ExprNode> {
        let l = self.parse_prefix_op()?;
        let kind = match self.cur.kind {
            TokenKind::Less => CompareKind::Less,
            TokenKind::LessEq => CompareKind::LessEq,
            TokenKind::Greater => CompareKind::Greater,
            TokenKind::GreaterEq => CompareKind::GreaterEq,
            TokenKind::Eq => CompareKind::Eq,
            TokenKind::NotEq => CompareKind::NotEq,
            _ => return Some(l),
        };
        self.next();
        let r = self.parse_compare_bin_op()?;
        Some(ExprNode::Compare {
            kind,
            left: Box::new(l),
            right: Box::new(r),
        })
    }

    fn parse_logical_and(&mut self) -> Option<ExprNode> {
        let l = self.parse_compare_bin_op()?;
        if self.cur.kind != TokenKind::And {
            return Some(l);
        }
        self.next();
        let r = self.parse_logical_and()?;
        Some(ExprNode::Logical {
            kind: LogicalKind::And,
            left: Box::new(l),
            right: Box::new(r),
        })
    }

    fn parse_logical_or(&mut self) -> Option<ExprNode> {
        let l = self.parse_logical_and()?;
        if self.cur.kind != TokenKind::Or {
            return Some(l);
        }
        self.next();
        let r = self.parse_logical_or()?;
        Some(ExprNode::Logical {
            kind: LogicalKind::Or,
            left: Box::new(l),
            right: Box::new(r),
        })
    }
}
