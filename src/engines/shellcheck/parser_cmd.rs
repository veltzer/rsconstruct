//! `ShellCheck`'s parser, part three: commands, compound commands, `source`,
//! the script itself, and `parseShell`, which turns a parse into comments.

use std::collections::HashMap;
use std::rc::Rc;

use super::ast::{Annotation, CaseType, Id, Inner, SourcePos, Token};
use super::astlib::{
    executable_from_shebang, get_associative_arrays, get_literal_string, get_word_parts,
    is_string_expansion,
};
use super::hchar::{is_alpha, is_space, lower_string, to_lower, to_upper};
use super::interface::{Comment, Position, PositionedComment, Shell};
use super::parsec::{
    Context, Environment, Fail, Msg, P, ParseError, ParseNote, R,
    Severity::{ErrorC, InfoC, WarningC},
    SystemInterface, context_disables_code,
};
use super::parser::{
    bx, is_extended_function_start_char, is_function_char, is_function_start_char, tk,
};

use Inner::{
    T_AND_IF, T_AndIf, T_Annotation, T_Array, T_Assignment, T_Backgrounded, T_Bang, T_Banged,
    T_BatsTest, T_BraceGroup, T_CLOBBER, T_Case, T_CaseExpression, T_CoProc, T_CoProcBody,
    T_DGREAT, T_DSEMI, T_Do, T_Done, T_EOF, T_Elif, T_Else, T_Esac, T_FdRedirect, T_Fi, T_For,
    T_ForArithmetic, T_ForIn, T_Function, T_GREATAND, T_Glob, T_HereDoc, T_If, T_IfExpression,
    T_In, T_Include, T_IndexedElement, T_LESSAND, T_LESSGREAT, T_Lbrace, T_Literal, T_Lparen,
    T_NormalWord, T_OR_IF, T_OrIf, T_Pipe, T_Pipeline, T_Rbrace, T_Redirecting, T_Rparen, T_Script,
    T_Select, T_SelectIn, T_Semi, T_SimpleCommand, T_SourceCommand, T_Subshell, T_Then,
    T_UnparsedIndex, T_Until, T_UntilExpression, T_While, T_WhileExpression, TA_Variable,
};

/// `ParseResult`.
pub struct ParseResult {
    pub comments: Vec<PositionedComment>,
    pub token_positions: HashMap<Id, (Position, Position)>,
    pub root: Option<Token>,
}

/// `ParseSpec`.
pub struct ParseSpec {
    pub filename: String,
    pub script: String,
    pub check_sourced: bool,
    pub ignore_rc: bool,
    pub shell_type_override: Option<Shell>,
}

fn is_command(strings: &[&str], t: &Token) -> bool {
    match &t.inner {
        T_NormalWord(parts) => {
            matches!(parts.as_slice(), [Token { inner: T_Literal(s), .. }] if strings.contains(&s.as_str()))
        }
        _ => false,
    }
}

impl P {
    // ----- tokens

    fn try_token(&mut self, s: &str, inner: Inner) -> R<Token> {
        self.try_(|p| {
            let start = p.start_span();
            p.string(s)?;
            let id = p.end_span(start);
            p.spacing()?;
            Ok(tk(id, inner))
        })
    }

    pub fn redir_token(&mut self, c: char, inner: Inner) -> R<Token> {
        self.try_(|p| {
            let start = p.start_span();
            p.char_(c)?;
            let id = p.end_span(start);
            p.not_followed_by2(|p| p.char_('('))?;
            Ok(tk(id, inner))
        })
    }

    fn try_word_token(&mut self, keyword: &str, inner: Inner) -> R<Token> {
        self.then_skip(|p| p.try_parse_word_token(keyword, inner), Self::spacing)
    }

    fn try_parse_word_token(&mut self, keyword: &str, inner: Inner) -> R<Token> {
        self.try_(|p| {
            let pos = p.pos;
            let start = p.start_span();
            let mut str_ = String::new();
            for c in keyword.chars() {
                let lower = to_lower(c);
                let upper = to_upper(c);
                str_.push(p.alt(|p| p.char_(lower), |p| p.char_(upper))?);
            }
            let id = p.end_span(start);
            p.optional(|p| {
                let c = p.try_(|p| p.look_ahead(Self::any_char))?;
                let code = match c {
                    '[' => Some(1069),
                    '#' => Some(1099),
                    '!' => Some(1129),
                    ':' => Some(1130),
                    _ => None,
                };
                if let Some(code) = code {
                    p.parse_problem(ErrorC, code, &format!("You need a space before the {c}."));
                }
                Ok(())
            })?;
            p.look_ahead(Self::keyword_separator)?;
            if str_ != keyword {
                p.parse_problem_at(
                    pos,
                    ErrorC,
                    1081,
                    &format!("Scripts are case sensitive. Use '{keyword}', not '{str_}' (or quote if literal)."),
                );
                return p.fail("");
            }
            Ok(tk(id, inner))
        })
    }

    pub fn g_and_if(&mut self) -> R<Token> {
        self.try_token("&&", T_AND_IF)
    }

    pub fn g_or_if(&mut self) -> R<Token> {
        self.try_token("||", T_OR_IF)
    }

    pub fn g_dsemi(&mut self) -> R<Token> {
        self.try_token(";;", T_DSEMI)
    }

    pub fn g_dgreat(&mut self) -> R<Token> {
        self.try_token(">>", T_DGREAT)
    }

    pub fn g_lessand(&mut self) -> R<Token> {
        self.try_token("<&", T_LESSAND)
    }

    pub fn g_greatand(&mut self) -> R<Token> {
        self.try_token(">&", T_GREATAND)
    }

    pub fn g_lessgreat(&mut self) -> R<Token> {
        self.try_token("<>", T_LESSGREAT)
    }

    pub fn g_clobber(&mut self) -> R<Token> {
        self.try_token(">|", T_CLOBBER)
    }

    fn g_if(&mut self) -> R<Token> {
        self.try_word_token("if", T_If)
    }

    fn g_then(&mut self) -> R<Token> {
        self.try_word_token("then", T_Then)
    }

    fn g_else(&mut self) -> R<Token> {
        self.try_word_token("else", T_Else)
    }

    fn g_elif(&mut self) -> R<Token> {
        self.try_word_token("elif", T_Elif)
    }

    fn g_fi(&mut self) -> R<Token> {
        self.try_word_token("fi", T_Fi)
    }

    fn g_do(&mut self) -> R<Token> {
        self.try_word_token("do", T_Do)
    }

    fn g_done(&mut self) -> R<Token> {
        self.try_word_token("done", T_Done)
    }

    fn g_case(&mut self) -> R<Token> {
        self.try_word_token("case", T_Case)
    }

    fn g_esac(&mut self) -> R<Token> {
        self.try_word_token("esac", T_Esac)
    }

    fn g_while(&mut self) -> R<Token> {
        self.try_word_token("while", T_While)
    }

    fn g_until(&mut self) -> R<Token> {
        self.try_word_token("until", T_Until)
    }

    fn g_for(&mut self) -> R<Token> {
        self.try_word_token("for", T_For)
    }

    fn g_select(&mut self) -> R<Token> {
        self.try_word_token("select", T_Select)
    }

    fn g_in(&mut self) -> R<Token> {
        let t = self.try_word_token("in", T_In)?;
        self.skip_annotation_and_warn()?;
        Ok(t)
    }

    fn g_lbrace(&mut self) -> R<Token> {
        self.try_word_token("{", T_Lbrace)
    }

    fn g_rbrace(&mut self) -> R<Token> {
        let start = self.start_span();
        self.char_('}')?;
        let id = self.end_span(start);
        Ok(tk(id, T_Rbrace))
    }

    fn g_lparen(&mut self) -> R<Token> {
        self.try_token("(", T_Lparen)
    }

    fn g_rparen(&mut self) -> R<Token> {
        self.try_token(")", T_Rparen)
    }

    fn g_bang(&mut self) -> R<Token> {
        let start = self.start_span();
        self.char_('!')?;
        let id = self.end_span(start);
        self.alt(
            |p| p.spacing1().map(|_| ()),
            |p| {
                let pos = p.pos;
                p.parse_problem_at(
                    pos,
                    ErrorC,
                    1035,
                    "You are missing a required space after the !.",
                );
                Ok(())
            },
        )?;
        Ok(tk(id, T_Bang))
    }

    fn g_semi(&mut self) -> R<Token> {
        self.not_followed_by2(Self::g_dsemi)?;
        self.try_token(";", T_Semi)
    }

    fn keyword_separator(&mut self) -> R<()> {
        self.alt(Self::eof, |p| {
            p.alt(
                |p| p.try_(Self::allspacing_or_fail).map(|_| ()),
                |p| p.one_of(";()[<>&|").map(|_| ()),
            )
        })
    }

    fn read_keyword(&mut self) -> R<Token> {
        self.choice(&mut [
            &mut Self::g_then,
            &mut Self::g_else,
            &mut Self::g_elif,
            &mut Self::g_fi,
            &mut Self::g_do,
            &mut Self::g_done,
            &mut Self::g_esac,
            &mut Self::g_rbrace,
            &mut Self::g_rparen,
            &mut Self::g_dsemi,
        ])
    }

    // ----- separators

    fn read_newline_list(&mut self) -> R<Vec<char>> {
        let v = self.many1(|p| {
            p.then_skip(
                |p| p.alt(Self::linefeed, Self::carriage_return),
                Self::spacing,
            )
        })?;
        self.optional(|p| {
            let pos = p.pos;
            p.try_(|p| p.look_ahead(|p| p.one_of("|&")))?;
            p.not_followed_by2(|p| p.string("&>"))?;
            p.parse_problem_at(
                pos,
                ErrorC,
                1133,
                "Unexpected start of line. If breaking lines, |/||/&& should be at the end of the previous one.",
            );
            Ok(())
        })?;
        Ok(v)
    }

    fn read_line_break(&mut self) -> R<()> {
        self.optional(Self::read_newline_list)
    }

    fn read_separator_op(&mut self) -> R<(char, (SourcePos, SourcePos))> {
        self.not_followed_by2(|p| {
            p.alt(
                |p| p.g_and_if().map(|_| ()),
                |p| p.read_case_separator().map(|_| ()),
            )
        })?;
        self.not_followed_by2(|p| p.string("&>"))?;
        let start = self.pos;
        let f = self.alt(
            |p| {
                p.try_(|p| {
                    let pos = p.pos;
                    p.char_('&')?;
                    p.optional(|p| {
                        p.choice(&mut [
                            &mut |p: &mut Self| {
                                p.look_ahead(|p| {
                                    p.choice(&mut [
                                        &mut |p: &mut Self| p.try_(|p| p.string("amp;")),
                                        &mut |p: &mut Self| p.try_(|p| p.string("gt;")),
                                        &mut |p: &mut Self| p.try_(|p| p.string("lt;")),
                                    ])
                                })?;
                                p.parse_problem_at(
                                    pos,
                                    ErrorC,
                                    1109,
                                    "This is an unquoted HTML entity. Replace with corresponding character.",
                                );
                                Ok(())
                            },
                            &mut |p: &mut Self| {
                                p.try_(|p| p.look_ahead(Self::variable_start))?;
                                p.parse_problem_at(
                                    pos,
                                    WarningC,
                                    1132,
                                    "This & terminates the command. Escape it or add space after & to silence.",
                                );
                                Ok(())
                            },
                        ])
                    })?;
                    p.spacing()?;
                    let pos = p.pos;
                    p.char_(';')?;
                    p.not_followed_by2(|p| p.char_(';'))?;
                    p.parse_problem_at(pos, ErrorC, 1045, "It's not 'foo &; bar', just 'foo & bar'.");
                    Ok('&')
                })
            },
            |p| p.alt(|p| p.char_(';'), |p| p.char_('&')),
        )?;
        let end = self.pos;
        self.spacing()?;
        Ok((f, (start, end)))
    }

    fn read_sequential_sep(&mut self) -> R<()> {
        self.alt(
            |p| {
                p.g_semi()?;
                p.read_line_break()
            },
            |p| p.read_newline_list().map(|_| ()),
        )
    }

    fn read_separator(&mut self) -> R<(char, (SourcePos, SourcePos))> {
        self.alt(
            |p| {
                let separator = p.read_separator_op()?;
                p.read_line_break()?;
                Ok(separator)
            },
            |p| {
                let start = p.pos;
                p.read_newline_list()?;
                let end = p.pos;
                Ok(('\n', (start, end)))
            },
        )
    }

    // ----- simple commands

    fn read_simple_command(&mut self) -> R<Token> {
        self.called("simple command", |p| {
            let prefix = p.option(Vec::new(), Self::read_cmd_prefix)?;
            p.skip_annotation_and_warn()?;
            let cmd = p.option(None, |p| p.read_cmd_name().map(Some))?;
            if prefix.is_empty() && cmd.is_none() {
                return p.fail("Expected a command");
            }
            match cmd {
                None => {
                    let refs: Vec<&Token> = prefix.iter().collect();
                    let id1 = p.id_spanning_list(&refs);
                    let id2 = p.new_id_for(id1);
                    Ok(make_simple_command(
                        id1,
                        id2,
                        prefix,
                        Vec::new(),
                        Vec::new(),
                    ))
                }
                Some(cmd) => {
                    p.validate_command(&cmd);
                    let first_argument = p.ignore_problems_of(|p| {
                        p.option_maybe(|p| p.try_(|p| p.look_ahead(Self::read_cmd_word)))
                    })?;
                    let selector = if is_command(&["builtin"], &cmd) {
                        first_argument.as_ref().unwrap_or(&cmd)
                    } else {
                        &cmd
                    };
                    let suffix_parser: fn(&mut Self) -> R<Vec<Token>> = if is_command(
                        &["declare", "export", "local", "readonly", "typeset"],
                        selector,
                    ) {
                        Self::read_modifier_suffix
                    } else if is_command(&["time"], selector) {
                        Self::read_time_suffix
                    } else if is_command(&["let"], selector) {
                        Self::read_let_suffix
                    } else if is_command(&["eval"], selector) {
                        Self::read_eval_suffix
                    } else {
                        Self::read_cmd_suffix
                    };
                    let suffix = p.option(Vec::new(), suffix_parser)?;
                    let mut all: Vec<&Token> = prefix.iter().collect();
                    all.push(&cmd);
                    all.extend(suffix.iter());
                    let id1 = p.id_spanning_list(&all);
                    let id2 = p.new_id_for(id1);
                    let is_source = is_command(&["source", "."], &cmd);
                    let is_trap = is_command(&["trap"], &cmd);
                    let result = make_simple_command(id1, id2, prefix, vec![cmd], suffix);
                    if is_source {
                        p.read_source(result)
                    } else {
                        if is_trap {
                            p.syntax_check_trap(&result)?;
                        }
                        Ok(result)
                    }
                }
            }
        })
    }

    fn validate_command(&mut self, cmd: &Token) {
        let T_NormalWord(parts) = &cmd.inner else {
            return;
        };
        match parts.as_slice() {
            [
                Token {
                    inner: T_Literal(s),
                    ..
                },
            ] if s == "//" => self.comment_warning(cmd.id),
            [
                Token {
                    inner: T_Literal(s),
                    ..
                },
                Token {
                    inner: T_Glob(g), ..
                },
                ..,
            ] if s == "/" && g == "*" => {
                self.comment_warning(cmd.id);
            }
            [
                Token {
                    inner: T_Literal(s),
                    ..
                },
                ..,
            ] => {
                let cmd_string =
                    lower_string(&s.chars().take_while(|&c| is_alpha(c)).collect::<String>());
                if cmd_string == "elsif" || cmd_string == "elseif" {
                    self.parse_problem_at_id(
                        cmd.id,
                        ErrorC,
                        1131,
                        "Use 'elif' to start another branch.",
                    );
                }
            }
            _ => {}
        }
    }

    fn comment_warning(&mut self, id: Id) {
        self.parse_problem_at_id(
            id,
            ErrorC,
            1127,
            "Was this intended as a comment? Use # in sh.",
        );
    }

    fn syntax_check_trap(&mut self, cmd: &Token) -> R<()> {
        let T_Redirecting(_, inner) = &cmd.inner else {
            return Ok(());
        };
        let T_SimpleCommand(_, words) = &inner.inner else {
            return Ok(());
        };
        if words.len() < 2 {
            return Ok(());
        }
        let arg = &words[1];
        let Some(str_) = get_literal_string(arg) else {
            return Ok(());
        };
        if str_.starts_with('-') {
            return Ok(());
        }
        let (start, _) = self.span_for_id(arg.id);
        self.sub_parse(start, &str_, |p| {
            p.alt(
                |p| {
                    p.try_with_errors(|p| {
                        p.read_compound_list_or_empty()?;
                        p.verify_eof()
                    })
                },
                |p| p.ok(()),
            )
        })
    }

    fn get_source_override(&self) -> Option<String> {
        for c in self.sys.contexts.iter().rev() {
            match c {
                Context::Source(_) => return None,
                Context::Annotation(list) => {
                    if let Some(f) = list.iter().find_map(|a| match a {
                        Annotation::SourceOverride(s) => Some(s.clone()),
                        _ => None,
                    }) {
                        return Some(f);
                    }
                }
                Context::Name(..) => {}
            }
        }
        None
    }

    /// `getCurrentAnnotations includeSource`.
    fn get_current_annotations(&self, include_source: bool) -> Vec<Annotation> {
        let mut out = Vec::new();
        for c in self.sys.contexts.iter().rev() {
            match c {
                Context::Source(_) if !include_source => break,
                Context::Annotation(list) => out.extend(list.iter().cloned()),
                _ => {}
            }
        }
        out
    }

    fn should_follow(&mut self, file: &str) -> bool {
        let contexts = &self.sys.contexts;
        if contexts
            .iter()
            .any(|c| matches!(c, Context::Source(name) if name == file))
        {
            return false;
        }
        if contexts
            .iter()
            .filter(|c| matches!(c, Context::Source(_)))
            .count()
            >= 100
        {
            self.parse_problem(ErrorC, 1092, "Stopping at 100 'source' frames :O");
            return false;
        }
        true
    }

    fn read_source(&mut self, t: Token) -> R<Token> {
        let (cmd_id, cmd, args) = {
            let T_Redirecting(_, inner) = &t.inner else {
                return Ok(t);
            };
            let T_SimpleCommand(_, words) = &inner.inner else {
                return Ok(t);
            };
            let Some((cmd, args)) = words.split_first() else {
                return Ok(t);
            };
            (inner.id, cmd.clone(), args.to_vec())
        };
        let file: Option<&Token> = match args.first() {
            Some(first) => match get_literal_string(first).as_deref() {
                Some("--") => args.get(1),
                Some("-p") => args.get(2),
                _ => Some(first),
            },
            None => None,
        };
        let override_ = self.get_source_override();
        let strip_dynamic_prefix = |word: &Token| -> Option<String> {
            let parts = get_word_parts(word);
            let (exp, rest) = parts.split_first()?;
            if !is_string_expansion(exp) {
                return None;
            }
            let rest_word = Token::new(
                Id(0),
                T_NormalWord(rest.iter().map(|t| (*t).clone()).collect()),
            );
            let s = get_literal_string(&rest_word)?;
            if !s.starts_with('/') {
                return None;
            }
            Some(format!(".{s}"))
        };
        let literal_file = override_
            .or_else(|| file.and_then(get_literal_string))
            .or_else(|| file.and_then(strip_dynamic_prefix))
            .filter(|name| !name.starts_with("~/"));
        let file_id = file.map_or(cmd.id, |f| f.id);
        let Some(filename) = literal_file else {
            self.parse_note_at_id(
                file_id,
                WarningC,
                1090,
                "ShellCheck can't follow non-constant source. Use a directive to specify location.",
            );
            return Ok(t);
        };
        if !self.should_follow(&filename) {
            self.parse_note_at_id(
                file_id,
                InfoC,
                1093,
                "This file appears to be recursively sourced. Ignoring.",
            );
            return Ok(t);
        }
        let sys = Rc::clone(&self.env.system);
        let (input, resolved_file) = if filename == "/dev/null" {
            (Ok(String::new()), filename.clone())
        } else {
            let all_annotations = self.get_current_annotations(true);
            let current_script = self.env.current_filename.clone();
            let paths: Vec<String> = all_annotations
                .iter()
                .filter_map(|a| match a {
                    Annotation::SourcePath(x) => Some(x.clone()),
                    _ => None,
                })
                .collect();
            let external_sources = all_annotations.iter().find_map(|a| match a {
                Annotation::ExternalSources(b) => Some(*b),
                _ => None,
            });
            let resolved = sys.find_source(&current_script, external_sources, &paths, &filename);
            let contents = sys.read_file(external_sources, &resolved);
            (contents, resolved)
        };
        match input {
            Err(err) => {
                self.parse_note_at_id(file_id, InfoC, 1091, &format!("Not following: {err}"));
                Ok(t)
            }
            Ok(script) => {
                let id1 = self.new_id_for(cmd_id);
                let id2 = self.new_id_for(cmd_id);
                let m = self.mark();
                let included = self.with_context(Context::Source(resolved_file.clone()), |p| {
                    p.in_separate_context(|p| {
                        let old_pending = std::mem::take(&mut p.user.pending_here_docs);
                        let file_index = p.file_index(&resolved_file);
                        let initial = SourcePos {
                            file: file_index,
                            line: 1,
                            column: 1,
                        };
                        let result = p.sub_parse(initial, &script, |p| p.read_script_file(true))?;
                        p.user.pending_here_docs = old_pending;
                        Ok(result)
                    })
                });
                match included {
                    Ok(src) => Ok(tk(
                        id1,
                        T_SourceCommand(bx(t), bx(tk(id2, T_Include(bx(src))))),
                    )),
                    Err(f) if self.consumed_since(&f, &m) => Err(f),
                    Err(f) => {
                        self.restore(&m);
                        self.err = f.err;
                        self.parse_note_at_id(
                            file_id,
                            WarningC,
                            1094,
                            "Parsing of sourced file failed. Ignoring it.",
                        );
                        Ok(t)
                    }
                }
            }
        }
    }

    fn read_pipeline(&mut self) -> R<Token> {
        self.unexpecting("keyword/token", Self::read_keyword)?;
        self.read_banged(Self::read_pipe_sequence)
    }

    fn read_banged(&mut self, parser: fn(&mut Self) -> R<Token>) -> R<Token> {
        self.alt(
            |p| {
                let bang = p.g_bang()?;
                let next = p.read_banged(parser)?;
                Ok(tk(bang.id, T_Banged(bx(next))))
            },
            parser,
        )
    }

    fn read_and_or(&mut self) -> R<Token> {
        let start = self.start_span();
        let apos = self.pos;
        let annotations = self.read_annotations()?;
        let aid = self.end_span(start);
        if !annotations.is_empty() {
            self.optional(|p| {
                p.try_(|p| p.look_ahead(Self::read_keyword))?;
                p.parse_problem_at(
                    apos,
                    ErrorC,
                    1123,
                    "ShellCheck directives are only valid in front of complete compound commands, like 'if', not e.g. individual 'elif' branches.",
                );
                Ok(())
            })?;
        }
        let and_or = self.with_annotations(annotations.clone(), |p| {
            p.chainl1(Self::read_pipeline, |p| {
                let op = p.alt(Self::g_and_if, Self::g_or_if)?;
                p.read_line_break()?;
                let id = op.id;
                let f: Box<dyn FnOnce(Token, Token) -> Token> = if matches!(op.inner, T_AND_IF) {
                    Box::new(move |a, b| tk(id, T_AndIf(bx(a), bx(b))))
                } else {
                    Box::new(move |a, b| tk(id, T_OrIf(bx(a), bx(b))))
                };
                Ok(f)
            })
        })?;
        Ok(if annotations.is_empty() {
            and_or
        } else {
            tk(aid, T_Annotation(annotations, bx(and_or)))
        })
    }

    pub fn read_term(&mut self) -> R<Vec<Token>> {
        self.allspacing()?;
        let mut current = self.read_and_or()?;
        let mut out = Vec::new();
        loop {
            let m = self.mark();
            let attempt = (|| -> R<(Id, char, Option<Token>)> {
                let (sep, (start, end)) = self.read_separator()?;
                let id = self.next_id_between(start, end);
                let more = self.option(None, |p| p.read_and_or().map(Some))?;
                Ok((id, sep, more))
            })();
            match attempt {
                Ok((id, sep, more)) => {
                    let transformed = if sep == '&' {
                        tk(id, T_Backgrounded(bx(current)))
                    } else {
                        current
                    };
                    out.push(transformed);
                    match more {
                        None => return Ok(out),
                        Some(next) => current = next,
                    }
                }
                Err(f) if self.consumed_since(&f, &m) => return Err(f),
                Err(f) => {
                    self.restore(&m);
                    self.err = f.err;
                    out.push(current);
                    return Ok(out);
                }
            }
        }
    }

    fn read_pipe_sequence(&mut self) -> R<Token> {
        let start = self.start_span();
        type Acc = (Vec<Token>, Vec<Token>);
        let (cmds, pipes) = self.chainl1(
            |p| {
                p.read_banged(Self::read_command)
                    .map(|x| (vec![x], Vec::new()))
            },
            |p| {
                let separator = p.then_skip(Self::read_pipe, |p| {
                    p.spacing()?;
                    p.read_line_break()
                })?;
                let f: Box<dyn FnOnce(Acc, Acc) -> Acc> =
                    Box::new(move |(mut a, mut b), (c, d)| {
                        a.extend(c);
                        b.extend(d);
                        b.push(separator);
                        (a, b)
                    });
                Ok(f)
            },
        )?;
        let id = self.end_span(start);
        self.spacing()?;
        Ok(tk(id, T_Pipeline(pipes, cmds)))
    }

    fn read_pipe(&mut self) -> R<Token> {
        self.not_followed_by2(Self::g_or_if)?;
        let start = self.start_span();
        self.char_('|')?;
        let qualifier = self.alt(|p| p.string("&"), |p| p.ok(String::new()))?;
        let id = self.end_span(start);
        self.spacing()?;
        Ok(tk(id, T_Pipe(format!("|{qualifier}"))))
    }

    fn read_command(&mut self) -> R<Token> {
        self.choice(&mut [
            &mut Self::read_compound_command,
            &mut Self::read_condition_command,
            &mut Self::read_co_proc,
            &mut Self::read_simple_command,
        ])
    }

    fn read_cmd_name(&mut self) -> R<Token> {
        self.optional(|p| {
            p.look_ahead(|p| {
                p.try_(|p| {
                    p.char_('!')?;
                    p.whitespace()
                })
            })
        })?;
        self.optional(|p| {
            p.try_(|p| {
                p.char_('\\')?;
                p.look_ahead(|p| p.alt(Self::variable_chars, |p| p.one_of(":.")))
            })
        })?;
        self.read_cmd_word()
    }

    fn read_cmd_word(&mut self) -> R<Token> {
        self.skip_annotation_and_warn()?;
        let w = self.read_normal_word()?;
        self.spacing()?;
        Ok(w)
    }

    pub fn skip_annotation_and_warn(&mut self) -> R<()> {
        self.optional(|p| {
            p.try_(|p| p.look_ahead(Self::read_annotation_prefix))?;
            p.parse_problem(
                ErrorC,
                1126,
                "Place shellcheck directives before commands, not after.",
            );
            p.read_any_comment()
        })
    }

    // ----- if

    fn read_if_clause(&mut self) -> R<Token> {
        self.called("if expression", |p| {
            let start = p.start_span();
            let pos = p.pos;
            let first = p.read_if_part()?;
            let elifs = p.many(Self::read_elif_part)?;
            let elses = p.option(Vec::new(), Self::read_else_part)?;
            p.or_fail(Self::g_fi, |p| {
                p.parse_problem_at(pos, ErrorC, 1046, "Couldn't find 'fi' for this 'if'.");
                p.parse_problem(
                    ErrorC,
                    1047,
                    "Expected 'fi' matching previously mentioned 'if'.",
                );
                "Expected 'fi'".to_string()
            })?;
            let id = p.end_span(start);
            let mut branches = vec![first];
            branches.extend(elifs);
            Ok(tk(id, T_IfExpression(branches, elses)))
        })
    }

    fn verify_not_empty_if(&mut self, s: &str) -> R<()> {
        self.optional(|p| {
            let empty_pos = p.pos;
            p.try_(|p| p.look_ahead(|p| p.alt(Self::g_fi, |p| p.alt(Self::g_elif, Self::g_else))))?;
            p.parse_problem_at(
                empty_pos,
                ErrorC,
                1048,
                &format!("Can't have empty {s} clauses (use 'true' as a no-op)."),
            );
            Ok(())
        })
    }

    fn read_if_part(&mut self) -> R<(Vec<Token>, Vec<Token>)> {
        let pos = self.pos;
        self.g_if()?;
        self.allspacing()?;
        let condition = self.read_term()?;
        self.if_next_token(
            |p| p.alt(Self::g_fi, |p| p.alt(Self::g_elif, Self::g_else)),
            |p| {
                p.parse_problem_at(
                    pos,
                    ErrorC,
                    1049,
                    "Did you forget the 'then' for this 'if'?",
                );
            },
        )?;
        self.called("then clause", |p| {
            p.or_fail(Self::g_then, |p| {
                p.parse_problem(ErrorC, 1050, "Expected 'then'.");
                "Expected 'then'".to_string()
            })?;
            p.accept_but_warn(
                Self::g_semi,
                ErrorC,
                1051,
                "Semicolons directly after 'then' are not allowed. Just remove it.",
            )?;
            p.allspacing()?;
            p.verify_not_empty_if("then")?;
            let action = p.read_term()?;
            Ok((condition, action))
        })
    }

    fn read_elif_part(&mut self) -> R<(Vec<Token>, Vec<Token>)> {
        self.called("elif clause", |p| {
            let pos = p.pos;
            p.g_elif()?;
            p.allspacing()?;
            let condition = p.read_term()?;
            p.if_next_token(
                |p| p.alt(Self::g_fi, |p| p.alt(Self::g_elif, Self::g_else)),
                |p| {
                    p.parse_problem_at(
                        pos,
                        ErrorC,
                        1049,
                        "Did you forget the 'then' for this 'elif'?",
                    );
                },
            )?;
            p.g_then()?;
            p.accept_but_warn(
                Self::g_semi,
                ErrorC,
                1052,
                "Semicolons directly after 'then' are not allowed. Just remove it.",
            )?;
            p.allspacing()?;
            p.verify_not_empty_if("then")?;
            let action = p.read_term()?;
            Ok((condition, action))
        })
    }

    fn read_else_part(&mut self) -> R<Vec<Token>> {
        self.called("else clause", |p| {
            let pos = p.pos;
            p.g_else()?;
            p.optional(|p| {
                p.try_(|p| p.look_ahead(Self::g_if))?;
                p.parse_problem_at(
                    pos,
                    ErrorC,
                    1075,
                    "Use 'elif' instead of 'else if' (or put 'if' on new line if nesting).",
                );
                Ok(())
            })?;
            p.accept_but_warn(
                Self::g_semi,
                ErrorC,
                1053,
                "Semicolons directly after 'else' are not allowed. Just remove it.",
            )?;
            p.allspacing()?;
            p.verify_not_empty_if("else")?;
            p.read_term()
        })
    }

    // ----- groups

    fn read_subshell(&mut self) -> R<Token> {
        self.called("explicit subshell", |p| {
            let start = p.start_span();
            p.char_('(')?;
            p.allspacing()?;
            let list = p.read_compound_list()?;
            p.allspacing()?;
            p.alt(
                |p| p.char_(')').map(|_| ()),
                |p| p.fail("Expected ) closing the subshell"),
            )?;
            let id = p.end_span(start);
            p.spacing()?;
            Ok(tk(id, T_Subshell(list)))
        })
    }

    fn read_brace_group(&mut self) -> R<Token> {
        self.called("brace group", |p| {
            let start = p.start_span();
            p.char_('{')?;
            p.alt(
                |p| p.allspacing_or_fail().map(|_| ()),
                |p| {
                    p.optional(|p| {
                        p.look_ahead(|p| p.none_of("("))?;
                        p.parse_problem(ErrorC, 1054, "You need a space after the '{'.");
                        Ok(())
                    })
                },
            )?;
            p.optional(|p| {
                let pos = p.pos;
                p.look_ahead(|p| p.char_('}'))?;
                p.parse_problem_at(
                    pos,
                    ErrorC,
                    1055,
                    "You need at least one command here. Use 'true;' as a no-op.",
                );
                Ok(())
            })?;
            let list = p.read_term()?;
            p.alt(
                |p| p.char_('}').map(|_| ()),
                |p| {
                    p.parse_problem(
                        ErrorC,
                        1056,
                        "Expected a '}'. If you have one, try a ; or \\n in front of it.",
                    );
                    p.fail("Missing '}'")
                },
            )?;
            let id = p.end_span(start);
            p.spacing()?;
            Ok(tk(id, T_BraceGroup(list)))
        })
    }

    fn read_bats_test(&mut self) -> R<Token> {
        self.called("bats @test", |p| {
            let start = p.start_span();
            p.try_(|p| p.string("@test "))?;
            p.spacing()?;
            let line: String = p
                .try_(|p| p.look_ahead(|p| p.many1(|p| p.none_of("\n"))))?
                .into_iter()
                .collect();
            let name = bats_name(&line);
            let name = p.string(&name)?;
            p.spacing()?;
            let test = p.read_brace_group()?;
            let id = p.end_span(start);
            Ok(tk(id, T_BatsTest(name, bx(test))))
        })
    }

    // ----- loops

    fn read_while_clause(&mut self) -> R<Token> {
        self.called("while loop", |p| {
            let start = p.start_span();
            let kw_id = p.g_while()?.id;
            let condition = p.read_term()?;
            let statements = p.read_do_group(kw_id)?;
            let id = p.end_span(start);
            Ok(tk(id, T_WhileExpression(condition, statements)))
        })
    }

    fn read_until_clause(&mut self) -> R<Token> {
        self.called("until loop", |p| {
            let start = p.start_span();
            let kw_id = p.g_until()?.id;
            let condition = p.read_term()?;
            let statements = p.read_do_group(kw_id)?;
            let id = p.end_span(start);
            Ok(tk(id, T_UntilExpression(condition, statements)))
        })
    }

    fn read_do_group(&mut self, kw_id: Id) -> R<Vec<Token>> {
        self.optional(|p| {
            p.try_(|p| p.look_ahead(Self::g_done))?;
            p.parse_problem_at_id(
                kw_id,
                ErrorC,
                1057,
                "Did you forget the 'do' for this loop?",
            );
            Ok(())
        })?;
        let do_kw = self.or_fail(Self::g_do, |p| {
            p.parse_problem(ErrorC, 1058, "Expected 'do'.");
            "Expected 'do'".to_string()
        })?;
        self.accept_but_warn(
            Self::g_semi,
            ErrorC,
            1059,
            "Semicolon is not allowed directly after 'do'. You can just delete it.",
        )?;
        self.allspacing()?;
        self.optional(|p| {
            p.try_(|p| p.look_ahead(Self::g_done))?;
            p.parse_problem_at_id(
                do_kw.id,
                ErrorC,
                1060,
                "Can't have empty do clauses (use 'true' as a no-op).",
            );
            Ok(())
        })?;
        let commands = self.read_compound_list()?;
        self.or_fail(Self::g_done, |p| {
            p.parse_problem_at_id(
                do_kw.id,
                ErrorC,
                1061,
                "Couldn't find 'done' for this 'do'.",
            );
            p.parse_problem(
                ErrorC,
                1062,
                "Expected 'done' matching previously mentioned 'do'.",
            );
            "Expected 'done'".to_string()
        })?;
        self.optional(|p| {
            p.look_ahead(|p| {
                let pos = p.pos;
                p.try_(|p| p.string("<("))?;
                p.parse_problem_at(
                    pos,
                    ErrorC,
                    1142,
                    "Use 'done < <(cmd)' to redirect from process substitution (currently missing one '<').",
                );
                Ok(())
            })
        })?;
        Ok(commands)
    }

    fn read_for_clause(&mut self) -> R<Token> {
        self.called("for loop", |p| {
            let id = p.g_for()?.id;
            p.spacing()?;
            p.alt(|p| p.read_arithmetic_for(id), |p| p.read_regular_for(id))
        })
    }

    fn read_arithmetic_for(&mut self, id: Id) -> R<Token> {
        self.called("arithmetic for condition", |p| {
            p.read_arithmetic_delimiter(
                '(',
                "Missing second '(' to start arithmetic for ((;;)) loop",
            )?;
            let x = p.read_arithmetic_contents()?;
            p.char_(';')?;
            p.spacing()?;
            let y = p.read_arithmetic_contents()?;
            p.char_(';')?;
            p.spacing()?;
            let z = p.read_arithmetic_contents()?;
            p.spacing()?;
            p.read_arithmetic_delimiter(
                ')',
                "Missing second ')' to terminate 'for ((;;))' loop condition",
            )?;
            p.spacing()?;
            p.optional(|p| {
                p.read_sequential_sep()?;
                p.spacing()
            })?;
            let group = p.alt(Self::read_braced_group_list, |p| p.read_do_group(id))?;
            Ok(tk(id, T_ForArithmetic(bx(x), bx(y), bx(z), group)))
        })
    }

    fn read_arithmetic_delimiter(&mut self, c: char, msg: &str) -> R<()> {
        self.char_(c)?;
        let start_pos = self.pos;
        let sp = self.spacing()?;
        let end_pos = self.pos;
        self.alt(
            |p| p.char_(c).map(|_| ()),
            |p| {
                p.parse_problem_at(start_pos, ErrorC, 1137, msg);
                p.fail("")
            },
        )?;
        if !sp.is_empty() {
            self.parse_problem_at_with_end(
                start_pos,
                end_pos,
                ErrorC,
                1138,
                &format!("Remove spaces between {c}{c} in arithmetic for loop."),
            );
        }
        Ok(())
    }

    fn read_braced_group_list(&mut self) -> R<Vec<Token>> {
        let g = self.read_brace_group()?;
        match g.inner {
            T_BraceGroup(list) => Ok(list),
            _ => self.fail("pattern match failure"),
        }
    }

    fn read_regular_for(&mut self, id: Id) -> R<Token> {
        self.accept_but_warn(
            |p| p.char_('$'),
            ErrorC,
            1086,
            "Don't use $ on the iterator name in for loops.",
        )?;
        let name = self.then_skip(Self::read_variable_name, Self::allspacing)?;
        let values = self.alt(Self::read_in_clause, |p| {
            p.optional(Self::read_sequential_sep)?;
            Ok(Vec::new())
        })?;
        let group = self.alt(Self::read_braced_group_list, |p| p.read_do_group(id))?;
        Ok(tk(id, T_ForIn(name, values, group)))
    }

    fn read_select_clause(&mut self) -> R<Token> {
        self.called("select loop", |p| {
            let id = p.g_select()?.id;
            p.spacing()?;
            let name = p.read_variable_name()?;
            p.spacing()?;
            let values = p.alt(Self::read_in_clause, |p| {
                p.read_sequential_sep()?;
                Ok(Vec::new())
            })?;
            let group = p.read_do_group(id)?;
            Ok(tk(id, T_SelectIn(name, values, group)))
        })
    }

    fn read_in_clause(&mut self) -> R<Vec<Token>> {
        self.g_in()?;
        let things = self.reluctantly_till(Self::read_cmd_word, |p| {
            p.alt(
                |p| p.g_semi().map(|_| ()),
                |p| p.alt(|p| p.linefeed().map(|_| ()), |p| p.g_do().map(|_| ())),
            )
        })?;
        self.alt(
            |p| {
                p.look_ahead(Self::g_do)?;
                p.parse_note(
                    ErrorC,
                    1063,
                    "You need a line feed or semicolon before the 'do'.",
                );
                Ok(())
            },
            |p| {
                p.optional(Self::g_semi)?;
                p.allspacing().map(|_| ())
            },
        )?;
        Ok(things)
    }

    // ----- case

    fn read_case_clause(&mut self) -> R<Token> {
        self.called("case expression", |p| {
            let start = p.start_span();
            p.g_case()?;
            let word = p.read_normal_word()?;
            p.allspacing()?;
            p.alt(|p| p.g_in().map(|_| ()), |p| p.fail("Expected 'in'"))?;
            p.read_line_break()?;
            let list = p.many(Self::read_case_item)?;
            p.alt(
                |p| p.g_esac().map(|_| ()),
                |p| p.fail("Expected 'esac' to close the case statement"),
            )?;
            let id = p.end_span(start);
            Ok(tk(id, T_CaseExpression(bx(word), list)))
        })
    }

    fn read_case_item(&mut self) -> R<(CaseType, Vec<Token>, Vec<Token>)> {
        self.called("case item", |p| {
            p.not_followed_by2(Self::g_esac)?;
            p.optional(|p| {
                p.try_(|p| p.look_ahead(Self::read_annotation_prefix))?;
                p.parse_problem(
                    ErrorC,
                    1124,
                    "ShellCheck directives are only valid in front of complete commands like 'case' statements, not individual case branches.",
                );
                Ok(())
            })?;
            p.optional(Self::g_lparen)?;
            p.spacing()?;
            let pattern = p.read_pattern()?;
            p.alt(
                |p| p.g_rparen().map(|_| ()),
                |p| {
                    p.parse_problem(ErrorC, 1085, "Did you forget to move the ;; after extending this case item?");
                    p.fail("Expected ) to open a new case item")
                },
            )?;
            p.read_line_break()?;
            let list = p.alt(
                |p| {
                    p.look_ahead(Self::read_case_separator)?;
                    Ok(Vec::new())
                },
                Self::read_compound_list,
            )?;
            let separator = p.attempting(Self::read_case_separator, |p| {
                let pos = p.pos;
                p.look_ahead(Self::g_rparen)?;
                p.parse_problem_at(pos, ErrorC, 1074, "Did you forget the ;; after the previous case item?");
                Ok(())
            })?;
            p.read_line_break()?;
            Ok((separator, pattern, list))
        })
    }

    fn read_case_separator(&mut self) -> R<CaseType> {
        self.choice(&mut [
            &mut |p: &mut Self| p.try_token(";;&", T_EOF).map(|_| CaseType::CaseContinue),
            &mut |p: &mut Self| p.try_token(";&", T_EOF).map(|_| CaseType::CaseFallThrough),
            &mut |p: &mut Self| p.g_dsemi().map(|_| CaseType::CaseBreak),
            &mut |p: &mut Self| {
                p.look_ahead(|p| {
                    p.read_line_break()?;
                    p.g_esac()
                })?;
                Ok(CaseType::CaseBreak)
            },
        ])
    }

    fn read_pattern(&mut self) -> R<Vec<Token>> {
        self.sep_by1(
            |p| p.then_skip(Self::read_pattern_word, Self::spacing),
            |p| p.then_skip(|p| p.char_('|'), Self::spacing),
        )
    }

    // ----- functions and coprocesses

    fn read_function_definition(&mut self) -> R<Token> {
        self.called("function", |p| {
            let start = p.start_span();
            let (keyword, parens, name) = p.try_(Self::read_function_signature)?;
            p.allspacing()?;
            p.alt(
                |p| p.look_ahead(|p| p.one_of("{(")).map(|_| ()),
                |p| {
                    p.parse_problem(
                        ErrorC,
                        1064,
                        "Expected a { to open the function definition.",
                    );
                    Ok(())
                },
            )?;
            let group = p.alt(Self::read_brace_group, Self::read_subshell)?;
            let id = p.end_span(start);
            Ok(tk(id, T_Function(keyword, parens, name, bx(group))))
        })
    }

    fn read_function_signature(&mut self) -> R<(bool, bool, String)> {
        self.alt(
            |p| {
                p.try_(|p| {
                    p.string("function")?;
                    p.whitespace()
                })?;
                p.spacing()?;
                let first = p.satisfy(is_extended_function_start_char)?;
                let rest = p.many(|p| p.satisfy(is_extended_function_start_char))?;
                let mut name = String::new();
                name.push(first);
                name.extend(rest);
                let spaces = p.spacing()?;
                let has_parens = p.was_included(Self::read_parens)?;
                if !has_parens && spaces.is_empty() {
                    p.accept_but_warn(
                        |p| p.look_ahead(|p| p.one_of("{(")),
                        ErrorC,
                        1095,
                        "You need a space or linefeed between the function name and body.",
                    )?;
                }
                Ok((true, has_parens, name))
            },
            |p| {
                p.try_(|p| {
                    let first = p.satisfy(is_function_start_char)?;
                    let rest = p.many(|p| p.satisfy(is_function_char))?;
                    let mut name = String::new();
                    name.push(first);
                    name.extend(rest);
                    p.guard(name != "time")?;
                    p.spacing()?;
                    p.read_parens()?;
                    Ok((false, true, name))
                })
            },
        )
    }

    fn read_parens(&mut self) -> R<()> {
        self.g_lparen()?;
        self.spacing()?;
        self.alt(
            |p| p.g_rparen().map(|_| ()),
            |p| {
                p.parse_problem(
                    ErrorC,
                    1065,
                    "Trying to declare parameters? Don't. Use () and refer to params as $1, $2..",
                );
                p.many(|p| p.none_of("\n){"))?;
                p.g_rparen().map(|_| ())
            },
        )
    }

    fn read_co_proc(&mut self) -> R<Token> {
        self.called("coproc", |p| {
            let start = p.start_span();
            p.try_(|p| {
                p.string("coproc")?;
                p.spacing1()
            })?;
            p.choice(&mut [
                &mut |p: &mut Self| p.try_(|p| p.read_compound_co_proc(start)),
                &mut |p: &mut Self| {
                    let body = p.read_co_proc_body(Self::read_simple_command)?;
                    let id = p.end_span(start);
                    Ok(tk(id, T_CoProc(None, bx(body))))
                },
            ])
        })
    }

    fn read_compound_co_proc(&mut self, start: SourcePos) -> R<Token> {
        self.not_followed_by2(Self::read_assignment_word)?;
        let (var, body) = self.choice(&mut [
            &mut |p: &mut Self| {
                p.try_(|p| {
                    let body = p.read_co_proc_body(Self::read_compound_command)?;
                    Ok((None, body))
                })
            },
            &mut |p: &mut Self| {
                p.try_(|p| {
                    let var = p.then_skip(Self::read_normal_word, Self::spacing)?;
                    let body = p.read_co_proc_body(Self::read_compound_command)?;
                    Ok((Some(bx(var)), body))
                })
            },
        ])?;
        let id = self.end_span(start);
        Ok(tk(id, T_CoProc(var, bx(body))))
    }

    fn read_co_proc_body(&mut self, parser: fn(&mut Self) -> R<Token>) -> R<Token> {
        let start = self.start_span();
        let body = parser(self)?;
        let id = self.end_span(start);
        Ok(tk(id, T_CoProcBody(bx(body))))
    }

    // ----- compound commands

    fn read_condition_command(&mut self) -> R<Token> {
        let cmd = self.read_condition()?;
        let redirs = self.many(Self::read_io_redirect)?;
        let mut all: Vec<&Token> = vec![&cmd];
        all.extend(redirs.iter());
        let id = self.id_spanning_list(&all);
        let pos = self.pos;
        let has_dash_ao = self.is_followed_by(|p| {
            let c = p.choice(&mut [
                &mut |p: &mut Self| p.try_(|p| p.string("-o")),
                &mut |p: &mut Self| p.try_(|p| p.string("-a")),
                &mut |p: &mut Self| p.try_(|p| p.string("or")),
                &mut |p: &mut Self| p.try_(|p| p.string("and")),
            ])?;
            let pos_end = p.pos;
            let alt = match c.as_str() {
                "or" | "-o" => "||",
                "and" | "-a" => "&&",
                _ => "|| or &&",
            };
            p.parse_problem_at_with_end(
                pos,
                pos_end,
                ErrorC,
                1139,
                &format!("Use {alt} instead of '{c}' between test commands."),
            );
            Ok(())
        })?;
        let has_keyword = self.is_followed_by(Self::read_keyword)?;
        let has_word = self.is_followed_by(Self::read_normal_word)?;
        if has_word && !(has_keyword || has_dash_ao) {
            let pos_end = self.pos;
            self.parse_problem_at_with_end(
                pos,
                pos_end,
                ErrorC,
                1140,
                "Unexpected parameters after condition. Missing &&/||, or bad expression?",
            );
        }
        Ok(tk(id, T_Redirecting(redirs, bx(cmd))))
    }

    fn read_compound_command(&mut self) -> R<Token> {
        let cmd = self.choice(&mut [
            &mut Self::read_brace_group,
            &mut |p: &mut Self| {
                p.read_ambiguous("((", Self::read_arithmetic_expression, Self::read_subshell, |p, pos| {
                    p.parse_note_at(
                        pos,
                        ErrorC,
                        1105,
                        "Shells disambiguate (( differently or not at all. For subshell, add spaces around ( . For ((, fix parsing errors.",
                    );
                })
            },
            &mut Self::read_subshell,
            &mut Self::read_while_clause,
            &mut Self::read_until_clause,
            &mut Self::read_if_clause,
            &mut Self::read_for_clause,
            &mut Self::read_select_clause,
            &mut Self::read_case_clause,
            &mut Self::read_bats_test,
            &mut Self::read_function_definition,
        ])?;
        let redirs = self.many(Self::read_io_redirect)?;
        let mut all: Vec<&Token> = vec![&cmd];
        all.extend(redirs.iter());
        let id = self.id_spanning_list(&all);
        self.optional(|p| {
            p.look_ahead(|p| {
                p.not_followed_by2(|p| p.choice(&mut [&mut Self::read_keyword, &mut Self::g_lbrace]))?;
                let pos = p.pos;
                p.many1(Self::read_normal_word)?;
                let pos_end = p.pos;
                p.parse_problem_at_with_end(
                    pos,
                    pos_end,
                    ErrorC,
                    1141,
                    "Unexpected tokens after compound command. Bad redirection or missing ;/&&/||/|?",
                );
                Ok(())
            })
        })?;
        Ok(tk(id, T_Redirecting(redirs, bx(cmd))))
    }

    pub fn read_compound_list(&mut self) -> R<Vec<Token>> {
        self.read_term()
    }

    pub fn read_compound_list_or_empty(&mut self) -> R<Vec<Token>> {
        self.allspacing()?;
        self.alt(Self::read_term, |p| p.ok(Vec::new()))
    }

    fn read_cmd_prefix(&mut self) -> R<Vec<Token>> {
        self.many1(|p| p.alt(Self::read_io_redirect, Self::read_assignment_word))
    }

    fn read_cmd_suffix(&mut self) -> R<Vec<Token>> {
        self.many1(|p| p.alt(Self::read_io_redirect, Self::read_cmd_word))
    }

    fn read_modifier_suffix(&mut self) -> R<Vec<Token>> {
        self.many1(|p| {
            p.alt(Self::read_io_redirect, |p| {
                p.alt(Self::read_well_formed_assignment, Self::read_cmd_word)
            })
        })
    }

    fn read_time_suffix(&mut self) -> R<Vec<Token>> {
        let mut flags = self.many(|p| {
            p.look_ahead(|p| p.char_('-'))?;
            p.read_cmd_word()
        })?;
        let pipeline = self.read_pipeline()?;
        flags.push(pipeline);
        Ok(flags)
    }

    fn read_let_suffix(&mut self) -> R<Vec<Token>> {
        self.many1(|p| {
            p.alt(Self::read_io_redirect, |p| {
                p.alt(|p| p.try_(Self::read_let_expression), Self::read_cmd_word)
            })
        })
    }

    fn read_let_expression(&mut self) -> R<Token> {
        let start_pos = self.pos;
        let expression = self.read_string_for_parser(Self::read_cmd_word)?;
        let (unquoted, new_pos) = kludge_away_quotes(&expression, start_pos);
        self.sub_parse(new_pos, &unquoted, |p| {
            let t = p.read_arithmetic_contents()?;
            p.eof()?;
            Ok(t)
        })
    }

    fn read_eval_suffix(&mut self) -> R<Vec<Token>> {
        self.many1(|p| {
            p.alt(Self::read_io_redirect, |p| {
                p.alt(Self::read_cmd_word, |p| {
                    let pos = p.pos;
                    p.look_ahead(|p| p.char_('('))?;
                    p.parse_problem_at(
                        pos,
                        WarningC,
                        1098,
                        "Quote/escape special characters when using eval, e.g. eval \"a=(b)\".",
                    );
                    p.fail("Unexpected parentheses. Make sure to quote when eval'ing as shell parsers differ.")
                })
            })
        })
    }

    // ----- assignments

    pub fn read_assignment_word(&mut self) -> R<Token> {
        self.read_assignment_word_ext(true)
    }

    fn read_well_formed_assignment(&mut self) -> R<Token> {
        self.read_assignment_word_ext(false)
    }

    fn read_assignment_word_ext(&mut self, lenient: bool) -> R<Token> {
        self.called("variable assignment", |p| {
            let (id, variable, op, indices) = p.try_(|p| {
                let start = p.start_span();
                let leading_dollar_pos = if lenient {
                    p.option_maybe(|p| {
                        let start = p.pos;
                        p.char_('$')?;
                        Ok((start, p.pos))
                    })?
                } else {
                    None
                };
                let variable = p.read_variable_name()?;
                let indices = p.many(Self::read_array_index)?;
                let has_left_space = !p.spacing()?.is_empty();
                let id = p.end_span(start);
                let op = p.read_assignment_op()?;
                if leading_dollar_pos.is_some() || has_left_space {
                    let has_paren = p.is_followed_by(|p| {
                        p.spacing()?;
                        p.char_('(')
                    })?;
                    if has_paren && let Some((l, r)) = leading_dollar_pos {
                        p.parse_problem_at_with_end(l, r, ErrorC, 1066, "Don't use $ on the left side of assignments.");
                    }
                    return p.fail("");
                }
                Ok((id, variable, op, indices))
            })?;
            let right_pos_start = p.pos;
            let has_right_space = !p.spacing()?.is_empty();
            let right_pos_end = p.pos;
            let is_end_of_command = p
                .option_maybe(|p| p.try_(|p| p.look_ahead(|p| p.alt(|p| p.one_of("\r\n;&|)").map(|_| ()), Self::eof))))?
                .is_some();
            if has_right_space || is_end_of_command {
                if variable != "IFS" && has_right_space && !is_end_of_command {
                    p.parse_problem_at_with_end(
                        right_pos_start,
                        right_pos_end,
                        WarningC,
                        1007,
                        "Remove space after = if trying to assign a value (for empty string, use var='' ... ).",
                    );
                }
                let value = p.read_empty_literal()?;
                Ok(tk(id, T_Assignment(op, variable, indices, bx(value))))
            } else {
                p.optional(|p| {
                    p.look_ahead(|p| p.char_('='))?;
                    p.parse_problem(
                        ErrorC,
                        1097,
                        "Unexpected ==. For assignment, use =. For comparison, use [/[[. Or quote for literal string.",
                    );
                    Ok(())
                })?;
                let value = p.alt(Self::read_array, Self::read_normal_word)?;
                p.spacing()?;
                Ok(tk(id, T_Assignment(op, variable, indices, bx(value))))
            }
        })
    }

    fn read_assignment_op(&mut self) -> R<super::ast::AssignmentMode> {
        self.unexpecting("===", |p| p.string("==="))?;
        self.choice(&mut [
            &mut |p: &mut Self| p.string("+=").map(|_| super::ast::AssignmentMode::Append),
            &mut |p: &mut Self| p.string("=").map(|_| super::ast::AssignmentMode::Assign),
        ])
    }

    #[allow(
        clippy::unnecessary_wraps,
        reason = "a parser that always succeeds, used as an `alt` alternative"
    )]
    fn read_empty_literal(&mut self) -> R<Token> {
        let start = self.start_span();
        let id = self.end_span(start);
        Ok(tk(id, T_Literal(String::new())))
    }

    fn read_array_index(&mut self) -> R<Token> {
        let start = self.start_span();
        self.char_('[')?;
        let pos = self.pos;
        let str_ = self.read_string_for_parser(Self::read_index_span)?;
        self.char_(']')?;
        let id = self.end_span(start);
        Ok(tk(id, T_UnparsedIndex(pos, str_)))
    }

    fn read_array(&mut self) -> R<Token> {
        self.called("array assignment", |p| {
            let start = p.start_span();
            let opening = p.pos;
            p.char_('(')?;
            p.optional(|p| {
                p.look_ahead(|p| p.char_('('))?;
                p.parse_problem_at(
                    opening,
                    ErrorC,
                    1116,
                    "Missing $ on a $((..)) expression? (or use ( ( for arrays).",
                );
                Ok(())
            })?;
            p.allspacing()?;
            let words = p.reluctantly_till(
                |p| {
                    p.then_skip(
                        |p| p.alt(Self::read_array_indexed, Self::read_array_regular),
                        Self::allspacing,
                    )
                },
                |p| p.char_(')'),
            )?;
            p.alt(
                |p| p.char_(')').map(|_| ()),
                |p| p.fail("Expected ) to close array assignment"),
            )?;
            let id = p.end_span(start);
            Ok(tk(id, T_Array(words)))
        })
    }

    fn read_array_indexed(&mut self) -> R<Token> {
        let start = self.start_span();
        let index = self.try_(|p| {
            let x = p.many1(Self::read_array_index)?;
            p.char_('=')?;
            Ok(x)
        })?;
        let value = self.alt(Self::read_array_regular, Self::read_empty_literal)?;
        let id = self.end_span(start);
        Ok(tk(id, T_IndexedElement(index, bx(value))))
    }

    fn read_array_regular(&mut self) -> R<Token> {
        self.alt(Self::read_array, Self::read_normal_word)
    }

    // ----- shebang, end of file, the script

    fn read_shebang(&mut self) -> R<Token> {
        let start = self.start_span();
        self.alt(Self::any_shebang, |p| {
            p.alt(
                |p| p.try_(Self::read_missing_bang),
                |p| {
                    p.try_(|p| {
                        p.many1(|p| {
                            p.not_followed_by2(Self::any_shebang)?;
                            p.many(Self::linewhitespace)?;
                            p.optional(Self::read_any_comment)?;
                            p.linefeed()
                        })?;
                        let pos = p.pos;
                        p.any_shebang()?;
                        p.parse_problem_at(
                        pos,
                        ErrorC,
                        1128,
                        "The shebang must be on the first line. Delete blanks and move comments.",
                    );
                        Ok(())
                    })
                },
            )
        })?;
        self.many(Self::linewhitespace)?;
        let str_: String = self.many(|p| p.none_of("\r\n"))?.into_iter().collect();
        let id = self.end_span(start);
        self.optional(Self::carriage_return)?;
        self.optional(Self::linefeed)?;
        Ok(tk(id, T_Literal(str_)))
    }

    fn any_shebang(&mut self) -> R<()> {
        self.choice(&mut [
            &mut |p: &mut Self| p.try_(|p| p.string("#!").map(|_| ())),
            &mut |p: &mut Self| {
                p.try_(|p| {
                    let start = p.start_span();
                    p.string("!#")?;
                    let id = p.end_span(start);
                    p.parse_problem_at_id(id, ErrorC, 1084, "Use #!, not !#, for the shebang.");
                    Ok(())
                })
            },
            &mut |p: &mut Self| {
                p.try_(|p| {
                    let start_pos = p.pos;
                    let start_spaces = !p.many(Self::linewhitespace)?.is_empty();
                    p.char_('#')?;
                    let middle_pos = p.pos;
                    let middle_spaces = !p.many(Self::linewhitespace)?.is_empty();
                    p.char_('!')?;
                    if start_spaces {
                        p.parse_problem_at(
                            start_pos,
                            ErrorC,
                            1114,
                            "Remove leading spaces before the shebang.",
                        );
                    }
                    if middle_spaces {
                        p.parse_problem_at(
                            middle_pos,
                            ErrorC,
                            1115,
                            "Remove spaces between # and ! in the shebang.",
                        );
                    }
                    Ok(())
                })
            },
            &mut |p: &mut Self| {
                p.try_(|p| {
                    let pos = p.pos;
                    p.char_('!')?;
                    p.ensure_path_ahead()?;
                    p.parse_problem_at(pos, ErrorC, 1104, "Use #!, not just !, for the shebang.");
                    Ok(())
                })
            },
        ])
    }

    fn read_missing_bang(&mut self) -> R<()> {
        self.char_('#')?;
        let pos = self.pos;
        self.ensure_path_ahead()?;
        self.parse_problem_at(pos, ErrorC, 1113, "Use #!, not just #, for the shebang.");
        Ok(())
    }

    fn ensure_path_ahead(&mut self) -> R<char> {
        self.look_ahead(|p| {
            p.many(Self::linewhitespace)?;
            p.char_('/')
        })
    }

    pub fn verify_eof(&mut self) -> R<()> {
        self.alt(Self::eof, |p| {
            p.choice(&mut [
                &mut |p: &mut Self| {
                    p.try_(|p| p.look_ahead(Self::g_lparen))?;
                    p.parse_problem(
                        ErrorC,
                        1088,
                        "Parsing stopped here. Invalid use of parentheses?",
                    );
                    Ok(())
                },
                &mut |p: &mut Self| {
                    p.try_(|p| p.look_ahead(Self::read_keyword))?;
                    p.parse_problem(
                        ErrorC,
                        1089,
                        "Parsing stopped here. Is this keyword correctly matched up?",
                    );
                    Ok(())
                },
                &mut |p: &mut Self| {
                    p.parse_problem(
                        ErrorC,
                        1070,
                        "Parsing stopped here. Mismatched keywords or invalid parentheses?",
                    );
                    Ok(())
                },
            ])
        })
    }

    /// `tryWithErrors`: like `try`, but a failure is reported.
    pub fn try_with_errors<T>(&mut self, parser: impl FnOnce(&mut Self) -> R<T>) -> R<T> {
        let m = self.mark();
        let old_context = self.sys.contexts.clone();
        let input_was_empty = self.at_eof();
        // The inner parse is a separate Parsec run: fresh error, fresh
        // consumption.
        self.err = ParseError::unknown(self.pos);
        match parser(self) {
            Ok(v) => {
                if !input_was_empty && self.counter <= m.counter {
                    self.counter = m.counter + 1;
                }
                self.err = ParseError::unknown(self.pos);
                Ok(v)
            }
            Err(f) => {
                let new_context = self.sys.contexts.clone();
                self.sys.problems.push(make_error_for(&f.err));
                for note in notes_for_context(&new_context) {
                    self.sys.problems.push(note);
                }
                self.sys.contexts = old_context;
                self.restore(&m);
                self.fail("")
            }
        }
    }

    /// `readConfigFile`.
    fn read_config_file(&mut self, filename: &str) -> Vec<Annotation> {
        if self.env.ignore_rc {
            return Vec::new();
        }
        let sys = Rc::clone(&self.env.system);
        let Some((file, contents)) = sys.get_config(filename) else {
            return Vec::new();
        };
        // A separate Parsec run on the rc file, sharing the system state.
        let saved_input = self.input.clone();
        let saved_idx = self.idx;
        let saved_pos = self.pos;
        let saved_counter = self.counter;
        let saved_err = self.err.clone();
        let saved_user = std::mem::take(&mut self.user);
        let file_index = self.file_index(&file);
        self.input = Rc::new(contents.chars().collect());
        self.idx = 0;
        self.pos = SourcePos {
            file: file_index,
            line: 1,
            column: 1,
        };
        self.err = ParseError::unknown(self.pos);
        let result = (|| -> R<Vec<Annotation>> {
            self.any_spacing_or_comment()?;
            let annotations = self.many(|p| {
                let a = p.read_annotation_without_prefix(false)?;
                p.any_spacing_or_comment()?;
                Ok(a)
            })?;
            self.eof()?;
            Ok(annotations.concat())
        })();
        self.input = saved_input;
        self.idx = saved_idx;
        self.pos = saved_pos;
        self.counter = saved_counter;
        self.err = saved_err;
        self.user = saved_user;
        match result {
            Ok(v) => v,
            Err(Fail { err, .. }) => {
                let msg = format!(
                    "Failed to process {}, line {}: {}",
                    super::astlib::e4m(&file),
                    err.pos.line,
                    get_string_from_parsec(&err)
                );
                self.parse_problem(ErrorC, 1134, &msg);
                Vec::new()
            }
        }
    }

    fn any_spacing_or_comment(&mut self) -> R<()> {
        self.many(|p| {
            p.alt(
                |p| p.allspacing_or_fail().map(|_| ()),
                |p| p.read_any_comment().map(|_| ()),
            )
        })?;
        Ok(())
    }

    /// `readScriptFile sourced`.
    pub fn read_script_file(&mut self, sourced: bool) -> R<Token> {
        let start = self.start_span();
        let pos = self.pos;
        let rc_annotations = if sourced {
            Vec::new()
        } else {
            let filename = self.env.current_filename.clone();
            self.read_config_file(&filename)
        };
        self.with_annotations(rc_annotations.clone(), |p| {
            let has_bom = p.was_included(|p| p.called("Byte Order Mark", |p| p.string("\u{FEFF}")))?;
            let shebang = p.alt(Self::read_shebang, Self::read_empty_literal)?;
            let shebang_string = match &shebang.inner {
                T_Literal(s) => s.clone(),
                _ => String::new(),
            };
            p.allspacing()?;
            let annotation_start = p.start_span();
            let file_annotations = p.read_annotations()?;
            p.with_annotations(file_annotations.clone(), |p| {
                if has_bom {
                    p.parse_problem_at(
                        pos,
                        ErrorC,
                        1082,
                        "This file has a UTF-8 BOM. Remove it with: LC_CTYPE=C sed '1s/^...//' < yourscript .",
                    );
                }
                let mut annotations = file_annotations.clone();
                annotations.extend(rc_annotations.iter().cloned());
                let annotation_id = p.end_span(annotation_start);
                let shell_annotation_specified = annotations.iter().any(|a| matches!(a, Annotation::ShellOverride(_)));
                let shell_flag_specified = p.env.shell_type_override.is_some();
                let ignore_shebang = shell_annotation_specified || shell_flag_specified;
                let executable = executable_from_shebang(&shebang_string);
                if !ignore_shebang {
                    match is_valid_shell(&executable) {
                        Some(true) => {}
                        Some(false) => p.parse_problem_at(
                            pos,
                            ErrorC,
                            1071,
                            "ShellCheck only supports sh/bash/dash/ksh/'busybox sh' scripts. Sorry!",
                        ),
                        None => p.parse_problem_at(
                            pos,
                            ErrorC,
                            1008,
                            "This shebang was unrecognized. ShellCheck only supports sh/bash/dash/ksh/'busybox sh'. Add a 'shell' directive to specify.",
                        ),
                    }
                }
                if ignore_shebang || is_valid_shell(&executable) != Some(false) {
                    let commands = p.read_compound_list_or_empty()?;
                    let id = p.end_span(start);
                    p.read_pending_here_docs()?;
                    p.verify_eof()?;
                    let script = tk(annotation_id, T_Annotation(annotations, bx(tk(id, T_Script(bx(shebang), commands)))));
                    let mut here_docs: HashMap<Id, Vec<Token>> = HashMap::new();
                    for (hid, list) in &p.user.here_docs {
                        here_docs.insert(*hid, list.clone());
                    }
                    let script = reattach_here_docs(script, &here_docs);
                    p.reparse_indices(script)
                } else {
                    p.many(Self::any_char)?;
                    let id = p.end_span(start);
                    Ok(tk(id, T_Script(bx(shebang), Vec::new())))
                }
            })
        })
    }

    /// `reparseIndices`.
    fn reparse_indices(&mut self, root: Token) -> R<Token> {
        let associative = get_associative_arrays(&root);
        self.reparse_process(root, &associative)
    }

    fn reparse_process(&mut self, mut t: Token, associative: &[String]) -> R<Token> {
        let mut failure: Option<Fail> = None;
        t.for_each_child_mut(&mut |child| {
            if failure.is_some() {
                return;
            }
            let taken = std::mem::replace(child, tk(Id(0), T_EOF));
            match self.reparse_process(taken, associative) {
                Ok(new) => *child = new,
                Err(f) => failure = Some(f),
            }
        });
        if let Some(f) = failure {
            return Err(f);
        }
        match &mut t.inner {
            T_Assignment(_, name, indices, value) => {
                let name = name.clone();
                let old = std::mem::take(indices);
                let mut new_indices = Vec::new();
                for idx in old {
                    new_indices.push(self.fix_assignment_index(&name, idx, associative)?);
                }
                *indices = new_indices;
                if let T_Array(words) = &mut value.inner {
                    let old_words = std::mem::take(words);
                    let mut new_words = Vec::new();
                    for w in old_words {
                        new_words.push(match w.inner {
                            T_IndexedElement(idxs, v) => {
                                let mut new = Vec::new();
                                for i in idxs {
                                    new.push(self.fix_assignment_index(&name, i, associative)?);
                                }
                                tk(w.id, T_IndexedElement(new, v))
                            }
                            other => tk(w.id, other),
                        });
                    }
                    *words = new_words;
                }
            }
            TA_Variable(name, indices) => {
                let name = name.clone();
                let old = std::mem::take(indices);
                let mut new_indices = Vec::new();
                for idx in old {
                    new_indices.push(self.fix_assignment_index(&name, idx, associative)?);
                }
                *indices = new_indices;
            }
            _ => {}
        }
        Ok(t)
    }

    fn fix_assignment_index(
        &mut self,
        name: &str,
        word: Token,
        associative: &[String],
    ) -> R<Token> {
        match word.inner {
            T_UnparsedIndex(pos, src) => {
                let idx = if associative.iter().any(|a| a == name) {
                    self.sub_parse(pos, &src, |p| {
                        p.called("associative array index", Self::read_index_span)
                    })?
                } else {
                    self.sub_parse(pos, &src, |p| {
                        p.called("arithmetic array index expression", |p| {
                            p.optional(|p| p.satisfy(is_space))?;
                            p.read_arithmetic_contents()
                        })
                    })?
                };
                self.reparse_process(idx, associative)
            }
            other => Ok(tk(word.id, other)),
        }
    }
}

fn make_simple_command(
    id1: Id,
    id2: Id,
    prefix: Vec<Token>,
    cmd: Vec<Token>,
    suffix: Vec<Token>,
) -> Token {
    let (pre_assigned, pre_rest): (Vec<Token>, Vec<Token>) = prefix
        .into_iter()
        .partition(|t| matches!(t.inner, T_Assignment(..)));
    let (pre_redirected, pre_rest2): (Vec<Token>, Vec<Token>) = pre_rest
        .into_iter()
        .partition(|t| matches!(t.inner, T_FdRedirect(..)));
    let (post_redirected, post_rest): (Vec<Token>, Vec<Token>) = suffix
        .into_iter()
        .partition(|t| matches!(t.inner, T_FdRedirect(..)));
    let mut redirs = pre_redirected;
    redirs.extend(post_redirected);
    let mut args = cmd;
    args.extend(pre_rest2);
    args.extend(post_rest);
    tk(
        id1,
        T_Redirecting(redirs, bx(tk(id2, T_SimpleCommand(pre_assigned, args)))),
    )
}

fn bats_name(line: &str) -> String {
    // Everything before the last " {", without trailing spaces.
    let rev: Vec<char> = line.chars().rev().collect();
    let mut i = 0;
    while i < rev.len() {
        if rev[i] == '{' && i + 1 < rev.len() && rev[i + 1] == ' ' {
            let rest: String = rev[i + 2..].iter().collect();
            let trimmed: String = rest.chars().skip_while(|&c| is_space(c)).collect();
            return trimmed.chars().rev().collect();
        }
        i += 1;
    }
    String::new()
}

fn kludge_away_quotes(s: &str, p: SourcePos) -> (String, SourcePos) {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() >= 2 {
        let first = chars[0];
        let last = chars[chars.len() - 1];
        if (first == '\'' || first == '"') && first == last {
            return (
                chars[1..chars.len() - 1].iter().collect(),
                super::parsec::update_pos_char(p, first),
            );
        }
    }
    (s.to_string(), p)
}

fn is_valid_shell(s: &str) -> Option<bool> {
    const GOOD: &[&str] = &[
        "sh",
        "ash",
        "dash",
        "busybox sh",
        "bash",
        "bats",
        "ksh",
        "oksh",
    ];
    const BAD: &[&str] = &[
        "awk", "csh", "expect", "fish", "perl", "python", "python3", "ruby", "tcsh", "zsh",
    ];
    let good = s.is_empty() || GOOD.iter().any(|g| s.starts_with(g));
    let bad = BAD.iter().any(|b| s.starts_with(b));
    if good {
        Some(true)
    } else if bad {
        Some(false)
    } else {
        None
    }
}

fn reattach_here_docs(mut root: Token, map: &HashMap<Id, Vec<Token>>) -> Token {
    super::ast::do_transform(&mut root, &mut |t| {
        if let T_HereDoc(_, _, _, list) = &mut t.inner
            && list.is_empty()
            && let Some(found) = map.get(&t.id)
        {
            list.clone_from(found);
        }
    });
    root
}

/// `getStringFromParsec`.
pub fn get_string_from_parsec(err: &ParseError) -> String {
    let msgs = err.sorted_messages();
    let last = msgs.iter().rev().find_map(|m| match m {
        Msg::Message(s) if !s.is_empty() => Some(format!("{s}.")),
        _ => None,
    });
    format!(
        "{} Fix any mentioned problems and try again.",
        last.unwrap_or_default()
    )
}

fn make_error_for(err: &ParseError) -> ParseNote {
    ParseNote {
        start: err.pos,
        end: err.pos,
        severity: ErrorC,
        code: 1072,
        msg: get_string_from_parsec(err),
    }
}

/// `notesForContext`: the innermost two named contexts.
fn notes_for_context(contexts: &[Context]) -> Vec<ParseNote> {
    let named: Vec<(SourcePos, &String)> = contexts
        .iter()
        .rev()
        .filter_map(|c| match c {
            Context::Name(pos, s) => Some((*pos, s)),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    if let Some((pos, s)) = named.first() {
        out.push(ParseNote {
            start: *pos,
            end: *pos,
            severity: ErrorC,
            code: 1073,
            msg: format!("Couldn't parse this {s}. Fix to allow more checks."),
        });
    }
    if let Some((pos, s)) = named.get(1) {
        out.push(ParseNote {
            start: *pos,
            end: *pos,
            severity: InfoC,
            code: 1009,
            msg: format!("The mentioned syntax error was in this {s}."),
        });
    }
    out
}

fn to_positioned_comment(files: &[String], note: &ParseNote) -> PositionedComment {
    PositionedComment {
        start: pos_to_pos(files, note.start),
        end: pos_to_pos(files, note.end),
        comment: Comment {
            severity: note.severity,
            code: note.code,
            message: note.msg.clone(),
        },
        fix: None,
    }
}

pub fn pos_to_pos(files: &[String], sp: SourcePos) -> Position {
    Position {
        file: files[sp.file as usize].clone(),
        line: i64::from(sp.line),
        column: i64::from(sp.column),
    }
}

/// `parseScript`.
pub fn parse_script(system: Rc<dyn SystemInterface>, spec: &ParseSpec) -> ParseResult {
    let env = Environment {
        system,
        check_sourced: spec.check_sourced,
        ignore_rc: spec.ignore_rc,
        current_filename: spec.filename.clone(),
        shell_type_override: spec.shell_type_override,
    };
    let mut p = P::new(env, &spec.filename, &spec.script);
    let result = p.read_script_file(false);
    match result {
        Ok(script) => {
            // Parsec's lists are newest first.
            let mut notes: Vec<ParseNote> = p.user.notes.iter().rev().cloned().collect();
            notes.extend(p.sys.problems.iter().rev().cloned());
            let mut unique: Vec<ParseNote> = Vec::new();
            for n in notes {
                if !unique.contains(&n) {
                    unique.push(n);
                }
            }
            let comments = unique
                .iter()
                .map(|n| to_positioned_comment(&p.files, n))
                .collect();
            let mut token_positions = HashMap::new();
            if let Some(last) = p.user.last_id {
                for i in 0..=last {
                    let (s, e) = p.user.positions[i];
                    token_positions
                        .insert(Id(i), (pos_to_pos(&p.files, s), pos_to_pos(&p.files, e)));
                }
            }
            ParseResult {
                comments,
                token_positions,
                root: Some(script),
            }
        }
        Err(f) => {
            let context = p.sys.contexts.clone();
            let is_ignored = |note: &ParseNote| {
                context
                    .iter()
                    .any(|c| context_disables_code(false, note.code, c))
            };
            let mut notes: Vec<ParseNote> = notes_for_context(&context);
            notes.push(make_error_for(&f.err));
            let mut comments: Vec<PositionedComment> = notes
                .iter()
                .filter(|n| !is_ignored(n))
                .map(|n| to_positioned_comment(&p.files, n))
                .collect();
            comments.extend(
                p.sys
                    .problems
                    .iter()
                    .rev()
                    .map(|n| to_positioned_comment(&p.files, n)),
            );
            ParseResult {
                comments,
                token_positions: HashMap::new(),
                root: None,
            }
        }
    }
}
